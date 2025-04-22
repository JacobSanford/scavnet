use std::fs::File;
use std::io::{self, BufReader, Read};

use rand::seq::SliceRandom;

use crate::scavnet::fft::fft_cached_normalized;
use crate::scavnet::networks::RadioNetworks;

#[derive(Clone)]
pub struct Transmission {
    pub id: String,
    pub frequency: u32,
    pub items: Vec<TransmissionItem>,
    pub hiss_preroll: f32,
    pub hiss_postroll: f32,
}

#[derive(Clone)]
pub struct TransmissionItem {
    pub id: String,
    pub duration: f32,
    pub caption: String,
    pub file_path: String,
    pub sleep_after: f32,
    pub file_bytes: Vec<u8>,
    pub fft_data: Vec<Vec<f32>>,
}

impl TransmissionItem {
    pub fn new(id: String, caption: String, file_path: String, sleep_after: f32) -> Self {
        let reader: hound::WavReader<io::BufReader<File>> = hound::WavReader::open(file_path.clone()).unwrap();
        let duration = reader.duration();
        let sample_rate = reader.spec().sample_rate as f32;
        let duration_seconds = duration as f32 / sample_rate;
        let fft_data = fft_cached_normalized(file_path.as_str()).unwrap();

        let file = File::open(file_path.clone()).unwrap();
        let mut buf_reader = BufReader::new(file);
        let mut file_bytes = Vec::new();
        buf_reader.read_to_end(&mut file_bytes).unwrap();

        Self {
            id,
            duration: duration_seconds,
            caption,
            file_path,
            sleep_after,
            file_bytes,
            fft_data,
        }
    }

}

impl Transmission {
    pub fn new(frequency: u32) -> Self {
        Self {
            id: String::new(),
            frequency,
            items: Vec::new(),
            hiss_preroll: 0.0,
            hiss_postroll: 0.0,
        }
    }

    pub fn random_from_networks(networks: RadioNetworks) -> Self {
        let mut rng = rand::thread_rng();
        let freq = networks.scan_frequencies().choose(&mut rng).cloned().unwrap_or(0);
        Self::new(freq)
    }

    pub fn add_item(&mut self, item: TransmissionItem) {
        self.items.push(item);
    }
}
