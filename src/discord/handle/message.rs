use crate::db::blp_queue::ConversionType;
use crate::state;
use crate::types::discord::Message;
use serde_json::json;

use crate::discord::send_message::{MessagePayload, MessageReference, send_message};
use regex::Regex;
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
    /// BLP quality (1..=100). Ignored for PNG/REMBG.
    pub quality: u8,
    /// REMBG threshold (0..=255). Ignored for BLP/PNG.
    pub threshold: u8,
    /// Zip output (applies to any command).
    pub should_zip: bool,
    /// REMBG "clean cut" (binary alpha).
    pub binary_mode: bool,
    /// REMBG include mask file.
    pub include_mask: bool,
}

impl Default for CommandArgs {
    fn default() -> Self {
        Self {
            kind: CommandKind::Png,
            quality: 80,
            threshold: 60,
            should_zip: false,
            binary_mode: false,
            include_mask: false,
        }
    }
}

/// Parse a Discord message into structured, JSON-serializable command arguments.
/// Supports:
///   - Commands: blp, png, rembg, bg (alias of rembg)
///   - Compact forms: blp80, rembg128, bg200, q85, t200
///   - Flags:
///       zip | -z | --zip | zip=true/1/yes
///       binary | -b | --binary
///       mask | -m | --mask
///       BLP:    qNN | -q NN | --quality NN | --quality=NN     (1..=100)
///       REMBG:  tNN | -t NN | --threshold NN | --threshold=NN (0..=255)
pub fn parse_command_args(content: &str, bot_id: &str) -> Option<CommandArgs> {
    // 1) Strip all mentions (<@id>, <@!id>, role, channel) and normalize whitespace.
    let mention_re = Regex::new(r"(?i)<@!?\d+>|<@&\d+>|<#\d+>").ok()?;
    let mut cleaned = mention_re.replace_all(content, " ").to_string();

    // Extra safety: remove explicit forms with our bot_id if present verbatim.
    cleaned = cleaned
        .replace(&format!("<@{}>", bot_id), " ")
        .replace(&format!("<@!{}>", bot_id), " ");

    let cleaned = cleaned
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if cleaned.is_empty() {
        return None;
    }

    // 2) Detect leading command token, optionally with an inline number.
    //    Accept: blp, png, rembg, bg
    //    Inline numeric suffix up to 3 digits: blp80, rembg128, bg200
    let head_re = Regex::new(r"^(blp|png|rembg|bg)(\d{1,3})?(?:\s|$)").ok()?;
    let caps = head_re.captures(&cleaned)?;
    let cmd_str = caps.get(1).unwrap().as_str();
    let inline_num = caps.get(2).map(|m| m.as_str());

    let mut args = CommandArgs::default();
    args.kind = match cmd_str {
        "blp" => CommandKind::Blp,
        "png" => CommandKind::Png,
        "rembg" | "bg" => CommandKind::Rembg,
        _ => return None,
    };

    // 3) Apply inline numeric (if present) with proper ranges.
    if let Some(num) = inline_num {
        if let Ok(n) = num.parse::<u16>() {
            match args.kind {
                CommandKind::Blp => {
                    if (1..=100).contains(&n) {
                        args.quality = n as u8;
                    }
                }
                CommandKind::Rembg => {
                    if n <= 255 {
                        args.threshold = n as u8;
                    } // 0..=255 allowed
                }
                CommandKind::Png => { /* no inline numeric for PNG */ }
            }
        }
    }

    // 4) Parse tail flags and key-values.
    let tail = cleaned[caps.get(0).unwrap().end()..].trim();
    if tail.is_empty() {
        return Some(args);
    }

    let tokens = tail.split_whitespace().collect::<Vec<_>>();
    let mut i = 0usize;

    let parse_u8_in = |s: &str, min: u16, max: u16| -> Option<u8> {
        let n = s.parse::<u16>().ok()?;
        (min..=max).contains(&n).then_some(n as u8)
    };

    while i < tokens.len() {
        let tok = tokens[i];

        // Common flags
        match tok {
            "zip" | "-z" | "--zip" => {
                args.should_zip = true;
                i += 1;
                continue;
            }
            "binary" | "-b" | "--binary" => {
                args.binary_mode = true;
                i += 1;
                continue;
            }
            "mask" | "-m" | "--mask" => {
                args.include_mask = true;
                i += 1;
                continue;
            }
            _ => {}
        }

        if tok.starts_with("zip=") {
            let v = &tok[4..];
            if matches!(v, "1" | "true" | "yes") {
                args.should_zip = true;
            }
            i += 1;
            continue;
        }

        match args.kind {
            CommandKind::Blp => {
                // quality: qNN / -q NN / --quality NN / --quality=NN
                if tok.len() > 1 && tok.starts_with('q') {
                    if let Some(n) = parse_u8_in(&tok[1..], 1, 100) {
                        args.quality = n;
                    }
                    i += 1;
                    continue;
                }
                if tok == "-q" || tok == "--quality" {
                    if let Some(next) = tokens.get(i + 1).and_then(|s| parse_u8_in(s, 1, 100)) {
                        args.quality = next;
                        i += 2;
                        continue;
                    } else {
                        i += 1;
                        continue;
                    }
                }
                if let Some(rest) = tok.strip_prefix("--quality=") {
                    if let Some(n) = parse_u8_in(rest, 1, 100) {
                        args.quality = n;
                    }
                    i += 1;
                    continue;
                }
            }
            CommandKind::Rembg => {
                // threshold: tNN / -t NN / --threshold NN / --threshold=NN  (0..=255)
                if tok.len() > 1 && tok.starts_with('t') {
                    if let Some(n) = parse_u8_in(&tok[1..], 0, 255) {
                        args.threshold = n;
                    }
                    i += 1;
                    continue;
                }
                if tok == "-t" || tok == "--threshold" {
                    if let Some(next) = tokens.get(i + 1).and_then(|s| parse_u8_in(s, 0, 255)) {
                        args.threshold = next;
                        i += 2;
                        continue;
                    } else {
                        i += 1;
                        continue;
                    }
                }
                if let Some(rest) = tok.strip_prefix("--threshold=") {
                    if let Some(n) = parse_u8_in(rest, 0, 255) {
                        args.threshold = n;
                    }
                    i += 1;
                    continue;
                }
            }
            CommandKind::Png => { /* no numeric flags */ }
        }

        i += 1;
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
        CommandKind::Blp => {
            if message.attachments.is_empty() {
                return Ok(());
            }
            handle_blp_conversion(
                message,
                ConversionType::ToBLP,
                args.quality, // 1..=100
                args.should_zip,
            )
            .await
        }
        CommandKind::Png => {
            if message.attachments.is_empty() {
                return Ok(());
            }
            handle_blp_conversion(
                message,
                ConversionType::ToPNG,
                0, // unused for PNG
                args.should_zip,
            )
            .await
        }
        CommandKind::Rembg => {
            if message.attachments.is_empty() {
                return Ok(());
            }
            if !crate::workers::is_rembg_available() {
                // send "unavailable" message here
                return Ok(());
            }
            handle_rembg_pipeline(
                message,
                args.threshold,    // 0..=255
                args.binary_mode,  // clean cut
                args.include_mask, // export mask
                args.should_zip,
            )
            .await
        }
    }
}

/// Handle BLP/PNG conversion commands extracted from `handle_message`.
async fn handle_blp_conversion(
    message: Message,
    conversion_type: ConversionType,
    quality: u8,
    zip: bool,
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
        ConversionType::ToBLP => format!("to BLP (quality: {})", quality),
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
        quality,
        zip,
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
async fn handle_rembg_pipeline(
    message: Message,
    threshold: u8,
    binary_mode: bool,
    include_mask: bool,
    should_zip: bool,
) -> crate::error::Result<()> {
    // Check if there are attachments
    if message.attachments.is_empty() {
        return Ok(());
    }

    println!("threshold {:?}", threshold);

    // Check if rembg is available
    if !crate::workers::is_rembg_available() {
        // Send error message immediately
        send_message(
            message.channel_id.as_str(),
            &MessagePayload {
                content: "❌ **Background removal is currently unavailable**\n\nONNX Runtime is not installed on the server.\nPlease contact the administrator to run: `./signal-download-models.sh`".to_string(), //
                message_reference: Some(MessageReference{
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
        message.author.id,
        message.channel_id,
        message_id.clone(),
        message_id,    // Use message ID as interaction ID for mention commands
        String::new(), // No token for mention commands
        status_message_id,
        attachments,
        threshold,
        binary_mode,
        include_mask,
        should_zip,
    );

    println!("threshold {:?}", threshold);
    println!("MESSAGE {:?}", queue_item);

    let db = state::db().await;
    let _queue_id = queue_item.insert(&*db).await?;

    // Notify workers that a new task is available
    crate::workers::notify_rembg_task();

    Ok(())
}
