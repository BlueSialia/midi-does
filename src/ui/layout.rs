use std::collections::HashMap;

use iced::alignment::Vertical;
use iced::widget::{
    button, column, container, pick_list, row, scrollable, text, text_input, Column,
};
use iced::{Element, Length, Padding};

use crate::app::App;
use crate::config::DeviceConfig;
use crate::ui::faceplate::{self, FaceplateState};

use super::styles::{mmon_style, panel_style, sbar_style};
use super::Message;

pub(super) fn status_bar(app: &App) -> Element<'_, Message> {
    let status_text = text(&app.status_message).size(11.0);
    let layer_info = if let Some(layer) = &app.selected_layer {
        format!("  Layer: {layer}")
    } else {
        String::new()
    };
    let info = text(layer_info).size(11.0);
    container(row![status_text, info].spacing(8.0))
        .style(sbar_style)
        .padding(Padding::new(2.0))
        .width(Length::Fill)
        .into()
}

pub(super) fn midi_monitor(app: &App) -> Element<'_, Message> {
    let lines: Column<Message> = app
        .midi_monitor
        .iter()
        .rev()
        .take(5)
        .fold(Column::new(), |col, line| col.push(text(line).size(9.0)));
    container(scrollable(lines).height(Length::Fixed(60.0)))
        .style(mmon_style)
        .padding(4.0)
        .width(Length::Fill)
        .into()
}

pub(super) fn device_list_panel(app: &App) -> Element<'_, Message> {
    let title = text("Devices").size(16.0);

    let mut dev_list = Column::new().spacing(4.0);
    for i in 0..app.config.device_count() {
        let name = app.config.device_name(i).unwrap_or("?");
        let is_selected = app.selected_device_name.as_deref() == app.config.device_name(i);
        let mut btn = button(text(name).size(13.0));
        if is_selected {
            btn = btn.style(button::primary);
        }
        let remove_btn = button(text("✕")).on_press(Message::RemoveDevice(i));
        dev_list = dev_list.push(
            row![
                btn.on_press(Message::SelectDevice(i)).width(Length::Fill),
                remove_btn,
            ]
            .spacing(4.0),
        );
    }

    let add_btn = button("+ Add Device")
        .on_press(Message::AddDeviceClicked)
        .width(Length::Fill);

    container(
        column![title, scrollable(dev_list).height(Length::Fill), add_btn]
            .spacing(8.0)
            .padding(8.0)
            .height(Length::Fill),
    )
    .style(panel_style)
    .into()
}

pub(super) fn device_view_panel(app: &App) -> Element<'_, Message> {
    let device = match app.selected_device() {
        Some(d) => d,
        None => {
            return container(text("Select a device from the list").size(14.0))
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into();
        }
    };

    let layer_picker = layer_picker_row(app, device);
    let layer_mgmt = layer_management_row(app);
    let faceplate = faceplate_view(app, device);
    let hint =
        text("Click an empty cell on the grid to add hardware, or click an icon to edit it.")
            .size(12.0);

    let content = column![layer_picker, layer_mgmt, faceplate, hint]
        .spacing(8.0)
        .padding(8.0);

    scrollable(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn layer_picker_row<'a>(app: &'a App, device: &'a DeviceConfig) -> Element<'a, Message> {
    let layer_names: Vec<String> = device.layers.iter().map(|l| l.name.clone()).collect();

    let picker = {
        let selected = app.selected_layer.clone();
        pick_list(layer_names.clone(), selected, Message::SelectLayer).width(Length::Fixed(100.0))
    };

    let bank_info = if let Some(layer_name) = &app.selected_layer {
        if let Some(layer) = device.layers.iter().find(|l| &l.name == layer_name) {
            text(format!("Bank: {}", layer.bank_offset)).size(12.0)
        } else {
            text("").size(12.0)
        }
    } else {
        text("").size(12.0)
    };

    let col_input = text_input("cols", &device.grid_columns.to_string())
        .on_input(|s| Message::UpdateGridCols(s.parse().unwrap_or(8)))
        .width(Length::Fixed(40.0))
        .size(12.0);
    let row_input = text_input("rows", &device.grid_rows.to_string())
        .on_input(|s| Message::UpdateGridRows(s.parse().unwrap_or(4)))
        .width(Length::Fixed(40.0))
        .size(12.0);

    row![
        text("Layer:").size(13.0),
        picker,
        bank_info,
        text("Grid:").size(12.0),
        col_input,
        text("×").size(12.0),
        row_input,
    ]
    .spacing(6.0)
    .align_y(Vertical::Center)
    .into()
}

fn layer_management_row<'a>(app: &'a App) -> Element<'a, Message> {
    let layer_name = app.new_layer_name.clone();
    let add_input = text_input("new layer name", &app.new_layer_name)
        .on_input(Message::UpdateNewLayerName)
        .width(Length::Fixed(100.0))
        .size(12.0);
    let add_btn = button(text("Add Layer")).on_press(Message::AddLayer(layer_name));

    let rename_input = if let Some(current) = &app.selected_layer {
        let old = current.clone();
        text_input("rename", current)
            .on_input(move |s| Message::RenameLayer {
                old: old.clone(),
                new: s,
            })
            .width(Length::Fixed(100.0))
            .size(12.0)
    } else {
        text_input("rename", "")
            .width(Length::Fixed(100.0))
            .size(12.0)
    };

    let remove_btn = if let Some(current) = &app.selected_layer {
        let layer = current.clone();
        button(text("Remove Layer")).on_press(Message::RemoveLayer(layer))
    } else {
        button(text("Remove Layer"))
    };

    row![
        add_input,
        add_btn,
        text("Rename:").size(11.0),
        rename_input,
        remove_btn,
    ]
    .spacing(4.0)
    .align_y(Vertical::Center)
    .into()
}

fn faceplate_view<'a>(app: &'a App, device: &'a DeviceConfig) -> Element<'a, Message> {
    let icon_labels: HashMap<String, String> = match &app.selected_layer {
        Some(layer_name) => device
            .layers
            .iter()
            .find(|l| l.name == *layer_name)
            .map(|layer| {
                layer
                    .icons
                    .iter()
                    .map(|ic| (ic.hardware.clone(), ic.software.label.clone()))
                    .collect()
            })
            .unwrap_or_default(),
        None => HashMap::new(),
    };
    let visual_values: HashMap<String, f64> = app
        .visual_values
        .iter()
        .filter(|((dn, _), _)| dn == &device.port_name)
        .map(|((_, hw), v)| (hw.clone(), *v))
        .collect();
    let state = FaceplateState {
        grid_columns: device.grid_columns,
        grid_rows: device.grid_rows,
        hardware: device.hardware.clone(),
        visual_values,
        icon_labels,
    };

    faceplate::draw(state).map(Message::Faceplate)
}
