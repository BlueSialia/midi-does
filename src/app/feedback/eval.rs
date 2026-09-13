use std::collections::HashMap;
use std::io::Read;
use std::process::Command as ShellCommand;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::app::user_shell;
use crate::config::{CmpOp, Comparison, OutputKind, Source, MIN_BLINK_INTERVAL_MS};
use crate::pipewire::PwObject;

use super::{
    find_pw_object, route_is_active_with_edges, FeedbackResult, MCU_LED_RING_CMD,
    MCU_SCRIBBLE_STRIP_CMD, MCU_SYSEX_HEADER, SYSEX_TERMINATOR,
};

const VOLUME_EQ_EPSILON: f64 = 0.001;
const CUSTOM_SOURCE_TIMEOUT: Duration = Duration::from_secs(1);
/// Cap on captured stdout from a `Source::Custom` command so a runaway
/// command cannot exhaust memory before it is killed.
const MAX_CUSTOM_OUTPUT_BYTES: u64 = 64 * 1024;

pub(super) struct EvalContext<'a> {
    pub(super) pw_objects: &'a [PwObject],
    pub(super) layer_name: &'a str,
    pub(super) bank_offset: u8,
    pub(super) raw_midi_values: &'a HashMap<String, u8>,
    pub(super) pw_link_edges: &'a [(String, String)],
    pub(super) published_at: std::time::SystemTime,
}

fn is_true(text: &str) -> bool {
    text == "true" || text == "1"
}

fn as_normalized(text: &str) -> f64 {
    text.parse::<f64>().unwrap_or(0.0).clamp(0.0, 1.0)
}

pub(super) fn visual_value(text: &str) -> f64 {
    if is_true(text) {
        1.0
    } else {
        as_normalized(text)
    }
}

pub(super) fn interpret_feedback(kind: OutputKind, line1: &str, line2: &str) -> FeedbackResult {
    match kind {
        OutputKind::Led => FeedbackResult::Led(is_true(line1)),
        OutputKind::LedRing | OutputKind::LedRingCc => {
            FeedbackResult::LedRing(as_normalized(line1))
        }
        OutputKind::Scribble => FeedbackResult::Scribble(line1.to_string(), line2.to_string()),
    }
}

fn kill_process_group(pid: u32) {
    unsafe {
        libc::killpg(pid as libc::pid_t, libc::SIGKILL);
    }
}

fn run_custom_with_timeout(cmd: &str, timeout: Duration) -> String {
    use std::os::unix::process::CommandExt;

    let mut child = match ShellCommand::new(user_shell())
        .arg("-c")
        .arg(cmd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return String::new(),
    };

    let Some(mut stdout) = child.stdout.take() else {
        return String::new();
    };

    let output = Arc::new(Mutex::new(Vec::<u8>::new()));
    let output_shared = Arc::clone(&output);
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut stdout)
            .take(MAX_CUSTOM_OUTPUT_BYTES)
            .read_to_end(&mut buf);
        *output_shared.lock().unwrap() = buf;
    });

    let deadline = Instant::now() + timeout;
    let mut exited = false;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                exited = true;
                break;
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(_) => break,
        }
    }

    // Always kill the whole process group before joining the reader. After a
    // timeout this terminates a runaway command; after a normal exit it reaps
    // any grandchild that inherited stdout, so the reader's `read_to_end`
    // reaches EOF instead of blocking forever.
    kill_process_group(child.id());
    let _ = child.wait();

    let _ = reader.join();
    if !exited {
        return String::new();
    }
    let bytes = output.lock().unwrap();
    String::from_utf8_lossy(bytes.as_slice()).trim().to_string()
}

pub(super) fn evaluate_source_with_context(source: &Source, context: &EvalContext<'_>) -> String {
    match source {
        Source::Direct { value } => value.clone(),
        Source::HardwareInput { input_label } => {
            let raw = context
                .raw_midi_values
                .get(input_label)
                .copied()
                .unwrap_or(0) as f64;
            format!("{:.4}", raw / 127.0)
        }
        Source::Volume { target } => find_pw_object(context.pw_objects, target)
            .and_then(|o| o.volume)
            .map(|v| format!("{:.4}", v))
            .unwrap_or_else(|| "0.0".to_string()),
        Source::Muted { target } => {
            let muted = find_pw_object(context.pw_objects, target).is_some_and(|o| o.muted);
            (if muted { "true" } else { "false" }).to_string()
        }
        Source::RouteActive { stream, sink } => {
            let active =
                route_is_active_with_edges(context.pw_link_edges, context.pw_objects, stream, sink);
            (if active { "true" } else { "false" }).to_string()
        }
        Source::LayerActive { layer } => (if layer == context.layer_name {
            "true"
        } else {
            "false"
        })
        .to_string(),
        Source::Bank => context.bank_offset.to_string(),
        Source::Custom { cmd } => run_custom_with_timeout(cmd, CUSTOM_SOURCE_TIMEOUT),
        Source::If {
            condition,
            then_source,
            else_source,
        } => {
            let cond_true = evaluate_comparison_with_context(condition, context);
            if cond_true {
                evaluate_source_with_context(then_source, context)
            } else {
                evaluate_source_with_context(else_source, context)
            }
        }
        Source::RingRange { source, min, max } => {
            let inner = evaluate_source_with_context(source, context);
            let val: f64 = inner.parse::<f64>().unwrap_or(0.0);
            let range = max.saturating_sub(*min) as f64;
            let mapped = *min as f64 + val * range;
            format!("{:.4}", mapped / 127.0)
        }
        Source::Blink { a, b, interval_ms } => {
            let interval_ms = (*interval_ms).max(MIN_BLINK_INTERVAL_MS);
            let phase = context
                .published_at
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64
                % (2 * interval_ms);
            if phase < interval_ms {
                evaluate_source_with_context(a, context)
            } else {
                evaluate_source_with_context(b, context)
            }
        }
    }
}

pub(super) fn evaluate_comparison_with_context(
    comparison: &Comparison,
    context: &EvalContext<'_>,
) -> bool {
    let left = evaluate_source_with_context(&comparison.left, context);
    let right = evaluate_source_with_context(&comparison.right, context);

    if let (Ok(la), Ok(ra)) = (left.parse::<f64>(), right.parse::<f64>()) {
        match comparison.op {
            CmpOp::Lt => la < ra,
            CmpOp::Lte => la <= ra,
            CmpOp::Gt => la > ra,
            CmpOp::Gte => la >= ra,
            CmpOp::Eq => (la - ra).abs() < VOLUME_EQ_EPSILON,
        }
    } else {
        match comparison.op {
            CmpOp::Lt => left < right,
            CmpOp::Lte => left <= right,
            CmpOp::Gt => left > right,
            CmpOp::Gte => left >= right,
            CmpOp::Eq => left == right,
        }
    }
}

/// Build an MCU LED ring SysEx message (Single mode): header, command, ring
pub(super) fn ring_value(normalized: f64) -> u8 {
    (normalized.clamp(0.0, 1.0) * 127.0).round() as u8
}

/// Build an MCU LED ring SysEx message (Single mode): header, command, ring
/// index, mode, value, and terminator.
pub(super) fn build_led_ring_sysex(normalized: f64, ring: u8) -> Vec<u8> {
    let mut data = MCU_SYSEX_HEADER.to_vec();
    data.extend_from_slice(&[
        MCU_LED_RING_CMD,
        ring & 0x07,
        0x00, // Single mode
        ring_value(normalized),
        SYSEX_TERMINATOR,
    ]);
    data
}

/// Build an MCU scribble strip SysEx message: header, command, two padded
/// 7-character lines, and the terminator.
pub(super) fn build_scribble_sysex(line1: &str, line2: &str, strip: u8) -> Vec<u8> {
    let mut data: Vec<u8> = MCU_SYSEX_HEADER
        .iter()
        .copied()
        .chain([MCU_SCRIBBLE_STRIP_CMD, strip & 0x07])
        .collect();
    for line in [line1, line2] {
        let sanitized: String = line
            .chars()
            .take(7)
            .map(|c| if c.is_ascii() { c } else { ' ' })
            .collect();
        data.extend(format!("{:<7}", sanitized).bytes());
    }
    data.push(SYSEX_TERMINATOR);
    data
}
