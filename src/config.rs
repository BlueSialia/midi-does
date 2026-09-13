use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::actions::Action;
use crate::midi::stable_port_key;

mod source;

pub use source::{CmpOp, Comparison, Source, SourceBranch};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ControlRef {
    pub device: String,
    pub hardware: String,
    pub label: String,
}

impl ControlRef {
    pub fn new(device: &str, hardware: &str, label: &str) -> Self {
        Self {
            device: device.to_string(),
            hardware: hardware.to_string(),
            label: label.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Config {
    #[serde(default)]
    pub devices: HashMap<String, DeviceConfig>,
    /// Stable insertion order for device list display and index-based access.
    #[serde(default)]
    pub(crate) device_order: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceConfig {
    pub port_name: String,
    #[serde(default = "default_grid_cols")]
    pub grid_columns: u8,
    #[serde(default = "default_grid_rows")]
    pub grid_rows: u8,
    /// Icon definitions (hardware element + grid placement + inputs/outputs),
    /// shared across all layers.
    #[serde(default)]
    pub hardware: Vec<HardwareDef>,
    #[serde(default = "default_layers")]
    pub layers: Vec<LayerDef>,
}

pub(crate) fn default_layers() -> Vec<LayerDef> {
    vec![LayerDef {
        name: "A".to_string(),
        bank_offset: 0,
        icons: Vec::new(),
    }]
}

pub(crate) fn default_grid_cols() -> u8 {
    8
}
pub(crate) fn default_grid_rows() -> u8 {
    4
}

/// Physical element on a MIDI controller, with its grid placement. Shared
/// across all layers; layers only add per-layer software on top.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HardwareDef {
    pub id: String,
    #[serde(rename = "type")]
    pub hw_type: IconType,
    #[serde(default)]
    pub col: u8,
    #[serde(default)]
    pub row: u8,
    #[serde(default = "default_span")]
    pub col_span: u8,
    #[serde(default = "default_span")]
    pub row_span: u8,
    #[serde(default)]
    pub inputs: Vec<HardwareInput>,
    #[serde(default)]
    pub outputs: Vec<HardwareOutput>,
}

fn default_span() -> u8 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HardwareInput {
    pub label: String,
    pub kind: InputKind,
    pub channel: u8,
    #[serde(default)]
    pub number: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HardwareOutput {
    pub label: String,
    pub kind: OutputKind,
    pub channel: u8,
    #[serde(default)]
    pub number: u8,
}

/// Smallest allowed blink interval. `Source::Blink` evaluation and the UI both
/// clamp to this so the blink phase can never divide by zero.
pub(crate) const MIN_BLINK_INTERVAL_MS: u64 = 100;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    Cc,
    Note,
    PitchBend,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutputKind {
    Led,
    /// LED ring driven by MCU SysEx (channel = ring index 0-7).
    LedRing,
    /// LED ring driven by a standard MIDI CC (channel = MIDI channel,
    /// number = controller, value 0-127).
    LedRingCc,
    /// MCU scribble strip (channel = strip index 0-7).
    Scribble,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayerDef {
    pub name: String,
    #[serde(default)]
    pub bank_offset: u8,
    #[serde(default)]
    pub icons: Vec<LayerIcon>,
}

/// Per-layer software bound to a device-level icon (identified by hardware id).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayerIcon {
    pub hardware: String,
    pub software: SoftwareDef,
}

/// Per-layer software: icon label, signal actions, feedback sources, and
/// visual feedback source. The label lives here so the same icon can show a
/// different name on each layer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SoftwareDef {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub signal_entries: Vec<SignalEntry>,
    #[serde(default)]
    pub feedback_entries: Vec<FeedbackEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual_source: Option<Source>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SignalEntry {
    pub input: String,
    #[serde(default)]
    pub actions: Vec<Action>,
}

/// A source mapped to an output. The output kind determines how the evaluated
/// source text is interpreted: boolean → LED, number → LED ring, text → scribble lines.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FeedbackEntry {
    pub output: String,
    /// Source for LED/ring outputs; line 1 source for scribble outputs.
    pub source: Source,
    /// Line 2 source, only used by scribble outputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line2_source: Option<Source>,
}

impl LayerDef {
    pub fn software(&self, hardware_id: &str) -> Option<&SoftwareDef> {
        self.icons
            .iter()
            .find(|i| i.hardware == hardware_id)
            .map(|i| &i.software)
    }

    /// Per-layer software for the icon with the given hardware id, creating an
    /// empty entry on first access so edits have somewhere to live.
    pub fn software_mut(&mut self, hardware_id: &str) -> &mut SoftwareDef {
        if let Some(idx) = self.icons.iter().position(|i| i.hardware == hardware_id) {
            return &mut self.icons[idx].software;
        }
        self.icons.push(LayerIcon {
            hardware: hardware_id.to_string(),
            software: SoftwareDef::default(),
        });
        &mut self.icons.last_mut().expect("just pushed").software
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum IconType {
    #[default]
    Button,
    Knob,
    Fader,
    Encoder,
}

impl Config {
    pub fn device_names(&self) -> &[String] {
        &self.device_order
    }

    pub fn device_name(&self, idx: usize) -> Option<&str> {
        self.device_order.get(idx).map(|s| s.as_str())
    }

    pub fn device(&self, idx: usize) -> Option<&DeviceConfig> {
        let name = self.device_order.get(idx)?;
        self.devices.get(name)
    }

    pub fn device_mut(&mut self, idx: usize) -> Option<&mut DeviceConfig> {
        let name = self.device_order.get(idx)?.clone();
        self.devices.get_mut(&name)
    }

    pub fn insert_device(&mut self, name: String, device: DeviceConfig) {
        if !self.devices.contains_key(&name) {
            self.device_order.push(name.clone());
        }
        self.devices.insert(name, device);
    }

    pub fn remove_device(&mut self, name: &str) {
        self.devices.remove(name);
        self.device_order.retain(|n| n != name);
    }

    pub fn device_count(&self) -> usize {
        self.device_order.len()
    }

    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("midi-does").join("config.toml"))
    }

    pub fn load() -> Result<Self, LoadError> {
        let path = Self::config_path().ok_or(LoadError::NoConfigDir)?;
        if !path.exists() {
            return Ok(Config::default());
        }
        let content = std::fs::read_to_string(&path).map_err(LoadError::Io)?;
        let mut config: Config = toml::from_str(&content).map_err(LoadError::Parse)?;
        config.normalize();
        Ok(config)
    }

    /// Rebuild `devices`/`device_order` into a canonical form after
    /// deserialization: keep the stored display order, append devices missing
    /// from it in a deterministic order, and replace volatile ALSA
    /// `client_id:port_id` suffixes in port names with the stable
    /// `client_name:port_name` identity, so a device replugged under a new
    /// client ID still resolves to the same config entry.
    fn normalize(&mut self) {
        let mut remaining = std::mem::take(&mut self.devices);

        let ordered: Vec<DeviceConfig> = std::mem::take(&mut self.device_order)
            .into_iter()
            .filter_map(|name| remaining.remove(&name))
            .collect();
        let mut missing: Vec<DeviceConfig> = remaining.into_values().collect();
        missing.sort_by(|a, b| a.port_name.cmp(&b.port_name));

        let mut devices = HashMap::new();
        let mut order = Vec::new();
        for mut device in ordered.into_iter().chain(missing) {
            let key = stable_port_key(&device.port_name).to_string();
            device.port_name = key.clone();
            if devices.insert(key.clone(), device).is_none() {
                order.push(key);
            } else {
                log::warn!("Device '{key}' maps to a duplicate identity; keeping the last one");
            }
        }

        self.devices = devices;
        self.device_order = order;
    }

    pub fn save(&self) -> Result<(), SaveError> {
        let path = Self::config_path().ok_or(SaveError::NoConfigDir)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(SaveError::Io)?;
        }
        let content = toml::to_string_pretty(self).map_err(SaveError::Serialize)?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, &content).map_err(SaveError::Io)?;
        std::fs::rename(&tmp, &path).map_err(SaveError::Io)?;
        Ok(())
    }
}

impl std::fmt::Display for IconType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IconType::Button => write!(f, "Button"),
            IconType::Knob => write!(f, "Knob"),
            IconType::Fader => write!(f, "Fader"),
            IconType::Encoder => write!(f, "Encoder"),
        }
    }
}

impl std::fmt::Display for InputKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InputKind::Cc => write!(f, "CC"),
            InputKind::Note => write!(f, "Note"),
            InputKind::PitchBend => write!(f, "PBend"),
        }
    }
}

impl std::fmt::Display for OutputKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputKind::Led => write!(f, "LED"),
            OutputKind::LedRing => write!(f, "LED Ring"),
            OutputKind::LedRingCc => write!(f, "LED Ring (CC)"),
            OutputKind::Scribble => write!(f, "Scribble"),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("no config directory found")]
    NoConfigDir,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    Parse(#[from] toml::de::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("no config directory found")]
    NoConfigDir,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialize(#[from] toml::ser::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::Action;
    use std::collections::HashMap;

    fn make_test_config() -> Config {
        let hardware = HardwareDef {
            id: "enc1".into(),
            hw_type: IconType::Knob,
            col: 0,
            row: 0,
            col_span: 1,
            row_span: 1,
            inputs: vec![
                HardwareInput {
                    label: "turn".into(),
                    kind: InputKind::Cc,
                    channel: 0,
                    number: 21,
                },
                HardwareInput {
                    label: "press".into(),
                    kind: InputKind::Note,
                    channel: 0,
                    number: 64,
                },
            ],
            outputs: vec![HardwareOutput {
                label: "ring".into(),
                kind: OutputKind::LedRing,
                channel: 0,
                number: 21,
            }],
        };

        let layer_a = LayerDef {
            name: "A".into(),
            bank_offset: 0,
            icons: vec![LayerIcon {
                hardware: "enc1".into(),
                software: SoftwareDef {
                    label: "enc1".into(),
                    visual_source: None,
                    signal_entries: vec![SignalEntry {
                        input: "turn".into(),
                        actions: vec![Action::SetVolume {
                            target: "alsa_output.pci-0000_00_1f.3.analog-stereo".into(),
                            target_kind: crate::actions::VolumeTarget::Endpoint,
                            min: 0.0,
                            max: 1.0,
                            mode: crate::actions::VolumeMode::Absolute,
                            encoder: crate::actions::EncoderMode::SignMagnitude,
                            step: 0.01,
                        }],
                    }],
                    feedback_entries: vec![FeedbackEntry {
                        output: "ring".into(),
                        source: Source::Direct {
                            value: 0.75.to_string(),
                        },
                        line2_source: None,
                    }],
                },
            }],
        };

        let mut devices = HashMap::new();
        devices.insert(
            "twister".into(),
            DeviceConfig {
                port_name: "MIDI Fighter Twister".into(),
                grid_columns: 4,
                grid_rows: 4,
                hardware: vec![hardware],
                layers: vec![layer_a],
            },
        );

        Config {
            devices,
            device_order: vec!["twister".into()],
        }
    }

    // #feature CFG-DEV
    #[test]
    fn device_config_roundtrip() {
        let config = make_test_config();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let restored: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(restored, config);
        assert_eq!(restored.devices.len(), 1);
        let dev = &restored.devices["twister"];
        assert_eq!(dev.grid_columns, 4);
        assert_eq!(dev.grid_rows, 4);
        assert_eq!(dev.hardware.len(), 1);
        assert_eq!(dev.layers.len(), 1);
    }

    // #feature CFG-DEV
    #[test]
    fn source_replace_at_path() {
        let mut src = Source::If {
            condition: Comparison {
                left: Box::new(Source::Direct {
                    value: "true".into(),
                }),
                op: CmpOp::Eq,
                right: Box::new(Source::Direct {
                    value: "true".into(),
                }),
            },
            then_source: Box::new(Source::Direct {
                value: "true".into(),
            }),
            else_source: Box::new(Source::Direct {
                value: "false".into(),
            }),
        };
        src.replace_at(
            &[SourceBranch::IfThen],
            Source::Muted {
                target: "sink1".into(),
            },
        );
        assert!(matches!(
            src,
            Source::If {
                then_source,
                ..
            } if matches!(*then_source, Source::Muted { .. })
        ));
    }

    // #feature CFG-DEV
    #[test]
    fn source_roundtrip_all_variants() {
        let sources = vec![
            Source::Muted {
                target: "sink1".into(),
            },
            Source::Volume {
                target: "sink2".into(),
            },
            Source::RouteActive {
                stream: "spotify".into(),
                sink: "hdmi".into(),
            },
            Source::LayerActive {
                layer: "mixing".into(),
            },
            Source::Bank,
            Source::Custom {
                cmd: "test -f /tmp/flag".into(),
            },
            Source::HardwareInput {
                input_label: "default".into(),
            },
            Source::Direct {
                value: "hello".into(),
            },
            Source::RingRange {
                source: Box::new(Source::Direct {
                    value: "0.5".into(),
                }),
                min: 10,
                max: 120,
            },
            Source::Blink {
                a: Box::new(Source::Direct {
                    value: "true".into(),
                }),
                b: Box::new(Source::Direct {
                    value: "false".into(),
                }),
                interval_ms: 500,
            },
        ];

        for src in &sources {
            let toml_str = toml::to_string_pretty(src).unwrap();
            let restored: Source = toml::from_str(&toml_str).unwrap();
            assert_eq!(restored, *src);
        }
    }

    /// #feature CFG-DEV — comparison roundtrips through TOML.
    #[test]
    fn comparison_roundtrip() {
        let cmp = Comparison {
            left: Box::new(Source::Volume {
                target: "sink1".into(),
            }),
            op: CmpOp::Gt,
            right: Box::new(Source::Direct {
                value: "0.5".into(),
            }),
        };
        let toml_str = toml::to_string_pretty(&cmp).unwrap();
        let restored: Comparison = toml::from_str(&toml_str).unwrap();
        assert_eq!(restored.op, CmpOp::Gt);
        assert!(matches!(*restored.left, Source::Volume { .. }));
        assert!(matches!(*restored.right, Source::Direct { .. }));
    }

    /// #feature CFG-DEV — If source with comparison roundtrips.
    #[test]
    fn if_source_with_comparison_roundtrip() {
        let src = Source::If {
            condition: Comparison {
                left: Box::new(Source::Volume {
                    target: "sink".into(),
                }),
                op: CmpOp::Gt,
                right: Box::new(Source::Direct {
                    value: "0.5".into(),
                }),
            },
            then_source: Box::new(Source::Direct {
                value: "yes".into(),
            }),
            else_source: Box::new(Source::Direct { value: "no".into() }),
        };
        let toml_str = toml::to_string_pretty(&src).unwrap();
        let restored: Source = toml::from_str(&toml_str).unwrap();
        assert!(matches!(restored, Source::If { .. }));
    }

    /// #feature CFG-DEV — feedback entries roundtrip with an optional line 2
    /// source for scribble outputs.
    #[test]
    fn feedback_entry_roundtrip_with_line2() {
        let fe = FeedbackEntry {
            output: "scrib".into(),
            source: Source::Direct {
                value: "line1".into(),
            },
            line2_source: Some(Source::Volume {
                target: "alsa_output.pci-0000_00_1f.3".into(),
            }),
        };
        let toml_str = toml::to_string_pretty(&fe).unwrap();
        let restored: FeedbackEntry = toml::from_str(&toml_str).unwrap();
        assert_eq!(restored, fe);
    }

    // #feature CFG-LOAD
    #[test]
    fn empty_toml_deserializes_to_default() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config, Config::default());
        assert!(config.devices.is_empty());
    }

    // #feature CFG-LOAD
    #[test]
    fn normalize_rebuilds_device_order() {
        // Devices present but missing (or stale) in device_order must be repaired
        // on load so they stay visible in the UI. Keys are canonicalized to the
        // stable port identity, so the config key becomes the port name.
        let toml_str = r#"
[devices.A]
port_name = "port-a"

device_order = ["stale"]
"#;
        let mut config: Config = toml::from_str(toml_str).unwrap();
        config.normalize();
        assert_eq!(config.device_names(), &["port-a"]);

        // Missing device_order entirely: rebuilt from the map.
        let toml_str2 = r#"
[devices.B]
port_name = "port-b"
"#;
        let mut config2: Config = toml::from_str(toml_str2).unwrap();
        config2.normalize();
        assert_eq!(config2.device_names(), &["port-b"]);
    }

    // #feature CFG-DEV
    #[test]
    fn normalize_strips_client_port_suffix() {
        let toml_str = r#"
device_order = ["X-TOUCH MINI:X-TOUCH MINI MIDI 1 16:0"]

[devices."X-TOUCH MINI:X-TOUCH MINI MIDI 1 16:0"]
port_name = "X-TOUCH MINI:X-TOUCH MINI MIDI 1 16:0"
grid_columns = 10
grid_rows = 3
"#;
        let mut config: Config = toml::from_str(toml_str).unwrap();
        config.normalize();

        assert_eq!(config.device_names(), &["X-TOUCH MINI:X-TOUCH MINI MIDI 1"]);
        let device = config
            .devices
            .get("X-TOUCH MINI:X-TOUCH MINI MIDI 1")
            .unwrap();
        assert_eq!(device.port_name, "X-TOUCH MINI:X-TOUCH MINI MIDI 1");
        assert_eq!(device.grid_columns, 10);
    }

    #[test]
    fn output_kind_led_ring_cc_roundtrip() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Wrap {
            kind: OutputKind,
        }
        let w = Wrap {
            kind: OutputKind::LedRingCc,
        };
        let s = toml::to_string(&w).unwrap();
        assert_eq!(s.trim(), "kind = \"led_ring_cc\"");
        let back: Wrap = toml::from_str(&s).unwrap();
        assert_eq!(back.kind, OutputKind::LedRingCc);
    }
}
