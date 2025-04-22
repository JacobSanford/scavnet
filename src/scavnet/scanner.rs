use std::iter::Cycle;
use std::vec::IntoIter;

use crate::scavnet::networks::RadioNetworks;

#[derive(Clone)]
pub struct Scanner {
    networks: RadioNetworks,
    cur_frequency: u32,
    freq_iter: Cycle<IntoIter<u32>>,
    scanning: bool,
    fftdata: Vec<f32>,
    status: String,
    noise_profile: Vec<Vec<f32>>,
    noise_index: usize,
}

impl Scanner {
    pub fn empty() -> Self {
        Self {
            networks: RadioNetworks::empty(),
            cur_frequency: 0,
            freq_iter: vec![].into_iter().cycle(),
            scanning: false,
            fftdata: vec![],
            status: String::new(),
            noise_profile: vec![],
            noise_index: 0,
        }
    }

    pub fn new(networks: RadioNetworks) -> Self {
        let scan_frequencies = networks.scan_frequencies();
        let cur_frequency = scan_frequencies.first().cloned().unwrap_or(0);
        let freq_iter = scan_frequencies.into_iter().cycle();
        let noise_profile: Vec<Vec<f32>> = (0..256)
            .map(|_| {
                (0..=256)
                    .map(|_| rand::random::<u8>() as f32 / 50.0)
                    .collect()
            })
            .collect();
        Self {
            networks,
            cur_frequency,
            freq_iter,
            scanning: false,
            fftdata: vec![],
            status: String::new(),
            noise_profile,
            noise_index: 0,
        }
    }

    pub fn _get_networks(&self) -> RadioNetworks {
        self.networks.clone()
    }

    pub fn start(&mut self) {
        self.scanning = true;
        self.status = "Scanning...".to_string();
    }

    pub fn pause(&mut self) {
        self.scanning = false;
    }

    pub fn pause_for_playback(&mut self) {
        self.pause();
        self.status = "Recieving transmission...".to_string();
    }

    pub fn resume_after_playback(&mut self) {
        self.start();
    }

    pub fn next_freq(&mut self) -> Option<u32> {
        if (!self.scanning) {
            return None;
        }
        self.cur_frequency = self.freq_iter.next().unwrap_or(0);
        Some(self.cur_frequency)
    }

    pub fn cur_freq_display(&self) -> String {
        format!("{:.5} MHz", self.cur_frequency as f32 / 1_000_000.0)
    }

    pub fn update_fft_data(&mut self, fftdata: Vec<f32>) {
        self.fftdata = fftdata;
    }

    pub fn get_fft_data(&self) -> Vec<f32> {
        self.fftdata.clone()
    }

    pub fn is_scanning(&self) -> bool {
        self.scanning
    }

    pub fn simulate_noise(&mut self) {
        let data = &self.noise_profile[self.noise_index];
        self.update_fft_data(data.clone());
        self.noise_index = (self.noise_index + 1) % self.noise_profile.len();
    }

    pub fn simulate_hiss_noise(&mut self) {
        let data = self.noise_profile[self.noise_index].iter().map(|x| x * 2.5).collect::<Vec<f32>>();
        self.update_fft_data(data.clone());
        self.noise_index = (self.noise_index + 1) % self.noise_profile.len();
    }

    pub fn status(&self) -> String {
        self.status.clone()
    }

    pub fn cur_network_name(&self) -> String {
        self.networks.network_name_from_freq(self.cur_frequency as u64).unwrap_or("Unknown".to_string().to_uppercase())
    }

}