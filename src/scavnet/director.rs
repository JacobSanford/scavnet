use std::error::Error;

use quanta::Instant;
use rand::rngs::StdRng;
use rand::SeedableRng;

use crate::scavnet::networks::RadioNetworks;
use crate::scavnet::settings::{network_library_path, transmission_library_path, get_transmission_delay_times, get_hiss_preroll_times, get_hiss_postroll_times};
use crate::scavnet::settings::is_debug;
use crate::scavnet::time::{rand_time_from_now, rand_time_secs};
use crate::scavnet::transmission::core::Transmission;
use crate::scavnet::transmission::interfaces::core::build_transmission;
use crate::scavnet::transmission::library::build_transmission_library;
use crate::scavnet::transmission::library::TransmissionLibrary;
use crate::scavnet::transmission::queue::TransmissionQueue;
use crate::scavnet::transmission::sets::{TransmissionSet, Conversation};

#[derive(Clone)]
pub struct Director {
    network_path: String,
    networks: RadioNetworks,
    library_path: String,
    library: TransmissionLibrary,
    pub queue: TransmissionQueue,
    next_queue_time: Instant,
    min_queue_delay: f32,
    max_queue_delay: f32,
    rng: StdRng,
    hiss_preroll_min_time: f32,
    hiss_preroll_max_time: f32,
    hiss_postroll_min_time: f32,
    hiss_postroll_max_time: f32,
}

impl Director {
    pub fn empty() -> Self{
        let networks = RadioNetworks::empty();
        let library = TransmissionLibrary::empty();
        Self {
            network_path: String::new(),
            networks,
            library_path: String::new(),
            library,
            next_queue_time: Self::never(),
            queue: TransmissionQueue::empty(),
            min_queue_delay: 0.0,
            max_queue_delay: 0.0,
            rng: StdRng::from_entropy(),
            hiss_preroll_min_time: 0.0,
            hiss_preroll_max_time: 0.0,
            hiss_postroll_min_time: 0.0,
            hiss_postroll_max_time: 0.0,
        }
    }

    pub async fn new(rng: StdRng) -> Result<Self, Box<dyn Error>> {
        let mut director = Director::empty();
        director.network_path = network_library_path();
        director.library_path = transmission_library_path();
        director.rng = rng;
        director.reload().await?;
        director.set_queue_time_delays();
        director.set_initial_queue_time();
        Ok(director)
    }

    fn set_initial_queue_time(&mut self) {
        if is_debug() {
            self.next_queue_time = rand_time_from_now(&mut self.rng, 3.0, 5.0);
        } else {
            self.set_next_queue_time();
        }
    }

    pub fn set_next_queue_time(&mut self) {
        self.next_queue_time = rand_time_from_now(&mut self.rng, self.min_queue_delay, self.max_queue_delay);
    }

    fn set_queue_time_delays(&mut self) {
        let (min_delay, max_delay) = get_transmission_delay_times();
        self.min_queue_delay = min_delay;
        self.max_queue_delay = max_delay;
    }

    pub async fn reload(&mut self) -> Result<(), Box<dyn Error>> {
        self.load_networks().await?;
        self.load_library().await?;
        self.next_queue_time = Self::never();
        self.queue = TransmissionQueue::empty();
        self.set_queue_time_delays();

        let (preroll_min, preroll_max) = get_hiss_preroll_times();
        self.hiss_preroll_min_time = preroll_min;
        self.hiss_preroll_max_time = preroll_max;

        let (postroll_min, postroll_max) = get_hiss_postroll_times();
        self.hiss_postroll_min_time = postroll_min;
        self.hiss_postroll_max_time = postroll_max;

        Ok(())
    }

    async fn load_networks(&mut self) -> Result<(), Box<dyn Error>> {
        self.networks = RadioNetworks::from_yaml(&self.network_path).await?;
        Ok(())
    }

    async fn load_library(&mut self) -> Result<(), Box<dyn Error>> {
        self.library = build_transmission_library(&self.library_path).await?;
        Ok(())
    }

    pub fn get_networks(&self) -> &RadioNetworks {
        &self.networks
    }

    fn never() -> Instant {
        Instant::now() + std::time::Duration::from_secs(1000000000)
    }

    pub async fn add_incoming_transmissions(&mut self, transmissions: Vec<Transmission>) {
        for transmission in transmissions {
            self.queue.add(transmission);
        }
    }

    pub fn needs_queueing(&self) -> bool {
        Instant::now() >= self.next_queue_time
    }

    pub async fn get_new_transmission(&self) -> Transmission {
        let new_transmission = self.get_random_transmission().await.unwrap();
        new_transmission
    }

    pub async fn get_random_transmission(&self) -> Result<Transmission, Box<dyn Error>> {
        let conversation = self.get_random_conversation();
        let hiss_preroll = rand_time_secs(&mut self.rng.clone(), self.hiss_preroll_min_time, self.hiss_preroll_max_time);
        let hiss_postroll = rand_time_secs(&mut self.rng.clone(), self.hiss_postroll_min_time, self.hiss_postroll_max_time);
        build_transmission(conversation, self.networks.clone(), hiss_preroll, hiss_postroll).await
    }
    
    fn get_random_conversation(&self) -> Conversation {
        let path_string = {
            let selected_node = self.library.choose(&mut self.rng.clone());
            selected_node.data.clone().unwrap()
        };
        let mut set = TransmissionSet::from_yaml(&path_string).unwrap();
        let conversation = set.get_conversation().unwrap();
        conversation.clone()
    }

}

