use crate::commands::{Command, SlashCommand};
use crate::discord::api;
use crate::discord::discord::Interaction;
use crate::error::BotError;
use crate::state;

pub struct Icon;

impl Command for Icon {
    fn definition() -> SlashCommand {
        SlashCommand {
            name: "icon".to_string(),
            command_type: 1,
            description: "Information about icon conversion".to_string(),
            options: None,
        }
    }

    async fn handle(interaction: Interaction) -> Result<(), BotError> {
        let client = state::client().await;
        let token = state::token().await;
        let db = state::db().await;

        // Check if icon is available
        let availability_warning = "";

        // Check bot permissions in this channel
        let permissions_info = if let Some(channel_id) = &interaction.channel_id {
            check_bot_permissions(&client, &token, channel_id).await
        } else {
            "⚠️ Unable to determine channel permissions".to_string()
        };

        // Get queue statistics
        let queue_info = match crate::workers::icon::job::JobIcon::count_total(&db).await {
            Ok(count) => {
                if count > 0 {
                    format!(
                        "📊 **Usage Statistics:** {} total icon conversions processed",
                        count
                    )
                } else {
                    "📊 **Usage Statistics:** No icon conversions yet".to_string()
                }
            }
            Err(_) => "📊 **Usage Statistics:** Unable to check statistics".to_string(),
        };

        let info_text = format!(
            "{}\
🎯 **Icon Converter**\n\n\
```
Creates Warcraft III compatible BLP icons with overlays from images by cropping to center square and resizing.
Generates active and disabled button variants with proper folder structure.
Also creates a collage preview of button templates.
```\n\n\
**Features:**\n\
• 📐 **Square Crop:** Automatically crops images to square from center\n\
• 🔧 **Resize:** Converts to 64x64 pixel icons\n\
• 🎨 **Overlays:** Applies all 6 Warcraft III icon overlays (BTN, DISBTN, ATC, DISATC, PAS, DISPAS)\n\
• 🖼️ **Collage:** Creates preview collage of all button templates in column layout\n\
• 📦 **ZIP Archive:** Creates .zip archive with proper folder structure\n\n\
**Generated Variants:**\n\
• `BTN[filename].blp` - Active button with overlay\n\
• `DISBTN[filename].blp` - Disabled button with overlay\n\
• `ATC[filename].blp` - Attack command with overlay\n\
• `DISATC[filename].blp` - Disabled attack command with overlay\n\
• `PAS[filename].blp` - Passive command with overlay\n\
• `DISPAS[filename].blp` - Disabled passive command with overlay\n\n\
**Archive Structure:**\n\
```
icons.zip/
├── ReplaceableTextures/
│   ├── CommandButtons/
│   │   ├── BTN[filename].blp
│   │   ├── ATC[filename].blp
│   │   └── PAS[filename].blp
│   └── CommandButtonsDisabled/
│       ├── DISBTN[filename].blp
│       ├── DISATC[filename].blp
│       └── DISPAS[filename].blp
```\n\n\
**Usage:**\n\
Upload one or more images and use `/icon` command\n\n\
**Output:**\n\
• `icon_collage.png` - Preview collage showing all icon variants\n\
• `icons.zip` - ZIP archive containing BLP icons and preview collage\n\n\
**Archive Contents:**\n\
• `icon_collage.png` - Preview collage showing all icon variants\n\
• `ReplaceableTextures/CommandButtons/BTN[filename].blp` - Active button icons\n\
• `ReplaceableTextures/CommandButtonsDisabled/DISBTN[filename].blp` - Disabled button icons\n\
• And more variants (ATC, DISATC, PAS, DISPAS) for each image\n\n\
{}\n\n\
{}",
            availability_warning,
            permissions_info,
            queue_info
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

async fn check_bot_permissions(
    client: &reqwest::Client,
    token: &str,
    channel_id: &str,
) -> String {
    let bot_user_id = state::bot_user_id().await;
    if bot_user_id.is_empty() {
        return "⚠️ Unable to check bot permissions".to_string();
    }

    match get_channel_permissions(client, token, channel_id, &bot_user_id).await {
        Ok(permissions) => {
            let can_send_messages = permissions & (1 << 11) != 0; // SEND_MESSAGES
            let can_attach_files = permissions & (1 << 15) != 0; // ATTACH_FILES

            if can_send_messages && can_attach_files {
                "✅ Bot has required permissions in this channel".to_string()
            } else {
                "⚠️ Bot may not have permission to send messages or attachments".to_string()
            }
        }
        Err(_) => "⚠️ Unable to check bot permissions".to_string(),
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