//! Audio output. The engine hands over a whole transmission at once and keeps time itself.

use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use rodio::cpal::SampleRate;
use rodio::source::{Source, WhiteNoise, Zero};
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};

use crate::settings::Volumes;

const HISS_SAMPLE_RATE: u32 = 44_100;

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
    _stream: OutputStream,
    handle: OutputStreamHandle,
    sink: Option<Sink>,
}

impl RodioOut {
    pub fn open() -> Result<Self, String> {
        let (stream, handle) = OutputStream::try_default().map_err(|e| e.to_string())?;
        Ok(Self { _stream: stream, handle, sink: None })
    }
}

impl AudioOut for RodioOut {
    fn play(&mut self, clips: Vec<Clip>, volumes: Volumes) -> Vec<String> {
        self.stop();
        let sink = match Sink::try_new(&self.handle) {
            Ok(sink) => sink,
            Err(e) => return vec![format!("Audio output failed: {e}")],
        };
        let mut problems = Vec::new();
        for clip in clips {
            match clip {
                Clip::Hiss { seconds } => sink.append(
                    WhiteNoise::new(SampleRate(HISS_SAMPLE_RATE))
                        .take_duration(Duration::from_secs_f32(seconds))
                        .amplify(volumes.hiss),
                ),
                Clip::Audio { bytes, seconds } => match Decoder::new(Cursor::new(bytes)) {
                    Ok(source) => sink.append(source.convert_samples::<f32>().amplify(volumes.transmission)),
                    Err(e) => {
                        // Keep the timeline in step with the display: play silence instead.
                        problems.push(format!("Failed to decode audio: {e}"));
                        sink.append(
                            Zero::<f32>::new(1, HISS_SAMPLE_RATE).take_duration(Duration::from_secs_f32(seconds)),
                        );
                    }
                },
            }
        }
        self.sink = Some(sink);
        problems
    }

    fn stop(&mut self) {
        if let Some(sink) = self.sink.take() {
            sink.stop();
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
