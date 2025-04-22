use crate::scavnet::transmission::core::Transmission;

#[derive(Clone)]
pub struct TransmissionQueue {
    pub transmissions: Vec<Transmission>,
}

impl TransmissionQueue {
    pub fn new() -> Self {
        Self {
            transmissions: Vec::new(),
        }
    }

    pub fn empty() -> Self {
        Self {
            transmissions: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.transmissions.is_empty()
    }

    pub fn add(&mut self, transmission: Transmission) {
        self.transmissions.push(transmission);
    }

    pub fn extend(&mut self, transmissions: Vec<Transmission>) {
        self.transmissions.extend(transmissions);
    }

    pub fn get_queued_transmissions(&mut self) -> Vec<Transmission> {
        if !self.is_empty() {
            let return_transmissions = self.transmissions.clone();
            self.transmissions.clear();
            return_transmissions
        } else {
            return Vec::new();
        }
    }
}
