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
    #[serde(skip)]
    pub attachments: Option<Vec<(String, Vec<u8>)>>,
}

impl MessageSend {
    pub async fn send(&self, method: Method, channel_id: &str, message_id: Option<&str>) -> Result<Message, BotError> {
        let limiter = state::rate_limiter().await;
        limiter.acquire().await;

        let client = state::client().await;
        let token = state::token().await;

        let url = if let Some(msg_id) = message_id {
            format!(
                "https://discord.com/api/v10/channels/{}/messages/{}",
                channel_id, msg_id
            )
        } else {
            format!(
                "https://discord.com/api/v10/channels/{}/messages",
                channel_id
            )
        };

        let mut request = client
            .request(method.clone(), &url)
            .header("Authorization", format!("Bot {}", token));

        if let Some(attachments) = &self.attachments {
            use reqwest::multipart::{Form, Part};

            let mut form = Form::new();

            let payload = serde_json::to_string(self)?;
            form = form.text("payload_json", payload);

            for (idx, (filename, data)) in attachments.iter().enumerate() {
                let part = Part::bytes(data.clone())
                    .file_name(filename.clone())
                    .mime_str("application/octet-stream")?;
                form = form.part(format!("files[{}]", idx), part);
            }

            request = request.multipart(form);
        } else {
            request = request
                .header("Content-Type", "application/json")
                .json(self);
        }

        let response = request.send().await?;

        let _ = crate::db::rate_limits::RateLimit::update_from_headers(
            &*state::db().await,
            format!("{} /channels/{}/messages{}", method, channel_id, if message_id.is_some() { "/{message_id}" } else { "" }),
            response.headers(),
        )
        .await;

        let msg: Message = response.json().await?;
        Ok(msg)
    }
}
