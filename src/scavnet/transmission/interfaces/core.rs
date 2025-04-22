use std::error::Error;

use crate::scavnet::networks::RadioNetworks;
use crate::scavnet::transmission::core::Transmission;
use crate::scavnet::transmission::interfaces::localfilesystem::TransmissionLocalFileSystem;
use crate::scavnet::transmission::interfaces::openai::TransmissionOpenAI;
use crate::scavnet::transmission::sets::Conversation;

enum TransmissionInterface {
    LocalFilesystem,
    OpenAI,
}

pub async fn build_transmission(conversation: Conversation, networks: RadioNetworks, hiss_preroll: f32, hiss_postroll: f32) -> Result<Transmission, Box<dyn Error>> {
    let interface = conversation.interface.clone();
    match interface.as_str() {
        "LocalFileSystem" => build_transmission_via_interface(TransmissionInterface::LocalFilesystem, conversation, networks, hiss_preroll, hiss_postroll).await,
        "OpenAI" => build_transmission_via_interface(TransmissionInterface::OpenAI, conversation, networks, hiss_preroll, hiss_postroll).await,
        _ => panic!("Invalid transmission interface: {}", interface),
    }
}

async fn build_transmission_via_interface(interface: TransmissionInterface, conversation: Conversation, networks: RadioNetworks, hiss_preroll: f32, hiss_postroll: f32) -> Result<Transmission, Box<dyn Error>> {
    match interface {
        TransmissionInterface::LocalFilesystem => TransmissionLocalFileSystem::build(conversation, networks, hiss_preroll, hiss_postroll).await,
        TransmissionInterface::OpenAI => TransmissionOpenAI::build(conversation, networks, hiss_preroll, hiss_postroll).await,
    }
}

pub fn init_transmission(conversation: Conversation, networks: RadioNetworks) -> Result<Transmission, Box<dyn Error>> {
    let mut transmission = Transmission::random_from_networks(networks);
    if !conversation.transmissions.random_frequency {
        transmission.frequency = conversation.transmissions.frequency
    }
    Ok(transmission)
}
