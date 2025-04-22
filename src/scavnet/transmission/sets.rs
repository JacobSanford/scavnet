use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::BufReader;

use rand::seq::SliceRandom;
use serde_yaml::from_reader;
use serde_yaml::Value;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ConversationTransmissionItemSpec {
    pub id: String,
    pub captions: Vec<String>,
    pub duration: f32,
    pub delay_after_min: u64,
    pub delay_after_max: u64,
    pub data: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ConversationTransmissionSpec {
    pub random_frequency: bool,
    pub frequency: u32,
    pub items: Vec<ConversationTransmissionItemSpec>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Conversation {
    pub id: String,
    pub description: String,
    pub weight: u32,
    pub interface: String,
    pub transmissions: ConversationTransmissionSpec,
    #[serde(default = "String::new")]
    pub file_path: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TransmissionSet {
    pub name: String,
    pub description: String,
    pub playback: String,
    pub replay_mode: String,
    pub conversations: Vec<Conversation>,
    #[serde(default = "Vec::new")]
    pub conversations_state: Vec<String>,
    #[serde(default = "String::new")]
    pub file_path: String,
}

impl TransmissionSet {
    pub fn from_yaml(file_path: &str) -> Result<Self, Box<dyn Error>> {
        let file = File::open(file_path)?;
        let reader = BufReader::new(file);
        let mut transmission_set: TransmissionSet = from_reader(reader)?;
        transmission_set.file_path = file_path.to_string();

        // Load a sidecar file if it exists that lists the states of the conversations in the set.
        let sidecar_path = format!("{}.state", file_path);
        if fs::metadata(&sidecar_path).is_ok() {
            let sidecar_file = File::open(&sidecar_path)?;
            let sidecar_reader = BufReader::new(sidecar_file);
            transmission_set.conversations_state = from_reader(sidecar_reader)?;
        }

        Ok(transmission_set)
    }

    pub fn get_conversation(&mut self) -> Option<&Conversation> {
        let next_conversation = match self.playback.as_str() {
            "sequence" => {
                // Find the first conversation that hasn't been played yet.
                let unplayed_conversation = self.conversations.iter().find(|conv| !self.conversations_state.contains(&conv.id));
                
                if let Some(conversation) = unplayed_conversation {
                    Some(conversation)
                } else {
                    // All conversations have been played, reset the state and return the first conversation.
                    self.conversations_state.clear();
                    self.conversations.first()
                }
            }
            "random" => {
                let mut rng = rand::thread_rng();
                let replay_mode = self.replay_mode.as_str();

                let mut conversation_pool: Vec<_> = if replay_mode == "exhaust" {
                    self.conversations
                        .iter()
                        .filter(|conv| !self.conversations_state.contains(&conv.id))
                        .collect()
                } else {
                    self.conversations.iter().collect()
                };

                if replay_mode == "exhaust" && conversation_pool.is_empty() {
                    self.conversations_state.clear();
                    conversation_pool = self.conversations.iter().collect();
                }

                conversation_pool
                    .choose_weighted(&mut rng, |conv| conv.weight)
                    .ok()
                    .map(|conv| *conv)
            }
            _ => None,
        };

        if let Some(conversation) = next_conversation {
            self.conversations_state.push(conversation.id.clone());
            // Write the updated state to the sidecar file
            let sidecar_path = format!("{}.state", self.file_path);
            if let Ok(sidecar_contents) = serde_yaml::to_string(&self.conversations_state) {
                let _ = fs::write(&sidecar_path, sidecar_contents);
            }
            Some(conversation)
        } else {
            None
        }
    }
}
