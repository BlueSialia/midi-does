use std::sync::LazyLock;

use iced::widget::{column, pick_list, row, text, text_input};
use iced::{Element, Length};

use crate::app::App;
use crate::config::{CmpOp, Comparison, Source, SourceBranch, MIN_BLINK_INTERVAL_MS};
use crate::pipewire::PwObjectType;

use super::Message;

/// One representative of each source kind, in picker order.
static SOURCE_SAMPLES: LazyLock<[Source; 11]> = LazyLock::new(|| {
    [
        Source::Direct {
            value: String::new(),
        },
        Source::HardwareInput {
            input_label: String::new(),
        },
        Source::Volume {
            target: String::new(),
        },
        Source::Muted {
            target: String::new(),
        },
        Source::RouteActive {
            stream: String::new(),
            sink: String::new(),
        },
        Source::LayerActive {
            layer: String::new(),
        },
        Source::Bank,
        Source::Custom { cmd: String::new() },
        Source::If {
            condition: Comparison {
                left: Box::new(Source::Direct {
                    value: "true".to_string(),
                }),
                op: CmpOp::Eq,
                right: Box::new(Source::Direct {
                    value: "true".to_string(),
                }),
            },
            then_source: Box::new(Source::Direct {
                value: "true".to_string(),
            }),
            else_source: Box::new(Source::Direct {
                value: "false".to_string(),
            }),
        },
        Source::RingRange {
            source: Box::new(Source::Direct {
                value: "0.5".to_string(),
            }),
            min: 0,
            max: 127,
        },
        Source::Blink {
            a: Box::new(Source::Direct {
                value: "true".to_string(),
            }),
            b: Box::new(Source::Direct {
                value: "false".to_string(),
            }),
            interval_ms: 1000,
        },
    ]
});

/// Composite sources nest other sources and are excluded from the compact
/// comparison-operand picker.
fn source_is_composite(source: &Source) -> bool {
    matches!(
        source,
        Source::If { .. } | Source::RingRange { .. } | Source::Blink { .. }
    )
}

pub(super) fn source_type_names() -> Vec<&'static str> {
    SOURCE_SAMPLES.iter().map(Source::kind).collect()
}

pub(super) fn new_source(kind: &str) -> Source {
    SOURCE_SAMPLES
        .iter()
        .find(|source| source.kind() == kind)
        .cloned()
        .unwrap_or_else(|| Source::Direct {
            value: "true".to_string(),
        })
}

fn route_active_params_widget<'a>(
    app: &'a App,
    stream: &'a str,
    sink: &'a str,
    on_stream_change: impl Fn(String) -> Message + 'a,
    on_sink_change: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    row![
        pw_obj_picker(app, PwObjectType::StreamOrSource, stream, on_stream_change),
        text("->").size(11.0),
        pw_obj_picker(app, PwObjectType::Sink, sink, on_sink_change),
    ]
    .spacing(2.0)
    .align_y(iced::alignment::Vertical::Center)
    .into()
}

fn layer_active_params_widget<'a>(
    layer: &'a str,
    on_change: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    text_input("layer", layer)
        .on_input(on_change)
        .width(Length::Fixed(100.0))
        .size(11.0)
        .into()
}

/// Renders the parameter editors for the leaf `Source` variants shared by the
/// full picker and the compact comparison-operand picker.
fn source_leaf_params<'a>(
    source: &'a Source,
    app: &'a App,
    upd: impl Fn(Source) -> Message + Clone + 'a,
    hw_input_labels: &[String],
) -> Element<'a, Message> {
    match source {
        Source::Direct { value } => {
            let upd = upd.clone();
            text_input("value", value)
                .on_input(move |v| upd(Source::Direct { value: v }))
                .width(Length::Fixed(80.0))
                .size(11.0)
                .into()
        }
        Source::Muted { target } => {
            let upd = upd.clone();
            pw_obj_picker(app, PwObjectType::SinkOrSource, target, move |t| {
                upd(Source::Muted { target: t })
            })
        }
        Source::Volume { target } => {
            let upd = upd.clone();
            pw_obj_picker(app, PwObjectType::SinkOrSource, target, move |t| {
                upd(Source::Volume { target: t })
            })
        }
        Source::RouteActive { stream, sink } => {
            let sk = sink.clone();
            route_active_params_widget(
                app,
                stream,
                sink,
                {
                    let upd = upd.clone();
                    move |st| {
                        upd(Source::RouteActive {
                            stream: st,
                            sink: sk.clone(),
                        })
                    }
                },
                {
                    let upd = upd.clone();
                    move |si| {
                        upd(Source::RouteActive {
                            stream: stream.clone(),
                            sink: si,
                        })
                    }
                },
            )
        }
        Source::LayerActive { layer } => {
            let upd = upd.clone();
            layer_active_params_widget(layer, move |l| upd(Source::LayerActive { layer: l }))
        }
        Source::Bank => text("Bank").size(11.0).into(),
        Source::Custom { cmd } => {
            let upd = upd.clone();
            text_input("cmd", cmd)
                .on_input(move |c| upd(Source::Custom { cmd: c }))
                .width(Length::Fixed(120.0))
                .size(11.0)
                .into()
        }
        Source::HardwareInput { input_label } => {
            let upd = upd.clone();
            let labels: Vec<String> = hw_input_labels.to_vec();
            pick_list(labels, Some(input_label.clone()), move |l| {
                upd(Source::HardwareInput { input_label: l })
            })
            .width(Length::Fixed(100.0))
            .into()
        }
        Source::If { .. } | Source::RingRange { .. } | Source::Blink { .. } => text("").into(),
    }
}

/// Simplified source picker for comparison operands — composite sources are
/// excluded to keep the condition UI compact.
fn source_simple_widget<'a>(
    source: &'a Source,
    app: &'a App,
    upd: impl Fn(Source) -> Message + Clone + 'a,
    hw_input_labels: &[String],
) -> Element<'a, Message> {
    let leaf_names: Vec<&'static str> = SOURCE_SAMPLES
        .iter()
        .filter(|source| !source_is_composite(source))
        .map(Source::kind)
        .collect();
    let selected = if source_is_composite(source) {
        None
    } else {
        Some(source.kind())
    };
    let src_picker = {
        let upd = upd.clone();
        pick_list(leaf_names, selected, move |kind| upd(new_source(kind)))
    };
    let params = source_leaf_params(source, app, upd, hw_input_labels);
    column![src_picker, params].spacing(4.0).into()
}

fn comparison_editor<'a>(
    comparison: &'a Comparison,
    app: &'a App,
    upd_cond: impl Fn(Comparison) -> Message + Clone + 'a,
    hw_input_labels: &[String],
) -> Element<'a, Message> {
    let op_picker = {
        let cmp = comparison.clone();
        let upd_cond = upd_cond.clone();
        pick_list(
            vec![CmpOp::Lt, CmpOp::Lte, CmpOp::Gt, CmpOp::Gte, CmpOp::Eq],
            Some(comparison.op),
            move |op| {
                upd_cond(Comparison {
                    left: cmp.left.clone(),
                    op,
                    right: cmp.right.clone(),
                })
            },
        )
    };
    let left_editor = source_simple_widget(
        &comparison.left,
        app,
        {
            let cmp = comparison.clone();
            let upd_cond = upd_cond.clone();
            move |left| {
                upd_cond(Comparison {
                    left: Box::new(left),
                    op: cmp.op,
                    right: cmp.right.clone(),
                })
            }
        },
        hw_input_labels,
    );
    let right_editor = source_simple_widget(
        &comparison.right,
        app,
        {
            let cmp = comparison.clone();
            let upd_cond = upd_cond.clone();
            move |right| {
                upd_cond(Comparison {
                    left: cmp.left.clone(),
                    op: cmp.op,
                    right: Box::new(right),
                })
            }
        },
        hw_input_labels,
    );
    row![text("if:").size(10.0), left_editor, op_picker, right_editor,]
        .spacing(4.0)
        .align_y(iced::alignment::Vertical::Center)
        .into()
}

pub(super) fn source_picker_widget<'a>(
    source: &'a Source,
    app: &'a App,
    upd: impl Fn(&[SourceBranch], Source) -> Message + Clone + 'a,
    path: &[SourceBranch],
    upd_cond: impl Fn(&[SourceBranch], Comparison) -> Message + Clone + 'a,
    hw_input_labels: &[String],
) -> Element<'a, Message> {
    let cur_type = source.kind();
    let src_picker = {
        let upd = upd.clone();
        let owned = path.to_vec();
        pick_list(source_type_names(), Some(cur_type), move |kind| {
            upd(&owned, new_source(kind))
        })
    };
    let params: Element<'a, Message> = match source {
        Source::If {
            condition,
            then_source,
            else_source,
        } => {
            let mut then_path = path.to_vec();
            then_path.push(SourceBranch::IfThen);
            let mut else_path = path.to_vec();
            else_path.push(SourceBranch::IfElse);
            let cond_editor = comparison_editor(
                condition,
                app,
                {
                    let upd_cond = upd_cond.clone();
                    let owned = path.to_vec();
                    move |cmp| upd_cond(&owned, cmp)
                },
                hw_input_labels,
            );
            column![
                cond_editor,
                row![
                    text("then:").size(10.0),
                    source_picker_widget(
                        then_source,
                        app,
                        upd.clone(),
                        &then_path,
                        upd_cond.clone(),
                        hw_input_labels,
                    )
                ]
                .spacing(4.0),
                row![
                    text("else:").size(10.0),
                    source_picker_widget(
                        else_source,
                        app,
                        upd,
                        &else_path,
                        upd_cond,
                        hw_input_labels
                    )
                ]
                .spacing(4.0),
            ]
            .spacing(2.0)
            .into()
        }
        Source::RingRange { source, min, max } => {
            let mut inner_path = path.to_vec();
            inner_path.push(SourceBranch::RingRangeInner);
            let inner_picker = source_picker_widget(
                source,
                app,
                upd.clone(),
                &inner_path,
                upd_cond.clone(),
                hw_input_labels,
            );
            let min_input = {
                let upd = upd.clone();
                let owned = path.to_vec();
                text_input("min", &format!("{:02X}", min))
                    .on_input(move |s| {
                        let v = u8::from_str_radix(&s, 16).unwrap_or(0);
                        upd(
                            &owned,
                            Source::RingRange {
                                source: source.clone(),
                                min: v,
                                max: *max,
                            },
                        )
                    })
                    .width(Length::Fixed(35.0))
                    .size(11.0)
            };
            let max_input = {
                let upd = upd.clone();
                let owned = path.to_vec();
                text_input("max", &format!("{:02X}", max))
                    .on_input(move |s| {
                        let v = u8::from_str_radix(&s, 16).unwrap_or(127);
                        upd(
                            &owned,
                            Source::RingRange {
                                source: source.clone(),
                                min: *min,
                                max: v,
                            },
                        )
                    })
                    .width(Length::Fixed(35.0))
                    .size(11.0)
            };
            column![
                inner_picker,
                row![
                    text("min:").size(10.0),
                    min_input,
                    text("max:").size(10.0),
                    max_input
                ]
                .spacing(4.0)
            ]
            .spacing(2.0)
            .into()
        }
        Source::Blink { a, b, interval_ms } => {
            let mut a_path = path.to_vec();
            a_path.push(SourceBranch::BlinkA);
            let a_picker = source_picker_widget(
                a,
                app,
                upd.clone(),
                &a_path,
                upd_cond.clone(),
                hw_input_labels,
            );
            let mut b_path = path.to_vec();
            b_path.push(SourceBranch::BlinkB);
            let b_picker = source_picker_widget(
                b,
                app,
                upd.clone(),
                &b_path,
                upd_cond.clone(),
                hw_input_labels,
            );
            let interval_input = {
                let upd = upd.clone();
                let owned = path.to_vec();
                text_input("interval_ms", &interval_ms.to_string())
                    .on_input(move |s| {
                        let v: u64 = s.parse().unwrap_or(1000).max(MIN_BLINK_INTERVAL_MS);
                        upd(
                            &owned,
                            Source::Blink {
                                a: a.clone(),
                                b: b.clone(),
                                interval_ms: v,
                            },
                        )
                    })
                    .width(Length::Fixed(80.0))
                    .size(11.0)
            };
            column![
                text("a:").size(10.0),
                a_picker,
                text("b:").size(10.0),
                b_picker,
                row![text("interval_ms:").size(10.0), interval_input].spacing(4.0),
            ]
            .spacing(2.0)
            .into()
        }
        _ => {
            let owned = path.to_vec();
            let leaf_upd = {
                let upd = upd.clone();
                move |s| upd(&owned, s)
            };
            source_leaf_params(source, app, leaf_upd, hw_input_labels)
        }
    };
    column![src_picker, params].spacing(4.0).into()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PwOpt {
    label: String,
    id: String,
}
impl std::fmt::Display for PwOpt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

pub(super) fn pw_obj_picker<'a>(
    app: &'a App,
    ot: PwObjectType,
    current: &str,
    on_select: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    let mut opts: Vec<PwOpt> = Vec::new();
    for obj in app.pw_objects.iter() {
        let matches = match ot {
            PwObjectType::SinkOrSource => {
                obj.object_type == PwObjectType::Sink || obj.object_type == PwObjectType::Source
            }
            PwObjectType::StreamOrSource => {
                obj.object_type == PwObjectType::Stream || obj.object_type == PwObjectType::Source
            }
            _ => obj.object_type == ot,
        };
        if matches {
            opts.push(PwOpt {
                label: format!("{} ({})", obj.description, obj.id),
                // Persist the stable node name: numeric PipeWire ids change across reboots, names don't.
                id: obj.name.clone(),
            });
        }
    }
    if !current.is_empty() && !opts.iter().any(|o| o.id == current) {
        opts.push(PwOpt {
            label: format!("[custom] {current}"),
            id: current.to_string(),
        });
    }
    let selected = opts.iter().find(|o| o.id == current).cloned();
    pick_list(opts, selected, move |o| on_select(o.id))
        .width(Length::Fixed(200.0))
        .into()
}
