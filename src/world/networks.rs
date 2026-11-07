//! Radio networks and the frequency plan the scanner sweeps.
//!
//! Frequencies are never materialized: the plan maps a scan index to a frequency arithmetically.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworksFile {
    pub networks: Vec<NetworkSpec>,
}

/// One contiguous band. `end_freq` is inclusive and must sit on the step grid.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkSpec {
    pub name: String,
    pub start_freq: u64,
    pub end_freq: u64,
    pub step: u64,
}

#[derive(Clone, Debug)]
struct Band {
    name: String,
    start: u64,
    step: u64,
    count: u64,
    /// Scan index of this band's first frequency.
    offset: u64,
}

/// Ordered, non-overlapping bands. Scan index `0..total()` walks every band in file order.
#[derive(Clone, Debug)]
pub struct FrequencyPlan {
    bands: Vec<Band>,
    total: u64,
}

impl NetworkSpec {
    fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.name.trim().is_empty() {
            out.push("name must not be empty".to_string());
        }
        if self.step == 0 {
            out.push("step must be greater than 0".to_string());
        }
        if self.start_freq == 0 {
            out.push("start_freq must be greater than 0".to_string());
        }
        if self.start_freq >= self.end_freq {
            out.push(format!("start_freq ({}) must be less than end_freq ({})", self.start_freq, self.end_freq));
        } else if self.step > 0 && (self.end_freq - self.start_freq) % self.step != 0 {
            out.push(format!(
                "end_freq - start_freq ({}) must be a multiple of step ({})",
                self.end_freq - self.start_freq,
                self.step
            ));
        }
        out
    }
}

impl FrequencyPlan {
    /// Builds the plan, returning every problem found rather than the first.
    pub fn build(specs: &[NetworkSpec]) -> Result<Self, Vec<String>> {
        let mut problems = Vec::new();
        if specs.is_empty() {
            problems.push("at least one network must be defined".to_string());
        }
        for (i, spec) in specs.iter().enumerate() {
            let label = if spec.name.is_empty() { format!("networks[{i}]") } else { format!("network '{}'", spec.name) };
            problems.extend(spec.problems().into_iter().map(|p| format!("{label}: {p}")));
            for other in &specs[..i] {
                if other.name == spec.name {
                    problems.push(format!("{label}: duplicate network name"));
                }
                if spec.start_freq <= other.end_freq && other.start_freq <= spec.end_freq {
                    problems.push(format!("{label}: overlaps network '{}'", other.name));
                }
            }
        }
        if !problems.is_empty() {
            return Err(problems);
        }

        let mut bands = Vec::with_capacity(specs.len());
        let mut offset = 0;
        for spec in specs {
            let count = (spec.end_freq - spec.start_freq) / spec.step + 1;
            bands.push(Band { name: spec.name.clone(), start: spec.start_freq, step: spec.step, count, offset });
            offset += count;
        }
        Ok(Self { bands, total: offset })
    }

    /// Number of distinct scan positions.
    pub fn total(&self) -> u64 {
        self.total
    }

    /// Frequency at a scan index. Indexes wrap around.
    pub fn freq_at(&self, index: u64) -> u64 {
        let index = index % self.total;
        let band = self.band_for_index(index);
        band.start + (index - band.offset) * band.step
    }

    /// Scan index of a frequency, if it lies on the grid of some band.
    pub fn index_of(&self, freq: u64) -> Option<u64> {
        self.bands.iter().find_map(|b| {
            let end = b.start + (b.count - 1) * b.step;
            (freq >= b.start && freq <= end && (freq - b.start) % b.step == 0)
                .then(|| b.offset + (freq - b.start) / b.step)
        })
    }

    /// Name of the network whose range contains `freq` (end inclusive).
    pub fn network_of(&self, freq: u64) -> Option<&str> {
        self.bands
            .iter()
            .find(|b| freq >= b.start && freq <= b.start + (b.count - 1) * b.step)
            .map(|b| b.name.as_str())
    }

    pub fn network_names(&self) -> impl Iterator<Item = &str> {
        self.bands.iter().map(|b| b.name.as_str())
    }

    fn band_for_index(&self, index: u64) -> &Band {
        let pos = self.bands.partition_point(|b| b.offset <= index);
        &self.bands[pos - 1]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str, start: u64, end: u64, step: u64) -> NetworkSpec {
        NetworkSpec { name: name.into(), start_freq: start, end_freq: end, step }
    }

    fn plan() -> FrequencyPlan {
        FrequencyPlan::build(&[spec("pubnet", 25_000_000, 512_000_000, 1000), spec("scavnet", 758_000_000, 824_000_000, 1000)])
            .unwrap()
    }

    #[test]
    fn total_counts_inclusive_ends() {
        assert_eq!(plan().total(), 487_001 + 66_001);
    }

    #[test]
    fn index_and_freq_round_trip() {
        let p = plan();
        for idx in [0, 1, 487_000, 487_001, 487_002, p.total() - 1] {
            assert_eq!(p.index_of(p.freq_at(idx)), Some(idx));
        }
        assert_eq!(p.freq_at(0), 25_000_000);
        assert_eq!(p.freq_at(487_000), 512_000_000);
        assert_eq!(p.freq_at(487_001), 758_000_000);
        assert_eq!(p.freq_at(p.total()), 25_000_000, "index wraps");
    }

    #[test]
    fn off_grid_and_out_of_range_frequencies_have_no_index() {
        let p = plan();
        assert_eq!(p.index_of(25_000_001), None);
        assert_eq!(p.index_of(600_000_000), None);
        assert_eq!(p.index_of(10_000), None);
    }

    #[test]
    fn network_lookup_is_end_inclusive() {
        let p = plan();
        assert_eq!(p.network_of(25_000_001), Some("pubnet"));
        assert_eq!(p.network_of(512_000_000), Some("pubnet"));
        assert_eq!(p.network_of(823_000_000), Some("scavnet"));
        assert_eq!(p.network_of(10_000), None);
    }

    #[test]
    fn invalid_networks_report_every_problem() {
        let errs = FrequencyPlan::build(&[
            spec("", 0, 10, 0),
            spec("a", 100, 50, 10),
            spec("b", 100, 205, 10),
            spec("c", 1000, 2000, 10),
            spec("c", 1500, 2500, 10),
        ])
        .unwrap_err();
        let all = errs.join("\n");
        assert!(all.contains("name must not be empty"), "{all}");
        assert!(all.contains("step must be greater than 0"), "{all}");
        assert!(all.contains("start_freq must be greater than 0"), "{all}");
        assert!(all.contains("must be less than end_freq"), "{all}");
        assert!(all.contains("must be a multiple of step"), "{all}");
        assert!(all.contains("duplicate network name"), "{all}");
        assert!(all.contains("overlaps network 'c'"), "{all}");
    }

    #[test]
    fn empty_network_list_is_rejected() {
        assert!(FrequencyPlan::build(&[]).is_err());
    }
}
