//! Audio output. The engine hands over a whole transmission at once and keeps time itself.

use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use rodio::source::{Source, UniformSourceIterator, WhiteUniform, Zero};
use rodio::{nz, ChannelCount, Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, SampleRate};

use crate::settings::Volumes;

const HISS_SAMPLE_RATE: SampleRate = nz!(44_100);
const MONO: ChannelCount = nz!(1);

pub enum Clip {
    Hiss { seconds: f32 },
    Audio { bytes: Arc<[u8]>, seconds: f32 },
}

pub trait AudioOut {
    /// Replaces anything playing with `clips`, back to back. Returns non-fatal problems.
    fn play(&mut self, clips: Vec<Clip>, volumes: Volumes) -> Vec<String>;
    fn stop(&mut self);
}

pub struct RodioOut {
    device: MixerDeviceSink,
    player: Option<Player>,
}

impl RodioOut {
    pub fn open() -> Result<Self, String> {
        let mut device = DeviceSinkBuilder::open_default_sink().map_err(|e| e.to_string())?;
        // The default drop message would print over the restored terminal.
        device.log_on_drop(false);
        Ok(Self { device, player: None })
    }
}

impl AudioOut for RodioOut {
    fn play(&mut self, clips: Vec<Clip>, volumes: Volumes) -> Vec<String> {
        self.stop();
        let config = self.device.config();
        let (sources, problems) = sources(clips, volumes, config.channel_count(), config.sample_rate());
        let player = Player::connect_new(self.device.mixer());
        for source in sources {
            player.append(source);
        }
        self.player = Some(player);
        problems
    }

    fn stop(&mut self) {
        if let Some(player) = self.player.take() {
            player.stop();
        }
    }
}

type BoxedSource = Box<dyn Source + Send>;

/// Builds one source per clip, all converted to the output format.
///
/// rodio's queue misses a format change when an item is followed by an endless source cut with
/// `take_duration` (the hiss): the hiss is then read in the item's format, so 44.1 kHz mono hiss
/// after a 48 kHz stereo item plays for less than half its length and the display falls behind.
/// Giving every clip the same format means the queue never has to notice a change.
fn sources(
    clips: Vec<Clip>,
    volumes: Volumes,
    channels: ChannelCount,
    rate: SampleRate,
) -> (Vec<BoxedSource>, Vec<String>) {
    let uniform = |source: BoxedSource| -> BoxedSource { Box::new(UniformSourceIterator::new(source, channels, rate)) };
    let mut problems = Vec::new();
    let sources = clips
        .into_iter()
        .map(|clip| match clip {
            Clip::Hiss { seconds } => uniform(Box::new(
                WhiteUniform::new(HISS_SAMPLE_RATE)
                    .take_duration(Duration::from_secs_f32(seconds))
                    .amplify(volumes.hiss),
            )),
            Clip::Audio { bytes, seconds } => match Decoder::new(Cursor::new(bytes)) {
                Ok(source) => uniform(Box::new(source.amplify(volumes.transmission))),
                Err(e) => {
                    // Keep the timeline in step with the display: play silence instead.
                    problems.push(format!("Failed to decode audio: {e}"));
                    uniform(Box::new(
                        Zero::new(MONO, HISS_SAMPLE_RATE).take_duration(Duration::from_secs_f32(seconds)),
                    ))
                }
            },
        })
        .collect();
    (sources, problems)
}

/// Used when no audio device is available, and in tests.
#[derive(Default)]
pub struct NullOut {
    pub plays: usize,
    pub stops: usize,
}

impl AudioOut for NullOut {
    fn play(&mut self, _clips: Vec<Clip>, _volumes: Volumes) -> Vec<String> {
        self.plays += 1;
        Vec::new()
    }

    fn stop(&mut self) {
        self.stops += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::test_support::write_wav;

    #[test]
    fn decoder_reads_world_wavs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        write_wav(&path, 0.5, 24_000, 2);
        let bytes: Arc<[u8]> = std::fs::read(&path).unwrap().into();
        let source = Decoder::new(Cursor::new(bytes)).unwrap();
        assert_eq!(source.channels().get(), 2);
        assert_eq!(source.sample_rate().get(), 24_000);
        let samples: Vec<f32> = source.collect();
        assert_eq!(samples.len(), 12_000 * 2);
        assert!(samples.iter().any(|s| s.abs() > 0.1));
    }

    #[test]
    fn hiss_and_silence_last_their_duration() {
        let hiss = WhiteUniform::new(HISS_SAMPLE_RATE).take_duration(Duration::from_millis(100));
        assert_eq!(hiss.count(), 4_410);
        let zero = Zero::new(MONO, HISS_SAMPLE_RATE).take_duration(Duration::from_millis(100));
        assert_eq!(zero.count(), 4_410);
    }

    /// Seconds of output a mixer in the given format produces for `clips`.
    fn mixed_seconds(clips: Vec<Clip>, channels: ChannelCount, rate: SampleRate) -> f64 {
        let (mixer, mut out) = rodio::mixer::mixer(channels, rate);
        let player = Player::connect_new(&mixer);
        let volumes = Volumes {
            transmission: 1.0,
            hiss: 1.0,
        };
        for source in sources(clips, volumes, channels, rate).0 {
            player.append(source);
        }
        let mut samples = 0u64;
        while player.len() > 0 && out.next().is_some() {
            samples += 1;
        }
        samples as f64 / (f64::from(channels.get()) * f64::from(rate.get()))
    }

    #[test]
    fn mixed_output_keeps_every_clip_length() {
        let dir = tempfile::tempdir().unwrap();
        let stereo = dir.path().join("stereo.wav");
        let mono = dir.path().join("mono.wav");
        write_wav(&stereo, 1.5, 48_000, 2);
        write_wav(&mono, 1.0, 24_000, 1);
        let audio = |p: &std::path::Path, seconds| Clip::Audio {
            bytes: std::fs::read(p).unwrap().into(),
            seconds,
        };
        let clips = || {
            vec![
                Clip::Hiss { seconds: 1.0 },
                audio(&stereo, 1.5),
                Clip::Hiss { seconds: 3.0 },
                audio(&mono, 1.0),
                Clip::Hiss { seconds: 2.0 },
                Clip::Audio {
                    bytes: Arc::from(&b"not a wav"[..]),
                    seconds: 0.5,
                },
            ]
        };
        for (channels, rate) in [(nz!(2), nz!(48_000)), (nz!(2), nz!(44_100)), (nz!(1), nz!(22_050))] {
            let got = mixed_seconds(clips(), channels, rate);
            assert!(
                (got - 9.0).abs() < 0.02,
                "{channels}ch {rate} Hz: {got:.3}s, expected 9.0s"
            );
        }
    }
}
