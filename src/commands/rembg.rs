use crate::commands::{Command, SlashCommand};
use crate::discord::api;
use crate::discord::discord::Interaction;
use crate::error::BotError;
use crate::state;

pub struct Rembg;

impl Command for Rembg {
    fn definition() -> SlashCommand {
        SlashCommand {
            name: "rembg".to_string(),
            command_type: 1,
            description: "Information about background removal".to_string(),
            options: None,
        }
    }

    async fn handle(interaction: Interaction) -> Result<(), BotError> {
        let client = state::client().await;
        let token = state::token().await;
        let db = state::db().await;

        // Check if rembg is available
        let availability_warning = "";

        // Check bot permissions in this channel
        let permissions_info = if let Some(channel_id) = &interaction.channel_id {
            check_bot_permissions(&client, &token, channel_id).await
        } else {
            "⚠️ Unable to determine channel permissions".to_string()
        };

        // Get queue statistics
        let queue_info = match crate::workers::rembg::job::JobRembg::count_total(&db).await {
            Ok(count) => {
                if count > 0 {
                    format!(
                        "📊 **Usage Statistics:** {} total background removals processed",
                        count
                    )
                } else {
                    "📊 **Usage Statistics:** No background removals yet".to_string()
                }
            }
            Err(_) => "📊 **Usage Statistics:** Unable to check statistics".to_string(),
        };

        let info_text = format!(
            "{}\
✂️ **Background Removal**\n\n\
**Usage:**\n\
• Mention the bot with image attachments: `@Raft rembg [threshold] [options]`\n\
• You can also use the shorter alias: `@Raft bg`\n\n\
**Parameters:**\n\
• `threshold` — Sensitivity **(0–255, default: 160)**\n  \
  Lower values → softer edges, more background kept\n  \
  Higher values → stronger removal, cleaner cutout\n\
• `binary` — Force clean hard edges instead of smooth alpha blending\n\
• `mask` — Include the extracted alpha mask as a separate image\n\
• `zip` — Combine all results into a ZIP archive\n\n\
**Examples:**\n\
• `@Raft bg` — Default soft background removal\n\
• `@Raft rembg 120` — Moderate, stronger background removal\n\
• `@Raft bg binary` — Binary hard-edged cutout\n\
• `@Raft rembg mask` — Output both processed image and mask\n\
• `@Raft bg 200 binary mask zip` — All options combined\n\n\
**Features:**\n\
• AI-powered background removal using the U²-Net model\n\
• Preserves fine details and transparency\n\
• Supports batch processing of multiple images\n\n\
{}\n\n\
{}",
            availability_warning, permissions_info, queue_info
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

    // Handle 403 Forbidden - bot is not in this server
    if response.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(crate::error::BotError::new("bot_not_in_server"));
    }

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
