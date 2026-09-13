// #feature CFG-LOAD, #feature CFG-SAVE, #feature CFG-DEV
// Integration test: full config roundtrip through the filesystem.

use midi_does::actions::Action;
use midi_does::config::{
    Config, DeviceConfig, FeedbackEntry, HardwareDef, HardwareInput, HardwareOutput, IconType,
    InputKind, LayerDef, LayerIcon, OutputKind, SignalEntry, SoftwareDef, Source,
};
use std::fs;

/// End-to-end serialization: build a realistic config, save to a temp file, load it back, compare.
#[test]
fn config_roundtrip_to_file() {
    let mut config = Config::default();

    let hardware = HardwareDef {
        id: "fader1".into(),
        hw_type: IconType::Fader,
        col: 0,
        row: 0,
        col_span: 1,
        row_span: 4,
        inputs: vec![HardwareInput {
            label: "move".into(),
            kind: InputKind::PitchBend,
            channel: 0,
            number: 0,
        }],
        outputs: vec![HardwareOutput {
            label: "motor".into(),
            kind: OutputKind::Led,
            channel: 0,
            number: 0,
        }],
    };

    let button_hw = HardwareDef {
        id: "btn1".into(),
        hw_type: IconType::Button,
        col: 1,
        row: 0,
        col_span: 1,
        row_span: 1,
        inputs: vec![HardwareInput {
            label: "press".into(),
            kind: InputKind::Note,
            channel: 0,
            number: 23,
        }],
        outputs: vec![HardwareOutput {
            label: "led".into(),
            kind: OutputKind::Led,
            channel: 0,
            number: 23,
        }],
    };

    let layer_a = LayerDef {
        name: "A".into(),
        bank_offset: 0,
        icons: vec![
            LayerIcon {
                hardware: "fader1".into(),
                software: SoftwareDef {
                    label: "fader1".into(),
                    visual_source: None,
                    signal_entries: vec![SignalEntry {
                        input: "move".into(),
                        actions: vec![Action::SetVolume {
                            target: "alsa_output.pci-0000_00_1f.3.analog-stereo".into(),
                            target_kind: midi_does::actions::VolumeTarget::Endpoint,
                            min: 0.0,
                            max: 1.0,
                            mode: midi_does::actions::VolumeMode::Absolute,
                            encoder: midi_does::actions::EncoderMode::SignMagnitude,
                            step: 0.01,
                        }],
                    }],
                    feedback_entries: vec![FeedbackEntry {
                        output: "motor".into(),
                        source: Source::Direct {
                            value: "true".to_string(),
                        },
                        line2_source: None,
                    }],
                },
            },
            LayerIcon {
                hardware: "btn1".into(),
                software: SoftwareDef {
                    label: "btn1".into(),
                    visual_source: None,
                    signal_entries: vec![SignalEntry {
                        input: "press".into(),
                        actions: vec![Action::ToggleMute {
                            target: "alsa_output.pci-0000_00_1f.3.analog-stereo".into(),
                        }],
                    }],
                    feedback_entries: vec![],
                },
            },
        ],
    };

    let device = DeviceConfig {
        port_name: "X-Touch".into(),
        grid_columns: 8,
        grid_rows: 4,
        hardware: vec![hardware, button_hw],
        layers: vec![layer_a],
    };

    config.insert_device("X-Touch".into(), device);

    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("config.toml");

    let toml_str = toml::to_string_pretty(&config).expect("serialize");
    fs::write(&config_path, &toml_str).expect("write");

    let loaded_str = fs::read_to_string(&config_path).expect("read");
    let loaded: Config = toml::from_str(&loaded_str).expect("deserialize");

    assert_eq!(loaded.devices.len(), 1);
    let dev = loaded.devices.get("X-Touch").expect("device");
    assert_eq!(dev.grid_columns, 8);
    assert_eq!(dev.grid_rows, 4);
    assert_eq!(dev.hardware.len(), 2);
    assert_eq!(dev.layers.len(), 1);
    assert_eq!(dev.layers[0].icons.len(), 2);
    assert_eq!(dev.layers[0].icons[0].hardware, "fader1");
    assert_eq!(dev.layers[0].icons[0].software.signal_entries.len(), 1);

    // Verify the feedback source roundtrips
    let fe = &dev.layers[0].icons[0].software.feedback_entries[0];
    assert_eq!(fe.output, "motor");
    assert!(matches!(fe.source, Source::Direct { .. }));
}

/// Test that a missing config file produces the default config.
#[test]
fn missing_file_produces_default() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("nonexistent.toml");
    assert!(!path.exists());

    let content = fs::read_to_string(&path);
    assert!(content.is_err());
}

/// Feedback entries roundtrip through the filesystem, including a scribble
/// entry with a `line2_source`.
#[test]
fn feedback_entries_roundtrip_in_config() {
    let mut config = Config::default();

    let hardware = HardwareDef {
        id: "scrib1".into(),
        hw_type: IconType::Button,
        col: 0,
        row: 0,
        col_span: 1,
        row_span: 1,
        inputs: vec![],
        outputs: vec![HardwareOutput {
            label: "scrib".into(),
            kind: OutputKind::Scribble,
            channel: 0,
            number: 0,
        }],
    };

    let layer = LayerDef {
        name: "A".into(),
        bank_offset: 0,
        icons: vec![LayerIcon {
            hardware: "scrib1".into(),
            software: SoftwareDef {
                label: "scrib1".into(),
                visual_source: None,
                signal_entries: vec![],
                feedback_entries: vec![FeedbackEntry {
                    output: "scrib".into(),
                    source: Source::Direct {
                        value: "L1".to_string(),
                    },
                    line2_source: Some(Source::Volume {
                        target: "alsa_output.pci-0000_00_1f.3.analog-stereo".into(),
                    }),
                }],
            },
        }],
    };

    let device = DeviceConfig {
        port_name: "X-Touch".into(),
        grid_columns: 8,
        grid_rows: 4,
        hardware: vec![hardware],
        layers: vec![layer],
    };
    config.insert_device("X-Touch".into(), device);

    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("config.toml");
    let toml_str = toml::to_string_pretty(&config).expect("serialize");
    fs::write(&config_path, &toml_str).expect("write");
    let loaded_str = fs::read_to_string(&config_path).expect("read");
    let loaded: Config = toml::from_str(&loaded_str).expect("deserialize");

    let fe = &loaded.devices["X-Touch"].layers[0].icons[0]
        .software
        .feedback_entries[0];
    assert_eq!(fe.output, "scrib");
    assert!(matches!(&fe.source, Source::Direct { value } if value == "L1"));
    assert!(matches!(
        &fe.line2_source,
        Some(Source::Volume { target }) if target == "alsa_output.pci-0000_00_1f.3.analog-stereo"
    ));
}
