//! Audio output. The engine hands over a whole transmission at once and keeps time itself.

use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use rodio::source::{Source, WhiteUniform, Zero};
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
        let player = Player::connect_new(self.device.mixer());
        let mut problems = Vec::new();
        for clip in clips {
            match clip {
                Clip::Hiss { seconds } => player.append(
                    WhiteUniform::new(HISS_SAMPLE_RATE)
                        .take_duration(Duration::from_secs_f32(seconds))
                        .amplify(volumes.hiss),
                ),
                Clip::Audio { bytes, seconds } => match Decoder::new(Cursor::new(bytes)) {
                    Ok(source) => player.append(source.amplify(volumes.transmission)),
                    Err(e) => {
                        // Keep the timeline in step with the display: play silence instead.
                        problems.push(format!("Failed to decode audio: {e}"));
                        player
                            .append(Zero::new(MONO, HISS_SAMPLE_RATE).take_duration(Duration::from_secs_f32(seconds)));
                    }
                },
            }
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
}
