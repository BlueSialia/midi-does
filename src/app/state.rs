use std::collections::{HashMap, HashSet};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use iced::Task;
use log::error;

use crate::config::{
    ControlRef, DeviceConfig, HardwareDef, HardwareInput, HardwareOutput, InputKind, LayerDef,
    OutputKind, SoftwareDef,
};
use crate::pipewire::PwEvent;
use crate::tray::TrayMessage;

use super::feedback::{apply_feedback_to_handle, FeedbackOutput};
use super::{next_input_label, next_output_label, take_at, App, AppMessage};

impl App {
    pub(super) fn mark_dirty(&mut self) {
        self.dirty = true;
        self.last_edit = Instant::now();
    }

    pub(super) fn save_config(&mut self) {
        match self.config.save() {
            Ok(()) => {
                self.dirty = false;
                self.last_edit = Instant::now();
                self.last_periodic_save = Instant::now();
            }
            Err(e) => error!("config.save failed: {e}"),
        }
    }

    pub(super) fn poll_events(&mut self) {
        loop {
            match self.midi_rx.try_recv() {
                Ok(event) => self.handle_midi_event(event),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    if !self.midi_rx_disconnected {
                        self.midi_rx_disconnected = true;
                        log::warn!("MIDI event channel disconnected");
                    }
                    break;
                }
            }
        }
        loop {
            match self.pw_rx.try_recv() {
                Ok(event) => self.handle_pw_event(event),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    if !self.pw_rx_disconnected {
                        self.pw_rx_disconnected = true;
                        log::warn!("PipeWire event channel disconnected");
                    }
                    break;
                }
            }
        }
    }

    pub(super) fn poll_worker_output(&mut self) {
        while let Ok(output) = self.feedback_rx.try_recv() {
            match output {
                FeedbackOutput::Feedback(results) => {
                    for out in results {
                        let key = ControlRef::new(&out.device, &out.hardware_id, &out.output_label);
                        if self.last_feedback_result.get(&key) == Some(&out.result) {
                            continue;
                        }
                        if let Some(handle) = self.output_handles.get_mut(&out.device) {
                            apply_feedback_to_handle(handle, &out.output, &out.result);
                            self.last_feedback_result.insert(key, out.result);
                        }
                    }
                }
                FeedbackOutput::Visuals(results) => {
                    for out in results {
                        self.visual_values
                            .insert((out.device, out.hardware_id), out.value);
                    }
                }
            }
        }
    }

    pub(super) fn poll_tray(&mut self) -> Task<AppMessage> {
        let mut task: Task<AppMessage> = Task::none();
        while let Ok(msg) = self.tray_rx.try_recv() {
            match msg {
                TrayMessage::ShowWindow => {
                    if let Some(id) = self.window_id {
                        task = iced::window::set_mode(id, iced::window::Mode::Windowed)
                            .chain(iced::window::gain_focus(id));
                    }
                }
                TrayMessage::Quit => {
                    self.save_config();
                    return iced::exit();
                }
            }
        }
        task
    }

    pub(super) fn poll_hotplug(&mut self) {
        if self.last_hotplug_check.elapsed() < Duration::from_secs(3) {
            return;
        }
        self.last_hotplug_check = Instant::now();

        let available: HashSet<String> = self
            .midi_manager
            .available_ports()
            .into_iter()
            .map(|p| p.name)
            .collect();

        for name in self.config.device_names().to_vec() {
            if let Some(device) = self.config.devices.get(&name) {
                if !available.contains(&device.port_name) {
                    self.connected_devices.remove(&device.port_name);
                    if self.output_handles.remove(&name).is_some() {
                        self.clear_feedback_state(&name);
                    }
                }
            }
        }

        for name in self.config.device_names().to_vec() {
            let port = match self.config.devices.get(&name).map(|d| d.port_name.clone()) {
                Some(p) => p,
                None => continue,
            };
            if self.connected_devices.contains(&port) {
                continue;
            }
            match self.midi_manager.connect(&port) {
                Ok(()) => {
                    self.connected_devices.insert(port.clone());
                }
                Err(e) => log::debug!("Hotplug retry failed for {port}: {e}"),
            }
            if !self.output_handles.contains_key(&name) {
                if let Ok(h) = self.midi_manager.connect_output(&port) {
                    self.output_handles.insert(name.clone(), h);
                    self.clear_feedback_state(&name);
                    self.send_feedback_for_device(&name);
                }
            }
        }
    }

    pub(crate) fn selected_device(&self) -> Option<&DeviceConfig> {
        self.selected_device_name
            .as_ref()
            .and_then(|name| self.config.devices.get(name))
    }

    pub(super) fn selected_device_mut(&mut self) -> Option<&mut DeviceConfig> {
        let name = self.selected_device_name.clone()?;
        self.config.devices.get_mut(&name)
    }

    fn hardware(&self, hw_id: &str) -> Option<&HardwareDef> {
        self.selected_device()?
            .hardware
            .iter()
            .find(|h| h.id == hw_id)
    }

    pub(super) fn hardware_mut(&mut self, hw_id: &str) -> Option<&mut HardwareDef> {
        self.selected_device_mut()?
            .hardware
            .iter_mut()
            .find(|h| h.id == hw_id)
    }

    /// The layer of `device` currently active for `device_name`, falling back
    /// to the first layer when the recorded active layer no longer exists.
    pub(super) fn active_layer<'a>(
        &'a self,
        device: &'a DeviceConfig,
        device_name: &str,
    ) -> Option<&'a LayerDef> {
        self.active_layers
            .get(device_name)
            .and_then(|name| device.layers.iter().find(|l| &l.name == name))
            .or_else(|| device.layers.first())
    }

    /// The active layer of `device_name` for mutation. Unlike `active_layer`,
    /// this does not fall back to the first layer: bank actions only operate on
    /// an explicitly active layer.
    pub(super) fn active_layer_mut(&mut self, device_name: &str) -> Option<&mut LayerDef> {
        let name = self.active_layers.get(device_name)?.clone();
        let device = self.config.devices.get_mut(device_name)?;
        let idx = device.layers.iter().position(|l| l.name == name)?;
        device.layers.get_mut(idx)
    }

    fn clear_selected_control_state<T>(
        &mut self,
        map: impl FnOnce(&mut Self) -> &mut HashMap<ControlRef, T>,
        hw_id: &str,
        label: &str,
    ) {
        let Some(device) = self.selected_device_name.clone() else {
            return;
        };
        let target = ControlRef::new(&device, hw_id, label);
        map(self).retain(|key, _| key != &target);
    }

    fn clear_feedback_state(&mut self, device: &str) {
        self.last_feedback_result
            .retain(|key, _| key.device != device);
    }

    pub(super) fn hardware_input_mut(
        &mut self,
        hw_id: &str,
        idx: usize,
    ) -> Option<&mut HardwareInput> {
        self.hardware_mut(hw_id).and_then(|h| h.inputs.get_mut(idx))
    }

    pub(super) fn hardware_output_mut(
        &mut self,
        hw_id: &str,
        idx: usize,
    ) -> Option<&mut HardwareOutput> {
        self.hardware_mut(hw_id)
            .and_then(|h| h.outputs.get_mut(idx))
    }

    fn for_each_icon_mut(&mut self, hw_id: &str, mut f: impl FnMut(&mut SoftwareDef)) {
        if let Some(device) = self.selected_device_mut() {
            for layer in &mut device.layers {
                for icon in &mut layer.icons {
                    if icon.hardware == hw_id {
                        f(&mut icon.software);
                    }
                }
            }
        }
    }

    fn mutate_hardware_endpoint<R>(
        &mut self,
        hw_id: &str,
        f: impl FnOnce(&mut HardwareDef) -> R,
    ) -> Option<R> {
        self.hardware_mut(hw_id).map(f)
    }

    pub(super) fn add_hardware_input(&mut self, hw_id: &str) {
        let added = self
            .mutate_hardware_endpoint(hw_id, |hw| {
                let label = next_input_label(hw);
                hw.inputs.push(HardwareInput {
                    label,
                    kind: InputKind::Cc,
                    channel: 0,
                    number: 0,
                });
            })
            .is_some();
        if added {
            self.mark_dirty();
        }
    }

    pub(super) fn add_hardware_output(&mut self, hw_id: &str) {
        let added = self
            .mutate_hardware_endpoint(hw_id, |hw| {
                let label = next_output_label(hw);
                hw.outputs.push(HardwareOutput {
                    label,
                    kind: OutputKind::Led,
                    channel: 0,
                    number: 0,
                });
            })
            .is_some();
        if added {
            self.mark_dirty();
        }
    }

    pub(super) fn remove_hardware_input(&mut self, hw_id: &str, idx: usize) {
        let removed = self
            .mutate_hardware_endpoint(hw_id, |hw| take_at(&mut hw.inputs, idx).map(|i| i.label))
            .flatten();
        if let Some(removed) = removed {
            self.for_each_icon_mut(hw_id, |software| {
                software.signal_entries.retain(|se| se.input != removed);
            });
            self.clear_selected_control_state(|s| &mut s.last_input_value, hw_id, &removed);
            self.mark_dirty();
        }
    }

    pub(super) fn remove_hardware_output(&mut self, hw_id: &str, idx: usize) {
        let removed = self
            .mutate_hardware_endpoint(hw_id, |hw| take_at(&mut hw.outputs, idx).map(|o| o.label))
            .flatten();
        if let Some(removed) = removed {
            self.for_each_icon_mut(hw_id, |software| {
                software.feedback_entries.retain(|fe| fe.output != removed);
            });
            self.clear_selected_control_state(|s| &mut s.last_feedback_result, hw_id, &removed);
            self.mark_dirty();
        }
    }

    pub(super) fn rename_hardware_input(&mut self, hw_id: &str, idx: usize, label: String) {
        let Some(old) = self
            .hardware(hw_id)
            .and_then(|h| h.inputs.get(idx))
            .map(|i| i.label.clone())
        else {
            return;
        };
        if old == label {
            return;
        }
        self.mutate_hardware_endpoint(hw_id, |hw| {
            if let Some(input) = hw.inputs.get_mut(idx) {
                input.label = label.clone();
            }
        });
        self.for_each_icon_mut(hw_id, |software| {
            for se in &mut software.signal_entries {
                if se.input == old {
                    se.input = label.clone();
                }
            }
        });
        self.clear_selected_control_state(|s| &mut s.last_input_value, hw_id, &old);
        self.mark_dirty();
    }

    pub(super) fn rename_hardware_output(&mut self, hw_id: &str, idx: usize, label: String) {
        let Some(old) = self
            .hardware(hw_id)
            .and_then(|h| h.outputs.get(idx))
            .map(|o| o.label.clone())
        else {
            return;
        };
        if old == label {
            return;
        }
        self.mutate_hardware_endpoint(hw_id, |hw| {
            if let Some(output) = hw.outputs.get_mut(idx) {
                output.label = label.clone();
            }
        });
        self.for_each_icon_mut(hw_id, |software| {
            for fe in &mut software.feedback_entries {
                if fe.output == old {
                    fe.output = label.clone();
                }
            }
        });
        self.clear_selected_control_state(|s| &mut s.last_feedback_result, hw_id, &old);
        self.mark_dirty();
    }
}

impl App {
    fn handle_pw_event(&mut self, event: PwEvent) {
        match event {
            PwEvent::ObjectsUpdated { objects, edges } => {
                self.pw_objects = Arc::new(objects);
                self.pw_edges = Arc::new(edges);
                self.pw_connected = true;
                self.send_feedback();
                self.send_visual_evaluation();
            }
            PwEvent::Connected => {
                self.pw_connected = true;
            }
            PwEvent::Disconnected => {
                self.pw_connected = false;
            }
        }
    }
}
