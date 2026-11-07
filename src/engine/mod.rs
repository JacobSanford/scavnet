//! The broadcast engine: one `tick(now)` drives scheduling, scanning and airing.
//!
//! Time, audio output and file loading are injected, so the engine runs headless in tests.
//! Exactly one transmission is in flight at a time: requested, then loaded, then found by the
//! scanner, then aired. The gap to the next one starts when an airing ends.

pub mod log;
pub mod scanner;
pub mod timeline;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use rand::rngs::StdRng;
use rand::{Rng, RngCore, SeedableRng};

use crate::audio::loader::{LoadRequest, LoadedClip, Loader};
use crate::audio::output::{AudioOut, Clip};
use crate::audio::spectrum::BANDS;
use crate::range::Range;
use crate::settings::Settings;
use crate::world::progress::Progress;
use crate::world::World;
use log::Log;
use scanner::Scanner;
use timeline::{Segment, Timeline};

/// First transmission delay in debug mode, so there is something to watch right away.
const DEBUG_FIRST_GAP: Range = Range::new(3.0, 5.0);
/// Peak of the idle noise floor and of hiss on the 0–255 chart scale.
const NOISE_PEAK: u8 = 13;
const HISS_PEAK: u8 = 33;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Scanning,
    Receiving,
}

struct Transmission {
    ticket: u64,
    set: usize,
    conversation: usize,
    scan_index: u64,
    timeline: Timeline,
    /// `None` until the loader delivers.
    clips: Option<Vec<LoadedClip>>,
}

struct OnAir {
    tx: Transmission,
    clips: Vec<LoadedClip>,
    started: Instant,
    span: Option<usize>,
}

pub struct Engine {
    world: World,
    settings: Settings,
    progress: Progress,
    progress_path: Option<PathBuf>,
    rng: StdRng,
    noise_rng: StdRng,
    scanner: Scanner,
    next_request_at: Instant,
    pending: Option<Transmission>,
    on_air: Option<OnAir>,
    next_ticket: u64,
    log: Log,
    display: Vec<u8>,
    last_tick: Instant,
    loader: Box<dyn Loader>,
    audio: Box<dyn AudioOut>,
}

impl Engine {
    pub fn new(
        world: World,
        settings: Settings,
        progress: Progress,
        progress_path: Option<PathBuf>,
        loader: Box<dyn Loader>,
        audio: Box<dyn AudioOut>,
        now: Instant,
    ) -> Self {
        let mut rng = settings.seed.map_or_else(StdRng::from_os_rng, StdRng::seed_from_u64);
        let noise_rng = StdRng::seed_from_u64(rng.next_u64());
        let first_gap = if settings.debug {
            DEBUG_FIRST_GAP
        } else {
            settings.timings.transmission_gap
        };
        let next_request_at = now + secs(first_gap.sample(&mut rng));
        let scanner = Scanner::new(world.plan.total(), settings.scanner.frequencies_per_second);
        let mut log = Log::new(settings.debug);
        log.debug(|| format!("Debug mode on. Seed: {:?}", settings.seed));
        Self {
            world,
            settings,
            progress,
            progress_path,
            rng,
            noise_rng,
            scanner,
            next_request_at,
            pending: None,
            on_air: None,
            next_ticket: 0,
            log,
            display: vec![0; BANDS],
            last_tick: now,
            loader,
            audio,
        }
    }

    pub fn tick(&mut self, now: Instant) {
        let dt = now.saturating_duration_since(self.last_tick).as_secs_f64();
        self.last_tick = now;
        self.drain_loader(now);
        if self.on_air.is_some() {
            self.tick_on_air(now);
        } else {
            self.tick_scanning(now, dt);
        }
    }

    /// Stops audio. An unfinished airing is not recorded, so it airs again next run.
    pub fn shutdown(&mut self) {
        self.audio.stop();
    }

    pub fn status(&self) -> Status {
        if self.on_air.is_some() {
            Status::Receiving
        } else {
            Status::Scanning
        }
    }

    pub fn frequency(&self) -> u64 {
        self.world.plan.freq_at(self.scanner.position())
    }

    pub fn network(&self) -> Option<&str> {
        self.world.plan.network_of(self.frequency())
    }

    /// Current chart values, `BANDS` long, 0–255.
    pub fn spectrum(&self) -> &[u8] {
        &self.display
    }

    pub fn log(&self) -> &Log {
        &self.log
    }

    /// Caption of the item airing right now, if it has one.
    pub fn caption(&self) -> Option<&str> {
        let on_air = self.on_air.as_ref()?;
        let span = on_air.tx.timeline.spans().get(on_air.span?)?;
        let Segment::Item(i) = span.segment else { return None };
        let caption = &self.world.sets[on_air.tx.set].conversations[on_air.tx.conversation].items[i].caption;
        (!caption.is_empty()).then_some(caption.as_str())
    }

    pub fn log_info(&mut self, message: impl AsRef<str>) {
        self.log.info(message);
    }

    fn drain_loader(&mut self, now: Instant) {
        while let Some(result) = self.loader.poll() {
            let Some(pending) = self.pending.as_mut().filter(|p| p.ticket == result.ticket) else {
                continue;
            };
            match result.clips {
                Ok(clips) => {
                    self.log.debug(|| format!("Loaded transmission #{}", result.ticket));
                    pending.clips = Some(clips);
                }
                Err(e) => {
                    self.log.info(format!("Transmission failed to load: {e}"));
                    self.pending = None;
                    self.schedule_next(now);
                }
            }
        }
    }

    fn tick_scanning(&mut self, now: Instant, dt: f64) {
        if self.pending.is_none() && now >= self.next_request_at {
            self.request_next(now);
        }
        let target = self
            .pending
            .as_ref()
            .filter(|p| p.clips.is_some())
            .map(|p| p.scan_index);
        if self.scanner.sweep(dt, target) {
            self.start_airing(now);
        } else {
            self.fill_noise(NOISE_PEAK);
        }
    }

    fn request_next(&mut self, now: Instant) {
        let progress = &self.progress;
        let sets = &self.world.sets;
        let Some(set_index) = self
            .world
            .library
            .choose(&mut self.rng, |s| progress.has_next(&sets[s]))
        else {
            self.log.debug(|| "Nothing left to air.".into());
            self.schedule_next(now);
            return;
        };
        let set = &self.world.sets[set_index];
        let conversation_index = self
            .progress
            .next_conversation(set, &mut self.rng)
            .expect("has_next guaranteed a candidate");
        let conversation = &set.conversations[conversation_index];

        let scan_index = match conversation.frequency {
            Some(freq) => self.world.plan.index_of(freq).expect("validated on load"),
            None => self.rng.random_range(0..self.world.plan.total()),
        };
        let timings = &self.settings.timings;
        let preroll = timings.hiss_preroll.sample(&mut self.rng);
        let items: Vec<(f32, f32)> = conversation
            .items
            .iter()
            .map(|i| (i.duration, i.gap_after.sample(&mut self.rng)))
            .collect();
        let postroll = timings.hiss_postroll.sample(&mut self.rng);
        let timeline = Timeline::build(preroll, &items, postroll);

        let ticket = self.next_ticket;
        self.next_ticket += 1;
        let files = conversation.items.iter().map(|i| i.path.clone()).collect();
        let freq = self.world.plan.freq_at(scan_index);
        self.log
            .debug(|| format!("Queued {}/{} on {}", set.name, conversation.id, mhz(freq)));
        if let Err(e) = self.loader.request(LoadRequest { ticket, files }) {
            self.log.info(format!("Cannot load transmissions: {e}"));
            self.schedule_next(now);
            return;
        }
        self.pending = Some(Transmission {
            ticket,
            set: set_index,
            conversation: conversation_index,
            scan_index,
            timeline,
            clips: None,
        });
    }

    fn start_airing(&mut self, now: Instant) {
        let Some(mut tx) = self.pending.take() else { return };
        let clips = tx.clips.take().expect("only loaded transmissions are detectable");
        let audio: Vec<Clip> = tx
            .timeline
            .spans()
            .iter()
            .map(|span| match span.segment {
                Segment::Hiss => Clip::Hiss {
                    seconds: span.seconds(),
                },
                Segment::Item(i) => Clip::Audio {
                    bytes: clips[i].bytes.clone(),
                    seconds: span.seconds(),
                },
            })
            .collect();
        for problem in self.audio.play(audio, self.settings.volumes) {
            self.log.info(problem);
        }
        self.log
            .info(format!("Signal detected! Frequency: {}", mhz(self.frequency())));
        self.on_air = Some(OnAir {
            tx,
            clips,
            started: now,
            span: None,
        });
        self.tick_on_air(now);
    }

    fn tick_on_air(&mut self, now: Instant) {
        let Some(on_air) = self.on_air.as_mut() else { return };
        let t = now.saturating_duration_since(on_air.started).as_secs_f32();
        let Some((index, span)) = on_air.tx.timeline.at(t).map(|(i, s)| (i, *s)) else {
            self.finish_airing(now);
            return;
        };
        let entered = on_air.span != Some(index);
        on_air.span = Some(index);
        match span.segment {
            Segment::Item(i) => {
                let frame = on_air.clips[i].spectrum.frame_at(t - span.start);
                match frame {
                    Some(frame) => self.display.copy_from_slice(frame),
                    None => self.display.fill(0),
                }
                if entered {
                    let item = &self.world.sets[on_air.tx.set].conversations[on_air.tx.conversation].items[i];
                    if !item.caption.is_empty() {
                        self.log.info(format!("\u{201c}{}\u{201d}", item.caption));
                    }
                    self.log.debug(|| format!("Playing item {}", item.id));
                }
            }
            Segment::Hiss => self.fill_noise(HISS_PEAK),
        }
    }

    fn finish_airing(&mut self, now: Instant) {
        let Some(on_air) = self.on_air.take() else { return };
        self.audio.stop();
        let set = &self.world.sets[on_air.tx.set];
        self.progress.record(set, &set.conversations[on_air.tx.conversation].id);
        if let Some(path) = &self.progress_path {
            if let Err(e) = self.progress.save(path) {
                self.log.info(format!("Failed to save progress: {e:#}"));
            }
        }
        self.log.info(format!("Signal lost on {}", mhz(self.frequency())));
        self.schedule_next(now);
    }

    fn schedule_next(&mut self, now: Instant) {
        self.next_request_at = now + secs(self.settings.timings.transmission_gap.sample(&mut self.rng));
    }

    fn fill_noise(&mut self, peak: u8) {
        for v in &mut self.display {
            *v = self.noise_rng.random_range(0..=peak);
        }
    }
}

fn secs(s: f32) -> Duration {
    Duration::from_secs_f32(s)
}

pub fn mhz(freq: u64) -> String {
    format!("{:.5} MHz", freq as f64 / 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::loader::LoadResult;
    use crate::audio::output::NullOut;
    use crate::audio::spectrum::Spectrum;
    use crate::world::test_support::sample_world;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Loads synchronously; fails every request when `fail` is set.
    struct FakeLoader {
        ready: Vec<LoadResult>,
        fail: bool,
        cache: PathBuf,
    }

    impl Loader for FakeLoader {
        fn request(&mut self, request: LoadRequest) -> Result<(), String> {
            let clips = if self.fail {
                Err("disk on fire".to_string())
            } else {
                crate::audio::loader::load(&request.files, &self.cache)
            };
            self.ready.push(LoadResult {
                ticket: request.ticket,
                clips,
            });
            Ok(())
        }

        fn poll(&mut self) -> Option<LoadResult> {
            self.ready.pop()
        }
    }

    #[derive(Clone, Default)]
    struct SharedAudio(Rc<RefCell<NullOut>>);

    impl AudioOut for SharedAudio {
        fn play(&mut self, clips: Vec<Clip>, volumes: crate::settings::Volumes) -> Vec<String> {
            self.0.borrow_mut().play(clips, volumes)
        }
        fn stop(&mut self) {
            self.0.borrow_mut().stop();
        }
    }

    struct Harness {
        engine: Engine,
        audio: SharedAudio,
        now: Instant,
        _dir: tempfile::TempDir,
    }

    fn harness(seed: u64, fail: bool) -> Harness {
        let dir = tempfile::tempdir().unwrap();
        let (net, lib) = sample_world(dir.path());
        let world = World::load(&net, &lib).unwrap();
        let mut settings = Settings::parse(&format!(
            "seed = {seed}\n[scanner]\nfrequencies_per_second = 1000\n[timings]\n\
             transmission_gap = {{ min = 10.0, max = 10.0 }}\nhiss_preroll = {{ min = 0.5, max = 0.5 }}\n\
             hiss_postroll = {{ min = 0.5, max = 0.5 }}\n"
        ))
        .unwrap();
        settings.paths.cache_dir = dir.path().join("cache");
        let audio = SharedAudio::default();
        let loader = FakeLoader {
            ready: Vec::new(),
            fail,
            cache: settings.paths.cache_dir.clone(),
        };
        let now = Instant::now();
        let progress_path = Some(dir.path().join("state/progress.yaml"));
        let engine = Engine::new(
            world,
            settings,
            Progress::default(),
            progress_path,
            Box::new(loader),
            Box::new(audio.clone()),
            now,
        );
        Harness {
            engine,
            audio,
            now,
            _dir: dir,
        }
    }

    impl Harness {
        fn advance(&mut self, seconds: f32) {
            let end = self.now + secs(seconds);
            while self.now < end {
                self.now = (self.now + Duration::from_millis(50)).min(end);
                self.engine.tick(self.now);
            }
        }

        /// Runs until the next airing ends; returns "set/conversation@freq".
        fn next_airing(&mut self) -> String {
            for _ in 0..2000 {
                self.advance(0.1);
                if let Some(on_air) = &self.engine.on_air {
                    let set = &self.engine.world.sets[on_air.tx.set];
                    let label = format!(
                        "{}/{}@{}",
                        set.name,
                        set.conversations[on_air.tx.conversation].id,
                        self.engine.frequency()
                    );
                    while self.engine.on_air.is_some() {
                        self.advance(0.1);
                    }
                    return label;
                }
            }
            panic!("nothing aired within 200 s");
        }
    }

    #[test]
    fn scan_detect_air_and_return() {
        let mut h = harness(1, false);
        h.advance(9.9);
        assert!(h.engine.pending.is_none(), "nothing requested before the gap");
        assert_eq!(h.engine.status(), Status::Scanning);
        h.advance(0.3);
        assert!(h.engine.pending.is_some() || h.engine.on_air.is_some());
        h.advance(0.5);
        assert_eq!(
            h.engine.status(),
            Status::Receiving,
            "1000 freq/s sweeps the 101-step plan in 0.1 s"
        );
        assert_eq!(h.audio.0.borrow().plays, 1);
        let total = h.engine.on_air.as_ref().unwrap().tx.timeline.total();
        h.advance(total + 0.1);
        assert_eq!(h.engine.status(), Status::Scanning);
        assert_eq!(h.audio.0.borrow().stops, 1);
        assert!(h.engine.log.contains("Signal lost"));
    }

    #[test]
    fn fixed_frequency_is_where_the_scanner_stops() {
        let mut h = harness(3, false);
        let mut seen_story = false;
        for _ in 0..8 {
            let airing = h.next_airing();
            if airing.starts_with("story/one") {
                assert!(airing.ends_with("@1500"), "{airing}");
                seen_story = true;
            }
        }
        assert!(seen_story);
    }

    #[test]
    fn sequence_once_story_airs_in_order_then_stops() {
        let mut h = harness(5, false);
        let story: Vec<String> = (0..20)
            .map(|_| h.next_airing())
            .filter(|a| a.starts_with("story/"))
            .map(|a| a[..9].to_string())
            .collect();
        assert_eq!(story, ["story/one", "story/two"]);
    }

    #[test]
    fn same_seed_same_broadcast() {
        let run = |seed| {
            let mut h = harness(seed, false);
            (0..6).map(|_| h.next_airing()).collect::<Vec<_>>()
        };
        assert_eq!(run(11), run(11));
        assert_ne!(run(11), run(12));
    }

    #[test]
    fn shutdown_mid_airing_does_not_record_progress() {
        let mut h = harness(1, false);
        while h.engine.on_air.is_none() {
            h.advance(0.1);
        }
        h.engine.shutdown();
        assert_eq!(h.audio.0.borrow().stops, 1);
        assert_eq!(h.engine.progress, Progress::default());
    }

    #[test]
    fn progress_is_saved_after_airing() {
        let mut h = harness(5, false);
        while !h.next_airing().starts_with("story/") {}
        let saved = Progress::load(h.engine.progress_path.as_ref().unwrap()).unwrap();
        assert_eq!(saved.played("story"), ["one"]);
    }

    #[test]
    fn load_failure_is_logged_and_scanning_continues() {
        let mut h = harness(1, true);
        h.advance(12.0);
        assert!(h.engine.log.contains("disk on fire"));
        assert!(h.engine.pending.is_none());
        assert_eq!(h.engine.status(), Status::Scanning);
        assert!(h.engine.next_request_at > h.now, "retries after another gap");
        h.advance(10.0);
        assert_eq!(h.audio.0.borrow().plays, 0);
    }

    #[test]
    fn chart_follows_item_spectrum() {
        let mut h = harness(1, false);
        while h.engine.on_air.is_none() {
            h.advance(0.05);
        }
        // Move past the 0.5 s preroll into the first item.
        h.advance(0.6);
        let on_air = h.engine.on_air.as_ref().unwrap();
        let span = on_air.tx.timeline.spans()[on_air.span.unwrap()];
        let Segment::Item(i) = span.segment else {
            panic!("expected an item to be playing")
        };
        let spectrum: &Spectrum = &on_air.clips[i].spectrum;
        let t = h.now.duration_since(on_air.started).as_secs_f32() - span.start;
        assert_eq!(h.engine.spectrum(), spectrum.frame_at(t).unwrap());
        assert!(
            h.engine.spectrum().iter().any(|&v| v > HISS_PEAK),
            "a 440 Hz tone stands out of the noise"
        );
    }
}
