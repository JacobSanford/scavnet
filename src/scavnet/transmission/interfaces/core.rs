use anyhow::{bail, Result};

use crate::scavnet::networks::RadioNetworks;
use crate::scavnet::transmission::core::Transmission;
use crate::scavnet::transmission::interfaces::localfilesystem::TransmissionLocalFileSystem;
use crate::scavnet::transmission::interfaces::openai::TransmissionOpenAI;
use crate::scavnet::transmission::sets::Conversation;

pub async fn build_transmission(conversation: Conversation, networks: RadioNetworks, hiss_preroll: f32, hiss_postroll: f32) -> Result<Transmission> {
    let interface = conversation.interface.clone();
    match interface.as_str() {
        "LocalFileSystem" => TransmissionLocalFileSystem::build(conversation, networks, hiss_preroll, hiss_postroll).await,
        "OpenAI" => TransmissionOpenAI::build(conversation, networks, hiss_preroll, hiss_postroll).await,
        _ => bail!("Invalid transmission interface: {}", interface),
    }
}

pub fn init_transmission(conversation: Conversation, networks: RadioNetworks) -> Result<Transmission> {
    let mut transmission = Transmission::random_from_networks(networks);
    if !conversation.transmissions.random_frequency {
        transmission.frequency = conversation.transmissions.frequency
    }
    Ok(transmission)
}
