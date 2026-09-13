use iced::keyboard::{self, Key};
use iced::Task;

use crate::actions::Action;
use crate::config::{
    default_grid_cols, default_grid_rows, default_layers, DeviceConfig, FeedbackEntry, HardwareDef,
    LayerDef, SignalEntry, SoftwareDef, Source,
};
use crate::ui;
use crate::ui::faceplate;

use super::{next_hardware_id, take_at, valid_layer_rename, App, AppMessage};

impl App {
    pub(super) fn handle_ui_message(&mut self, msg: ui::Message) -> Task<AppMessage> {
        match msg {
            ui::Message::SelectDevice(idx) => {
                self.selected_device_name = self.config.device_name(idx).map(|s| s.to_string());
                self.selected_layer = self
                    .selected_device_name
                    .as_ref()
                    .and_then(|name| self.active_layer(self.config.devices.get(name)?, name))
                    .map(|l| l.name.clone());
                self.clear_editing();
                self.send_visual_evaluation();
                Task::none()
            }
            ui::Message::SelectLayer(layer) => {
                if let Some(name) = &self.selected_device_name {
                    self.active_layers.insert(name.clone(), layer.clone());
                }
                self.selected_layer = Some(layer);
                self.clear_editing();
                self.send_visual_evaluation();
                Task::none()
            }

            ui::Message::AddLayer(name) => {
                let name = name.trim().to_string();
                if !name.is_empty() {
                    if let Some(device) = self.selected_device_mut() {
                        if !device.layers.iter().any(|l| l.name == name) {
                            device.layers.push(LayerDef {
                                name: name.clone(),
                                bank_offset: 0,
                                icons: Vec::new(),
                            });
                        }
                        self.mark_dirty();
                    }
                    if let Some(device_name) = &self.selected_device_name {
                        self.active_layers.insert(device_name.clone(), name.clone());
                    }
                    self.selected_layer = Some(name);
                    self.clear_editing();
                    self.send_visual_evaluation();
                }
                Task::none()
            }
            ui::Message::RemoveLayer(layer) => {
                let is_last = self.selected_device().is_some_and(|d| d.layers.len() <= 1);
                if is_last {
                    self.status_message = "Cannot remove the last layer".into();
                } else if let Some(device) = self.selected_device_mut() {
                    device.layers.retain(|l| l.name != layer);
                    self.mark_dirty();
                    if self.selected_layer.as_deref() == Some(&layer) {
                        self.selected_layer = None;
                    }
                    self.repair_active_layer();
                    self.clear_editing();
                    self.send_visual_evaluation();
                }
                Task::none()
            }
            ui::Message::RenameLayer { old, new } => {
                let new = new.trim().to_string();
                if new == old {
                    return Task::none();
                }
                let mut renamed = false;
                if let Some(device) = self.selected_device_mut() {
                    if valid_layer_rename(&device.layers, &old, &new) {
                        for layer in &mut device.layers {
                            if layer.name == old {
                                layer.name = new.clone();
                            }
                        }
                        self.mark_dirty();
                        renamed = true;
                    }
                }
                if renamed {
                    if self.selected_layer.as_deref() == Some(old.as_str()) {
                        self.selected_layer = Some(new.clone());
                    }
                    if let Some(device_name) = &self.selected_device_name {
                        if self.active_layers.get(device_name) == Some(&old) {
                            self.active_layers.insert(device_name.clone(), new.clone());
                        }
                    }
                    self.send_visual_evaluation();
                } else {
                    self.status_message = if new.is_empty() {
                        "Layer name cannot be empty".into()
                    } else {
                        format!("Layer '{new}' already exists")
                    };
                }
                Task::none()
            }
            ui::Message::UpdateNewLayerName(name) => {
                self.new_layer_name = name;
                Task::none()
            }

            ui::Message::RemoveDevice(idx) => {
                let name = self.config.device_name(idx).map(|s| s.to_string());
                if let Some(n) = name {
                    self.config.remove_device(&n);
                    self.connected_devices.remove(&n);
                    self.output_handles.remove(&n);
                    self.active_layers.remove(&n);
                    self.visual_values.retain(|(dn, _), _| dn != &n);
                    self.last_feedback_result.retain(|k, _| k.device != n);
                    self.mark_dirty();
                    if self.selected_device_name.as_deref() == Some(&n) {
                        self.selected_device_name = None;
                        self.selected_layer = None;
                        self.clear_editing();
                    }
                }
                Task::none()
            }
            ui::Message::AddDeviceClicked => {
                let all = self.midi_manager.available_ports();
                let configured: Vec<_> =
                    self.config.devices.values().map(|d| &d.port_name).collect();
                let filtered: Vec<_> = all
                    .into_iter()
                    .filter(|p| !configured.contains(&&p.name))
                    .collect();
                self.add_dialog_state = ui::add_dialog::AddDeviceDialog::new(filtered);
                self.show_add_dialog = true;
                Task::none()
            }
            ui::Message::AddDeviceDialog(dd) => {
                match dd {
                    ui::add_dialog::Message::SearchChanged(s) => {
                        self.add_dialog_state.search = s;
                        self.add_dialog_state.selected_port = None;
                    }
                    ui::add_dialog::Message::SelectPort(i) => {
                        self.add_dialog_state.selected_port = Some(i)
                    }
                    ui::add_dialog::Message::Cancel => self.show_add_dialog = false,
                    ui::add_dialog::Message::Confirm => return self.confirm_add_device(),
                }
                Task::none()
            }
            ui::Message::CancelAddDevice => {
                self.show_add_dialog = false;
                Task::none()
            }

            ui::Message::RemoveHardware(id) => {
                let was_editing = self.editing_icon_idx.is_some();
                if let Some(device) = self.selected_device_mut() {
                    device.hardware.retain(|h| h.id != id);
                    for layer in &mut device.layers {
                        layer.icons.retain(|icon| icon.hardware != id);
                    }
                    self.mark_dirty();
                }
                if let Some(device_name) = self.selected_device_name.clone() {
                    self.visual_values
                        .retain(|(d, h), _| d != &device_name || h != &id);
                    self.last_feedback_result
                        .retain(|k, _| k.device != device_name || k.hardware != id);
                    self.last_input_value
                        .retain(|k, _| k.device != device_name || k.hardware != id);
                }
                if was_editing {
                    self.editing_icon_idx = None;
                }
                if self
                    .learn_target
                    .as_ref()
                    .is_some_and(|(_, hw, _)| hw == &id)
                {
                    self.learn_target = None;
                }
                Task::none()
            }
            ui::Message::UpdateHardwareType(id, t) => {
                if let Some(hw) = self.hardware_mut(&id) {
                    hw.hw_type = t;
                    self.mark_dirty();
                }
                Task::none()
            }
            ui::Message::UpdateHardwareSpan(id, cs, rs) => {
                if let Some(hw) = self.hardware_mut(&id) {
                    hw.col_span = cs.max(1);
                    hw.row_span = rs.max(1);
                    self.mark_dirty();
                }
                Task::none()
            }
            ui::Message::RenameHardware(id, name) => {
                let old_id = id.clone();
                let device_name = self.selected_device_name.clone();
                if name == old_id {
                    return Task::none();
                }
                let duplicate = self
                    .selected_device()
                    .is_some_and(|d| d.hardware.iter().any(|h| h.id == name && h.id != old_id));
                if duplicate {
                    self.status_message = format!("Hardware id '{name}' already exists");
                    return Task::none();
                }
                if let Some(device) = self.selected_device_mut() {
                    if let Some(hw) = device.hardware.iter_mut().find(|h| h.id == old_id) {
                        for layer in &mut device.layers {
                            for icon in &mut layer.icons {
                                if icon.hardware == old_id {
                                    icon.hardware = name.clone();
                                }
                            }
                        }
                        hw.id = name;
                        self.mark_dirty();
                    }
                }
                if let Some(device_name) = device_name {
                    self.visual_values
                        .retain(|(d, h), _| d != &device_name || h != &old_id);
                    self.last_feedback_result
                        .retain(|k, _| k.device != device_name || k.hardware != old_id);
                    self.last_input_value
                        .retain(|k, _| k.device != device_name || k.hardware != old_id);
                }
                Task::none()
            }
            ui::Message::RenameHardwareInput(hw_id, idx, label) => {
                self.rename_hardware_input(&hw_id, idx, label);
                Task::none()
            }
            ui::Message::RenameHardwareOutput(hw_id, idx, label) => {
                self.rename_hardware_output(&hw_id, idx, label);
                Task::none()
            }
            ui::Message::AddHardwareInput(hw_id) => {
                self.add_hardware_input(&hw_id);
                Task::none()
            }
            ui::Message::RemoveHardwareInput(hw_id, idx) => {
                self.remove_hardware_input(&hw_id, idx);
                Task::none()
            }
            ui::Message::UpdateHardwareInput(hw_id, idx, field) => {
                if let Some(input) = self.hardware_input_mut(&hw_id, idx) {
                    match field {
                        ui::InputField::Kind(kind) => input.kind = kind,
                        ui::InputField::Channel(ch) => input.channel = ch,
                        ui::InputField::Number(num) => input.number = num,
                    }
                    self.mark_dirty();
                }
                Task::none()
            }
            ui::Message::AddHardwareOutput(hw_id) => {
                self.add_hardware_output(&hw_id);
                Task::none()
            }
            ui::Message::RemoveHardwareOutput(hw_id, idx) => {
                self.remove_hardware_output(&hw_id, idx);
                Task::none()
            }
            ui::Message::UpdateHardwareOutput(hw_id, idx, field) => {
                if let Some(output) = self.hardware_output_mut(&hw_id, idx) {
                    match field {
                        ui::OutputField::Kind(kind) => output.kind = kind,
                        ui::OutputField::Channel(ch) => output.channel = ch,
                        ui::OutputField::Number(num) => output.number = num,
                    }
                    self.mark_dirty();
                }
                Task::none()
            }
            ui::Message::StartLearn(hw_id, idx) => {
                self.learn_target = self
                    .selected_device_name
                    .clone()
                    .map(|device| (device, hw_id, idx));
                self.status_message = "MIDI Learn: waiting for event...".into();
                Task::none()
            }

            // Remaining messages edit the selected device's software/faceplate.
            other => self.handle_editor_message(other),
        }
    }

    fn handle_editor_message(&mut self, msg: ui::Message) -> Task<AppMessage> {
        match msg {
            ui::Message::Faceplate(msg) => self.handle_faceplate_message(msg),
            ui::Message::KeyPress(key, modifiers) => {
                self.handle_keypress(key, modifiers);
                Task::none()
            }
            ui::Message::UpdateGridCols(cols) => {
                if let Some(device) = self.selected_device_mut() {
                    device.grid_columns = cols.max(1);
                    self.mark_dirty();
                }
                Task::none()
            }
            ui::Message::UpdateGridRows(rows) => {
                if let Some(device) = self.selected_device_mut() {
                    device.grid_rows = rows.max(1);
                    self.mark_dirty();
                }
                Task::none()
            }
            other => {
                self.edit_software_message(other);
                Task::none()
            }
        }
    }

    /// These only mutate in-memory config, so no `Task` is produced.
    fn edit_software_message(&mut self, msg: ui::Message) {
        match msg {
            ui::Message::UpdateIconLabel(icon_idx, label) => {
                self.edit_software(icon_idx, false, |software| {
                    if software.label == label {
                        false
                    } else {
                        software.label = label;
                        true
                    }
                });
            }
            ui::Message::OpenIconEditor(icon_idx) => self.editing_icon_idx = Some(icon_idx),
            ui::Message::CloseIconEditor => self.clear_editing(),
            ui::Message::Nop => {}

            ui::Message::AddSignalEntry(icon_idx) => {
                self.edit_software(icon_idx, false, |software| {
                    software.signal_entries.push(SignalEntry {
                        input: String::new(),
                        actions: vec![Action::None],
                    });
                    true
                });
            }
            ui::Message::RemoveSignalEntry(icon_idx, se_idx) => {
                self.edit_software(icon_idx, false, |software| {
                    take_at(&mut software.signal_entries, se_idx).is_some()
                });
            }
            ui::Message::UpdateSignalInput(icon_idx, se_idx, input) => {
                self.edit_software(icon_idx, false, |software| {
                    match software.signal_entries.get_mut(se_idx) {
                        Some(se) => {
                            se.input = input;
                            true
                        }
                        None => false,
                    }
                });
            }
            ui::Message::AddSignalAction(icon_idx, se_idx) => {
                self.edit_software(icon_idx, false, |software| {
                    match software.signal_entries.get_mut(se_idx) {
                        Some(se) => {
                            se.actions.push(Action::None);
                            true
                        }
                        None => false,
                    }
                });
            }
            ui::Message::RemoveSignalAction(icon_idx, se_idx, a_idx) => {
                self.edit_software(icon_idx, false, |software| {
                    match software.signal_entries.get_mut(se_idx) {
                        Some(se) => take_at(&mut se.actions, a_idx).is_some(),
                        None => false,
                    }
                });
            }
            ui::Message::UpdateSignalAction(icon_idx, se_idx, a_idx, action) => {
                self.edit_software(icon_idx, false, |software| {
                    match software.signal_entries.get_mut(se_idx) {
                        Some(se) if a_idx < se.actions.len() => {
                            se.actions[a_idx] = action;
                            true
                        }
                        _ => false,
                    }
                });
            }

            ui::Message::AddFeedbackEntry(icon_idx) => {
                self.edit_software(icon_idx, false, |software| {
                    software.feedback_entries.push(FeedbackEntry {
                        output: String::new(),
                        source: Source::Direct {
                            value: "false".into(),
                        },
                        line2_source: None,
                    });
                    true
                });
            }
            ui::Message::RemoveFeedbackEntry(icon_idx, fe_idx) => {
                self.edit_software(icon_idx, false, |software| {
                    take_at(&mut software.feedback_entries, fe_idx).is_some()
                });
            }
            ui::Message::UpdateFeedbackOutput(icon_idx, fe_idx, output) => {
                self.edit_software(icon_idx, false, |software| {
                    match software.feedback_entries.get_mut(fe_idx) {
                        Some(fe) => {
                            fe.output = output;
                            true
                        }
                        None => false,
                    }
                });
            }
            ui::Message::UpdateFeedbackSource(icon_idx, fe_idx, line, path, source) => {
                self.edit_software(icon_idx, false, |software| {
                    let Some(fe) = software.feedback_entries.get_mut(fe_idx) else {
                        return false;
                    };
                    match line {
                        ui::FeedbackLine::Line1 => fe.source.replace_at(&path, source),
                        ui::FeedbackLine::Line2 => match &mut fe.line2_source {
                            Some(s2) => s2.replace_at(&path, source),
                            None => fe.line2_source = Some(source),
                        },
                    }
                    true
                });
            }
            ui::Message::UpdateVisualSource(icon_idx, path, source) => {
                self.edit_software(icon_idx, true, |software| match source {
                    None => software.visual_source.take().is_some(),
                    Some(src) => match &mut software.visual_source {
                        Some(vs) => {
                            vs.replace_at(&path, src);
                            true
                        }
                        None => {
                            software.visual_source = Some(src);
                            true
                        }
                    },
                });
            }
            ui::Message::UpdateIfComparison(icon_idx, fe_idx, line, path, comparison) => {
                self.edit_software(icon_idx, false, |software| {
                    let Some(fe) = software.feedback_entries.get_mut(fe_idx) else {
                        return false;
                    };
                    let target = match line {
                        ui::FeedbackLine::Line1 => &mut fe.source,
                        ui::FeedbackLine::Line2 => {
                            fe.line2_source.get_or_insert_with(|| Source::Direct {
                                value: "false".into(),
                            })
                        }
                    };
                    target.replace_condition_at(&path, comparison);
                    true
                });
            }
            ui::Message::UpdateVisualIfComparison(icon_idx, path, comparison) => {
                self.edit_software(icon_idx, true, |software| {
                    match &mut software.visual_source {
                        Some(vs) => {
                            vs.replace_condition_at(&path, comparison);
                            true
                        }
                        None => false,
                    }
                });
            }

            _ => {}
        }
    }

    fn handle_faceplate_message(&mut self, msg: faceplate::FaceplateMessage) -> Task<AppMessage> {
        match msg {
            faceplate::FaceplateMessage::ClickIcon(icon_idx) => {
                return Task::done(AppMessage::Ui(ui::Message::OpenIconEditor(icon_idx)));
            }
            faceplate::FaceplateMessage::ClickEmptyCell(col, row) => {
                let id = {
                    if let Some(device) = self.selected_device_mut() {
                        let id = next_hardware_id(device);
                        device.hardware.push(HardwareDef {
                            id: id.clone(),
                            hw_type: crate::config::IconType::Button,
                            col,
                            row,
                            col_span: 1,
                            row_span: 1,
                            inputs: vec![],
                            outputs: vec![],
                        });
                        id
                    } else {
                        return Task::none();
                    }
                };
                self.mark_dirty();
                if let Some(device) = self.selected_device() {
                    let new_idx = device.hardware.iter().position(|h| h.id == id);
                    if let Some(idx) = new_idx {
                        return Task::done(AppMessage::Ui(ui::Message::OpenIconEditor(idx)));
                    }
                }
            }
        }
        Task::none()
    }

    fn selected_layer_software_mut(&mut self, icon_idx: usize) -> Option<&mut SoftwareDef> {
        let layer_name = self.selected_layer.clone()?;
        let device = self.selected_device_mut()?;
        let hw_id = device.hardware.get(icon_idx)?.id.clone();
        let layer = device.layers.iter_mut().find(|l| l.name == layer_name)?;
        Some(layer.software_mut(&hw_id))
    }

    fn with_software<R>(
        &mut self,
        icon_idx: usize,
        f: impl FnOnce(&mut SoftwareDef) -> R,
    ) -> Option<R> {
        self.selected_layer_software_mut(icon_idx).map(f)
    }

    /// Run `f` over the selected layer's software for `icon_idx`. Marks the
    /// config dirty when `f` reports a change, optionally re-evaluating
    /// faceplate visuals.
    fn edit_software(
        &mut self,
        icon_idx: usize,
        reevaluate_visuals: bool,
        f: impl FnOnce(&mut SoftwareDef) -> bool,
    ) {
        if self.with_software(icon_idx, f).unwrap_or(false) {
            self.mark_dirty();
            if reevaluate_visuals {
                self.send_visual_evaluation();
            }
        }
    }

    pub(super) fn clear_editing(&mut self) {
        if self.editing_icon_idx.is_some() {
            self.save_config();
        }
        self.editing_icon_idx = None;
        self.learn_target = None;
    }

    fn repair_active_layer(&mut self) {
        let device_name = match &self.selected_device_name {
            Some(n) => n.clone(),
            None => return,
        };
        let device = match self.config.devices.get(&device_name) {
            Some(d) => d,
            None => return,
        };
        if device.layers.is_empty() {
            self.active_layers.remove(&device_name);
            self.selected_layer = None;
            return;
        }
        let valid = self
            .active_layers
            .get(&device_name)
            .filter(|l| device.layers.iter().any(|x| &x.name == *l))
            .cloned();
        let fallback = device.layers.first().map(|l| l.name.clone());
        if let Some(chosen) = valid.or(fallback) {
            self.active_layers
                .insert(device_name.clone(), chosen.clone());
            if self.selected_layer.is_none() {
                self.selected_layer = Some(chosen);
            }
        }
    }

    fn handle_keypress(&mut self, key: Key, modifiers: iced::keyboard::Modifiers) {
        match key {
            Key::Named(keyboard::key::Named::Escape) => {
                if self.show_add_dialog {
                    self.show_add_dialog = false;
                } else if self.editing_icon_idx.is_some() {
                    self.clear_editing();
                }
            }
            Key::Character(c)
                if c.as_str() == "s"
                    && modifiers.control()
                    && !self.show_add_dialog
                    && self.editing_icon_idx.is_some() =>
            {
                self.clear_editing();
            }
            _ => {}
        }
    }

    fn confirm_add_device(&mut self) -> Task<AppMessage> {
        self.show_add_dialog = false;
        if let Some(idx) = self.add_dialog_state.selected_port {
            if let Some(port) = self.add_dialog_state.available_ports.get(idx) {
                let name = port.name.clone();
                if !self.config.devices.contains_key(&name) {
                    self.config.insert_device(
                        name.clone(),
                        DeviceConfig {
                            port_name: name.clone(),
                            grid_columns: default_grid_cols(),
                            grid_rows: default_grid_rows(),
                            hardware: Vec::new(),
                            layers: default_layers(),
                        },
                    );
                    self.mark_dirty();
                    self.active_layers.insert(name.clone(), "A".to_string());
                    match self.midi_manager.connect(&name) {
                        Ok(()) => {}
                        Err(e) => {
                            self.status_message = format!("Failed to connect: {e}");
                        }
                    }
                    if let Ok(h) = self.midi_manager.connect_output(&name) {
                        self.output_handles.insert(name.clone(), h);
                    }
                }
            }
        }
        Task::none()
    }
}
