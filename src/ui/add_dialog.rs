use iced::widget::{button, column, container, row, scrollable, text, text_input, Column};
use iced::{Element, Length};

use crate::midi::PortInfo;

use super::styles::dialog_style;

#[derive(Debug, Clone, Default)]
pub struct AddDeviceDialog {
    pub search: String,
    pub selected_port: Option<usize>,
    pub available_ports: Vec<PortInfo>,
}

#[derive(Debug, Clone)]
pub enum Message {
    SearchChanged(String),
    SelectPort(usize),
    Cancel,
    Confirm,
}

impl AddDeviceDialog {
    pub fn new(ports: Vec<PortInfo>) -> Self {
        AddDeviceDialog {
            search: String::new(),
            selected_port: None,
            available_ports: ports,
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let title = text("Add MIDI Device").size(18);

        let search_input = text_input("Filter ports...", &self.search)
            .on_input(Message::SearchChanged)
            .padding(4)
            .width(Length::Fill);

        let filtered_ports: Vec<(usize, &PortInfo)> = self
            .available_ports
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                self.search.is_empty()
                    || p.name.to_lowercase().contains(&self.search.to_lowercase())
            })
            .collect();

        let ports_list: Column<Message> =
            filtered_ports
                .iter()
                .fold(Column::new().spacing(2), |col, (idx, port)| {
                    let is_selected = self.selected_port == Some(*idx);
                    let mut btn = button(text(&port.name).size(13));
                    if is_selected {
                        btn = btn.style(button::primary);
                    }
                    col.push(btn.on_press(Message::SelectPort(*idx)).width(Length::Fill))
                });

        let ports_scrollable = scrollable(ports_list).height(Length::Fixed(200.0));

        let cancel_btn = button("Cancel").on_press(Message::Cancel);
        let confirm_btn = match self.selected_port {
            Some(_) => button("Add Device")
                .on_press(Message::Confirm)
                .style(button::primary),
            None => button("Add Device"),
        };

        let buttons = row![cancel_btn, confirm_btn].spacing(8);

        container(
            column![title, search_input, ports_scrollable, buttons]
                .spacing(12)
                .width(Length::Fixed(350.0))
                .padding(16),
        )
        .style(dialog_style)
        .into()
    }
}
