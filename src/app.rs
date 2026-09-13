use std::collections::{HashMap, HashSet};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use iced::keyboard;
use iced::{Element, Subscription, Task, Theme};

use crate::actions::{EncoderMode, VolumeMode};
use crate::config::{Config, ControlRef, DeviceConfig, HardwareDef, LayerDef};
use crate::midi::{MidiEvent, MidiManager, MidiOutputHandle};
use crate::pipewire::{PwCommand, PwEvent, PwObject};
use crate::tray::{self, TrayMessage};
use crate::ui;

mod dispatch;
mod feedback;
mod state;
mod ui_handlers;

use feedback::{feedback_worker, FeedbackBus, FeedbackOutput, FeedbackResult};

/// How long a volume set by the app is protected from stale PipeWire refresh values.
const VOLUME_FIGHT_WINDOW: Duration = Duration::from_millis(500);
/// Debounce before persisting a dirty config, measured from the last edit.
const CONFIG_SAVE_DEBOUNCE: Duration = Duration::from_secs(5);
/// Maximum time unsaved edits may accumulate while editing continuously.
const CONFIG_SAVE_PERIOD: Duration = Duration::from_secs(300);

fn user_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string())
}

pub struct App {
    pub config: Config,
    pub midi_manager: MidiManager,
    pub midi_rx: mpsc::Receiver<MidiEvent>,
    pub pw_rx: mpsc::Receiver<PwEvent>,
    pub tray_rx: mpsc::Receiver<TrayMessage>,
    pub output_handles: HashMap<String, MidiOutputHandle>,
    pub system_theme: Theme,
    /// The main window id, captured on open so the tray can show/hide it.
    window_id: Option<iced::window::Id>,

    /// Selected device by its port name (the key in Config.devices).
    pub selected_device_name: Option<String>,
    pub selected_layer: Option<String>,
    /// Active layer per device (device name → layer name), used for MIDI event
    /// dispatch, bank actions, and feedback. Distinct from `selected_layer`,
    /// which is what the UI shows for the selected device.
    pub active_layers: HashMap<String, String>,
    pub show_add_dialog: bool,
    pub add_dialog_state: ui::add_dialog::AddDeviceDialog,
    pub new_layer_name: String,

    /// Index of the icon being edited within the device's shared hardware
    /// list (device-level icons are shared across all layers).
    pub editing_icon_idx: Option<usize>,
    /// Hardware input index currently being targeted by MIDI learn.
    /// `(device name, hardware id, input index)`: the device is recorded so
    /// learn writes to the device being edited even if the selection changes.
    pub learn_target: Option<(String, String, usize)>,

    pub connected_devices: HashSet<String>,
    pub pw_objects: Arc<Vec<PwObject>>,
    pub pw_connected: bool,
    pub status_message: String,
    pub midi_monitor: Vec<String>,

    /// Faceplate visual value per icon (keyed by (device, hardware id)),
    /// evaluated from each icon's `visual_source` on the feedback worker.
    pub visual_values: HashMap<(String, String), f64>,

    /// The last raw MIDI value received per hardware input, keyed by ControlRef.
    /// Used to populate `raw_midi_values` when building feedback/visual jobs.
    last_input_value: HashMap<ControlRef, u8>,

    /// Whether config has unsaved changes.
    dirty: bool,
    /// Timestamp of the last config edit, used to debounce saves.
    last_edit: Instant,
    /// Timestamp of the last save, used to bound how long continuous edits go unsaved.
    last_periodic_save: Instant,
    last_hotplug_check: Instant,
    /// Set once the MIDI event channel is observed disconnected, to avoid logging on every tick.
    midi_rx_disconnected: bool,
    /// Same, for the PipeWire event channel.
    pw_rx_disconnected: bool,
    /// Last feedback value sent per control, used to skip redundant writes.
    last_feedback_result: HashMap<ControlRef, FeedbackResult>,
    /// Timestamp of last volume set per target ID.
    last_volume_set: HashMap<String, Instant>,
    /// Last volume value set per target ID, used as the basis for
    /// subsequent relative encoder changes during the fight window.
    last_set_volume: HashMap<String, f64>,
    /// Feedback/visual evaluation worker: latest-snapshot-wins bus, results
    /// come back via `feedback_rx` on each tick.
    feedback_bus: Arc<FeedbackBus>,
    feedback_rx: mpsc::Receiver<FeedbackOutput>,
    feedback_thread: Option<std::thread::JoinHandle<()>>,
    /// Sender for native PipeWire commands (volume/mute/routing).
    pw_cmd_tx: mpsc::Sender<PwCommand>,
    pw_thread: Option<std::thread::JoinHandle<()>>,
    /// Most recent routing edges from the PipeWire snapshot.
    pw_edges: Arc<Vec<(String, String)>>,
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = self.pw_cmd_tx.send(PwCommand::Shutdown);
        self.feedback_bus.shutdown();
        if let Some(thread) = self.feedback_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.pw_thread.take() {
            let _ = thread.join();
        }
    }
}

struct VolumeParams {
    min: f64,
    max: f64,
    mode: VolumeMode,
    encoder: EncoderMode,
    step: f64,
    raw: u8,
}

/// Compute the target volume for a raw MIDI value. Relative mode returns
/// `None` when the current volume is unknown, so callers skip the command
/// rather than snapping the volume toward zero.
fn apply_volume(p: &VolumeParams, current: Option<f64>) -> Option<f64> {
    if !p.min.is_finite()
        || !p.max.is_finite()
        || !p.step.is_finite()
        || p.min < 0.0
        || p.max > 2.0
        || p.min > p.max
        || p.step < 0.0
    {
        return None;
    }
    match p.mode {
        VolumeMode::Absolute => {
            let range = p.max - p.min;
            Some(p.min + (p.raw as f64 / 127.0) * range)
        }
        VolumeMode::Relative => {
            let delta = encoder_delta(p.raw, p.encoder) as f64 * p.step;
            let current = current?;
            Some((current + delta).clamp(p.min, p.max))
        }
    }
}

#[derive(Debug, Clone)]
pub enum AppMessage {
    Ui(ui::Message),
    Tick,
    WindowOpened(iced::window::Id),
    CloseRequested(iced::window::Id),
}

impl App {
    pub fn new() -> (Self, Task<AppMessage>) {
        let config = match Config::load() {
            Ok(c) => c,
            Err(e) => {
                log::error!("Failed to load config: {e}");
                Config::default()
            }
        };

        let (mut midi_manager, midi_rx) = MidiManager::new().unwrap_or_else(|e| {
            log::error!("Failed to init MIDI: {e}");
            (MidiManager::dead(), mpsc::channel().1)
        });

        let (pw_event_tx, pw_rx) = mpsc::channel();
        let (pw_cmd_tx, pw_cmd_rx) = mpsc::channel();
        let pw_thread = Some(crate::pipewire::spawn(pw_event_tx, pw_cmd_rx));

        let feedback_bus = FeedbackBus::new();
        let (feedback_result_tx, feedback_rx) = mpsc::channel();
        let feedback_bus_worker = Arc::clone(&feedback_bus);
        let feedback_thread = Some(std::thread::spawn(move || {
            feedback_worker(feedback_bus_worker, feedback_result_tx)
        }));

        let mut active_layers = HashMap::new();
        let mut output_handles = HashMap::new();
        for name in config.device_names() {
            let device = config.devices.get(name).unwrap();
            if let Some(first) = device.layers.first() {
                active_layers.insert(name.clone(), first.name.clone());
            }
            match midi_manager.connect(&device.port_name) {
                Ok(()) => {}
                Err(e) => log::warn!("Failed to connect to {name}: {e}"),
            }
            match midi_manager.connect_output(&device.port_name) {
                Ok(h) => {
                    output_handles.insert(name.clone(), h);
                }
                Err(e) => log::debug!("No output for {name}: {e}"),
            }
        }

        let tray_rx = tray::spawn("midi-does".into());

        (
            App {
                config,
                midi_manager,
                midi_rx,
                pw_rx,
                tray_rx,
                output_handles,
                system_theme: detect_system_theme(),
                window_id: None,
                selected_device_name: None,
                selected_layer: None,
                active_layers,
                show_add_dialog: false,
                add_dialog_state: ui::add_dialog::AddDeviceDialog::default(),
                new_layer_name: String::new(),
                editing_icon_idx: None,
                learn_target: None,
                connected_devices: HashSet::new(),
                pw_objects: Arc::new(Vec::new()),
                pw_connected: false,
                status_message: "Starting...".into(),
                midi_monitor: Vec::new(),
                visual_values: HashMap::new(),
                last_input_value: HashMap::new(),
                dirty: false,
                last_edit: Instant::now(),
                last_periodic_save: Instant::now(),
                last_hotplug_check: Instant::now(),
                midi_rx_disconnected: false,
                pw_rx_disconnected: false,
                last_feedback_result: HashMap::new(),
                last_volume_set: HashMap::new(),
                last_set_volume: HashMap::new(),
                feedback_bus,
                feedback_rx,
                feedback_thread,
                pw_cmd_tx,
                pw_thread,
                pw_edges: Arc::new(Vec::new()),
            },
            Task::none(),
        )
    }

    pub fn title(&self) -> String {
        "midi-does".into()
    }

    pub fn theme(&self) -> Theme {
        self.system_theme.clone()
    }

    pub fn update(&mut self, message: AppMessage) -> Task<AppMessage> {
        match message {
            AppMessage::Ui(msg) => self.handle_ui_message(msg),
            AppMessage::WindowOpened(id) => {
                self.window_id = Some(id);
                Task::none()
            }
            AppMessage::CloseRequested(id) => {
                self.window_id = Some(id);
                if std::env::var_os("DISPLAY").is_some() {
                    iced::window::set_mode(id, iced::window::Mode::Hidden)
                } else {
                    iced::window::minimize(id, true)
                }
            }
            AppMessage::Tick => {
                self.poll_events();
                self.poll_worker_output();
                let tray_task = self.poll_tray();
                self.poll_hotplug();
                let debounce_elapsed = self.last_edit.elapsed() > CONFIG_SAVE_DEBOUNCE;
                let periodic_elapsed = self.last_periodic_save.elapsed() > CONFIG_SAVE_PERIOD;
                if self.dirty && (debounce_elapsed || periodic_elapsed) {
                    self.save_config();
                }
                tray_task
            }
        }
    }

    pub fn view(&self) -> Element<'_, AppMessage> {
        ui::view(self)
    }

    pub fn subscription(&self) -> Subscription<AppMessage> {
        Subscription::batch([
            iced::time::every(Duration::from_millis(33)).map(|_| AppMessage::Tick),
            iced::keyboard::listen().filter_map(|event| match event {
                keyboard::Event::KeyPressed { key, modifiers, .. } => {
                    Some(AppMessage::Ui(ui::Message::KeyPress(key, modifiers)))
                }
                _ => None,
            }),
            iced::window::open_events().map(AppMessage::WindowOpened),
            iced::window::close_requests().map(AppMessage::CloseRequested),
        ])
    }
}

pub(crate) fn encoder_delta(raw: u8, mode: EncoderMode) -> i8 {
    match mode {
        EncoderMode::SignMagnitude => {
            let sign = (raw & 0x40) != 0;
            let mag = (raw & 0x3F) as i8;
            if sign {
                -mag
            } else {
                mag
            }
        }
        EncoderMode::TwosComplement => {
            if raw < 64 {
                raw as i8
            } else {
                -((128 - raw) as i8)
            }
        }
        EncoderMode::BinaryOffset => (raw as i8) - 64,
    }
}

fn detect_system_theme() -> Theme {
    match dark_light::detect() {
        Ok(dark_light::Mode::Dark) => Theme::Dark,
        Ok(_) => Theme::Light,
        Err(_) => Theme::Dark,
    }
}

fn next_free_id(prefix: &str, is_taken: impl Fn(&str) -> bool) -> String {
    (0..)
        .map(|i| format!("{prefix}{i}"))
        .find(|id| !is_taken(id))
        .expect("id space exhausted")
}

fn take_at<T>(list: &mut Vec<T>, idx: usize) -> Option<T> {
    (idx < list.len()).then(|| list.remove(idx))
}

fn next_hardware_id(device: &DeviceConfig) -> String {
    next_free_id("hw", |id| device.hardware.iter().any(|h| h.id == id))
}

fn next_input_label(hw: &HardwareDef) -> String {
    next_free_id("in", |label| hw.inputs.iter().any(|i| i.label == label))
}

fn next_output_label(hw: &HardwareDef) -> String {
    next_free_id("out", |label| hw.outputs.iter().any(|o| o.label == label))
}

fn valid_layer_rename(layers: &[LayerDef], old: &str, new: &str) -> bool {
    if new.trim().is_empty() {
        return false;
    }
    if old == new {
        return true;
    }
    !layers.iter().any(|l| l.name == new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{InputKind, OutputKind};

    /// #feature ACT-ENC — encoder_delta SignMagnitude
    #[test]
    fn test_encoder_delta_sign_magnitude() {
        assert_eq!(encoder_delta(0x41, EncoderMode::SignMagnitude), -1);
        assert_eq!(encoder_delta(0x01, EncoderMode::SignMagnitude), 1);
        assert_eq!(encoder_delta(0x00, EncoderMode::SignMagnitude), 0);
        assert_eq!(encoder_delta(0x7F, EncoderMode::SignMagnitude), -63);
        assert_eq!(encoder_delta(0x3F, EncoderMode::SignMagnitude), 63);
    }

    /// #feature ACT-ENC — encoder_delta TwosComplement
    #[test]
    fn test_encoder_delta_twos_complement() {
        assert_eq!(encoder_delta(0, EncoderMode::TwosComplement), 0);
        assert_eq!(encoder_delta(1, EncoderMode::TwosComplement), 1);
        assert_eq!(encoder_delta(63, EncoderMode::TwosComplement), 63);
        assert_eq!(encoder_delta(64, EncoderMode::TwosComplement), -64);
        assert_eq!(encoder_delta(127, EncoderMode::TwosComplement), -1);
        assert_eq!(encoder_delta(65, EncoderMode::TwosComplement), -63);
    }

    /// #feature ACT-ENC — encoder_delta BinaryOffset
    #[test]
    fn test_encoder_delta_binary_offset() {
        assert_eq!(encoder_delta(64, EncoderMode::BinaryOffset), 0);
        assert_eq!(encoder_delta(65, EncoderMode::BinaryOffset), 1);
        assert_eq!(encoder_delta(127, EncoderMode::BinaryOffset), 63);
        assert_eq!(encoder_delta(63, EncoderMode::BinaryOffset), -1);
        assert_eq!(encoder_delta(0, EncoderMode::BinaryOffset), -64);
    }

    /// #feature ACT-VOL — relative mode with an unknown current volume is a
    /// no-op instead of snapping toward zero.
    #[test]
    fn test_apply_volume_relative_unknown_is_none() {
        let p = VolumeParams {
            min: 0.0,
            max: 1.0,
            mode: VolumeMode::Relative,
            encoder: EncoderMode::SignMagnitude,
            step: 0.01,
            raw: 0x01,
        };
        assert_eq!(apply_volume(&p, None), None);
    }

    /// #feature ACT-VOL — relative mode applies a delta to the known current
    /// volume and clamps to the configured range.
    #[test]
    fn test_apply_volume_relative_uses_current() {
        let p = VolumeParams {
            min: 0.0,
            max: 1.0,
            mode: VolumeMode::Relative,
            encoder: EncoderMode::SignMagnitude,
            step: 0.01,
            raw: 0x01,
        };
        let vol = apply_volume(&p, Some(0.5)).expect("known current yields a volume");
        assert!((vol - 0.51).abs() < 1e-9);
    }

    /// #feature ACT-VOL — absolute mode maps the raw value into the range.
    #[test]
    fn test_apply_volume_absolute() {
        let p = VolumeParams {
            min: 0.0,
            max: 1.0,
            mode: VolumeMode::Absolute,
            encoder: EncoderMode::SignMagnitude,
            step: 0.01,
            raw: 64,
        };
        let vol = apply_volume(&p, None).expect("absolute always yields a volume");
        assert!((vol - 64.0 / 127.0).abs() < 1e-9);
    }

    #[test]
    fn test_apply_volume_rejects_invalid_range() {
        let p = VolumeParams {
            min: 1.0,
            max: 0.5,
            mode: VolumeMode::Relative,
            encoder: EncoderMode::SignMagnitude,
            step: 0.01,
            raw: 1,
        };
        assert_eq!(apply_volume(&p, Some(0.75)), None);
    }

    #[test]
    fn test_apply_volume_supports_amplification() {
        let p = VolumeParams {
            min: 0.0,
            max: 2.0,
            mode: VolumeMode::Absolute,
            encoder: EncoderMode::SignMagnitude,
            step: 0.01,
            raw: 127,
        };
        assert_eq!(apply_volume(&p, None), Some(2.0));
    }

    #[test]
    fn test_next_hardware_id() {
        let device = DeviceConfig {
            port_name: "test".into(),
            grid_columns: 8,
            grid_rows: 4,
            hardware: vec![
                HardwareDef {
                    id: "hw0".into(),
                    hw_type: crate::config::IconType::Button,
                    col: 0,
                    row: 0,
                    col_span: 1,
                    row_span: 1,
                    inputs: Vec::new(),
                    outputs: Vec::new(),
                },
                HardwareDef {
                    id: "hw2".into(),
                    hw_type: crate::config::IconType::Button,
                    col: 0,
                    row: 0,
                    col_span: 1,
                    row_span: 1,
                    inputs: Vec::new(),
                    outputs: Vec::new(),
                },
            ],
            layers: Vec::new(),
        };
        assert_eq!(next_hardware_id(&device), "hw1");

        let empty = DeviceConfig {
            port_name: "test".into(),
            grid_columns: 8,
            grid_rows: 4,
            hardware: Vec::new(),
            layers: Vec::new(),
        };
        assert_eq!(next_hardware_id(&empty), "hw0");
    }

    #[test]
    fn test_next_input_output_label() {
        let hw = HardwareDef {
            id: "hw0".into(),
            hw_type: crate::config::IconType::Button,
            col: 0,
            row: 0,
            col_span: 1,
            row_span: 1,
            inputs: vec![
                crate::config::HardwareInput {
                    label: "in0".into(),
                    kind: InputKind::Cc,
                    channel: 0,
                    number: 0,
                },
                crate::config::HardwareInput {
                    label: "volume".into(),
                    kind: InputKind::Cc,
                    channel: 0,
                    number: 1,
                },
            ],
            outputs: vec![crate::config::HardwareOutput {
                label: "out0".into(),
                kind: OutputKind::Led,
                channel: 0,
                number: 0,
            }],
        };
        assert_eq!(next_input_label(&hw), "in1");
        assert_eq!(next_output_label(&hw), "out1");
    }

    #[test]
    fn test_valid_layer_rename() {
        let layers = vec![
            LayerDef {
                name: "A".into(),
                bank_offset: 0,
                icons: Vec::new(),
            },
            LayerDef {
                name: "B".into(),
                bank_offset: 0,
                icons: Vec::new(),
            },
        ];
        assert!(valid_layer_rename(&layers, "A", "C"));
        assert!(valid_layer_rename(&layers, "A", "A"));
        assert!(!valid_layer_rename(&layers, "A", "B"));
        assert!(!valid_layer_rename(&layers, "A", ""));
        assert!(!valid_layer_rename(&layers, "A", "  "));
    }
}
