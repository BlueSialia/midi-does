use midir::{MidiInput, MidiOutput};
use std::collections::HashMap;
use std::sync::mpsc;

#[derive(Debug, Clone)]
pub enum MidiEvent {
    Connected {
        port_name: String,
    },
    Cc {
        port_name: String,
        channel: u8,
        controller: u8,
        value: u8,
    },
    NoteOn {
        port_name: String,
        channel: u8,
        note: u8,
        velocity: u8,
    },
    PitchBend {
        port_name: String,
        channel: u8,
        value: u16,
    },
    SysEx {
        port_name: String,
        data: Vec<u8>,
    },
}

pub struct MidiManager {
    sender: mpsc::Sender<MidiEvent>,
    /// Active input connections keyed by device port name. Reconnecting a
    /// device replaces its previous connection instead of accumulating, so a
    /// hotplug reconnect never delivers duplicate events.
    connections: HashMap<String, midir::MidiInputConnection<()>>,
    /// Cached MidiInput used for port enumeration, so hotplug checks don't
    /// allocate a new midir client every 3 seconds.
    midi_input: Option<MidiInput>,
}

pub struct MidiOutputHandle {
    _conn: midir::MidiOutputConnection,
}

impl MidiManager {
    pub fn new() -> Result<(Self, mpsc::Receiver<MidiEvent>), midir::InitError> {
        let input = MidiInput::new("midi-does")?;
        let (tx, rx) = mpsc::channel();
        Ok((
            MidiManager {
                sender: tx,
                connections: HashMap::new(),
                midi_input: Some(input),
            },
            rx,
        ))
    }

    pub fn dead() -> Self {
        let (tx, _) = mpsc::channel();
        MidiManager {
            sender: tx,
            connections: HashMap::new(),
            midi_input: None,
        }
    }

    pub fn available_ports(&self) -> Vec<PortInfo> {
        let Some(input) = &self.midi_input else {
            return Vec::new();
        };
        input
            .ports()
            .into_iter()
            .filter_map(|p| {
                let name = input.port_name(&p).unwrap_or_default();
                if name.split(':').next() == Some("midi-does") {
                    None
                } else {
                    Some(PortInfo {
                        name: stable_port_key(&name).to_string(),
                    })
                }
            })
            .collect()
    }

    pub fn connect(&mut self, port_name: &str) -> Result<(), ConnectError> {
        let port = {
            let Some(input) = &self.midi_input else {
                return Err(ConnectError::Io(std::io::Error::other(
                    "MIDI input unavailable",
                )));
            };
            input
                .ports()
                .into_iter()
                .find(|p| {
                    input
                        .port_name(p)
                        .map(|n| stable_port_key(&n) == port_name)
                        .unwrap_or(false)
                })
                .ok_or(ConnectError::PortNotFound)?
        };

        let input = MidiInput::new("midi-does")
            .map_err(|e| ConnectError::Io(std::io::Error::other(e.to_string())))?;

        let port_name_cb = port_name.to_string();
        let sender_cb = self.sender.clone();

        let conn = input.connect(
            &port,
            "midi-does-input",
            move |_timestamp, message, _data| {
                handle_midi_message(message, &port_name_cb, &sender_cb);
            },
            (),
        )?;

        // Replacing an existing connection drops the old one (closing its ALSA port).
        self.connections.insert(port_name.to_string(), conn);

        let _ = self.sender.send(MidiEvent::Connected {
            port_name: port_name.to_string(),
        });
        Ok(())
    }

    pub fn connect_output(&self, port_name: &str) -> Result<MidiOutputHandle, ConnectError> {
        let output = MidiOutput::new("midi-does")
            .map_err(|e| ConnectError::Io(std::io::Error::other(e.to_string())))?;

        let ports = output.ports();
        let port = ports
            .iter()
            .find(|p| {
                output
                    .port_name(p)
                    .map(|n| stable_port_key(&n) == port_name)
                    .unwrap_or(false)
            })
            .ok_or(ConnectError::PortNotFound)?;

        let conn = output.connect(port, "midi-does-output")?;

        Ok(MidiOutputHandle { _conn: conn })
    }
}

impl MidiOutputHandle {
    pub fn send_note(&mut self, channel: u8, note: u8, velocity: u8) {
        let msg = [0x90 | (channel & 0x0F), note.min(127), velocity.min(127)];
        let _ = self._conn.send(&msg);
    }

    pub fn send_cc(&mut self, channel: u8, controller: u8, value: u8) {
        let msg = [0xB0 | (channel & 0x0F), controller.min(127), value.min(127)];
        let _ = self._conn.send(&msg);
    }

    pub fn send_sysex(&mut self, data: &[u8]) {
        let _ = self._conn.send(data);
    }
}

#[derive(Debug)]
pub enum ConnectError {
    PortNotFound,
    Io(std::io::Error),
    MidiInput(midir::ConnectError<MidiInput>),
    MidiOutput(midir::ConnectError<MidiOutput>),
}

impl From<midir::ConnectError<MidiInput>> for ConnectError {
    fn from(e: midir::ConnectError<MidiInput>) -> Self {
        ConnectError::MidiInput(e)
    }
}

impl From<midir::ConnectError<MidiOutput>> for ConnectError {
    fn from(e: midir::ConnectError<MidiOutput>) -> Self {
        ConnectError::MidiOutput(e)
    }
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectError::PortNotFound => write!(f, "port not found"),
            ConnectError::Io(e) => write!(f, "IO error: {e}"),
            ConnectError::MidiInput(e) => write!(f, "MIDI input error: {e}"),
            ConnectError::MidiOutput(e) => write!(f, "MIDI output error: {e}"),
        }
    }
}

impl std::error::Error for ConnectError {}

pub(crate) fn handle_midi_message(
    message: &[u8],
    port_name: &str,
    sender: &mpsc::Sender<MidiEvent>,
) {
    if message.is_empty() {
        return;
    }
    let status = message[0];

    match status {
        0xF0 => {
            let _ = sender.send(MidiEvent::SysEx {
                port_name: port_name.to_string(),
                data: message.to_vec(),
            });
            return;
        }
        _ if status < 0x80 => return,
        _ => {}
    }

    let channel = status & 0x0F;
    let msg_type = status & 0xF0;

    match msg_type {
        0xB0 if message.len() >= 3 => {
            let _ = sender.send(MidiEvent::Cc {
                port_name: port_name.to_string(),
                channel,
                controller: message[1],
                value: message[2],
            });
        }
        0x90 if message.len() >= 3 && message[2] > 0 => {
            let _ = sender.send(MidiEvent::NoteOn {
                port_name: port_name.to_string(),
                channel,
                note: message[1],
                velocity: message[2],
            });
        }
        // Note On with velocity 0 (a Note Off) and truncated Note On are ignored.
        0x90 => {}
        // Note Off (0x80) is ignored so actions fire only once, on Note On.
        0x80 => {}
        0xE0 if message.len() >= 3 => {
            let lsb = message[1] as u16;
            let msb = message[2] as u16;
            let value = lsb | (msb << 7);
            let _ = sender.send(MidiEvent::PitchBend {
                port_name: port_name.to_string(),
                channel,
                value,
            });
        }
        _ => {}
    }
}

/// Returns the stable `client_name:port_name` portion of an ALSA port name.
///
/// `midir` appends a volatile numeric `client_id:port_id` suffix.
pub(crate) fn stable_port_key(port_name: &str) -> &str {
    let Some((head, tail)) = port_name.rsplit_once(' ') else {
        return port_name;
    };

    let is_client_port = tail.split_once(':').is_some_and(|(client, port)| {
        !client.is_empty()
            && !port.is_empty()
            && client.bytes().all(|b| b.is_ascii_digit())
            && port.bytes().all(|b| b.is_ascii_digit())
    });

    if is_client_port {
        head
    } else {
        port_name
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortInfo {
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn collect_event(bytes: &[u8]) -> Option<MidiEvent> {
        let (tx, rx) = mpsc::channel();
        handle_midi_message(bytes, "test-port", &tx);
        // Bounded receive: messages that are ignored (e.g. Note Off) send
        // nothing, so an unbounded recv() would block forever.
        rx.recv_timeout(std::time::Duration::from_millis(100)).ok()
    }

    /// #feature MIDI-CC — test that raw CC bytes produce MidiEvent::Cc
    #[test]
    fn test_midi_cc() {
        let event = collect_event(&[0xB0, 0x07, 0x40]);
        match event {
            Some(MidiEvent::Cc {
                channel,
                controller,
                value,
                port_name,
                ..
            }) => {
                assert_eq!(channel, 0);
                assert_eq!(controller, 7);
                assert_eq!(value, 64);
                assert_eq!(port_name, "test-port");
            }
            other => panic!("Expected Cc event, got {other:?}"),
        }
    }

    /// #feature MIDI-NOTE — test Note On with velocity > 0
    #[test]
    fn test_note_on() {
        let event = collect_event(&[0x90, 0x3C, 0x7F]);
        match event {
            Some(MidiEvent::NoteOn {
                channel,
                note,
                velocity,
                ..
            }) => {
                assert_eq!(channel, 0);
                assert_eq!(note, 0x3C);
                assert_eq!(velocity, 0x7F);
            }
            other => panic!("Expected NoteOn event, got {other:?}"),
        }
    }

    /// #feature MIDI-NOTE — Note On with velocity 0 (a Note Off) is ignored so
    /// actions fire only once, on Note On.
    #[test]
    fn test_note_on_zero_velocity_is_ignored() {
        let event = collect_event(&[0x90, 0x3C, 0x00]);
        assert!(event.is_none());
    }

    /// #feature MIDI-NOTE — explicit Note Off (0x80) is ignored so actions
    /// fire only once, on Note On.
    #[test]
    fn test_note_off_is_ignored() {
        let event = collect_event(&[0x80, 0x3C, 0x40]);
        assert!(event.is_none());
    }

    /// #feature MIDI-PB — test pitch bend message
    #[test]
    fn test_pitch_bend() {
        let event = collect_event(&[0xE0, 0x00, 0x40]);
        match event {
            Some(MidiEvent::PitchBend { channel, value, .. }) => {
                assert_eq!(channel, 0);
                assert_eq!(value, 0x2000);
            }
            other => panic!("Expected PitchBend event, got {other:?}"),
        }
    }

    /// #feature MIDI-SX — test SysEx message
    #[test]
    fn test_sysex() {
        let event = collect_event(&[0xF0, 0x7E, 0x7F, 0x06, 0x01, 0xF7]);
        match event {
            Some(MidiEvent::SysEx { data, .. }) => {
                assert_eq!(data, vec![0xF0, 0x7E, 0x7F, 0x06, 0x01, 0xF7]);
            }
            other => panic!("Expected SysEx event, got {other:?}"),
        }
    }

    #[test]
    fn test_empty_message_returns_nothing() {
        let event = collect_event(&[]);
        assert!(event.is_none());
    }

    #[test]
    fn test_status_below_0x80_is_ignored() {
        let event = collect_event(&[0x7F, 0x42, 0x42]);
        assert!(event.is_none());
    }

    /// #feature MIDI-PORTS — the ALSA `client_id:port_id` suffix is stripped
    /// so replugging under a new client ID yields the same stable key.
    #[test]
    fn test_stable_port_key_strips_client_port_ids() {
        assert_eq!(
            stable_port_key("X-TOUCH MINI:X-TOUCH MINI MIDI 1 16:0"),
            "X-TOUCH MINI:X-TOUCH MINI MIDI 1"
        );
        assert_eq!(
            stable_port_key("X-TOUCH MINI:X-TOUCH MINI MIDI 1 32:0"),
            "X-TOUCH MINI:X-TOUCH MINI MIDI 1"
        );
    }

    #[test]
    fn test_stable_port_key_leaves_plain_names_untouched() {
        assert_eq!(stable_port_key("X-Touch"), "X-Touch");
        assert_eq!(
            stable_port_key("MIDI Fighter Twister"),
            "MIDI Fighter Twister"
        );
    }
}
