use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PwObject {
    pub id: u32,
    pub object_type: PwObjectType,
    pub name: String,
    pub description: String,
    /// Stores the normalized volume when PipeWire reports it.
    /// `None` represents an unknown volume.
    #[serde(default)]
    pub volume: Option<f64>,
    pub muted: bool,
}

impl PwObject {
    /// Finds an object by numeric ID, exact name, or name substring, in that order.
    pub fn find<'a>(objects: &'a [Self], target: &str) -> Option<&'a Self> {
        if target.is_empty() {
            return None;
        }
        objects
            .iter()
            .find(|o| o.id.to_string() == target)
            .or_else(|| objects.iter().find(|o| o.name == target))
            .or_else(|| objects.iter().find(|o| o.name.contains(target)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PwObjectType {
    Sink,
    Source,
    Stream,
    Unknown,
    /// Filter sentinel matching sinks or sources (audio endpoints), used only
    /// by object pickers, never assigned to a `PwObject`.
    SinkOrSource,
    /// Filter sentinel matching playback streams or sources, used only by
    /// object pickers, never assigned to a `PwObject`. Sources are included so
    /// routing can wire a hardware input into a sink (e.g. a virtual mic).
    StreamOrSource,
}

#[derive(Debug, Clone)]
pub enum PwCommand {
    Shutdown,
    Volume {
        /// Node name or numeric id string.
        target: String,
        /// Normalized volume (0.0..1.0 and above).
        volume: f64,
    },
    Mute {
        target: String,
        muted: bool,
    },
    Route {
        /// Stream node name or id.
        stream: String,
        /// Sink node name or id.
        sink: String,
        /// `true` to link, `false` to unlink.
        connect: bool,
    },
}

#[derive(Debug, Clone)]
pub enum PwEvent {
    /// Snapshot of the current audio graph: objects plus resolved routing
    /// edges `(output_node_name, input_node_name)`.
    ObjectsUpdated {
        objects: Vec<PwObject>,
        edges: Vec<(String, String)>,
    },
    Connected,
    Disconnected,
}
