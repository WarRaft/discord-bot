use serde::{Deserialize, Serialize};

/// https://discord.com/developers/docs/resources/message
#[derive(Debug, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub author: User,
    pub channel_id: String,
    pub content: String,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub mentions: Vec<User>,
    pub message_reference: Option<MessageReference>,
}

/// https://discord.com/developers/docs/resources/message#message-reference-structure
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct MessageReference {
    #[serde(rename = "type")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guild_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fail_if_not_exists: Option<bool>,
}

/// https://discord.com/developers/docs/resources/message#attachment-object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub url: String,
    pub filename: String,
}

/// https://discord.com/developers/docs/resources/user#user-object
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct User {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bot: Option<bool>,
}
