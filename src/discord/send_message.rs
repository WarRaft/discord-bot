use crate::error::BotError;
use crate::state;
use serde::Serialize;

/// Message reference object for replying to another message
#[derive(Debug, Clone, Serialize)]
pub struct MessageReference {
    pub message_id: String,
}

/// Payload structure for sending a Discord message
#[derive(Debug, Clone, Serialize)]
pub struct MessagePayload {
    /// Text content of the message
    pub content: String,

    /// Optional reference to another message (used for replies)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_reference: Option<MessageReference>,
}

pub async fn send_message(
    channel_id: &str,
    payload: &MessagePayload,
) -> Result<serde_json::Value, BotError> {
    let limiter = state::rate_limiter().await;
    limiter.acquire().await;

    let client = state::client().await;
    let token = state::token().await;

    let response = client
        .post(&format!(
            "https://discord.com/api/v10/channels/{}/messages",
            channel_id
        ))
        .header("Authorization", format!("Bot {}", token))
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await?;

    let _ = crate::db::rate_limits::RateLimit::update_from_headers(
        &*state::db().await,
        format!("POST /channels/{}/messages", channel_id),
        response.headers(),
    )
    .await;

    let msg: serde_json::Value = response.json().await?;
    Ok(msg)
}
