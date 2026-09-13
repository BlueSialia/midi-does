use iced::alignment::Vertical;
use iced::widget::{button, container, pick_list, row, text, text_input, Column};
use iced::{Element, Length};

use crate::app::App;
use crate::config::{
    Comparison, DeviceConfig, FeedbackEntry, IconType, InputKind, OutputKind, SignalEntry, Source,
    SourceBranch,
};

use super::action_editor::signal_action_row;
use super::source_picker::{new_source, source_picker_widget, source_type_names};
use super::styles::{editor_style, separator};
use super::{FeedbackLine, InputField, Message, OutputField};

enum EndpointTarget {
    Input { hw_id: String, index: usize },
    Output { hw_id: String, index: usize },
}

fn endpoint_row<'a>(
    label: &'a str,
    channel: u8,
    number: u8,
    kind_picker: Element<'a, Message>,
    target: EndpointTarget,
) -> Element<'a, Message> {
    let (hw_id, index, is_input) = match target {
        EndpointTarget::Input { hw_id, index } => (hw_id, index, true),
        EndpointTarget::Output { hw_id, index } => (hw_id, index, false),
    };

    let rename = {
        let hw_id = hw_id.clone();
        move |label: String| {
            if is_input {
                Message::RenameHardwareInput(hw_id.clone(), index, label)
            } else {
                Message::RenameHardwareOutput(hw_id.clone(), index, label)
            }
        }
    };
    let set_channel = {
        let hw_id = hw_id.clone();
        move |ch: u8| {
            if is_input {
                Message::UpdateHardwareInput(hw_id.clone(), index, InputField::Channel(ch))
            } else {
                Message::UpdateHardwareOutput(hw_id.clone(), index, OutputField::Channel(ch))
            }
        }
    };
    let set_number = {
        let hw_id = hw_id.clone();
        move |num: u8| {
            if is_input {
                Message::UpdateHardwareInput(hw_id.clone(), index, InputField::Number(num))
            } else {
                Message::UpdateHardwareOutput(hw_id.clone(), index, OutputField::Number(num))
            }
        }
    };

    let label_input = text_input("name", label)
        .on_input(rename)
        .width(Length::Fixed(60.0))
        .size(11.0);
    let ch_input = text_input("ch", &channel.to_string())
        .on_input(move |s| set_channel(s.parse().unwrap_or(0)))
        .width(Length::Fixed(35.0))
        .size(11.0);
    let num_input = text_input("num", &number.to_string())
        .on_input(move |s| set_number(s.parse().unwrap_or(0)))
        .width(Length::Fixed(35.0))
        .size(11.0);

    let mut children: Vec<Element<'a, Message>> = vec![
        label_input.into(),
        ch_input.into(),
        num_input.into(),
        kind_picker,
    ];
    if is_input {
        children.push(
            button(text("Learn"))
                .on_press(Message::StartLearn(hw_id.clone(), index))
                .into(),
        );
    }
    children.push(
        button(text("X"))
            .on_press(if is_input {
                Message::RemoveHardwareInput(hw_id, index)
            } else {
                Message::RemoveHardwareOutput(hw_id, index)
            })
            .into(),
    );

    iced::widget::Row::with_children(children)
        .spacing(2.0)
        .align_y(Vertical::Center)
        .into()
}

pub(super) fn hw_sw_editor<'a>(
    app: &'a App,
    device: &'a DeviceConfig,
    icon_idx: usize,
) -> Element<'a, Message> {
    let layer_name = match &app.selected_layer {
        Some(l) => l.clone(),
        None => return text("Select a layer first").into(),
    };

    let layer = match device.layers.iter().find(|l| l.name == layer_name) {
        Some(l) => l,
        None => return text("Layer not found").into(),
    };

    // Icons are device-level: `icon_idx` indexes the shared hardware list. The
    // layer contributes per-icon software, created lazily on first edit.
    let hw = match device.hardware.get(icon_idx) {
        Some(h) => h,
        None => return text("Icon not found").into(),
    };
    let hw_input_labels: Vec<String> = hw.inputs.iter().map(|i| i.label.clone()).collect();
    let software = layer.software(&hw.id);
    let signal_entries: &[SignalEntry] =
        software.map(|s| s.signal_entries.as_slice()).unwrap_or(&[]);
    let feedback_entries: &[FeedbackEntry] = software
        .map(|s| s.feedback_entries.as_slice())
        .unwrap_or(&[]);
    let visual_source: Option<&Source> = software.and_then(|s| s.visual_source.as_ref());

    let mut col = Column::new().spacing(6.0);

    col = col.push(
        row![
            button("Close").on_press(Message::CloseIconEditor),
            text(format!(
                "Editing: {} at ({},{}) [{}]",
                hw.id, hw.col, hw.row, hw.hw_type,
            ))
            .size(14.0),
        ]
        .spacing(8.0)
        .align_y(Vertical::Center),
    );

    col = col.push(separator());
    col = col.push(text("Hardware").size(13.0));

    let type_picker = pick_list(
        vec![
            IconType::Button,
            IconType::Knob,
            IconType::Fader,
            IconType::Encoder,
        ],
        Some(hw.hw_type),
        {
            let id = hw.id.clone();
            move |t| Message::UpdateHardwareType(id.clone(), t)
        },
    );
    col = col.push(
        row![text("Type:").size(12.0), type_picker]
            .spacing(4.0)
            .align_y(Vertical::Center),
    );

    let col_span_input = text_input("col span", &hw.col_span.to_string())
        .on_input({
            let id = hw.id.clone();
            move |s| Message::UpdateHardwareSpan(id.clone(), s.parse().unwrap_or(1), hw.row_span)
        })
        .width(Length::Fixed(40.0))
        .size(11.0);
    let row_span_input = text_input("row span", &hw.row_span.to_string())
        .on_input({
            let id = hw.id.clone();
            move |s| Message::UpdateHardwareSpan(id.clone(), hw.col_span, s.parse().unwrap_or(1))
        })
        .width(Length::Fixed(40.0))
        .size(11.0);
    col = col.push(
        row![
            text("Span:").size(12.0),
            col_span_input,
            text("×").size(11.0),
            row_span_input
        ]
        .spacing(4.0)
        .align_y(Vertical::Center),
    );

    col = col.push(
        row![text("Inputs:").size(12.0), {
            let id = hw.id.clone();
            button("+").on_press(Message::AddHardwareInput(id))
        },]
        .spacing(4.0),
    );

    for (idx, input) in hw.inputs.iter().enumerate() {
        let kind_picker = pick_list(
            vec![InputKind::Cc, InputKind::Note, InputKind::PitchBend],
            Some(input.kind),
            {
                let id = hw.id.clone();
                move |k| Message::UpdateHardwareInput(id.clone(), idx, InputField::Kind(k))
            },
        );
        col = col.push(endpoint_row(
            &input.label,
            input.channel,
            input.number,
            kind_picker.into(),
            EndpointTarget::Input {
                hw_id: hw.id.clone(),
                index: idx,
            },
        ));
    }

    col = col.push(
        row![text("Outputs:").size(12.0), {
            let id = hw.id.clone();
            button("+").on_press(Message::AddHardwareOutput(id))
        },]
        .spacing(4.0),
    );

    for (idx, output) in hw.outputs.iter().enumerate() {
        let kind_picker = pick_list(
            vec![
                OutputKind::Led,
                OutputKind::LedRing,
                OutputKind::LedRingCc,
                OutputKind::Scribble,
            ],
            Some(output.kind),
            {
                let id = hw.id.clone();
                move |k| Message::UpdateHardwareOutput(id.clone(), idx, OutputField::Kind(k))
            },
        );
        col = col.push(endpoint_row(
            &output.label,
            output.channel,
            output.number,
            kind_picker.into(),
            EndpointTarget::Output {
                hw_id: hw.id.clone(),
                index: idx,
            },
        ));
    }

    col = col.push(button("Remove Hardware").on_press(Message::RemoveHardware(hw.id.clone())));

    col = col.push(separator());
    col = col.push(text("Software").size(13.0));

    let layer_label = software.map(|s| s.label.as_str()).unwrap_or("");
    let label_input = text_input("label", layer_label)
        .on_input(move |s| Message::UpdateIconLabel(icon_idx, s))
        .width(Length::Fixed(140.0))
        .size(12.0);
    col = col.push(
        row![text("Label:").size(12.0), label_input]
            .spacing(4.0)
            .align_y(Vertical::Center),
    );

    let vis_row: Element<'a, Message> = if let Some(vs) = visual_source {
        let source_editor = source_picker_widget(
            vs,
            app,
            move |pth: &[SourceBranch], cond: Source| {
                Message::UpdateVisualSource(icon_idx, pth.to_vec(), Some(cond))
            },
            &[],
            move |pth: &[SourceBranch], cmp: Comparison| {
                Message::UpdateVisualIfComparison(icon_idx, pth.to_vec(), cmp)
            },
            &hw_input_labels,
        );
        row![
            text("Visual:").size(12.0),
            source_editor,
            button(text("x")).on_press(Message::UpdateVisualSource(icon_idx, Vec::new(), None)),
        ]
        .spacing(4.0)
        .into()
    } else {
        let idx = icon_idx;
        let mut names = vec!["None"];
        names.extend(source_type_names());
        row![
            text("Visual:").size(12.0),
            pick_list(names, Some("None"), move |kind| {
                let src = if kind == "None" {
                    None
                } else {
                    Some(new_source(kind))
                };
                Message::UpdateVisualSource(idx, Vec::new(), src)
            }),
        ]
        .spacing(4.0)
        .into()
    };
    col = col.push(vis_row);

    col = col.push(separator());
    col = col.push(
        row![
            text("Signal Entries (actions when input fires)").size(13.0),
            button("+ Add Entry")
                .on_press(Message::AddSignalEntry(icon_idx))
                .style(button::primary),
        ]
        .spacing(8.0),
    );

    for (se_idx, se) in signal_entries.iter().enumerate() {
        let input_picker = pick_list(
            hw.inputs
                .iter()
                .map(|i| i.label.clone())
                .filter(|label| {
                    !signal_entries
                        .iter()
                        .enumerate()
                        .any(|(i, e)| i != se_idx && e.input == *label)
                })
                .collect::<Vec<_>>(),
            if se.input.is_empty() {
                None
            } else {
                Some(se.input.clone())
            },
            move |label| Message::UpdateSignalInput(icon_idx, se_idx, label),
        );

        let mut action_rows = Column::new().spacing(2.0);
        for (a_idx, action) in se.actions.iter().enumerate() {
            action_rows = action_rows.push(signal_action_row(icon_idx, se_idx, a_idx, action, app));
        }

        let add_action_btn =
            button("+ Action").on_press(Message::AddSignalAction(icon_idx, se_idx));

        col = col.push(
            row![
                text(format!("Entry {se_idx}:"),).size(12.0),
                input_picker,
                button("✕").on_press(Message::RemoveSignalEntry(icon_idx, se_idx)),
            ]
            .spacing(4.0)
            .align_y(Vertical::Center),
        );
        col = col.push(action_rows);
        col = col.push(add_action_btn);
    }

    col = col.push(separator());
    col = col.push(
        row![
            text("Feedback Entries (source per output)").size(13.0),
            button("+ Add Feedback")
                .on_press(Message::AddFeedbackEntry(icon_idx))
                .style(button::primary),
        ]
        .spacing(8.0),
    );

    for (fe_idx, fe) in feedback_entries.iter().enumerate() {
        let output_picker = pick_list(
            hw.outputs
                .iter()
                .map(|o| o.label.clone())
                .filter(|label| {
                    !feedback_entries
                        .iter()
                        .enumerate()
                        .any(|(i, e)| i != fe_idx && e.output == *label)
                })
                .collect::<Vec<_>>(),
            if fe.output.is_empty() {
                None
            } else {
                Some(fe.output.clone())
            },
            move |label| Message::UpdateFeedbackOutput(icon_idx, fe_idx, label),
        );

        let is_scribble = hw
            .outputs
            .iter()
            .find(|o| o.label == fe.output)
            .is_some_and(|o| o.kind == OutputKind::Scribble);

        let source1 = source_picker_widget(
            &fe.source,
            app,
            move |pth: &[SourceBranch], cond: Source| {
                Message::UpdateFeedbackSource(
                    icon_idx,
                    fe_idx,
                    FeedbackLine::Line1,
                    pth.to_vec(),
                    cond,
                )
            },
            &[],
            move |pth: &[SourceBranch], cmp: Comparison| {
                Message::UpdateIfComparison(
                    icon_idx,
                    fe_idx,
                    FeedbackLine::Line1,
                    pth.to_vec(),
                    cmp,
                )
            },
            &hw_input_labels,
        );

        let line2: Element<'a, Message> = if is_scribble {
            match &fe.line2_source {
                Some(s2) => row![
                    text("line 2:").size(10.0),
                    source_picker_widget(
                        s2,
                        app,
                        move |pth: &[SourceBranch], cond: Source| {
                            Message::UpdateFeedbackSource(
                                icon_idx,
                                fe_idx,
                                FeedbackLine::Line2,
                                pth.to_vec(),
                                cond,
                            )
                        },
                        &[],
                        move |pth: &[SourceBranch], cmp: Comparison| {
                            Message::UpdateIfComparison(
                                icon_idx,
                                fe_idx,
                                FeedbackLine::Line2,
                                pth.to_vec(),
                                cmp,
                            )
                        },
                        &hw_input_labels,
                    ),
                ]
                .spacing(4.0)
                .into(),
                None => row![
                    text("line 2:").size(10.0),
                    pick_list(source_type_names(), None::<&str>, move |kind| {
                        Message::UpdateFeedbackSource(
                            icon_idx,
                            fe_idx,
                            FeedbackLine::Line2,
                            Vec::new(),
                            new_source(kind),
                        )
                    },),
                ]
                .spacing(4.0)
                .into(),
            }
        } else {
            text("").into()
        };

        col = col.push(
            row![
                text(format!("Feedback {fe_idx}:"),).size(12.0),
                output_picker,
                button("✕").on_press(Message::RemoveFeedbackEntry(icon_idx, fe_idx)),
            ]
            .spacing(4.0)
            .align_y(Vertical::Center),
        );
        col = col.push(row![text("source:").size(10.0), source1].spacing(4.0));
        col = col.push(line2);
    }

    container(col.spacing(4.0).padding(8.0))
        .style(editor_style)
        .into()
}
