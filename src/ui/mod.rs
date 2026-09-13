pub mod add_dialog;
pub mod faceplate;

mod action_editor;
mod editor;
mod layout;
mod source_picker;
mod styles;

use iced::keyboard::{Key, Modifiers};
use iced::widget::{self, column, container, row, scrollable, text};
use iced::{Element, Length};

use crate::actions::Action;
use crate::config::{Comparison, IconType, InputKind, OutputKind, Source, SourceBranch};

use faceplate::FaceplateMessage;

#[derive(Debug, Clone, Copy)]
pub enum FeedbackLine {
    Line1,
    Line2,
}

#[derive(Debug, Clone, Copy)]
pub enum InputField {
    Kind(InputKind),
    Channel(u8),
    Number(u8),
}

#[derive(Debug, Clone, Copy)]
pub enum OutputField {
    Kind(OutputKind),
    Channel(u8),
    Number(u8),
}

#[derive(Debug, Clone)]
pub enum Message {
    // Navigation
    SelectDevice(usize),
    RemoveDevice(usize),
    SelectLayer(String),

    // Layer management
    AddLayer(String),
    RemoveLayer(String),
    RenameLayer {
        old: String,
        new: String,
    },
    UpdateNewLayerName(String),

    // Add device dialog
    AddDeviceClicked,
    AddDeviceDialog(add_dialog::Message),
    CancelAddDevice,

    // Hardware editor
    RemoveHardware(String),
    UpdateHardwareType(String, IconType),
    UpdateHardwareSpan(String, u8, u8),
    RenameHardware(String, String),
    AddHardwareInput(String),
    RemoveHardwareInput(String, usize),
    UpdateHardwareInput(String, usize, InputField),
    RenameHardwareInput(String, usize, String),
    AddHardwareOutput(String),
    RemoveHardwareOutput(String, usize),
    UpdateHardwareOutput(String, usize, OutputField),
    RenameHardwareOutput(String, usize, String),
    StartLearn(String, usize),

    // Icon label
    UpdateIconLabel(usize, String),

    // Icon software editor
    OpenIconEditor(usize),
    CloseIconEditor,
    /// Absorbs clicks on a modal surface so they don't reach the close-on-outside backdrop.
    Nop,
    AddSignalEntry(usize),
    RemoveSignalEntry(usize, usize),
    UpdateSignalInput(usize, usize, String),
    AddSignalAction(usize, usize),
    RemoveSignalAction(usize, usize, usize),
    UpdateSignalAction(usize, usize, usize, Action),
    AddFeedbackEntry(usize),
    RemoveFeedbackEntry(usize, usize),
    UpdateFeedbackOutput(usize, usize, String),
    UpdateFeedbackSource(usize, usize, FeedbackLine, Vec<SourceBranch>, Source),
    UpdateVisualSource(usize, Vec<SourceBranch>, Option<Source>),

    UpdateIfComparison(usize, usize, FeedbackLine, Vec<SourceBranch>, Comparison),
    UpdateVisualIfComparison(usize, Vec<SourceBranch>, Comparison),

    // Faceplate
    Faceplate(FaceplateMessage),

    // Grid config
    UpdateGridCols(u8),
    UpdateGridRows(u8),

    // Keyboard
    KeyPress(Key, Modifiers),
}

/// Place a modal `panel` over `base`, with a full-size backdrop that emits
/// `on_dismiss` when clicked. Clicks on the panel itself are absorbed.
fn overlay<'a>(
    base: Element<'a, Message>,
    panel: Element<'a, Message>,
    on_dismiss: Message,
) -> Element<'a, Message> {
    let backdrop = widget::mouse_area(
        container(text("").size(1.0))
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .on_press(on_dismiss);

    widget::stack([
        base,
        container(backdrop)
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        container(widget::mouse_area(panel).on_press(Message::Nop))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into(),
    ])
    .into()
}

pub fn view(app: &crate::app::App) -> Element<'_, crate::app::AppMessage> {
    let left = container(layout::device_list_panel(app))
        .width(Length::FillPortion(1))
        .height(Length::Fill);
    let right = container(layout::device_view_panel(app))
        .width(Length::FillPortion(3))
        .height(Length::Fill);

    let body: Element<Message> = row![left, right].into();

    let main: Element<Message> = if app.show_add_dialog {
        let dialog = app.add_dialog_state.view().map(Message::AddDeviceDialog);
        overlay(body, dialog, Message::CancelAddDevice)
    } else if let Some(icon_idx) = app.editing_icon_idx {
        match app.selected_device() {
            Some(device) => {
                let editor = editor::hw_sw_editor(app, device, icon_idx);
                let modal = container(scrollable(editor))
                    .style(styles::modal_bg)
                    .width(Length::Fixed(700.0))
                    .height(Length::Fixed(500.0))
                    .padding(8.0);
                overlay(body, modal.into(), Message::CloseIconEditor)
            }
            None => body,
        }
    } else {
        body
    };

    let col: Element<'_, Message> =
        column![main, layout::status_bar(app), layout::midi_monitor(app)]
            .height(Length::Fill)
            .into();
    col.map(crate::app::AppMessage::Ui)
}
