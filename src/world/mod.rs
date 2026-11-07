//! The world: everything under `data_dir` that is tracked in git.
//!
//! Loading validates the whole tree up front and reports every problem with its file path,
//! so a broken world fails at startup (or in `scavnet validate`), never mid-broadcast.

pub mod library;
pub mod networks;
pub mod progress;
pub mod sets;

use std::collections::HashSet;
use std::fmt;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

use serde::de::DeserializeOwned;

use crate::range::Range;
use library::{Library, LibraryFile, Node, NodeSpec};
use networks::{FrequencyPlan, NetworksFile};
use sets::{Conversation, Item, Playback, Replay, Set, SetFile};

pub const SET_FILE_NAME: &str = "set.yaml";

pub struct World {
    pub plan: FrequencyPlan,
    pub library: Library,
    pub sets: Vec<Set>,
}

/// Every problem found while loading, each prefixed with the file it came from.
#[derive(Debug)]
pub struct WorldErrors(pub Vec<String>);

impl fmt::Display for WorldErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "The world has {} problem(s):", self.0.len())?;
        for p in &self.0 {
            writeln!(f, "  - {p}")?;
        }
        Ok(())
    }
}

impl std::error::Error for WorldErrors {}

impl World {
    pub fn load(networks_path: &Path, library_path: &Path) -> Result<Self, WorldErrors> {
        let mut problems = Vec::new();

        let plan = read_yaml::<NetworksFile>(networks_path).and_then(|file| {
            FrequencyPlan::build(&file.networks).map_err(|errs| {
                errs.into_iter()
                    .map(|e| format!("{}: {e}", networks_path.display()))
                    .collect()
            })
        });
        let plan = plan.map_err(|e| problems.extend(e)).ok();

        let mut sets = Vec::new();
        let library = match read_yaml::<LibraryFile>(library_path) {
            Ok(file) => {
                let base = library_path.parent().unwrap_or(Path::new(""));
                let mut ctx = LoadCtx {
                    library_path,
                    base,
                    plan: plan.as_ref(),
                    sets: &mut sets,
                    problems: &mut problems,
                };
                if file.sets.is_empty() {
                    ctx.problem("sets must contain at least one entry".into());
                }
                let roots = file
                    .sets
                    .iter()
                    .enumerate()
                    .filter_map(|(i, n)| ctx.node(n, &format!("sets[{i}]")))
                    .collect();
                Library { roots }
            }
            Err(e) => {
                problems.extend(e);
                Library::default()
            }
        };

        let mut names = HashSet::new();
        for set in &sets {
            if !names.insert(set.name.as_str()) {
                problems.push(format!(
                    "{}: set name '{}' is used by another set",
                    set.file.display(),
                    set.name
                ));
            }
        }

        match plan {
            Some(plan) if problems.is_empty() => Ok(World { plan, library, sets }),
            _ => Err(WorldErrors(problems)),
        }
    }

    pub fn conversation_count(&self) -> usize {
        self.sets.iter().map(|s| s.conversations.len()).sum()
    }

    pub fn items(&self) -> impl Iterator<Item = &Item> {
        self.sets.iter().flat_map(|s| &s.conversations).flat_map(|c| &c.items)
    }

    #[cfg(test)]
    pub fn empty_for_tests() -> Self {
        let net = networks::NetworkSpec {
            name: "n".into(),
            start_freq: 1000,
            end_freq: 2000,
            step: 10,
        };
        World {
            plan: FrequencyPlan::build(&[net]).unwrap(),
            library: Library::default(),
            sets: Vec::new(),
        }
    }
}

struct LoadCtx<'a> {
    library_path: &'a Path,
    base: &'a Path,
    plan: Option<&'a FrequencyPlan>,
    sets: &'a mut Vec<Set>,
    problems: &'a mut Vec<String>,
}

impl LoadCtx<'_> {
    fn problem(&mut self, msg: String) {
        self.problems.push(format!("{}: {msg}", self.library_path.display()));
    }

    fn node(&mut self, spec: &NodeSpec, at: &str) -> Option<Node> {
        if spec.weight == 0 {
            self.problem(format!("{at}: weight must be greater than 0"));
        }
        match (&spec.sets, &spec.set) {
            (Some(children), None) => {
                if spec.name.as_deref().is_none_or(|n| n.trim().is_empty()) {
                    self.problem(format!("{at}: a group needs a 'name'"));
                }
                if children.is_empty() {
                    self.problem(format!("{at}: 'sets' must not be empty"));
                }
                let children = children
                    .iter()
                    .enumerate()
                    .filter_map(|(i, c)| self.node(c, &format!("{at}.sets[{i}]")))
                    .collect();
                Some(Node::Group {
                    weight: spec.weight,
                    children,
                })
            }
            (None, Some(dir)) => {
                if spec.name.is_some() {
                    self.problem(format!(
                        "{at}: a leaf takes its name from {SET_FILE_NAME}; remove 'name'"
                    ));
                }
                let set = self.set(&self.base.join(dir).join(SET_FILE_NAME))?;
                self.sets.push(set);
                Some(Node::Leaf {
                    weight: spec.weight,
                    set: self.sets.len() - 1,
                })
            }
            (Some(_), Some(_)) => {
                self.problem(format!("{at}: use either 'sets' (a group) or 'set' (a leaf), not both"));
                None
            }
            (None, None) => {
                self.problem(format!("{at}: needs either 'sets' (a group) or 'set' (a leaf)"));
                None
            }
        }
    }

    fn set(&mut self, file: &Path) -> Option<Set> {
        if self.sets.iter().any(|s| s.file == file) {
            self.problem(format!("{} is referenced more than once", file.display()));
            return None;
        }
        let spec = read_yaml::<SetFile>(file).map_err(|e| self.problems.extend(e)).ok()?;
        let mut problems = Vec::new();
        let set = resolve_set(spec, file, self.plan, &mut problems);
        self.problems
            .extend(problems.into_iter().map(|p| format!("{}: {p}", file.display())));
        Some(set)
    }
}

fn resolve_set(spec: SetFile, file: &Path, plan: Option<&FrequencyPlan>, problems: &mut Vec<String>) -> Set {
    let dir = file.parent().unwrap_or(Path::new(""));
    if spec.name.trim().is_empty() {
        problems.push("name must not be empty".into());
    }
    if spec.playback == Playback::Sequence && spec.replay == Replay::Repeat {
        problems.push("replay: repeat requires playback: random (use exhaust or once with sequence)".into());
    }
    if spec.conversations.is_empty() {
        problems.push("conversations must not be empty".into());
    }

    let mut conv_ids = HashSet::new();
    let mut conversations = Vec::with_capacity(spec.conversations.len());
    for c in spec.conversations {
        let at = format!("conversation '{}'", c.id);
        if c.id.trim().is_empty() {
            problems.push("conversation id must not be empty".into());
        } else if !conv_ids.insert(c.id.clone()) {
            problems.push(format!("{at}: duplicate id"));
        }
        if c.weight == 0 {
            problems.push(format!("{at}: weight must be greater than 0"));
        }
        if let (Some(freq), Some(plan)) = (c.frequency, plan) {
            if plan.index_of(freq).is_none() {
                problems.push(format!("{at}: frequency {freq} is not on any network's step grid"));
            }
        }
        if c.items.is_empty() {
            problems.push(format!("{at}: items must not be empty"));
        }

        let mut item_ids = HashSet::new();
        let mut items = Vec::with_capacity(c.items.len());
        for item in c.items {
            let at = format!("{at}, item '{}'", item.id);
            if item.id.trim().is_empty() {
                problems.push(format!("{at}: id must not be empty"));
            } else if !item_ids.insert(item.id.clone()) {
                problems.push(format!("{at}: duplicate id"));
            }
            let gap_after = item.gap_after.unwrap_or(Range::new(0.0, 0.0));
            if let Err(e) = gap_after.validate() {
                problems.push(format!("{at}: gap_after: {e}"));
            }
            if item.file.is_absolute() {
                problems.push(format!("{at}: file must be relative to the set directory"));
            }
            let path = dir.join(&item.file);
            let duration = wav_duration(&path).unwrap_or_else(|e| {
                problems.push(format!("{at}: {}: {e}", path.display()));
                0.0
            });
            items.push(Item {
                id: item.id,
                caption: item.caption,
                path,
                duration,
                gap_after,
            });
        }
        conversations.push(Conversation {
            id: c.id,
            weight: c.weight,
            frequency: c.frequency,
            items,
        });
    }

    Set {
        name: spec.name,
        file: file.to_path_buf(),
        playback: spec.playback,
        replay: spec.replay,
        conversations,
    }
}

/// Duration in seconds from the WAV header, rejecting formats the spectrum analyser can't read.
pub fn wav_duration(path: &Path) -> Result<f32, String> {
    let reader = hound::WavReader::open(path).map_err(|e| e.to_string())?;
    let spec = reader.spec();
    let supported = match spec.sample_format {
        hound::SampleFormat::Int => matches!(spec.bits_per_sample, 8 | 16 | 24 | 32),
        hound::SampleFormat::Float => spec.bits_per_sample == 32,
    };
    if !supported || spec.channels == 0 || spec.sample_rate == 0 {
        return Err(format!(
            "unsupported WAV format ({:?}, {} bit, {} ch, {} Hz)",
            spec.sample_format, spec.bits_per_sample, spec.channels, spec.sample_rate
        ));
    }
    let seconds = reader.duration() as f32 / spec.sample_rate as f32;
    if seconds <= 0.0 {
        return Err("audio is empty".into());
    }
    Ok(seconds)
}

fn read_yaml<T: DeserializeOwned>(path: &Path) -> Result<T, Vec<String>> {
    let file = File::open(path).map_err(|e| vec![format!("{}: {e}", path.display())])?;
    serde_yaml::from_reader(BufReader::new(file)).map_err(|e| vec![format!("{}: {e}", path.display())])
}

/// Paths of the files that make up the world, for display.
pub fn describe(world: &World) -> String {
    let items = world.items().count();
    let audio: f32 = world.items().map(|i| i.duration).sum();
    format!(
        "{} networks ({} frequencies), {} sets, {} conversations, {} items, {:.1}s of audio",
        world.plan.network_names().count(),
        world.plan.total(),
        world.sets.len(),
        world.conversation_count(),
        items,
        audio
    )
}

#[cfg(test)]
pub mod test_support {
    //! Builds throwaway worlds on disk for tests.
    use super::*;

    pub fn write_wav(path: &Path, seconds: f32, rate: u32, channels: u16) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let spec = hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        let frames = (seconds * rate as f32) as u32;
        for n in 0..frames {
            let v = ((n as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 8000.0) as i16;
            for _ in 0..channels {
                w.write_sample(v).unwrap();
            }
        }
        w.finalize().unwrap();
    }

    pub const NETWORKS: &str = "networks:\n  - { name: a, start_freq: 1000, end_freq: 2000, step: 10 }\n";

    /// A world with one sequence set ("story", two conversations) and one random set ("noise").
    pub fn sample_world(dir: &Path) -> (PathBuf, PathBuf) {
        let net = dir.join("networks.yaml");
        std::fs::write(&net, NETWORKS).unwrap();
        let t = dir.join("transmissions");
        std::fs::create_dir_all(t.join("story")).unwrap();
        std::fs::create_dir_all(t.join("noise")).unwrap();
        let lib = t.join("sets.yaml");
        std::fs::write(
            &lib,
            "sets:\n  - name: stories\n    weight: 1\n    sets:\n      - { set: story, weight: 1 }\n  - { set: noise, weight: 1 }\n",
        )
        .unwrap();
        std::fs::write(
            t.join("story/set.yaml"),
            "name: story\nplayback: sequence\nreplay: once\nconversations:\n\
             \x20 - id: one\n    frequency: 1500\n    items:\n\
             \x20     - { id: a, caption: hello, file: a.wav, gap_after: { min: 1, max: 1 } }\n\
             \x20     - { id: b, file: b.wav }\n\
             \x20 - id: two\n    items:\n      - { id: a, file: a.wav }\n",
        )
        .unwrap();
        write_wav(&t.join("story/a.wav"), 0.5, 8000, 1);
        write_wav(&t.join("story/b.wav"), 0.25, 8000, 2);
        std::fs::write(
            t.join("noise/set.yaml"),
            "name: noise\nplayback: random\nreplay: repeat\nconversations:\n\
             \x20 - { id: hum, weight: 2, items: [ { id: h, file: h.wav } ] }\n",
        )
        .unwrap();
        write_wav(&t.join("noise/h.wav"), 0.5, 8000, 1);
        (net, lib)
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

    fn load(dir: &Path) -> Result<World, WorldErrors> {
        World::load(&dir.join("networks.yaml"), &dir.join("transmissions/sets.yaml"))
    }

    fn errors(dir: &Path) -> String {
        load(dir).err().expect("world should be invalid").to_string()
    }

    fn edit(path: &Path, from: &str, to: &str) {
        let text = std::fs::read_to_string(path).unwrap();
        assert!(text.contains(from), "fixture lacks {from:?}");
        std::fs::write(path, text.replace(from, to)).unwrap();
    }

    #[test]
    fn sample_world_loads() {
        let dir = tempfile::tempdir().unwrap();
        sample_world(dir.path());
        let world = load(dir.path()).unwrap();
        assert_eq!(world.sets.len(), 2);
        assert_eq!(world.conversation_count(), 3);
        let story = &world.sets[0];
        assert_eq!(story.playback, Playback::Sequence);
        assert_eq!(story.conversations[0].frequency, Some(1500));
        assert!((story.conversations[0].items[0].duration - 0.5).abs() < 1e-3);
        assert!(
            (story.conversations[0].items[1].duration - 0.25).abs() < 1e-3,
            "stereo duration is per-channel"
        );
        assert_eq!(
            story.conversations[0].items[0].path,
            dir.path().join("transmissions/story/a.wav")
        );
    }

    #[test]
    fn typo_in_enum_is_reported_with_path() {
        let dir = tempfile::tempdir().unwrap();
        sample_world(dir.path());
        edit(
            &dir.path().join("transmissions/story/set.yaml"),
            "playback: sequence",
            "playback: sequense",
        );
        let e = errors(dir.path());
        assert!(e.contains("story/set.yaml") && e.contains("sequense"), "{e}");
    }

    #[test]
    fn unknown_field_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        sample_world(dir.path());
        edit(
            &dir.path().join("transmissions/noise/set.yaml"),
            "weight: 2",
            "wieght: 2",
        );
        assert!(errors(dir.path()).contains("wieght"));
    }

    #[test]
    fn off_grid_frequency_missing_file_and_bad_combo_are_all_reported() {
        let dir = tempfile::tempdir().unwrap();
        sample_world(dir.path());
        let story = dir.path().join("transmissions/story/set.yaml");
        edit(&story, "frequency: 1500", "frequency: 1505");
        edit(&story, "file: b.wav", "file: missing.wav");
        edit(&story, "replay: once", "replay: repeat");
        let e = errors(dir.path());
        assert!(e.contains("1505 is not on any network"), "{e}");
        assert!(e.contains("missing.wav"), "{e}");
        assert!(e.contains("replay: repeat requires playback: random"), "{e}");
    }

    #[test]
    fn library_structure_errors() {
        let dir = tempfile::tempdir().unwrap();
        sample_world(dir.path());
        let lib = dir.path().join("transmissions/sets.yaml");
        std::fs::write(
            &lib,
            "sets:\n  - { set: story, weight: 0 }\n  - { set: story, weight: 1 }\n  - { weight: 1 }\n",
        )
        .unwrap();
        let e = errors(dir.path());
        assert!(e.contains("weight must be greater than 0"), "{e}");
        assert!(e.contains("referenced more than once"), "{e}");
        assert!(e.contains("needs either 'sets'"), "{e}");
    }

    #[test]
    fn duplicate_ids_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        sample_world(dir.path());
        edit(&dir.path().join("transmissions/story/set.yaml"), "id: two", "id: one");
        assert!(errors(dir.path()).contains("duplicate id"));
    }

    #[test]
    fn unsupported_wav_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        hound::WavWriter::create(&path, spec).unwrap().finalize().unwrap();
        assert_eq!(wav_duration(&path), Err("audio is empty".into()));
    }
}
