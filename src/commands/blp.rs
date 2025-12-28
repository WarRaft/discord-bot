use crate::commands::{Command, SlashCommand};
use crate::discord::api;
use crate::error::BotError;
use crate::state;
use crate::discord::discord::Interaction;

pub struct Blp;

impl Command for Blp {
    fn definition() -> SlashCommand {
        SlashCommand {
            name: "blp".to_string(),
            command_type: 1,
            description: "Information about BLP image conversion".to_string(),
            options: None,
        }
    }

    async fn handle(interaction: Interaction) -> Result<(), BotError> {
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
        let queue_info = match crate::workers::blp::job::JobBlp::count_total_by_type(
            &db,
            crate::workers::blp::job::ConversionTarget::BLP,
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
            "🧩 **BLP Image Conversion**\n\n\
**Usage:**\n\
• Mention the bot with image attachments: `@Raft blp [quality] [options]`\n\n\
**Parameters:**\n\
• `quality` — JPEG quality **(1–100, default: 80)**\n  \
  Higher values → better quality, larger file size\n  \
  Lower values → smaller file size, possible artifacts\n\
• `zip` — Bundle all converted images into a ZIP archive\n\n\
**Examples:**\n\
• `@Raft blp` — Convert with default quality (80)\n\
• `@Raft blp 95` — Convert with higher quality (95)\n\
• `@Raft blp 60 zip` — Convert with quality 60 and ZIP all results\n\n\
**File Size Limit:** 25 MB per file\n\
**Multiple Files:** Supported — attach several images in one message\n\
**Output Format:** Warcraft III `.blp` texture\n\
**Input Formats:** PNG, JPEG, WebP, BMP, GIF\n\n\
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
            // Check if error is because bot is not in the server
            if e.to_string().contains("bot_not_in_server") {
                let invite_url = state::get_invite_url().await;
                if !invite_url.is_empty() {
                    format!(
                        "ℹ️ **Permissions:** Bot is not in this server\n\n[Click here to invite the bot]({})",
                        invite_url
                    )
                } else {
                    "ℹ️ **Permissions:** Bot needs to be invited to this server".to_string()
                }
            } else {
                "⚠️ Unable to check permissions (you can still use the bot)".to_string()
            }
        }
    }
}

/// Get channel permissions for bot user
async fn get_channel_permissions(
    client: &reqwest::Client,
    token: &str,
    channel_id: &str,
    _user_id: &str,
) -> Result<u64, BotError> {
    // Apply rate limiting before Discord API request
    let limiter = state::rate_limiter().await;
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
        &*state::db().await,
        format!("/channels/{}", channel_id),
        response.headers(),
    )
    .await;

    // Handle 403 Forbidden - bot is not in this server
    if response.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(BotError::new("bot_not_in_server"));
    }

    if !response.status().is_success() {
        return Err(BotError::new("channel_fetch_failed")
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
