use std::process::Command as ShellCommand;
use std::time::Instant;

use log::error;

use crate::actions::Action;
use crate::config::{ControlRef, InputKind};
use crate::midi::MidiEvent;
use crate::pipewire::PwCommand;

use super::feedback::{find_pw_object, route_is_active_with_edges};
use super::{apply_volume, user_shell, App, VolumeParams, VOLUME_FIGHT_WINDOW};

impl App {
    fn push_monitor(&mut self, line: String) {
        self.midi_monitor.push(line);
        if self.midi_monitor.len() > 50 {
            self.midi_monitor.remove(0);
        }
    }

    pub(super) fn handle_midi_event(&mut self, event: MidiEvent) {
        if !matches!(&event, MidiEvent::SysEx { .. }) {
            self.push_monitor(format!("{event:?}"));
        }

        match &event {
            MidiEvent::Connected { port_name } => {
                self.connected_devices.insert(port_name.clone());
                self.status_message = format!("Connected: {port_name}");
            }
            MidiEvent::Cc {
                port_name,
                channel,
                controller,
                value,
            } => {
                if let Some((device, hw_id, input_idx)) = self.learn_target.clone() {
                    self.apply_learn(
                        &device,
                        &hw_id,
                        input_idx,
                        InputKind::Cc,
                        *channel,
                        *controller,
                    );
                    return;
                }
                self.dispatch_hardware_event(
                    port_name,
                    InputKind::Cc,
                    *channel,
                    *controller,
                    *value,
                );
            }
            MidiEvent::NoteOn {
                port_name,
                channel,
                note,
                velocity,
            } => {
                if let Some((device, hw_id, input_idx)) = self.learn_target.clone() {
                    self.apply_learn(&device, &hw_id, input_idx, InputKind::Note, *channel, *note);
                    return;
                }
                self.dispatch_hardware_event(
                    port_name,
                    InputKind::Note,
                    *channel,
                    *note,
                    *velocity,
                );
            }
            MidiEvent::PitchBend {
                port_name,
                channel,
                value,
            } => {
                if let Some((device, hw_id, input_idx)) = self.learn_target.clone() {
                    self.apply_learn(
                        &device,
                        &hw_id,
                        input_idx,
                        InputKind::PitchBend,
                        *channel,
                        0,
                    );
                    return;
                }
                let scaled = (*value >> 7) as u8;
                self.dispatch_hardware_event(port_name, InputKind::PitchBend, *channel, 0, scaled);
            }
            MidiEvent::SysEx { port_name, data } => {
                let hex: String = data
                    .iter()
                    .map(|b| format!("{b:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                self.push_monitor(format!("SX: {port_name}  {hex}"));
            }
        }
    }

    fn apply_learn(
        &mut self,
        device_name: &str,
        hw_id: &str,
        input_idx: usize,
        kind: InputKind,
        ch: u8,
        num: u8,
    ) {
        if let Some(device) = self.config.devices.get_mut(device_name) {
            if let Some(hw) = device.hardware.iter_mut().find(|h| h.id == hw_id) {
                if let Some(input) = hw.inputs.get_mut(input_idx) {
                    input.kind = kind;
                    input.channel = ch;
                    input.number = if kind == InputKind::PitchBend { 0 } else { num };
                    let captured = format!("MIDI Learn: captured {} ch{} #{}", hw.id, ch, num);
                    self.mark_dirty();
                    self.status_message = captured;
                }
            }
        }
        self.learn_target = None;
    }

    fn dispatch_hardware_event(
        &mut self,
        port_name: &str,
        kind: InputKind,
        ch: u8,
        num: u8,
        raw: u8,
    ) {
        let device = match self.config.devices.get(port_name) {
            Some(d) => d,
            None => return,
        };

        let matching_hw_id = device.hardware.iter().find_map(|hw| {
            hw.inputs
                .iter()
                .find(|input| {
                    input.kind == kind
                        && input.channel == ch
                        && (kind == InputKind::PitchBend || input.number == num)
                })
                .map(|input| (hw.id.clone(), input.label.clone()))
        });

        let (hw_id, input_label) = match matching_hw_id {
            Some(v) => v,
            None => return,
        };

        let layer = match self.active_layer(device, port_name) {
            Some(l) => l,
            None => return,
        };

        let software = match layer.software(&hw_id) {
            Some(s) => s,
            None => return,
        };

        let actions_to_execute: Vec<Action> = software
            .signal_entries
            .iter()
            .filter(|se| se.input == input_label)
            .flat_map(|se| se.actions.clone())
            .collect();

        let layer_switch: Option<String> = software
            .signal_entries
            .iter()
            .filter(|se| se.input == input_label)
            .flat_map(|se| &se.actions)
            .find_map(|a| {
                if let Action::SelectLayer { layer } = a {
                    Some(layer.clone())
                } else {
                    None
                }
            });

        if let Some(new_layer) = layer_switch {
            let exists = self
                .config
                .devices
                .get(port_name)
                .is_some_and(|d| d.layers.iter().any(|l| l.name == new_layer));
            if exists {
                self.active_layers
                    .insert(port_name.to_string(), new_layer.clone());
                // Keep the UI in sync when the triggering device is the one selected.
                if self.selected_device_name.as_deref() == Some(port_name) {
                    self.selected_layer = Some(new_layer);
                }
                self.clear_editing();
            }
        }

        for action in &actions_to_execute {
            self.execute_action(action, raw, port_name);
        }

        self.last_input_value
            .insert(ControlRef::new(port_name, &hw_id, &input_label), raw);
        self.send_feedback();
        self.send_visual_evaluation();
    }
}

impl App {
    fn execute_action(&mut self, action: &Action, raw_value: u8, device_name: &str) {
        match action {
            Action::None => {}
            Action::SetVolume {
                target: id,
                min,
                max,
                mode,
                encoder,
                step,
                ..
            } => self.execute_volume_action(
                id,
                VolumeParams {
                    min: *min,
                    max: *max,
                    mode: *mode,
                    encoder: *encoder,
                    step: *step,
                    raw: raw_value,
                },
            ),
            Action::ToggleMute { target } => {
                let muted =
                    find_pw_object(self.pw_objects.as_slice(), target).is_some_and(|o| o.muted);
                let _ = self.pw_cmd_tx.send(PwCommand::Mute {
                    target: target.clone(),
                    muted: !muted,
                });
            }
            Action::RouteStream { stream_id, sink_id } => {
                let active = route_is_active_with_edges(
                    self.pw_edges.as_slice(),
                    self.pw_objects.as_slice(),
                    stream_id,
                    sink_id,
                );
                let _ = self.pw_cmd_tx.send(PwCommand::Route {
                    stream: stream_id.clone(),
                    sink: sink_id.clone(),
                    connect: !active,
                });
            }
            Action::SelectLayer { .. } => {}
            Action::RunCommand { command } => {
                let bank = self
                    .config
                    .devices
                    .get(device_name)
                    .and_then(|d| self.active_layer(d, device_name))
                    .map(|l| l.bank_offset)
                    .unwrap_or(0);
                let cmd = command
                    .replace("$RAW_VALUE", &raw_value.to_string())
                    .replace("$NORM_VALUE", &format!("{:.4}", raw_value as f64 / 127.0))
                    .replace("$BANK_OFFSET", &bank.to_string());
                if let Err(e) = ShellCommand::new(user_shell()).arg("-c").arg(&cmd).spawn() {
                    error!("RunCommand failed: {e}");
                }
            }
            Action::MidiSendNote {
                channel,
                note,
                velocity,
            } => {
                if let Some(handle) = self.output_handles.get_mut(device_name) {
                    handle.send_note(*channel, *note, *velocity);
                }
            }
            Action::MidiSendCc {
                channel,
                controller,
                value,
            } => {
                if let Some(handle) = self.output_handles.get_mut(device_name) {
                    handle.send_cc(*channel, *controller, *value);
                }
            }
            Action::SetBankOffset { offset } => {
                if let Some(layer) = self.active_layer_mut(device_name) {
                    layer.bank_offset = *offset;
                    self.mark_dirty();
                }
            }
            Action::BankIncrement => self.adjust_bank(device_name, 8),
            Action::BankDecrement => self.adjust_bank(device_name, -8),
        }
    }

    fn adjust_bank(&mut self, device_name: &str, delta: i8) {
        let Some(layer) = self.active_layer_mut(device_name) else {
            return;
        };
        let new = if delta > 0 {
            layer.bank_offset.saturating_add(delta as u8)
        } else {
            layer.bank_offset.saturating_sub(delta.unsigned_abs())
        };
        layer.bank_offset = new;
        self.mark_dirty();
    }

    fn execute_volume_action(&mut self, id: &str, params: VolumeParams) {
        // Relative mode needs a known starting volume; when it is unknown
        // (streams/filter nodes, or before the first Props event) do nothing
        // instead of snapping the volume toward zero.
        let Some(vol) = apply_volume(&params, self.lookup_volume(id)) else {
            return;
        };
        let _ = self.pw_cmd_tx.send(PwCommand::Volume {
            target: id.to_string(),
            volume: vol,
        });
        self.last_volume_set.insert(id.to_string(), Instant::now());
        self.last_set_volume.insert(id.to_string(), vol);
    }

    fn lookup_volume(&self, id: &str) -> Option<f64> {
        let obj = find_pw_object(self.pw_objects.as_slice(), id)?;
        // During the fight window, use the last-set volume so relative
        // encoder changes have a known starting point.
        if let Some(last) = self.last_volume_set.get(id) {
            if last.elapsed() < VOLUME_FIGHT_WINDOW {
                return self.last_set_volume.get(id).copied();
            }
        }
        obj.volume
    }
}
