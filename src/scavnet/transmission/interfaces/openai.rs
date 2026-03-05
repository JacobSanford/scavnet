use anyhow::Result;

use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::scavnet::{networks::RadioNetworks, transmission::{core::Transmission, sets::{Conversation, ConversationTransmissionItemSpec}}};
use crate::scavnet::transmission::core::TransmissionItem;
use crate::scavnet::transmission::interfaces::core::init_transmission;

use openai_api_rust::*;
use uuid::Uuid;
use openai_api_rust::chat::*;
use openai_dive::v1::api::Client;
use openai_dive::v1::resources::audio::AudioSpeechParametersBuilder;
use openai_dive::v1::models::TTSEngine;
use openai_dive::v1::resources::audio::{AudioVoice, AudioSpeechResponseFormat};

pub struct TransmissionOpenAI {
    pub path: String,
}

impl TransmissionOpenAI {
    pub async fn build(conversation: Conversation, networks: RadioNetworks, hiss_preroll: f32, hiss_postroll: f32) -> Result<Transmission> {
        let mut transmission = init_transmission(conversation.clone(), networks)?;
        transmission.hiss_preroll = hiss_preroll;
        transmission.hiss_postroll = hiss_postroll;

        let conversation = conversation.transmissions;

        let mut items_iter = conversation.items.iter().peekable();
        while let Some(item) = items_iter.next() {
            let data: OpenAIData = serde_yaml::from_value(item.data.clone())?;

            let messages = vec![Message { role: Role::User, content: data.prompt.clone() }];

            // Get the API key from the environment.
            let api_key = std::env::var("OPENAI_API_KEY")
                .map_err(|_| anyhow::anyhow!("OPENAI_API_KEY environment variable not set"))?;
            let auth = Auth::new(&api_key);
            let openai = OpenAI::new(auth, "https://api.openai.com/v1/");
            let body = ChatBody {
                model: "gpt-4o".to_string(),
                max_tokens: Some(7),
                temperature: Some(0_f32),
                top_p: Some(0_f32),
                n: Some(2),
                stream: Some(false),
                stop: None,
                presence_penalty: None,
                frequency_penalty: None,
                logit_bias: None,
                user: None,
                messages: messages,
            };
            let rs = openai.chat_completion_create(&body);
            let choice = rs.unwrap().choices;
            let message = &choice[0].message.as_ref().unwrap();
            let text_script = message.content.clone();

            let client = Client::new(api_key.to_string());

            for line in text_script.lines() {
                let parameters = AudioSpeechParametersBuilder::default()
                .model(TTSEngine::Tts1HD.to_string())
                .input(line)
                .voice(AudioVoice::Alloy)
                .response_format(AudioSpeechResponseFormat::Wav)
                .build()?;
    
                let response = client
                    .audio()
                    .create_speech(parameters)
                    .await?;
                
                let file_path = format!("{}/{}.wav", std::env::temp_dir().display(), Uuid::new_v4());
                response
                    .save(file_path.clone())
                    .await?;
    
                let mut sleep_after: f32 = 0.0;
                if items_iter.peek().is_some() {
                    sleep_after = Self::get_delay(item.clone()) as f32;
                }
    
                let id = item.id.clone();
                let caption = item.captions[0].clone();
                let path = file_path;
                let transmission_item = tokio::task::spawn_blocking(move || {
                    TransmissionItem::new(id, caption, path, sleep_after)
                }).await??;

                transmission.add_item(transmission_item);
            }
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
struct OpenAIData {
    pub prompt: String,
    pub model: String,

}
