use config::Config;

use super::super::SETTINGS;

const SCREEN_REDRAW_RATE: u128 = 288;
const DATA_DIR: &str = "data";
const DEBUG_STATUS: bool = false;
const NETWORK_LIBRARY_PATH: &str = "networks.yaml";
const TRANSMISSION_LIBRARY_PATH: &str = "transmissions/sets.yaml";
const TRANSMISSION_GAP_MIN_TIME: f32 = 120.0;
const TRANSMISSION_GAP_MAX_TIME: f32 = 240.0;
const HISS_PREROLL_MIN_TIME: f32 = 0.7;
const HISS_PREROLL_MAX_TIME: f32 = 1.1;
const HISS_POSTROLL_MIN_TIME: f32 = 0.5;
const HISS_POSTROLL_MAX_TIME: f32 = 2.0;
const TRANSMISSION_SINK_VOLUME: f32 = 1.0;
const HISS_SINK_VOLUME: f32 = 0.3;

pub fn init_settings() -> (u128, bool) {
    load_settings();

    let screen_redraw_rate = SETTINGS.lock()
        .get_int("performance.screen_redraw_rate")
        .unwrap_or(SCREEN_REDRAW_RATE as i64) as u128;

    let debug = SETTINGS.lock()
        .get_bool("debug")
        .unwrap_or(DEBUG_STATUS);

    (screen_redraw_rate, debug)
}

pub fn load_settings() {
    *SETTINGS.lock() = Config::builder()
        .add_source(config::File::with_name("Settings"))
        .build()
        .unwrap();
}

pub fn network_library_path() -> String {
    let data_dir = get_data_dir();
    let network_library_base_path = SETTINGS.lock()
        .get_string("paths.network_library")
        .unwrap_or(NETWORK_LIBRARY_PATH.to_string());
    let network_library_path = format!("{}/{}", data_dir, network_library_base_path);
    network_library_path
}

pub fn transmission_library_path() -> String {
    let data_dir = get_data_dir();
    let transmission_library_base_path = SETTINGS.lock()
        .get_string("paths.transmission_library")
        .unwrap_or(TRANSMISSION_LIBRARY_PATH.to_string());
    let transmission_library_path = format!("{}/{}", data_dir, transmission_library_base_path);
    transmission_library_path
}

pub fn get_data_dir() -> String {
    SETTINGS.lock()
        .get_string("paths.data_dir")
        .unwrap_or(DATA_DIR.to_string())
}

pub fn is_debug() -> bool {
    SETTINGS.lock()
        .get_bool("debug")
        .unwrap_or(DEBUG_STATUS)
}

pub fn get_transmission_delay_times() -> (f32, f32) {
    let min_delay = SETTINGS.lock()
        .get_float("transmission_gap_min_time")
        .unwrap_or(TRANSMISSION_GAP_MIN_TIME as f64) as f32;
    let max_delay = SETTINGS.lock()
        .get_float("transmission_gap_max_time")
        .unwrap_or(TRANSMISSION_GAP_MAX_TIME as f64) as f32;
    (min_delay, max_delay)
}

pub fn get_hiss_preroll_times() -> (f32, f32) {
    let min_time = SETTINGS.lock()
        .get_float("hiss_preroll_min_time")
        .unwrap_or(HISS_PREROLL_MIN_TIME as f64) as f32;
    let max_time = SETTINGS.lock()
        .get_float("hiss_preroll_max_time")
        .unwrap_or(HISS_PREROLL_MAX_TIME as f64) as f32;
    (min_time, max_time)
}

pub fn get_hiss_postroll_times() -> (f32, f32) {
    let min_time = SETTINGS.lock()
        .get_float("hiss_postroll_min_time")
        .unwrap_or(HISS_POSTROLL_MIN_TIME as f64) as f32;
    let max_time = SETTINGS.lock()
        .get_float("hiss_postroll_max_time")
        .unwrap_or(HISS_POSTROLL_MAX_TIME as f64) as f32;
    (min_time, max_time)
}

pub fn get_volumes() -> (f32, f32) {
    let transmission_sink_volume = SETTINGS.lock()
        .get_float("volumes.transmission_sink_volume")
        .unwrap_or(TRANSMISSION_SINK_VOLUME as f64) as f32;
    let hiss_sink_volume = SETTINGS.lock()
        .get_float("volumes.hiss_whitenoise_sink_volume")
        .unwrap_or(HISS_SINK_VOLUME as f64) as f32;
    (transmission_sink_volume, hiss_sink_volume)
}