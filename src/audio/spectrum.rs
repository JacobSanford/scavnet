//! Spectrum profiles: what the SIGNAL chart shows while a clip plays.
//!
//! A profile is a sequence of frames, each `BANDS` bytes (0–255) of log-spaced band energy in dB,
//! normalized against the loudest point in the file. Profiles are derived data, cached under
//! `cache_dir` and keyed by file identity plus the analysis parameters, so changing either
//! simply produces a new cache entry.

use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use spectrum_analyzer::windows::hann_window;
use spectrum_analyzer::{samples_fft_to_spectrum, FrequencyLimit};

pub const BANDS: usize = 64;
pub const MIN_HZ: f32 = 80.0;
pub const MAX_HZ: f32 = 8000.0;
const WINDOW: usize = 1024;
/// Dynamic range shown on the chart: the loudest band maps to 255, `FLOOR_DB` below it to 0.
const FLOOR_DB: f32 = 60.0;
const MAGIC: &[u8; 4] = b"SVSP";

#[derive(Clone, Debug, PartialEq)]
pub struct Spectrum {
    pub frame_secs: f32,
    frames: Arc<[u8]>,
}

impl Spectrum {
    pub fn frame_count(&self) -> usize {
        self.frames.len() / BANDS
    }

    /// The frame playing `secs` into the clip, if any.
    pub fn frame_at(&self, secs: f32) -> Option<&[u8]> {
        if !(secs >= 0.0) {
            return None;
        }
        let i = (secs / self.frame_secs) as usize;
        self.frames.get(i * BANDS..(i + 1) * BANDS)
    }

    /// Reads a cached profile for `wav`, analysing and caching it on a miss.
    /// A cache that cannot be written is not an error; the profile is still returned.
    pub fn load_or_analyze(wav: &Path, cache_dir: &Path) -> Result<Self> {
        let cache_file = cache_path(wav, cache_dir)?;
        if let Some(cached) = read_cache(&cache_file) {
            return Ok(cached);
        }
        let spectrum = Self::analyze(wav)?;
        let _ = write_cache(&cache_file, &spectrum);
        Ok(spectrum)
    }

    pub fn analyze(wav: &Path) -> Result<Self> {
        let (samples, rate) = read_mono(wav).with_context(|| format!("Failed to read {}", wav.display()))?;
        Ok(analyze_samples(&samples, rate))
    }
}

/// Mono samples in -1.0..=1.0 for any integer or 32-bit float WAV.
fn read_mono(path: &Path) -> Result<(Vec<f32>, u32)> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let channels = usize::from(spec.channels);
    if channels == 0 {
        bail!("WAV has no channels");
    }
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().map(|s| s.map(|v| v as f32 * scale)).collect::<Result<_, _>>()?
        }
    };
    let mono = interleaved.chunks_exact(channels).map(|frame| frame.iter().sum::<f32>() / channels as f32).collect();
    Ok((mono, spec.sample_rate))
}

fn analyze_samples(samples: &[f32], rate: u32) -> Spectrum {
    let frame_secs = WINDOW as f32 / rate as f32;
    let bands = band_bins(rate);
    let mut magnitudes = Vec::with_capacity(samples.len().div_ceil(WINDOW) * BANDS);
    let mut window = vec![0.0f32; WINDOW];

    for chunk in samples.chunks(WINDOW) {
        window[..chunk.len()].copy_from_slice(chunk);
        window[chunk.len()..].fill(0.0);
        let bins: Vec<f32> = match samples_fft_to_spectrum(&hann_window(&window), rate, FrequencyLimit::All, None) {
            Ok(s) => s.data().iter().map(|(_, v)| v.val()).collect(),
            Err(_) => vec![0.0; WINDOW / 2 + 1],
        };
        for &(lo, hi) in &bands {
            let band = bins.get(lo..hi).unwrap_or_default();
            magnitudes.push(band.iter().copied().fold(0.0f32, f32::max));
        }
    }

    let peak = magnitudes.iter().copied().fold(0.0f32, f32::max);
    let frames: Vec<u8> = if peak <= 1e-9 {
        vec![0; magnitudes.len()]
    } else {
        let peak_db = 20.0 * peak.log10();
        magnitudes
            .iter()
            .map(|&m| {
                let db = 20.0 * m.max(1e-12).log10();
                (((db - (peak_db - FLOOR_DB)) / FLOOR_DB).clamp(0.0, 1.0) * 255.0).round() as u8
            })
            .collect()
    };
    Spectrum { frame_secs, frames: frames.into() }
}

/// FFT bin range `[lo, hi)` for each log-spaced band. Narrow low bands get at least one bin.
fn band_bins(rate: u32) -> Vec<(usize, usize)> {
    let hz_per_bin = rate as f32 / WINDOW as f32;
    let top = MAX_HZ.min(rate as f32 / 2.0).max(MIN_HZ * 2.0);
    let ratio = (top / MIN_HZ).powf(1.0 / BANDS as f32);
    let last_bin = WINDOW / 2;
    (0..BANDS)
        .map(|b| {
            let lo_hz = MIN_HZ * ratio.powi(b as i32);
            let hi_hz = lo_hz * ratio;
            let lo = ((lo_hz / hz_per_bin).round() as usize).min(last_bin);
            let hi = ((hi_hz / hz_per_bin).round() as usize).clamp(lo + 1, last_bin + 1);
            (lo, hi)
        })
        .collect()
}

fn cache_path(wav: &Path, cache_dir: &Path) -> Result<PathBuf> {
    let meta = std::fs::metadata(wav).with_context(|| format!("Failed to stat {}", wav.display()))?;
    let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).unwrap_or_default();
    let canonical = wav.canonicalize().unwrap_or_else(|_| wav.to_path_buf());
    let identity = format!(
        "{}|{}|{}|{WINDOW}|{BANDS}|{MIN_HZ}|{MAX_HZ}|{FLOOR_DB}",
        canonical.display(),
        meta.len(),
        mtime.as_nanos()
    );
    Ok(cache_dir.join("spectrum").join(format!("{:016x}.bin", fnv1a(identity.as_bytes()))))
}

/// Stable across builds and platforms, unlike `std::hash::DefaultHasher`.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3))
}

fn read_cache(path: &Path) -> Option<Spectrum> {
    let mut buf = Vec::new();
    BufReader::new(File::open(path).ok()?).read_to_end(&mut buf).ok()?;
    let (header, frames) = buf.split_at_checked(12)?;
    let frame_secs = f32::from_le_bytes(header[4..8].try_into().ok()?);
    let bands = u32::from_le_bytes(header[8..12].try_into().ok()?) as usize;
    let valid = &header[..4] == MAGIC && bands == BANDS && frames.len() % BANDS == 0 && frame_secs > 0.0;
    valid.then(|| Spectrum { frame_secs, frames: frames.into() })
}

fn write_cache(path: &Path, spectrum: &Spectrum) -> Result<()> {
    std::fs::create_dir_all(path.parent().unwrap_or(Path::new("")))?;
    let tmp = path.with_extension("tmp");
    let mut f = File::create(&tmp)?;
    f.write_all(MAGIC)?;
    f.write_all(&spectrum.frame_secs.to_le_bytes())?;
    f.write_all(&(BANDS as u32).to_le_bytes())?;
    f.write_all(&spectrum.frames)?;
    f.sync_all()?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::test_support::write_wav;

    fn sine(hz: f32, rate: u32, secs: f32) -> Vec<f32> {
        (0..(rate as f32 * secs) as usize).map(|n| (n as f32 * hz * std::f32::consts::TAU / rate as f32).sin()).collect()
    }

    fn peak_band(frame: &[u8]) -> usize {
        frame.iter().enumerate().max_by_key(|(_, &v)| v).unwrap().0
    }

    fn band_of(hz: f32, rate: u32) -> usize {
        let per_bin = rate as f32 / WINDOW as f32;
        let bin = (hz / per_bin).round() as usize;
        band_bins(rate).iter().position(|&(lo, hi)| bin >= lo && bin < hi).unwrap()
    }

    #[test]
    fn sine_peaks_in_its_band() {
        for (hz, rate) in [(440.0, 24_000), (2000.0, 48_000), (150.0, 8000)] {
            let s = analyze_samples(&sine(hz, rate, 0.5), rate);
            let frame = s.frame_at(0.1).unwrap();
            assert_eq!(peak_band(frame), band_of(hz, rate), "{hz} Hz at {rate}");
            assert_eq!(frame[peak_band(frame)], 255);
        }
    }

    #[test]
    fn higher_tones_land_in_higher_bands() {
        let low = analyze_samples(&sine(300.0, 24_000, 0.2), 24_000);
        let high = analyze_samples(&sine(3000.0, 24_000, 0.2), 24_000);
        assert!(peak_band(high.frame_at(0.0).unwrap()) > peak_band(low.frame_at(0.0).unwrap()));
    }

    #[test]
    fn silence_is_all_zero_not_nan() {
        let s = analyze_samples(&vec![0.0; 4096], 8000);
        assert!(s.frames.iter().all(|&v| v == 0));
        assert_eq!(s.frame_count(), 4);
    }

    #[test]
    fn frame_lookup_bounds() {
        let s = analyze_samples(&sine(440.0, 8000, 1.0), 8000);
        assert_eq!(s.frame_count(), 8000usize.div_ceil(WINDOW));
        assert!(s.frame_at(0.0).is_some());
        assert!(s.frame_at(0.99).is_some());
        assert!(s.frame_at(5.0).is_none());
        assert!(s.frame_at(-1.0).is_none());
        assert!(s.frame_at(f32::NAN).is_none());
    }

    #[test]
    fn stereo_equals_mono() {
        let dir = tempfile::tempdir().unwrap();
        write_wav(&dir.path().join("m.wav"), 0.5, 8000, 1);
        write_wav(&dir.path().join("s.wav"), 0.5, 8000, 2);
        let m = Spectrum::analyze(&dir.path().join("m.wav")).unwrap();
        let s = Spectrum::analyze(&dir.path().join("s.wav")).unwrap();
        assert_eq!(m, s);
    }

    #[test]
    fn float_and_24_bit_wavs_are_read() {
        let dir = tempfile::tempdir().unwrap();
        for (bits, format) in [(24, hound::SampleFormat::Int), (32, hound::SampleFormat::Float)] {
            let path = dir.path().join(format!("{bits}.wav"));
            let spec = hound::WavSpec { channels: 1, sample_rate: 8000, bits_per_sample: bits, sample_format: format };
            let mut w = hound::WavWriter::create(&path, spec).unwrap();
            for v in sine(440.0, 8000, 0.25) {
                match format {
                    hound::SampleFormat::Float => w.write_sample(v).unwrap(),
                    hound::SampleFormat::Int => w.write_sample((v * 4_000_000.0) as i32).unwrap(),
                }
            }
            w.finalize().unwrap();
            let s = Spectrum::analyze(&path).unwrap();
            assert_eq!(peak_band(s.frame_at(0.0).unwrap()), band_of(440.0, 8000), "{bits}-bit");
        }
    }

    #[test]
    fn cache_round_trip_and_invalidation() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("a.wav");
        let cache = dir.path().join("cache");
        write_wav(&wav, 0.5, 8000, 1);
        let first = Spectrum::load_or_analyze(&wav, &cache).unwrap();
        let file = cache_path(&wav, &cache).unwrap();
        assert!(file.exists());
        assert_eq!(read_cache(&file).unwrap(), first);

        // A corrupt cache entry is ignored and rewritten.
        std::fs::write(&file, b"junk").unwrap();
        assert_eq!(Spectrum::load_or_analyze(&wav, &cache).unwrap(), first);
        assert_eq!(read_cache(&file).unwrap(), first);

        // Changing the file changes the key.
        write_wav(&wav, 1.0, 8000, 1);
        assert_ne!(cache_path(&wav, &cache).unwrap(), file);
    }
}
