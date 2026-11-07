//! Playback progress: which conversations each set has aired in its current cycle.
//!
//! Stored outside the data tree. A conversation counts as played only after it finishes airing,
//! so quitting mid-transmission never skips a story beat. Entries are bounded by set size.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use rand::Rng;
use serde::{Deserialize, Serialize};

use super::library::pick_weighted;
use super::sets::{Playback, Replay, Set};
use super::World;

#[derive(Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Progress {
    #[serde(default)]
    played: BTreeMap<String, Vec<String>>,
}

impl Progress {
    /// Loads progress. A missing file is a fresh start.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_saphyr::from_str(&text).with_context(|| format!("Failed to parse {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("Failed to read {}", path.display())),
        }
    }

    /// Writes atomically: a crash leaves either the old file or the new one.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("Failed to create {}", dir.display()))?;
        }
        let tmp = path.with_extension("yaml.tmp");
        std::fs::write(&tmp, serde_saphyr::to_string(self)?)
            .with_context(|| format!("Failed to write {}", tmp.display()))?;
        std::fs::rename(&tmp, path).with_context(|| format!("Failed to replace {}", path.display()))?;
        Ok(())
    }

    /// Drops sets and conversation ids that no longer exist in the world.
    pub fn prune(&mut self, world: &World) {
        self.played
            .retain(|name, ids| match world.sets.iter().find(|s| &s.name == name) {
                Some(set) => {
                    ids.retain(|id| set.conversations.iter().any(|c| &c.id == id));
                    !ids.is_empty()
                }
                None => false,
            });
    }

    pub fn played(&self, set: &str) -> &[String] {
        self.played.get(set).map(Vec::as_slice).unwrap_or_default()
    }

    /// Records a finished airing. Completing an `exhaust` cycle starts the next one.
    pub fn record(&mut self, set: &Set, conversation_id: &str) {
        if set.replay == Replay::Repeat {
            return;
        }
        let ids = self.played.entry(set.name.clone()).or_default();
        if !ids.iter().any(|id| id == conversation_id) {
            ids.push(conversation_id.to_string());
        }
        let complete = set.conversations.iter().all(|c| ids.contains(&c.id));
        if complete && set.replay == Replay::Exhaust {
            self.played.remove(&set.name);
        }
    }

    /// Whether `set` has anything left to air.
    pub fn has_next(&self, set: &Set) -> bool {
        self.candidates(set).next().is_some()
    }

    /// The next conversation index for `set`, or `None` if it has nothing left to air.
    pub fn next_conversation(&self, set: &Set, rng: &mut impl Rng) -> Option<usize> {
        match set.playback {
            Playback::Sequence => self.candidates(set).next(),
            Playback::Random => {
                let candidates: Vec<usize> = self.candidates(set).collect();
                let weights: Vec<u32> = candidates.iter().map(|&i| set.conversations[i].weight).collect();
                pick_weighted(&weights, rng).map(|i| candidates[i])
            }
        }
    }

    fn candidates<'a>(&'a self, set: &'a Set) -> impl Iterator<Item = usize> + 'a {
        let played = self.played(&set.name);
        set.conversations
            .iter()
            .enumerate()
            .filter(move |(_, c)| set.replay == Replay::Repeat || !played.contains(&c.id))
            .map(|(i, _)| i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::range::Range;
    use crate::world::sets::{Conversation, Item};
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn set(playback: Playback, replay: Replay, ids: &[&str]) -> Set {
        Set {
            name: "s".into(),
            file: "s/set.yaml".into(),
            playback,
            replay,
            conversations: ids
                .iter()
                .map(|id| Conversation {
                    id: id.to_string(),
                    weight: 1,
                    frequency: None,
                    items: vec![Item {
                        id: "i".into(),
                        caption: String::new(),
                        path: "x.wav".into(),
                        duration: 1.0,
                        gap_after: Range::new(0.0, 0.0),
                    }],
                })
                .collect(),
        }
    }

    /// Airs `n` conversations, recording each, and returns the ids in order.
    fn air(progress: &mut Progress, set: &Set, n: usize, rng: &mut StdRng) -> Vec<String> {
        (0..n)
            .map_while(|_| {
                let i = progress.next_conversation(set, rng)?;
                let id = set.conversations[i].id.clone();
                progress.record(set, &id);
                Some(id)
            })
            .collect()
    }

    #[test]
    fn sequence_exhaust_loops_in_order() {
        let s = set(Playback::Sequence, Replay::Exhaust, &["a", "b", "c"]);
        let mut p = Progress::default();
        let mut rng = StdRng::seed_from_u64(0);
        assert_eq!(air(&mut p, &s, 7, &mut rng), ["a", "b", "c", "a", "b", "c", "a"]);
    }

    #[test]
    fn sequence_once_stops_after_last() {
        let s = set(Playback::Sequence, Replay::Once, &["a", "b"]);
        let mut p = Progress::default();
        let mut rng = StdRng::seed_from_u64(0);
        assert_eq!(air(&mut p, &s, 5, &mut rng), ["a", "b"]);
        assert_eq!(p.next_conversation(&s, &mut rng), None);
        assert!(!p.has_next(&s));
    }

    #[test]
    fn random_exhaust_plays_each_once_per_cycle() {
        let s = set(Playback::Random, Replay::Exhaust, &["a", "b", "c", "d"]);
        let mut p = Progress::default();
        let mut rng = StdRng::seed_from_u64(3);
        for _ in 0..5 {
            let mut cycle = air(&mut p, &s, 4, &mut rng);
            cycle.sort();
            assert_eq!(cycle, ["a", "b", "c", "d"]);
        }
    }

    #[test]
    fn random_repeat_keeps_no_history() {
        let s = set(Playback::Random, Replay::Repeat, &["a", "b"]);
        let mut p = Progress::default();
        let mut rng = StdRng::seed_from_u64(3);
        assert_eq!(air(&mut p, &s, 50, &mut rng).len(), 50);
        assert_eq!(p, Progress::default());
    }

    #[test]
    fn unfinished_airing_is_not_recorded() {
        let s = set(Playback::Sequence, Replay::Exhaust, &["a", "b"]);
        let p = Progress::default();
        let mut rng = StdRng::seed_from_u64(0);
        assert_eq!(p.next_conversation(&s, &mut rng), Some(0));
        // Chosen but never recorded: the next pick is the same chapter.
        assert_eq!(p.next_conversation(&s, &mut rng), Some(0));
    }

    #[test]
    fn save_load_round_trip_and_prune() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/progress.yaml");
        let s = set(Playback::Sequence, Replay::Once, &["a", "b"]);
        let mut p = Progress::default();
        p.record(&s, "a");
        p.played.insert("gone".into(), vec!["x".into()]);
        p.save(&path).unwrap();
        let mut loaded = Progress::load(&path).unwrap();
        assert_eq!(loaded, p);

        let world = World {
            sets: vec![s],
            ..World::empty_for_tests()
        };
        loaded.prune(&world);
        assert_eq!(loaded.played("s"), ["a"]);
        assert!(loaded.played("gone").is_empty());
    }

    #[test]
    fn missing_file_is_fresh_start() {
        assert_eq!(
            Progress::load(Path::new("/nonexistent/progress.yaml")).unwrap(),
            Progress::default()
        );
    }
}
