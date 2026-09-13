use serde::{Deserialize, Serialize};

fn default_max_val() -> u8 {
    127
}

fn default_blink_interval() -> u64 {
    1000
}

/// Path into a `Source` tree. Navigates into `If.then_source` and
/// `If.else_source`, `RingRange.source`, and `Blink.a`/`Blink.b`.
/// The comparison inside `If` is edited separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceBranch {
    IfThen,
    IfElse,
    RingRangeInner,
    BlinkA,
    BlinkB,
}

/// A value-producing expression evaluated against PipeWire state, layer, bank,
/// and hardware input. Sources compose via `If` into deep trees and evaluate to
/// a single text result interpreted by the consumer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Source {
    Direct {
        value: String,
    },
    /// The most recent raw MIDI value on the input with the given label,
    /// normalized to 0.0–1.0.
    HardwareInput {
        input_label: String,
    },
    Volume {
        target: String,
    },
    Muted {
        target: String,
    },
    RouteActive {
        stream: String,
        sink: String,
    },
    LayerActive {
        layer: String,
    },
    Bank,
    Custom {
        cmd: String,
    },
    If {
        condition: Comparison,
        then_source: Box<Source>,
        else_source: Box<Source>,
    },
    RingRange {
        source: Box<Source>,
        #[serde(default)]
        min: u8,
        #[serde(default = "default_max_val")]
        max: u8,
    },
    Blink {
        a: Box<Source>,
        b: Box<Source>,
        #[serde(default = "default_blink_interval")]
        interval_ms: u64,
    },
}

/// Comparison of two sources. Numeric comparison is attempted first;
/// falls back to lexicographic string ordering when either side does not
/// parse as a number.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Comparison {
    pub left: Box<Source>,
    pub op: CmpOp,
    pub right: Box<Source>,
}

impl Source {
    /// Stable, human-readable name of the source variant, used by pickers.
    pub fn kind(&self) -> &'static str {
        match self {
            Source::Direct { .. } => "Direct",
            Source::HardwareInput { .. } => "HardwareInput",
            Source::Volume { .. } => "Volume",
            Source::Muted { .. } => "Muted",
            Source::RouteActive { .. } => "RouteActive",
            Source::LayerActive { .. } => "LayerActive",
            Source::Bank => "Bank",
            Source::Custom { .. } => "Custom",
            Source::If { .. } => "If",
            Source::RingRange { .. } => "RingRange",
            Source::Blink { .. } => "Blink",
        }
    }

    pub fn replace_at(&mut self, path: &[SourceBranch], new_source: Source) {
        *self.node_at_mut(path) = new_source;
    }

    /// Navigate to the `If` source at `path` and replace its condition.
    /// The path leads to the `If` node itself (e.g. `[]` for the root,
    /// `[IfElse]` for an `If` nested in `else_source`).
    pub fn replace_condition_at(&mut self, path: &[SourceBranch], comparison: Comparison) {
        if let Source::If { condition, .. } = self.node_at_mut(path) {
            *condition = comparison;
        }
    }

    fn node_at_mut(&mut self, path: &[SourceBranch]) -> &mut Source {
        let mut node = self;
        for &branch in path {
            node = Self::walk(node, branch);
        }
        node
    }

    fn walk(node: &mut Source, branch: SourceBranch) -> &mut Source {
        match branch {
            SourceBranch::IfThen | SourceBranch::IfElse => {
                let Source::If {
                    then_source,
                    else_source,
                    ..
                } = node
                else {
                    return node;
                };
                match branch {
                    SourceBranch::IfThen => then_source,
                    SourceBranch::IfElse => else_source,
                    _ => unreachable!(),
                }
            }
            SourceBranch::RingRangeInner => {
                let Source::RingRange { source, .. } = node else {
                    return node;
                };
                source
            }
            SourceBranch::BlinkA => {
                let Source::Blink { a, .. } = node else {
                    return node;
                };
                a
            }
            SourceBranch::BlinkB => {
                let Source::Blink { b, .. } = node else {
                    return node;
                };
                b
            }
        }
    }
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.kind())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CmpOp {
    Lt,
    Lte,
    Gt,
    Gte,
    Eq,
}

impl std::fmt::Display for CmpOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CmpOp::Lt => write!(f, "<"),
            CmpOp::Lte => write!(f, "<="),
            CmpOp::Gt => write!(f, ">"),
            CmpOp::Gte => write!(f, ">="),
            CmpOp::Eq => write!(f, "="),
        }
    }
}
