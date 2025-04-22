use std::error::Error;

use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::scavnet::{networks::RadioNetworks, transmission::{core::Transmission, sets::{Conversation, ConversationTransmissionItemSpec}}};
use crate::scavnet::transmission::core::TransmissionItem;
use crate::scavnet::transmission::interfaces::core::init_transmission;

pub struct TransmissionLocalFileSystem {
    pub path: String,
}

impl TransmissionLocalFileSystem {
    pub async fn build(conversation: Conversation, networks: RadioNetworks, hiss_preroll: f32, hiss_postroll: f32) -> Result<Transmission, Box<dyn Error>>  {
        let mut transmission = init_transmission(conversation.clone(), networks)?;
        transmission.id = conversation.id.clone();
        transmission.hiss_preroll = hiss_preroll;
        transmission.hiss_postroll = hiss_postroll;

        let conversation = conversation.transmissions;

        let mut items_iter = conversation.items.iter().peekable();
        while let Some(item) = items_iter.next() {
            let data: LocalFileSystemData = serde_yaml::from_value(item.data.clone())?;
            let file_path = data.file.clone();
            let file_path_string = format!("data/transmissions/{}", file_path);

            let mut sleep_after: f32 = 0.0;
            if items_iter.peek().is_some() {
                sleep_after = Self::get_delay(item.clone()) as f32;
            }

            let transmission_item = TransmissionItem::new(
                item.id.clone(),
                item.captions[0].clone(),
                file_path_string.clone(),
                sleep_after,
            );

            transmission.add_item(transmission_item);
        }
        Ok(transmission)
    }

    fn get_delay(conversation: ConversationTransmissionItemSpec) -> f32 {
        let delaymin = conversation.delay_after_min as f32;
        let delaymax = conversation.delay_after_max as f32;
        let delay = rand::thread_rng().gen_range(delaymin..delaymax);
        delay
    }

}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct LocalFileSystemData {
    pub file: String,
}
