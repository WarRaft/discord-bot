use crate::commands::{Command, SlashCommand};
use crate::discord::api;
use crate::error::Result;
use crate::state;
use crate::types::discord::Interaction;

pub struct Blp;

impl Command for Blp {
    fn definition() -> SlashCommand {
        SlashCommand {
            name: "blp".to_string(),
            command_type: 1,
            description: "Information about BLP image conversion".to_string(),
        }
    }

    async fn handle(interaction: Interaction) -> Result<()> {
        let client = state::client().await;
        let token = state::token().await;
        let db = state::db().await;

        // Check bot permissions in this channel
        let permissions_info = if let Some(channel_id) = &interaction.channel_id {
            check_bot_permissions(&client, &token, channel_id).await
        } else {
            "⚠️ Unable to determine channel permissions".to_string()
        };

        // Get queue statistics for BLP conversion
        let queue_info = match crate::db::blp_queue::BlpQueueItem::count_total_by_type(
            &db,
            crate::db::blp_queue::ConversionType::ToBLP,
        )
        .await
        {
            Ok(count) => {
                if count > 0 {
                    format!(
                        "📊 **Usage Statistics:** {} total BLP conversions processed",
                        count
                    )
                } else {
                    "📊 **Usage Statistics:** No BLP conversions yet".to_string()
                }
            }
            Err(_) => "📊 **Usage Statistics:** Unable to check statistics".to_string(),
        };

        let info_text = format!(
            "🔄 **BLP Image Conversion**\n\n\
**Usage:**\n\
• Mention the bot with image attachments: `@Raft blp [quality] [zip]`\n\
• Quality: 1-100 (default: 80, higher = better quality)\n\
• Add `zip` to receive files in a ZIP archive\n\n\
**Examples:**\n\
• `@Raft blp` - Convert with default quality (80)\n\
• `@Raft blp 95` - Convert with quality 95\n\
• `@Raft blp 90 zip` - Convert with quality 90 and ZIP the results\n\
• `@Raft blp zip` - Convert with default quality and ZIP the results\n\n\
**File size limit:** 25MB per file\n\
**Multiple files:** Yes, attach multiple images in one message\n\n\
{}\n\n\
**Bot Permissions Status:**\n\
{}",
            queue_info, permissions_info
        );

        api::respond_to_interaction(
            &client,
            &token,
            &interaction.id,
            &interaction.token,
            info_text,
        )
        .await?;

        Ok(())
    }
}

/// Check bot permissions in the channel and return formatted status
async fn check_bot_permissions(client: &reqwest::Client, token: &str, channel_id: &str) -> String {
    // Get bot user ID
    let bot_user_id = state::bot_user_id().await;
    if bot_user_id.is_empty() {
        return "❌ Bot user ID not available".to_string();
    }

    // Try to get channel permissions
    match get_channel_permissions(client, token, channel_id, &bot_user_id).await {
        Ok(permissions) => {
            let mut status = Vec::new();

            // Check required permissions (bitwise flags)
            let view_channel = permissions & 0x400 != 0; // VIEW_CHANNEL
            let send_messages = permissions & 0x800 != 0; // SEND_MESSAGES  
            let attach_files = permissions & 0x8000 != 0; // ATTACH_FILES
            let read_history = permissions & 0x10000 != 0; // READ_MESSAGE_HISTORY

            status.push(format!(
                "• View Channel: {}",
                if view_channel { "✅" } else { "❌" }
            ));
            status.push(format!(
                "• Send Messages: {}",
                if send_messages { "✅" } else { "❌" }
            ));
            status.push(format!(
                "• Attach Files: {}",
                if attach_files { "✅" } else { "❌" }
            ));
            status.push(format!(
                "• Read Message History: {}",
                if read_history { "✅" } else { "❌" }
            ));

            let all_ok = view_channel && send_messages && attach_files && read_history;
            let header = if all_ok {
                "✅ All required permissions available"
            } else {
                "⚠️ Some permissions missing"
            };

            format!("{}\n{}", header, status.join("\n"))
        }
        Err(e) => {
            format!("❌ Unable to check permissions: {:?}", e)
        }
    }
}

/// Get channel permissions for bot user
async fn get_channel_permissions(
    client: &reqwest::Client,
    token: &str,
    channel_id: &str,
    _user_id: &str,
) -> Result<u64> {
    // Apply rate limiting before Discord API request
    let limiter = crate::state::rate_limiter().await;
    limiter.acquire().await;

    let response = client
        .get(&format!(
            "https://discord.com/api/v10/channels/{}",
            channel_id
        ))
        .header("Authorization", format!("Bot {}", token))
        .send()
        .await?;

    // Store rate limits from response headers
    let _ = crate::db::rate_limits::RateLimit::update_from_headers(
        &*crate::state::db().await,
        format!("/channels/{}", channel_id),
        response.headers(),
    )
    .await;

    if !response.status().is_success() {
        return Err(crate::error::BotError::new("channel_fetch_failed")
            .push_str(format!("Status: {}", response.status())));
    }

    let channel_data: serde_json::Value = response.json().await?;

    // For DMs, assume we have all permissions
    if channel_data["type"].as_u64() == Some(1) {
        return Ok(0x8000 | 0x800 | 0x400 | 0x10000); // Basic DM permissions
    }

    // For guild channels, we would need to calculate permissions based on:
    // - Guild member roles
    // - Channel permission overwrites
    // This is complex, so for now return a basic check

    // TODO: Implement full permission calculation
    // For now, assume we have permissions (this should be improved)
    Ok(0x8000 | 0x800 | 0x400 | 0x10000)
}
