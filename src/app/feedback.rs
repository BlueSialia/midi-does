use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex};

use crate::config::{ControlRef, HardwareDef, HardwareOutput, OutputKind, SoftwareDef, Source};
use crate::midi::MidiOutputHandle;
use crate::pipewire::PwObject;

use super::App;

mod eval;

use eval::{
    build_led_ring_sysex, build_scribble_sysex, evaluate_source_with_context, interpret_feedback,
    ring_value, visual_value, EvalContext,
};

pub(super) const MCU_SYSEX_HEADER: &[u8] = &[0xF0, 0x00, 0x00, 0x66, 0x14];
pub(super) const MCU_SCRIBBLE_STRIP_CMD: u8 = 0x12;
pub(super) const MCU_LED_RING_CMD: u8 = 0x20;
pub(super) const SYSEX_TERMINATOR: u8 = 0xF7;

#[derive(Clone)]
pub(super) struct JobBase {
    pub(super) device: String,
    pub(super) hardware_id: String,
    pub(super) layer_name: String,
    pub(super) bank_offset: u8,
    /// The most recent raw MIDI values (0–127) for each input label of this control.
    /// Used by `Source::HardwareInput { input_label }`.
    pub(super) raw_midi_values: Arc<std::collections::HashMap<String, u8>>,
}

impl JobBase {
    fn context<'a>(&'a self, env: &'a EvalEnv) -> EvalContext<'a> {
        EvalContext {
            pw_objects: env.pw_objects.as_slice(),
            layer_name: &self.layer_name,
            bank_offset: self.bank_offset,
            raw_midi_values: self.raw_midi_values.as_ref(),
            pw_link_edges: env.pw_link_edges.as_slice(),
            published_at: env.published_at,
        }
    }
}

pub(super) struct FeedbackJob {
    pub(super) base: JobBase,
    pub(super) output_label: String,
    pub(super) output: HardwareOutput,
    pub(super) source: Source,
    pub(super) line2_source: Option<Source>,
}

pub(super) struct VisualJob {
    pub(super) base: JobBase,
    pub(super) source: Source,
}

#[derive(Debug, Clone)]
pub(super) struct VisualResultOut {
    pub(super) device: String,
    pub(super) hardware_id: String,
    pub(super) value: f64,
}

pub(super) struct EvalEnv {
    pub(super) pw_objects: Arc<Vec<PwObject>>,
    pub(super) pw_link_edges: Arc<Vec<(String, String)>>,
    pub(super) published_at: std::time::SystemTime,
}

pub(super) struct FeedbackEval {
    pub(super) env: EvalEnv,
    pub(super) jobs: Vec<FeedbackJob>,
}

pub(super) struct VisualEval {
    pub(super) env: EvalEnv,
    pub(super) jobs: Vec<VisualJob>,
}

/// Pending evals drained from the bus in one cycle. Both slots are taken
/// together so a busy feedback slot can never starve the visuals slot.
struct EvalRequest {
    feedback: Option<FeedbackEval>,
    visuals: Option<VisualEval>,
}

/// A latest-snapshot-wins bus with one slot for feedback and one for visuals.
/// Writing overwrites the pending slot; the worker drains both slots together
/// each cycle so neither type of eval can starve the other, and always
/// evaluates the freshest snapshot without accumulating a backlog.
pub(super) struct FeedbackBus {
    state: Mutex<FeedbackBusState>,
    cv: Condvar,
}

#[derive(Default)]
struct FeedbackBusState {
    feedback: Option<FeedbackEval>,
    visuals: Option<VisualEval>,
    shutdown: bool,
}

impl FeedbackBus {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(FeedbackBus {
            state: Mutex::new(FeedbackBusState::default()),
            cv: Condvar::new(),
        })
    }

    pub(super) fn send_feedback(&self, eval: FeedbackEval) {
        let mut state = self.state.lock().unwrap();
        state.feedback = Some(eval);
        self.cv.notify_one();
    }

    pub(super) fn send_visuals(&self, eval: VisualEval) {
        let mut state = self.state.lock().unwrap();
        state.visuals = Some(eval);
        self.cv.notify_one();
    }

    pub(super) fn shutdown(&self) {
        let mut state = self.state.lock().unwrap();
        state.shutdown = true;
        self.cv.notify_one();
    }

    fn recv(&self) -> Option<EvalRequest> {
        let mut state = self.state.lock().unwrap();
        loop {
            if state.shutdown {
                return None;
            }
            let feedback = state.feedback.take();
            let visuals = state.visuals.take();
            if feedback.is_some() || visuals.is_some() {
                return Some(EvalRequest { feedback, visuals });
            }
            state = self.cv.wait(state).unwrap();
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum FeedbackOutput {
    Feedback(Vec<FeedbackResultOut>),
    Visuals(Vec<VisualResultOut>),
}

#[derive(Debug, Clone)]
pub(super) struct FeedbackResultOut {
    pub(super) device: String,
    pub(super) hardware_id: String,
    pub(super) output_label: String,
    pub(super) output: HardwareOutput,
    pub(super) result: FeedbackResult,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum FeedbackResult {
    Led(bool),
    LedRing(f64),
    Scribble(String, String),
}

pub(super) fn apply_feedback_to_handle(
    handle: &mut MidiOutputHandle,
    output: &HardwareOutput,
    result: &FeedbackResult,
) {
    match (result, output.kind) {
        (FeedbackResult::Led(on), OutputKind::Led) => {
            let vel: u8 = if *on { 127 } else { 0 };
            handle.send_note(output.channel, output.number, vel);
        }
        (FeedbackResult::LedRing(value), OutputKind::LedRing) => {
            handle.send_sysex(&build_led_ring_sysex(*value, output.channel));
        }
        (FeedbackResult::LedRing(value), OutputKind::LedRingCc) => {
            handle.send_cc(output.channel, output.number, ring_value(*value));
        }
        (FeedbackResult::Scribble(l1, l2), OutputKind::Scribble) => {
            handle.send_sysex(&build_scribble_sysex(l1, l2, output.channel));
        }
        _ => {
            log::warn!(
                "Feedback mismatch: result {:?} for output kind {:?}",
                result,
                output.kind
            );
        }
    }
}

pub(super) fn feedback_worker(
    bus: Arc<FeedbackBus>,
    out_tx: std::sync::mpsc::Sender<FeedbackOutput>,
) {
    while let Some(EvalRequest { feedback, visuals }) = bus.recv() {
        if let Some(FeedbackEval { env, jobs }) = feedback {
            run_guarded(|| {
                let mut results = Vec::with_capacity(jobs.len());
                for job in jobs {
                    let context = job.base.context(&env);
                    let line1 = evaluate_source_with_context(&job.source, &context);
                    let line2 = job
                        .line2_source
                        .as_ref()
                        .map(|s| evaluate_source_with_context(s, &context))
                        .unwrap_or_default();
                    let result = interpret_feedback(job.output.kind, &line1, &line2);
                    results.push(FeedbackResultOut {
                        device: job.base.device,
                        hardware_id: job.base.hardware_id,
                        output_label: job.output_label,
                        output: job.output,
                        result,
                    });
                }
                let _ = out_tx.send(FeedbackOutput::Feedback(results));
            });
        }

        if let Some(VisualEval { env, jobs }) = visuals {
            run_guarded(|| {
                let mut results = Vec::with_capacity(jobs.len());
                for job in jobs {
                    let context = job.base.context(&env);
                    let text = evaluate_source_with_context(&job.source, &context);
                    results.push(VisualResultOut {
                        device: job.base.device,
                        hardware_id: job.base.hardware_id,
                        value: visual_value(&text),
                    });
                }
                let _ = out_tx.send(FeedbackOutput::Visuals(results));
            });
        }
    }
}

/// Run a single evaluation pass, logging (but not propagating) any panic so a
/// failing feedback pass cannot drop a concurrently pending visuals pass.
fn run_guarded(action: impl FnOnce()) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(action));
    if result.is_err() {
        log::error!("feedback worker recovered from a panic");
    }
}

pub(super) fn find_pw_object<'a>(objects: &'a [PwObject], target: &str) -> Option<&'a PwObject> {
    PwObject::find(objects, target)
}

pub(super) fn node_matches(node: &str, target: &str) -> bool {
    node == target || node.contains(target)
}

fn resolve_node_name(pw_objects: &[PwObject], target: &str) -> String {
    if let Some(obj) = find_pw_object(pw_objects, target) {
        return obj.name.clone();
    }
    target.to_string()
}

pub(super) fn route_is_active_with_edges(
    edges: &[(String, String)],
    pw_objects: &[PwObject],
    stream: &str,
    sink: &str,
) -> bool {
    let stream_name = resolve_node_name(pw_objects, stream);
    let sink_name = resolve_node_name(pw_objects, sink);
    edges
        .iter()
        .any(|(from, to)| node_matches(from, &stream_name) && node_matches(to, &sink_name))
}

impl App {
    fn for_each_active_software(
        &self,
        mut visit: impl FnMut(&JobBase, &HardwareDef, &SoftwareDef),
    ) {
        for dev_name in self.config.device_names() {
            let Some(device) = self.config.devices.get(dev_name) else {
                continue;
            };
            let Some(layer) = self.active_layer(device, dev_name) else {
                continue;
            };

            for hw in &device.hardware {
                let Some(software) = layer.software(&hw.id) else {
                    continue;
                };
                let raw_midi_values: HashMap<String, u8> = hw
                    .inputs
                    .iter()
                    .filter_map(|input| {
                        self.last_input_value
                            .get(&ControlRef::new(dev_name, &hw.id, &input.label))
                            .map(|v| (input.label.clone(), *v))
                    })
                    .collect();
                let base = JobBase {
                    device: dev_name.to_string(),
                    hardware_id: hw.id.clone(),
                    layer_name: layer.name.clone(),
                    bank_offset: layer.bank_offset,
                    raw_midi_values: Arc::new(raw_midi_values),
                };
                visit(&base, hw, software);
            }
        }
    }

    fn eval_snapshot(&self) -> EvalEnv {
        EvalEnv {
            pw_objects: Arc::clone(&self.pw_objects),
            pw_link_edges: Arc::clone(&self.pw_edges),
            published_at: std::time::SystemTime::now(),
        }
    }

    pub(super) fn send_feedback(&mut self) {
        self.send_feedback_for_device_inner(None);
    }

    pub(super) fn send_feedback_for_device(&mut self, device: &str) {
        self.send_feedback_for_device_inner(Some(device));
    }

    fn send_feedback_for_device_inner(&mut self, only_device: Option<&str>) {
        let snap = self.eval_snapshot();
        let mut jobs: Vec<FeedbackJob> = Vec::new();

        self.for_each_active_software(|base, hw, software| {
            if only_device.is_some_and(|device| device != base.device.as_str()) {
                return;
            }
            for fe in &software.feedback_entries {
                let Some(output) = hw.outputs.iter().find(|o| o.label == fe.output) else {
                    continue;
                };
                jobs.push(FeedbackJob {
                    base: base.clone(),
                    output_label: fe.output.clone(),
                    output: output.clone(),
                    source: fe.source.clone(),
                    line2_source: fe.line2_source.clone(),
                });
            }
        });

        if jobs.is_empty() {
            return;
        }
        self.feedback_bus
            .send_feedback(FeedbackEval { env: snap, jobs });
    }

    pub(super) fn send_visual_evaluation(&mut self) {
        let snap = self.eval_snapshot();
        let mut jobs: Vec<VisualJob> = Vec::new();
        let mut live_controls: HashSet<(String, String)> = HashSet::new();

        self.for_each_active_software(|base, hw, software| {
            if let Some(source) = &software.visual_source {
                live_controls.insert((base.device.clone(), hw.id.clone()));
                jobs.push(VisualJob {
                    base: base.clone(),
                    source: source.clone(),
                });
            }
        });

        // Drop values for controls that no longer have a visual source (removed
        // source, deleted hardware, or a layer without one) so a stale value
        // does not linger on the faceplate.
        self.visual_values
            .retain(|key, _| live_controls.contains(key));

        if jobs.is_empty() {
            return;
        }
        self.feedback_bus
            .send_visuals(VisualEval { env: snap, jobs });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CmpOp, Comparison, OutputKind, Source};
    use crate::pipewire::{PwObject, PwObjectType};
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    use super::eval::*;

    fn evaluate_source(
        source: &Source,
        pw_objects: &[PwObject],
        layer_name: &str,
        bank_offset: u8,
        raw_midi_values: &HashMap<String, u8>,
        pw_link_edges: &[(String, String)],
        published_at: std::time::SystemTime,
    ) -> String {
        let context = EvalContext {
            pw_objects,
            layer_name,
            bank_offset,
            raw_midi_values,
            pw_link_edges,
            published_at,
        };
        evaluate_source_with_context(source, &context)
    }

    fn evaluate_comparison(
        comparison: &Comparison,
        pw_objects: &[PwObject],
        layer_name: &str,
        bank_offset: u8,
        raw_midi_values: &HashMap<String, u8>,
        pw_link_edges: &[(String, String)],
        published_at: std::time::SystemTime,
    ) -> bool {
        let context = EvalContext {
            pw_objects,
            layer_name,
            bank_offset,
            raw_midi_values,
            pw_link_edges,
            published_at,
        };
        evaluate_comparison_with_context(comparison, &context)
    }

    #[test]
    fn test_evaluate_source_muted() {
        let objects = vec![PwObject {
            id: 42,
            object_type: PwObjectType::Sink,
            name: "test_sink".into(),
            description: "Test".into(),
            volume: Some(0.5),
            muted: true,
        }];
        let result = evaluate_source(
            &Source::Muted {
                target: "test_sink".into(),
            },
            &objects,
            "A",
            0,
            &HashMap::new(),
            &[],
            std::time::SystemTime::UNIX_EPOCH,
        );
        assert_eq!(result, "true");

        let result2 = evaluate_source(
            &Source::Muted {
                target: "nonexistent".into(),
            },
            &objects,
            "A",
            0,
            &HashMap::new(),
            &[],
            std::time::SystemTime::UNIX_EPOCH,
        );
        assert_eq!(result2, "false");
    }

    #[test]
    fn test_evaluate_source_volume() {
        let objects = vec![PwObject {
            id: 1,
            object_type: PwObjectType::Sink,
            name: "sink".into(),
            description: "Test".into(),
            volume: Some(0.75),
            muted: false,
        }];
        let result = evaluate_source(
            &Source::Volume {
                target: "sink".into(),
            },
            &objects,
            "A",
            0,
            &HashMap::new(),
            &[],
            std::time::SystemTime::UNIX_EPOCH,
        );
        assert_eq!(result, "0.7500");
    }

    #[test]
    fn test_evaluate_source_blink_clamps_zero_interval() {
        let source = Source::Blink {
            a: Box::new(Source::Direct {
                value: "true".into(),
            }),
            b: Box::new(Source::Direct {
                value: "false".into(),
            }),
            interval_ms: 0,
        };
        let result = evaluate_source(
            &source,
            &[],
            "A",
            0,
            &HashMap::new(),
            &[],
            std::time::SystemTime::UNIX_EPOCH,
        );
        assert_eq!(result, "true");
    }

    #[test]
    fn test_evaluate_comparison_numeric() {
        assert!(evaluate_comparison(
            &Comparison {
                left: Box::new(Source::Direct {
                    value: "0.75".into()
                }),
                op: CmpOp::Gt,
                right: Box::new(Source::Direct {
                    value: "0.5".into()
                }),
            },
            &[],
            "A",
            0,
            &HashMap::new(),
            &[],
            std::time::SystemTime::UNIX_EPOCH
        ));
        assert!(!evaluate_comparison(
            &Comparison {
                left: Box::new(Source::Direct {
                    value: "0.75".into()
                }),
                op: CmpOp::Lt,
                right: Box::new(Source::Direct {
                    value: "0.5".into()
                }),
            },
            &[],
            "A",
            0,
            &HashMap::new(),
            &[],
            std::time::SystemTime::UNIX_EPOCH
        ));
    }

    #[test]
    fn test_evaluate_comparison_eq_tolerance() {
        assert!(evaluate_comparison(
            &Comparison {
                left: Box::new(Source::Direct {
                    value: "0.5004".into()
                }),
                op: CmpOp::Eq,
                right: Box::new(Source::Direct {
                    value: "0.5".into()
                }),
            },
            &[],
            "A",
            0,
            &HashMap::new(),
            &[],
            std::time::SystemTime::UNIX_EPOCH
        ));
        assert!(!evaluate_comparison(
            &Comparison {
                left: Box::new(Source::Direct {
                    value: "0.51".into()
                }),
                op: CmpOp::Eq,
                right: Box::new(Source::Direct {
                    value: "0.5".into()
                }),
            },
            &[],
            "A",
            0,
            &HashMap::new(),
            &[],
            std::time::SystemTime::UNIX_EPOCH
        ));
    }

    #[test]
    fn test_evaluate_comparison_string_fallback() {
        assert!(evaluate_comparison(
            &Comparison {
                left: Box::new(Source::Direct {
                    value: "apple".into()
                }),
                op: CmpOp::Lt,
                right: Box::new(Source::Direct {
                    value: "banana".into()
                }),
            },
            &[],
            "A",
            0,
            &HashMap::new(),
            &[],
            std::time::SystemTime::UNIX_EPOCH
        ));
        assert!(evaluate_comparison(
            &Comparison {
                left: Box::new(Source::Direct {
                    value: "hello".into()
                }),
                op: CmpOp::Eq,
                right: Box::new(Source::Direct {
                    value: "hello".into()
                }),
            },
            &[],
            "A",
            0,
            &HashMap::new(),
            &[],
            std::time::SystemTime::UNIX_EPOCH
        ));
    }

    #[test]
    fn test_find_pw_object_macro_no_match() {
        let objects = vec![PwObject {
            id: 99,
            object_type: PwObjectType::Sink,
            name: "random_sink".into(),
            description: "Random".into(),
            volume: Some(0.5),
            muted: false,
        }];
        assert!(find_pw_object(&objects, "nonexistent").is_none());
        assert!(find_pw_object(&objects, "99").is_some());
    }

    #[test]
    fn test_find_pw_object_by_name() {
        let objects = vec![PwObject {
            id: 44,
            object_type: PwObjectType::Sink,
            name: "my-sink".into(),
            description: "My Sink".into(),
            volume: None,
            muted: false,
        }];
        assert_eq!(find_pw_object(&objects, "my-sink").map(|o| o.id), Some(44));
        assert_eq!(find_pw_object(&objects, "my").map(|o| o.id), Some(44));
        assert!(find_pw_object(&objects, "media-sink").is_none());
    }

    #[test]
    fn test_visual_value() {
        assert_eq!(visual_value("true"), 1.0);
        assert_eq!(visual_value("1"), 1.0);
        assert_eq!(visual_value("false"), 0.0);
        assert_eq!(visual_value("0"), 0.0);
        assert_eq!(visual_value("0.4400"), 0.44);
        assert_eq!(visual_value("1.5000"), 1.0);
        assert_eq!(visual_value("-1.0"), 0.0);
        assert_eq!(visual_value("nonsense"), 0.0);
    }

    #[test]
    fn test_ring_value() {
        assert_eq!(ring_value(0.0), 0);
        assert_eq!(ring_value(1.0), 127);
        assert_eq!(ring_value(0.5), 64);
        assert_eq!(ring_value(0.69), 88);
        assert_eq!(ring_value(-1.0), 0);
        assert_eq!(ring_value(2.0), 127);
    }

    #[test]
    fn test_interpret_feedback() {
        assert!(matches!(
            interpret_feedback(OutputKind::Led, "true", ""),
            FeedbackResult::Led(true)
        ));
        assert!(matches!(
            interpret_feedback(OutputKind::Led, "1", ""),
            FeedbackResult::Led(true)
        ));
        assert!(matches!(
            interpret_feedback(OutputKind::Led, "false", ""),
            FeedbackResult::Led(false)
        ));
        assert!(matches!(
            interpret_feedback(OutputKind::LedRing, "0.5", ""),
            FeedbackResult::LedRing(v) if (v - 0.5).abs() < 1e-9
        ));
        assert!(matches!(
            interpret_feedback(OutputKind::LedRing, "not-a-number", ""),
            FeedbackResult::LedRing(v) if v == 0.0
        ));
        assert!(matches!(
            interpret_feedback(OutputKind::LedRing, "2.0", ""),
            FeedbackResult::LedRing(v) if v == 1.0
        ));
        assert!(matches!(
            interpret_feedback(OutputKind::LedRingCc, "0.25", ""),
            FeedbackResult::LedRing(v) if (v - 0.25).abs() < 1e-9
        ));
        assert!(matches!(
            interpret_feedback(OutputKind::Scribble, "l1", "l2"),
            FeedbackResult::Scribble(a, b) if a == "l1" && b == "l2"
        ));
    }

    #[test]
    fn test_build_scribble_sysex() {
        let data = build_scribble_sysex("T1", "abc", 3);
        assert_eq!(&data[..5], MCU_SYSEX_HEADER);
        assert_eq!(data[5], MCU_SCRIBBLE_STRIP_CMD);
        assert_eq!(data[6], 0x03);
        assert_eq!(&data[7..14], b"T1     ");
        assert_eq!(&data[14..21], b"abc    ");
        assert_eq!(*data.last().unwrap(), SYSEX_TERMINATOR);
        assert_eq!(data.len(), 22);

        let data2 = build_scribble_sysex("\u{00e9}\u{00e9}", "x", 0);
        assert_eq!(&data2[7..14], b"       ");
        assert_eq!(&data2[14..21], b"x      ");
    }

    #[test]
    fn test_build_led_ring_sysex() {
        let data = build_led_ring_sysex(0.5, 7);
        assert_eq!(&data[..5], MCU_SYSEX_HEADER);
        assert_eq!(data[5], MCU_LED_RING_CMD);
        assert_eq!(data[6], 0x07);
        assert_eq!(data[7], 0x00, "Single mode");
        assert_eq!(data[8], 64);
        assert_eq!(*data.last().unwrap(), SYSEX_TERMINATOR);
        assert_eq!(data.len(), 10);

        // The ring index is masked to 3 bits.
        assert_eq!(build_led_ring_sysex(1.0, 9)[6], 1);
    }

    #[test]
    fn test_evaluate_source_layer_active() {
        let result = evaluate_source(
            &Source::LayerActive { layer: "A".into() },
            &[],
            "A",
            0,
            &HashMap::new(),
            &[],
            std::time::SystemTime::UNIX_EPOCH,
        );
        assert_eq!(result, "true");
        let result = evaluate_source(
            &Source::LayerActive { layer: "B".into() },
            &[],
            "A",
            0,
            &HashMap::new(),
            &[],
            std::time::SystemTime::UNIX_EPOCH,
        );
        assert_eq!(result, "false");
    }

    #[test]
    fn test_evaluate_source_bank() {
        let result = evaluate_source(
            &Source::Bank,
            &[],
            "A",
            8,
            &HashMap::new(),
            &[],
            std::time::SystemTime::UNIX_EPOCH,
        );
        assert_eq!(result, "8");
    }

    #[test]
    fn test_evaluate_source_ring_range_extrapolates() {
        let in_range = Source::RingRange {
            source: Box::new(Source::Direct {
                value: "0.5".into(),
            }),
            min: 10,
            max: 20,
        };
        let result = evaluate_source(
            &in_range,
            &[],
            "A",
            0,
            &HashMap::new(),
            &[],
            std::time::SystemTime::UNIX_EPOCH,
        );
        assert_eq!(result, "0.1181");

        let above = Source::RingRange {
            source: Box::new(Source::Direct {
                value: "2.0".into(),
            }),
            min: 0,
            max: 127,
        };
        let result = evaluate_source(
            &above,
            &[],
            "A",
            0,
            &HashMap::new(),
            &[],
            std::time::SystemTime::UNIX_EPOCH,
        );
        assert_eq!(result, "2.0000");
    }

    #[test]
    fn test_evaluate_source_hardware_input() {
        let mut raw_midi_values = HashMap::new();
        raw_midi_values.insert("turn".to_string(), 64);
        let result = evaluate_source(
            &Source::HardwareInput {
                input_label: "turn".into(),
            },
            &[],
            "A",
            0,
            &raw_midi_values,
            &[],
            std::time::SystemTime::UNIX_EPOCH,
        );
        assert_eq!(result, "0.5039");
    }

    #[test]
    fn test_feedback_source_if_then_else() {
        let source = Source::If {
            condition: Comparison {
                left: Box::new(Source::LayerActive { layer: "A".into() }),
                op: CmpOp::Eq,
                right: Box::new(Source::Direct {
                    value: "true".into(),
                }),
            },
            then_source: Box::new(Source::Direct {
                value: "true".to_string(),
            }),
            else_source: Box::new(Source::Direct {
                value: "false".to_string(),
            }),
        };
        let line1 = evaluate_source(
            &source,
            &[],
            "A",
            0,
            &HashMap::new(),
            &[],
            std::time::SystemTime::UNIX_EPOCH,
        );
        assert_eq!(line1, "true");
        let line1_b = evaluate_source(
            &source,
            &[],
            "B",
            0,
            &HashMap::new(),
            &[],
            std::time::SystemTime::UNIX_EPOCH,
        );
        assert_eq!(line1_b, "false");
        assert!(matches!(
            interpret_feedback(OutputKind::Led, &line1, ""),
            FeedbackResult::Led(true)
        ));
        assert!(matches!(
            interpret_feedback(OutputKind::Led, &line1_b, ""),
            FeedbackResult::Led(false)
        ));
    }

    #[test]
    fn test_node_matches() {
        assert!(node_matches("alsa_output.usb-sink", "alsa_output.usb-sink"));
        assert!(node_matches("alsa_output.usb-sink:extra", "sink"));
        assert!(!node_matches("firefox", "spotify"));
    }

    #[test]
    fn test_route_is_active_with_edges() {
        let objects = vec![
            PwObject {
                id: 1,
                object_type: PwObjectType::Stream,
                name: "firefox".into(),
                description: "Firefox".into(),
                volume: None,
                muted: false,
            },
            PwObject {
                id: 2,
                object_type: PwObjectType::Sink,
                name: "alsa_output.hdmi".into(),
                description: "HDMI".into(),
                volume: Some(0.5),
                muted: false,
            },
        ];
        let edges = vec![("firefox".to_string(), "alsa_output.hdmi".to_string())];

        assert!(route_is_active_with_edges(
            edges.as_slice(),
            objects.as_slice(),
            "firefox",
            "alsa_output.hdmi"
        ));
        assert!(!route_is_active_with_edges(
            edges.as_slice(),
            objects.as_slice(),
            "firefox",
            "other_sink"
        ));
        assert!(!route_is_active_with_edges(
            &[],
            objects.as_slice(),
            "firefox",
            "alsa_output.hdmi"
        ));
    }

    #[test]
    fn test_recv_drains_feedback_and_visuals_together() {
        let bus = FeedbackBus::new();
        bus.send_feedback(FeedbackEval {
            env: env(),
            jobs: Vec::new(),
        });
        bus.send_visuals(VisualEval {
            env: env(),
            jobs: Vec::new(),
        });

        let request = bus.recv().expect("bus should not be shut down");
        assert!(request.feedback.is_some());
        assert!(request.visuals.is_some());
    }

    fn env() -> EvalEnv {
        EvalEnv {
            pw_objects: Arc::new(Vec::new()),
            pw_link_edges: Arc::new(Vec::new()),
            published_at: std::time::SystemTime::now(),
        }
    }

    fn feedback_eval(jobs: Vec<FeedbackJob>) -> FeedbackEval {
        FeedbackEval { env: env(), jobs }
    }

    fn visual_eval(jobs: Vec<VisualJob>) -> VisualEval {
        VisualEval { env: env(), jobs }
    }

    #[test]
    fn test_feedback_bus_shutdown_wakes_receiver() {
        let bus = FeedbackBus::new();
        let waiting = Arc::clone(&bus);
        let receiver = std::thread::spawn(move || waiting.recv().is_none());
        std::thread::sleep(Duration::from_millis(10));
        bus.shutdown();
        assert!(receiver.join().expect("receiver should exit"));
    }

    #[test]
    fn test_worker_processes_visuals_despite_feedback_flood() {
        let bus = FeedbackBus::new();
        let (out_tx, out_rx) = std::sync::mpsc::channel();
        let worker_bus = Arc::clone(&bus);
        std::thread::spawn(move || feedback_worker(worker_bus, out_tx));

        // Keep feedback evals arriving while the worker is blocked in a slow
        // custom command, mirroring the app's 50 ms PipeWire publish loop.
        let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let flood_bus = Arc::clone(&bus);
        let running_flood = Arc::clone(&running);
        let flood = std::thread::spawn(move || {
            while running_flood.load(std::sync::atomic::Ordering::Relaxed) {
                flood_bus.send_feedback(feedback_eval(vec![FeedbackJob {
                    base: JobBase {
                        device: "dev".into(),
                        hardware_id: "hw".into(),
                        layer_name: "layer".into(),
                        bank_offset: 0,
                        raw_midi_values: Arc::new(HashMap::new()),
                    },
                    output_label: "out".into(),
                    output: HardwareOutput {
                        label: "out".into(),
                        kind: OutputKind::Led,
                        channel: 0,
                        number: 0,
                    },
                    source: Source::Custom {
                        cmd: "sleep 0.2".into(),
                    },
                    line2_source: None,
                }]));
                std::thread::sleep(Duration::from_millis(10));
            }
        });

        bus.send_visuals(visual_eval(vec![VisualJob {
            base: JobBase {
                device: "dev".into(),
                hardware_id: "hw".into(),
                layer_name: "layer".into(),
                bank_offset: 0,
                raw_midi_values: Arc::new(HashMap::new()),
            },
            source: Source::Direct {
                value: "true".into(),
            },
        }]));

        let deadline = Instant::now() + Duration::from_secs(3);
        let mut saw_visuals = false;
        while Instant::now() < deadline {
            match out_rx.recv_timeout(Duration::from_millis(500)) {
                Ok(FeedbackOutput::Visuals(results)) => {
                    assert_eq!(results.len(), 1);
                    assert_eq!(results[0].value, 1.0);
                    saw_visuals = true;
                    break;
                }
                Ok(FeedbackOutput::Feedback(_)) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        running.store(false, std::sync::atomic::Ordering::Relaxed);
        let _ = flood.join();
        assert!(
            saw_visuals,
            "visuals eval was starved by the feedback flood"
        );
    }
}
