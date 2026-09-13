use iced::alignment::Vertical;
use iced::widget::{button, column, pick_list, row, text, text_input};
use iced::{Element, Length, Padding};

use crate::actions::{Action, EncoderMode, VolumeMode, VolumeTarget};
use crate::app::App;
use crate::pipewire::PwObjectType;

use super::source_picker::pw_obj_picker;
use super::Message;

pub(super) fn signal_action_row<'a>(
    icon_idx: usize,
    se_idx: usize,
    a_idx: usize,
    action: &'a Action,
    app: &'a App,
) -> Element<'a, Message> {
    let actions = Action::all();
    let type_picker = pick_list(
        actions.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        Some(action.display_name()),
        move |name: &str| {
            let new_action = actions
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, a)| a.clone())
                .unwrap_or(Action::None);
            Message::UpdateSignalAction(icon_idx, se_idx, a_idx, new_action)
        },
    );

    let params = signal_action_params(icon_idx, se_idx, a_idx, action, app);

    let remove_btn =
        button(text("✕")).on_press(Message::RemoveSignalAction(icon_idx, se_idx, a_idx));

    row![type_picker, params, remove_btn]
        .spacing(4.0)
        .align_y(Vertical::Center)
        .padding(Padding::new(0.0))
        .into()
}

struct VolUiParams<'a> {
    action: &'a Action,
    id: &'a str,
    target_kind: VolumeTarget,
    min: f64,
    max: f64,
    mode: VolumeMode,
    encoder: EncoderMode,
    step: f64,
}

/// Build an `on_input`/`on_selected` callback that clones the action, applies
/// `mutate`, and emits an `UpdateSignalAction` message.
fn update_action<T>(
    icon_idx: usize,
    se_idx: usize,
    a_idx: usize,
    action: Action,
    mutate: impl Fn(&mut Action, T) + 'static,
) -> impl Fn(T) -> Message {
    move |value| {
        let mut a = action.clone();
        mutate(&mut a, value);
        Message::UpdateSignalAction(icon_idx, se_idx, a_idx, a)
    }
}

/// Build an `on_input` callback for a percentage field, parsing, clamping, and
/// converting to a 0.0–1.0 fraction before applying it through `set`.
fn percent_input(
    icon_idx: usize,
    se_idx: usize,
    a_idx: usize,
    action: Action,
    default_pct: f64,
    max_pct: f64,
    set: impl Fn(&mut Action, f64) + 'static,
) -> impl Fn(String) -> Message {
    update_action(icon_idx, se_idx, a_idx, action, move |a, s: String| {
        let fraction = s
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .unwrap_or(default_pct)
            .clamp(0.0, max_pct)
            / 100.0;
        set(a, fraction);
    })
}

fn volume_action_params<'a>(
    icon_idx: usize,
    se_idx: usize,
    a_idx: usize,
    app: &'a App,
    p: VolUiParams<'a>,
) -> Element<'a, Message> {
    let pw_type = match p.target_kind {
        VolumeTarget::Endpoint => PwObjectType::SinkOrSource,
        VolumeTarget::Stream => PwObjectType::Stream,
    };

    let kind_pick = pick_list(
        vec![VolumeTarget::Endpoint, VolumeTarget::Stream],
        Some(p.target_kind),
        update_action(icon_idx, se_idx, a_idx, p.action.clone(), |a, k| {
            if let Action::SetVolume { target_kind, .. } = a {
                *target_kind = k;
            }
        }),
    );

    let id_pick = pw_obj_picker(
        app,
        pw_type,
        p.id,
        update_action(icon_idx, se_idx, a_idx, p.action.clone(), |a, s| {
            if let Action::SetVolume { target, .. } = a {
                *target = s;
            }
        }),
    );

    let min_pct = (p.min.clamp(0.0, 2.0) * 100.0) as u32;
    let min_input = text_input("min%", &min_pct.to_string())
        .on_input(percent_input(
            icon_idx,
            se_idx,
            a_idx,
            p.action.clone(),
            0.0,
            200.0,
            |a, v| {
                if let Action::SetVolume { min, .. } = a {
                    *min = v;
                }
            },
        ))
        .width(Length::Fixed(50.0))
        .size(11.0);

    let max_pct = (p.max.clamp(0.0, 2.0) * 100.0) as u32;
    let max_input = text_input("max%", &max_pct.to_string())
        .on_input(percent_input(
            icon_idx,
            se_idx,
            a_idx,
            p.action.clone(),
            100.0,
            200.0,
            |a, v| {
                if let Action::SetVolume { max, .. } = a {
                    *max = v;
                }
            },
        ))
        .width(Length::Fixed(50.0))
        .size(11.0);

    let mode_pick = pick_list(
        vec![VolumeMode::Absolute, VolumeMode::Relative],
        Some(p.mode),
        update_action(icon_idx, se_idx, a_idx, p.action.clone(), |a, m| {
            if let Action::SetVolume { mode, .. } = a {
                *mode = m;
            }
        }),
    );

    let extra: Element<'a, Message> = if p.mode == VolumeMode::Relative {
        let enc_pick = pick_list(
            vec![
                EncoderMode::SignMagnitude,
                EncoderMode::TwosComplement,
                EncoderMode::BinaryOffset,
            ],
            Some(p.encoder),
            update_action(icon_idx, se_idx, a_idx, p.action.clone(), |a, e| {
                if let Action::SetVolume { encoder, .. } = a {
                    *encoder = e;
                }
            }),
        );
        let step_pct = (p.step * 100.0) as u32;
        let step_input = text_input("step%", &step_pct.to_string())
            .on_input(percent_input(
                icon_idx,
                se_idx,
                a_idx,
                p.action.clone(),
                1.0,
                f64::INFINITY,
                |a, v| {
                    if let Action::SetVolume { step, .. } = a {
                        *step = v;
                    }
                },
            ))
            .width(Length::Fixed(50.0))
            .size(11.0);
        row![
            text("Enc:").size(11.0),
            enc_pick,
            text("Step:").size(11.0),
            step_input
        ]
        .spacing(4.0)
        .align_y(Vertical::Center)
        .into()
    } else {
        text("").into()
    };

    column![
        row![
            kind_pick,
            id_pick,
            text("Min:").size(11.0),
            min_input,
            text("Max:").size(11.0),
            max_input,
            mode_pick
        ]
        .spacing(4.0)
        .align_y(Vertical::Center),
        extra
    ]
    .spacing(2.0)
    .into()
}

fn midi_send_params<'a>(
    icon_idx: usize,
    se_idx: usize,
    a_idx: usize,
    action: &'a Action,
    fields: [(&'static str, u8); 3],
    set: impl Fn(&mut Action, [u8; 3]) + Clone + 'static,
) -> Element<'a, Message> {
    let widths = [35.0, 40.0, 40.0];
    let mut widgets: Vec<Element<'a, Message>> = Vec::new();
    for (i, (label, value)) in fields.iter().copied().enumerate() {
        widgets.push(text(label).size(11.0).into());
        let input = text_input(label, &value.to_string())
            .on_input(update_action(icon_idx, se_idx, a_idx, action.clone(), {
                let set = set.clone();
                move |a, s: String| {
                    let mut vals = fields.map(|(_, v)| v);
                    vals[i] = s.parse().unwrap_or(0);
                    set(a, vals);
                }
            }))
            .width(Length::Fixed(widths[i]))
            .size(11.0);
        widgets.push(input.into());
    }
    iced::widget::Row::with_children(widgets)
        .spacing(4.0)
        .align_y(Vertical::Center)
        .into()
}

fn signal_action_params<'a>(
    icon_idx: usize,
    se_idx: usize,
    a_idx: usize,
    action: &'a Action,
    app: &'a App,
) -> Element<'a, Message> {
    match action {
        Action::SetVolume {
            target,
            target_kind,
            min,
            max,
            mode,
            encoder,
            step,
        } => volume_action_params(
            icon_idx,
            se_idx,
            a_idx,
            app,
            VolUiParams {
                action,
                id: target,
                target_kind: *target_kind,
                min: *min,
                max: *max,
                mode: *mode,
                encoder: *encoder,
                step: *step,
            },
        ),
        Action::ToggleMute { target } => pw_obj_picker(
            app,
            PwObjectType::SinkOrSource,
            target,
            update_action(icon_idx, se_idx, a_idx, action.clone(), |a, s| {
                if let Action::ToggleMute { target } = a {
                    *target = s;
                }
            }),
        ),
        Action::RouteStream { stream_id, sink_id } => {
            let stream_pick = pw_obj_picker(
                app,
                PwObjectType::StreamOrSource,
                stream_id,
                update_action(icon_idx, se_idx, a_idx, action.clone(), |a, s| {
                    if let Action::RouteStream { stream_id, .. } = a {
                        *stream_id = s;
                    }
                }),
            );
            let sink_pick = pw_obj_picker(
                app,
                PwObjectType::Sink,
                sink_id,
                update_action(icon_idx, se_idx, a_idx, action.clone(), |a, s| {
                    if let Action::RouteStream { sink_id, .. } = a {
                        *sink_id = s;
                    }
                }),
            );
            row![stream_pick, text("->").size(11.0), sink_pick]
                .spacing(4.0)
                .align_y(Vertical::Center)
                .into()
        }
        Action::SelectLayer { layer } => {
            let layer_names: Vec<String> = app
                .selected_device()
                .map(|d| d.layers.iter().map(|l| l.name.clone()).collect())
                .unwrap_or_else(|| vec![layer.clone()]);
            pick_list(
                layer_names,
                if layer.is_empty() {
                    None
                } else {
                    Some(layer.clone())
                },
                update_action(icon_idx, se_idx, a_idx, action.clone(), |a, l| {
                    if let Action::SelectLayer { layer } = a {
                        *layer = l;
                    }
                }),
            )
            .into()
        }
        Action::RunCommand { command } => text_input("command", command)
            .on_input(update_action(
                icon_idx,
                se_idx,
                a_idx,
                action.clone(),
                |a, s| {
                    if let Action::RunCommand { command } = a {
                        *command = s;
                    }
                },
            ))
            .width(Length::Fixed(250.0))
            .size(11.0)
            .into(),
        Action::SetBankOffset { offset } => {
            let off_input = text_input("offset", &offset.to_string())
                .on_input(update_action(
                    icon_idx,
                    se_idx,
                    a_idx,
                    action.clone(),
                    |a, s: String| {
                        if let Action::SetBankOffset { offset } = a {
                            *offset = s.parse().unwrap_or(0);
                        }
                    },
                ))
                .width(Length::Fixed(50.0))
                .size(11.0);
            row![text("Offset:").size(11.0), off_input]
                .spacing(4.0)
                .align_y(Vertical::Center)
                .into()
        }
        Action::MidiSendNote {
            channel,
            note,
            velocity,
        } => midi_send_params(
            icon_idx,
            se_idx,
            a_idx,
            action,
            [("Ch:", *channel), ("Note:", *note), ("Vel:", *velocity)],
            |a, v| {
                if let Action::MidiSendNote {
                    channel,
                    note,
                    velocity,
                } = a
                {
                    *channel = v[0];
                    *note = v[1];
                    *velocity = v[2];
                }
            },
        ),
        Action::MidiSendCc {
            channel,
            controller,
            value,
        } => midi_send_params(
            icon_idx,
            se_idx,
            a_idx,
            action,
            [("Ch:", *channel), ("CC:", *controller), ("Val:", *value)],
            |a, v| {
                if let Action::MidiSendCc {
                    channel,
                    controller,
                    value,
                } = a
                {
                    *channel = v[0];
                    *controller = v[1];
                    *value = v[2];
                }
            },
        ),
        _ => text("").into(),
    }
}
