use crate::db::blp_queue::ConversionType;
use crate::state;
use crate::types::discord::Message;
use serde_json::json;

use crate::discord::send_message::{MessagePayload, MessageReference, send_message};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CommandKind {
    Blp,
    Png,
    Rembg, // includes "rembg" and "bg" aliases
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandArgs {
    pub kind: CommandKind,
    pub quality: u8,   // 1..=100 for BLP
    pub threshold: u8, // 0..=255 for REMBG
    pub should_zip: bool,
    pub binary_mode: bool,
    pub include_mask: bool,
}

impl Default for CommandArgs {
    fn default() -> Self {
        Self {
            kind: CommandKind::Png,
            quality: 80,
            threshold: 160,
            should_zip: false,
            binary_mode: false,
            include_mask: false,
        }
    }
}

pub fn parse_command_args(content: &str, _bot_id: &str) -> Option<CommandArgs> {
    let mut args = CommandArgs::default();

    let tokens: Vec<&str> = content
        .trim()
        .split_whitespace()
        .filter(|t| !t.starts_with('<')) // пропускаем упоминания
        .collect();

    if tokens.is_empty() {
        return None;
    }

    args.kind = match tokens[0] {
        "blp" => CommandKind::Blp,
        "png" => CommandKind::Png,
        "rembg" | "bg" => CommandKind::Rembg,
        _ => return None,
    };

    for &tok in &tokens[1..] {
        match tok {
            "zip" => args.should_zip = true,
            "binary" => args.binary_mode = true,
            "mask" => args.include_mask = true,
            _ => {
                if let Ok(num) = tok.parse::<u16>() {
                    match args.kind {
                        CommandKind::Blp if (1..=100).contains(&num) => args.quality = num as u8,
                        CommandKind::Rembg if num <= 255 => args.threshold = num as u8,
                        _ => {}
                    }
                }
            }
        }
    }

    Some(args)
}

pub async fn handle_message(message: Message) -> crate::error::Result<()> {
    if message.author.bot.unwrap_or(false) {
        return Ok(());
    }

    let bot_user_id = state::bot_user_id().await;
    if !message.mentions.iter().any(|m| m.id == bot_user_id) {
        return Ok(());
    }

    let Some(args) = parse_command_args(&message.content, &bot_user_id) else {
        return Ok(());
    };

    match args.kind {
        CommandKind::Blp => handle_blp_conversion(message, ConversionType::ToBLP, args).await,
        CommandKind::Png => handle_blp_conversion(message, ConversionType::ToPNG, args).await,
        CommandKind::Rembg => handle_rembg_pipeline(message, &args).await,
    }
}

/// Handle BLP/PNG conversion commands extracted from `handle_message`.
async fn handle_blp_conversion(
    message: Message,
    conversion_type: ConversionType,
    args: CommandArgs,
) -> crate::error::Result<()> {
    // Check if there are attachments
    if message.attachments.is_empty() {
        return Ok(());
    }

    // Add to queue
    use crate::db::blp_queue::{AttachmentItem, BlpQueueItem};

    let attachments: Vec<AttachmentItem> = message
        .attachments
        .into_iter()
        .map(|att| AttachmentItem {
            url: att.url,
            filename: att.filename,
            converted_path: None,
        })
        .collect();

    // Send confirmation message FIRST to get message ID
    // Acquire rate limit token before sending
    let limiter = state::rate_limiter().await;
    limiter.acquire().await;

    let client = state::client().await;
    let token = state::token().await;

    let format_desc = match conversion_type {
        ConversionType::ToBLP => format!("to BLP (quality: {})", args.quality),
        ConversionType::ToPNG => "to PNG".to_string(),
    };

    let response = client
        .post(&format!(
            "https://discord.com/api/v10/channels/{}/messages",
            message.channel_id
        ))
        .header("Authorization", format!("Bot {}", token))
        .header("Content-Type", "application/json")
        .json(&json!({
            "content": format!(
                "✅ Added {} image(s) to conversion queue {}\n⏳ Processing...",
                attachments.len(),
                format_desc
            ),
            "message_reference": {
                "message_id": message.id
            }
        }))
        .send()
        .await?;

    // Get status message ID
    let status_message_id = if response.status().is_success() {
        if let Ok(msg_data) = response.json::<serde_json::Value>().await {
            msg_data["id"].as_str().map(|s| s.to_string())
        } else {
            None
        }
    } else {
        None
    };

    // Create queue item with status_message_id already set
    let mut queue_item = BlpQueueItem::new(
        message.author.id.clone(),
        message.channel_id.clone(),
        message.id.clone(),
        String::new(), // No interaction_id for messages
        String::new(), // No interaction_token for messages
        attachments,
        conversion_type,
        args.quality,
        args.should_zip,
    );

    // Set status_message_id before inserting
    queue_item.status_message_id = status_message_id;

    let db = state::db().await;
    queue_item.insert(&*db).await?;

    // Notify workers that a new task is available
    crate::workers::notify_blp_task();

    Ok(())
}

/// Handle rembg (background removal) command
async fn handle_rembg_pipeline(message: Message, arg: &CommandArgs) -> crate::error::Result<()> {
    // Check if there are attachments
    if message.attachments.is_empty() {
        return Ok(());
    }

    // Check if rembg is available
    if !crate::workers::is_rembg_available() {
        // Send error message immediately
        send_message(
            message.channel_id.as_str(),
            &MessagePayload {
                content: "❌ **Background removal is currently unavailable**\n\nONNX Runtime is not installed on the server.\nPlease contact the administrator to run: `./signal-download-models.sh`".to_string(), //
                message_reference: Some(MessageReference {
                    message_id: message.id, //
                }),
            },
        )
            .await?;

        return Ok(());
    }

    // Add to queue
    use crate::db::rembg_queue::{AttachmentItem, RembgQueueItem};

    let attachments: Vec<AttachmentItem> = message
        .attachments
        .into_iter()
        .map(|att| AttachmentItem {
            url: att.url,
            filename: att.filename,
            processed_path: None,
        })
        .collect();

    // Send confirmation message FIRST to get message ID
    // Acquire rate limit token before sending

    let status_message = send_message(
        message.channel_id.as_str(),
        &MessagePayload {
            content: format!(
                "🔄 Processing {} image(s) for background removal...",
                attachments.len()
            ),
            message_reference: Some(MessageReference {
                message_id: message.id.clone(),
            }),
        },
    )
    .await?;

    let status_message_id = status_message["id"].as_str().map(String::from);

    // Create queue item
    let message_id = message.id.clone();
    let queue_item = RembgQueueItem::new(
        message.author.id, //
        message.channel_id,
        message_id.clone(),
        message_id,
        String::new(),
        status_message_id,
        attachments,
        arg.threshold,
        arg.binary_mode,
        arg.include_mask,
        arg.should_zip,
    );

    let db = state::db().await;
    let _queue_id = queue_item.insert(&*db).await?;

    // Notify workers that a new task is available
    crate::workers::notify_rembg_task();

    Ok(())
}
