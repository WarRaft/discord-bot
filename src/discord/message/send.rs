use crate::discord::message::message::{Message, MessageReference};
use crate::error::BotError;
use crate::state;
use reqwest::Method;
use serde::{Deserialize, Serialize};

/// https://discord.com/developers/docs/resources/message#create-message
#[derive(Debug, Deserialize, Serialize)]
pub struct MessageSend {
    pub content: Option<String>,
    pub message_reference: Option<MessageReference>,
}

impl MessageSend {
    pub async fn send(&self, method: Method, channel_id: &str) -> Result<Message, BotError> {
        let limiter = state::rate_limiter().await;
        limiter.acquire().await;

        let client = state::client().await;
        let token = state::token().await;

        let response = client
            .request(
                method,
                &format!(
                    "https://discord.com/api/v10/channels/{}/messages",
                    channel_id
                ),
            )
            .header("Authorization", format!("Bot {}", token))
            .header("Content-Type", "application/json")
            .json(self)
            .send()
            .await?;

        let _ = crate::db::rate_limits::RateLimit::update_from_headers(
            &*state::db().await,
            format!("POST /channels/{}/messages", channel_id),
            response.headers(),
        )
        .await;

        let msg: Message = response.json().await?;
        Ok(msg)
    }
}
