use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    None,
    SetVolume {
        target: String,
        #[serde(default)]
        target_kind: VolumeTarget,
        #[serde(default = "default_min")]
        min: f64,
        #[serde(default = "default_max")]
        max: f64,
        #[serde(default)]
        mode: VolumeMode,
        #[serde(default)]
        encoder: EncoderMode,
        #[serde(default = "default_step")]
        step: f64,
    },
    ToggleMute {
        target: String,
    },
    RouteStream {
        stream_id: String,
        sink_id: String,
    },
    SetBankOffset {
        offset: u8,
    },
    BankIncrement,
    BankDecrement,
    SelectLayer {
        layer: String,
    },
    RunCommand {
        command: String,
    },
    MidiSendNote {
        channel: u8,
        note: u8,
        velocity: u8,
    },
    MidiSendCc {
        channel: u8,
        controller: u8,
        value: u8,
    },
}

/// Identifies the PipeWire object types offered by a `SetVolume` picker.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum VolumeTarget {
    /// Targets an audio endpoint.
    #[default]
    Endpoint,
    /// Targets a playback or capture stream.
    Stream,
}

impl std::fmt::Display for VolumeTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VolumeTarget::Endpoint => write!(f, "Sink / Source"),
            VolumeTarget::Stream => write!(f, "Stream"),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum VolumeMode {
    #[default]
    Absolute,
    Relative,
}

impl std::fmt::Display for VolumeMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VolumeMode::Absolute => write!(f, "Absolute"),
            VolumeMode::Relative => write!(f, "Relative"),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum EncoderMode {
    #[default]
    SignMagnitude,
    TwosComplement,
    BinaryOffset,
}

impl std::fmt::Display for EncoderMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EncoderMode::SignMagnitude => write!(f, "Sign-Mag"),
            EncoderMode::TwosComplement => write!(f, "Two's C"),
            EncoderMode::BinaryOffset => write!(f, "BinOff"),
        }
    }
}

fn default_min() -> f64 {
    0.0
}
fn default_max() -> f64 {
    1.0
}
fn default_step() -> f64 {
    0.01
}

impl Action {
    pub fn display_name(&self) -> &'static str {
        match self {
            Action::None => "None",
            Action::SetVolume { .. } => "Set Volume",
            Action::ToggleMute { .. } => "Toggle Mute",
            Action::RouteStream { .. } => "Route Stream",
            Action::SetBankOffset { .. } => "Set Bank",
            Action::BankIncrement => "Bank +8",
            Action::BankDecrement => "Bank -8",
            Action::SelectLayer { .. } => "Select Layer",
            Action::RunCommand { .. } => "Run Command",
            Action::MidiSendNote { .. } => "MIDI Send Note",
            Action::MidiSendCc { .. } => "MIDI Send CC",
        }
    }

    /// Returns each action variant with its default values, in UI order.
    pub fn all() -> Vec<(&'static str, Action)> {
        [
            Action::None,
            Action::SetVolume {
                target: String::new(),
                target_kind: VolumeTarget::Endpoint,
                min: 0.0,
                max: 1.0,
                mode: VolumeMode::Absolute,
                encoder: EncoderMode::SignMagnitude,
                step: 0.01,
            },
            Action::ToggleMute {
                target: String::new(),
            },
            Action::RouteStream {
                stream_id: String::new(),
                sink_id: String::new(),
            },
            Action::SetBankOffset { offset: 0 },
            Action::BankIncrement,
            Action::BankDecrement,
            Action::SelectLayer {
                layer: String::new(),
            },
            Action::RunCommand {
                command: String::new(),
            },
            Action::MidiSendNote {
                channel: 0,
                note: 0,
                velocity: 0,
            },
            Action::MidiSendCc {
                channel: 0,
                controller: 0,
                value: 0,
            },
        ]
        .into_iter()
        .map(|action| (action.display_name(), action))
        .collect()
    }
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(action: &Action) -> Action {
        let toml_str = toml::to_string(action).expect("serialize failed");
        toml::from_str(&toml_str).expect("deserialize failed")
    }

    fn roundtrip_with_str(toml_str: &str) -> Action {
        let action: Action = toml::from_str(toml_str).expect("deserialize failed");
        let toml_str2 = toml::to_string(&action).expect("serialize failed");
        toml::from_str(&toml_str2).expect("re-deserialize failed")
    }

    // #feature ACT-VOL
    #[test]
    fn test_volume_actions_roundtrip() {
        let actions = vec![
            Action::SetVolume {
                target: "alsa_output.pci-0000_00_1f.3.analog-stereo".into(),
                target_kind: VolumeTarget::Endpoint,
                min: 0.0,
                max: 1.5,
                mode: VolumeMode::Relative,
                encoder: EncoderMode::BinaryOffset,
                step: 0.05,
            },
            Action::SetVolume {
                target: "alsa_input.usb-Blue_Microphones-00.analog-stereo".into(),
                target_kind: VolumeTarget::Endpoint,
                min: 0.1,
                max: 2.0,
                mode: VolumeMode::Absolute,
                encoder: EncoderMode::SignMagnitude,
                step: 0.02,
            },
            Action::SetVolume {
                target: "firefox-stream-42".into(),
                target_kind: VolumeTarget::Stream,
                min: 0.0,
                max: 1.0,
                mode: VolumeMode::Relative,
                encoder: EncoderMode::TwosComplement,
                step: 0.01,
            },
        ];

        for action in &actions {
            assert_eq!(roundtrip(action), *action);
        }
    }

    // #feature ACT-VOL
    #[test]
    fn test_volume_action_defaults() {
        let toml_str = r#"
type = "set_volume"
target = "default_sink"
"#;
        let action = roundtrip_with_str(toml_str);
        assert_eq!(
            action,
            Action::SetVolume {
                target: "default_sink".into(),
                target_kind: VolumeTarget::Endpoint,
                min: 0.0,
                max: 1.0,
                mode: VolumeMode::Absolute,
                encoder: EncoderMode::SignMagnitude,
                step: 0.01,
            }
        );
    }

    // #feature ACT-MUTE
    #[test]
    fn test_mute_actions_roundtrip() {
        let sink_mute = Action::ToggleMute {
            target: "alsa_output.pci-0000_00_1f.3.analog-stereo".into(),
        };
        let source_mute = Action::ToggleMute {
            target: "alsa_input.usb-Blue_Microphones-00.analog-stereo".into(),
        };

        assert_eq!(roundtrip(&sink_mute), sink_mute);
        assert_eq!(roundtrip(&source_mute), source_mute);
    }

    // #feature ACT-ROUTE
    #[test]
    fn test_route_stream_roundtrip() {
        let action = Action::RouteStream {
            stream_id: "spotify-stream".into(),
            sink_id: "hdmi-output".into(),
        };
        assert_eq!(roundtrip(&action), action);
    }

    // #feature ACT-CMD
    #[test]
    fn test_run_command_roundtrip() {
        let action = Action::RunCommand {
            command: "notify-send 'Hello, $NORM_VALUE!'".into(),
        };
        assert_eq!(roundtrip(&action), action);
    }

    // #feature ACT-MIDI
    #[test]
    fn test_midi_send_roundtrip() {
        let note = Action::MidiSendNote {
            channel: 1,
            note: 60,
            velocity: 100,
        };
        let cc = Action::MidiSendCc {
            channel: 15,
            controller: 7,
            value: 127,
        };

        assert_eq!(roundtrip(&note), note);
        assert_eq!(roundtrip(&cc), cc);
    }

    // #feature ACT-LAYER
    #[test]
    fn test_select_layer_roundtrip() {
        let action = Action::SelectLayer {
            layer: "mixing".into(),
        };
        assert_eq!(roundtrip(&action), action);
    }

    // #feature ACT-BANK
    #[test]
    fn test_bank_actions_roundtrip() {
        let offset = Action::SetBankOffset { offset: 16 };
        let inc = Action::BankIncrement;
        let dec = Action::BankDecrement;

        assert_eq!(roundtrip(&offset), offset);
        assert_eq!(roundtrip(&inc), inc);
        assert_eq!(roundtrip(&dec), dec);
    }

    // #feature ACT-MULTI
    #[test]
    fn test_multiple_actions_serialization() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct ActionList {
            actions: Vec<Action>,
        }

        let actions = vec![
            Action::SetVolume {
                target: "sink1".into(),
                target_kind: VolumeTarget::Endpoint,
                min: 0.0,
                max: 1.0,
                mode: VolumeMode::Absolute,
                encoder: EncoderMode::SignMagnitude,
                step: 0.01,
            },
            Action::MidiSendNote {
                channel: 0,
                note: 1,
                velocity: 127,
            },
            Action::RunCommand {
                command: "echo done".into(),
            },
        ];

        let list = ActionList {
            actions: actions.clone(),
        };
        let toml_str = toml::to_string(&list).expect("serialize failed");
        let restored: ActionList = toml::from_str(&toml_str).expect("deserialize failed");
        assert_eq!(restored.actions, actions);
    }

    #[test]
    fn test_none_roundtrip() {
        let toml_str = r#"
type = "none"
"#;
        let action = roundtrip_with_str(toml_str);
        assert_eq!(action, Action::None);
    }
}
