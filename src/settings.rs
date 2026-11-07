//! Engine settings, read from a TOML file. Every key is optional; unknown keys are errors.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::range::Range;

pub const DEFAULT_SETTINGS_FILE: &str = "Settings.toml";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Settings {
    /// Shows debug log lines and schedules the first transmission within seconds.
    pub debug: bool,
    /// Fixed RNG seed for reproducible runs. Omit for a random seed.
    pub seed: Option<u64>,
    pub paths: Paths,
    pub display: Display,
    pub scanner: Scanner,
    pub timings: Timings,
    pub volumes: Volumes,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Paths {
    /// Root of the git-tracked world.
    pub data_dir: PathBuf,
    /// Network definitions, relative to `data_dir`.
    pub networks: PathBuf,
    /// Weighted set tree, relative to `data_dir`.
    pub library: PathBuf,
    /// Mutable playback progress. Not tracked in git.
    pub state_dir: PathBuf,
    /// Derived data such as spectrum profiles. Safe to delete.
    pub cache_dir: PathBuf,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Display {
    pub redraw_hz: u32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Scanner {
    pub frequencies_per_second: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Timings {
    pub transmission_gap: Range,
    pub hiss_preroll: Range,
    pub hiss_postroll: Range,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Volumes {
    pub transmission: f32,
    pub hiss: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            debug: false,
            seed: None,
            paths: Paths::default(),
            display: Display::default(),
            scanner: Scanner::default(),
            timings: Timings::default(),
            volumes: Volumes::default(),
        }
    }
}

impl Default for Paths {
    fn default() -> Self {
        Self {
            data_dir: "data".into(),
            networks: "networks.yaml".into(),
            library: "transmissions/sets.yaml".into(),
            state_dir: "state".into(),
            cache_dir: "cache".into(),
        }
    }
}

impl Default for Display {
    fn default() -> Self {
        Self { redraw_hz: 30 }
    }
}

impl Default for Scanner {
    fn default() -> Self {
        Self { frequencies_per_second: 80_000 }
    }
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            transmission_gap: Range::new(120.0, 240.0),
            hiss_preroll: Range::new(0.7, 1.1),
            hiss_postroll: Range::new(0.5, 2.0),
        }
    }
}

impl Default for Volumes {
    fn default() -> Self {
        Self { transmission: 1.0, hiss: 0.05 }
    }
}

impl Settings {
    /// Loads settings. With no explicit path, a missing `Settings.toml` means defaults.
    /// Relative paths inside the file resolve against the file's directory.
    pub fn load(explicit: Option<&Path>) -> Result<Self> {
        let path = explicit.unwrap_or(Path::new(DEFAULT_SETTINGS_FILE));
        let mut settings = match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text).with_context(|| format!("{}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && explicit.is_none() => Self::default(),
            Err(e) => return Err(e).with_context(|| format!("Failed to read {}", path.display())),
        };
        let base = path.parent().unwrap_or(Path::new(""));
        settings.paths.resolve_against(base);
        Ok(settings)
    }

    pub fn parse(text: &str) -> Result<Self> {
        let settings: Settings = toml::from_str(text)?;
        settings.validate()?;
        Ok(settings)
    }

    pub fn validate(&self) -> Result<()> {
        if !(1..=240).contains(&self.display.redraw_hz) {
            bail!("display.redraw_hz must be between 1 and 240 (got {})", self.display.redraw_hz);
        }
        if self.scanner.frequencies_per_second == 0 {
            bail!("scanner.frequencies_per_second must be greater than 0");
        }
        let ranges = [
            ("timings.transmission_gap", &self.timings.transmission_gap),
            ("timings.hiss_preroll", &self.timings.hiss_preroll),
            ("timings.hiss_postroll", &self.timings.hiss_postroll),
        ];
        for (name, range) in ranges {
            range.validate().map_err(|e| anyhow::anyhow!("{name}: {e}"))?;
        }
        for (name, v) in [("volumes.transmission", self.volumes.transmission), ("volumes.hiss", self.volumes.hiss)] {
            if !(0.0..=4.0).contains(&v) {
                bail!("{name} must be between 0.0 and 4.0 (got {v})");
            }
        }
        Ok(())
    }

    pub fn networks_path(&self) -> PathBuf {
        self.paths.data_dir.join(&self.paths.networks)
    }

    pub fn library_path(&self) -> PathBuf {
        self.paths.data_dir.join(&self.paths.library)
    }
}

impl Paths {
    fn resolve_against(&mut self, base: &Path) {
        for p in [&mut self.data_dir, &mut self.state_dir, &mut self.cache_dir] {
            if p.is_relative() {
                *p = base.join(&*p);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_gives_defaults() {
        let s = Settings::parse("").unwrap();
        assert_eq!(s.display.redraw_hz, 30);
        assert_eq!(s.timings.transmission_gap, Range::new(120.0, 240.0));
    }

    #[test]
    fn nested_timings_are_read() {
        let s = Settings::parse("[timings]\ntransmission_gap = { min = 1.0, max = 2.0 }\n").unwrap();
        assert_eq!(s.timings.transmission_gap, Range::new(1.0, 2.0));
        assert_eq!(s.timings.hiss_preroll, Timings::default().hiss_preroll);
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(Settings::parse("transmission_gap_min_time = 3\n").is_err());
        assert!(Settings::parse("[timings]\nhiss = { min = 1.0, max = 2.0 }\n").is_err());
    }

    #[test]
    fn invalid_values_are_rejected() {
        assert!(Settings::parse("[timings]\nhiss_preroll = { min = 2.0, max = 1.0 }\n").is_err());
        assert!(Settings::parse("[display]\nredraw_hz = 0\n").is_err());
        assert!(Settings::parse("[scanner]\nfrequencies_per_second = 0\n").is_err());
        assert!(Settings::parse("[volumes]\nhiss = -1.0\n").is_err());
    }

    #[test]
    fn relative_paths_resolve_against_settings_dir() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("custom.toml");
        std::fs::write(&file, "[paths]\ndata_dir = \"world\"\n").unwrap();
        let s = Settings::load(Some(&file)).unwrap();
        assert_eq!(s.paths.data_dir, dir.path().join("world"));
        assert_eq!(s.paths.state_dir, dir.path().join("state"));
    }

    #[test]
    fn explicit_missing_file_is_an_error() {
        assert!(Settings::load(Some(Path::new("/nonexistent/settings.toml"))).is_err());
    }
}
