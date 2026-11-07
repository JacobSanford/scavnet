//! `set.yaml`: one story or theme, made of conversations, each a sequence of audio items.

use std::path::PathBuf;

use serde::Deserialize;

use crate::range::Range;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Playback {
    /// Conversations play in file order.
    Sequence,
    /// Conversations are picked by weight.
    Random,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Replay {
    /// Every conversation plays once per cycle, then the cycle restarts.
    Exhaust,
    /// Every conversation plays once, then the set goes silent for good.
    Once,
    /// Picks are independent; nothing is remembered. Only valid with `random`.
    Repeat,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetFile {
    pub name: String,
    /// For authors only; the engine does not read it.
    #[serde(default)]
    #[allow(dead_code)]
    pub description: String,
    pub playback: Playback,
    pub replay: Replay,
    pub conversations: Vec<ConversationSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationSpec {
    pub id: String,
    /// For authors only; the engine does not read it.
    #[serde(default)]
    #[allow(dead_code)]
    pub description: String,
    /// Relative pick weight under `playback: random`.
    #[serde(default = "default_weight")]
    pub weight: u32,
    /// Fixed frequency in Hz on the network grid. Omit for a random frequency per airing.
    #[serde(default)]
    pub frequency: Option<u64>,
    pub items: Vec<ItemSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemSpec {
    pub id: String,
    #[serde(default)]
    pub caption: String,
    /// WAV file, relative to the directory holding this set.yaml.
    pub file: PathBuf,
    /// Hiss between this item and the next. Ignored on the last item.
    #[serde(default)]
    pub gap_after: Option<Range>,
}

fn default_weight() -> u32 {
    1
}

/// A validated set with resolved paths and measured durations.
#[derive(Clone, Debug)]
pub struct Set {
    pub name: String,
    pub file: PathBuf,
    pub playback: Playback,
    pub replay: Replay,
    pub conversations: Vec<Conversation>,
}

#[derive(Clone, Debug)]
pub struct Conversation {
    pub id: String,
    pub weight: u32,
    pub frequency: Option<u64>,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub id: String,
    pub caption: String,
    pub path: PathBuf,
    pub duration: f32,
    pub gap_after: Range,
}
