use std::time::Duration;
use quanta::Instant;
use rand::rngs::StdRng;
use rand::Rng;

pub fn rand_time_from_now(rng: &mut StdRng, min: f32, max: f32) -> Instant {
    let delay = rng.gen_range(min..max);
    let delay_micros = (delay * 1_000_000.0) as u64;
    Instant::now() + Duration::from_micros(delay_micros)
}

pub fn rand_time_secs(rng: &mut StdRng, min: f32, max: f32) -> f32 {
    rng.gen_range(min..max)
}
