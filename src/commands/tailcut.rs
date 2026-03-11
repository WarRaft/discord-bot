use crate::commands::{Command, SlashCommand};
use crate::discord::api;
use crate::discord::discord::Interaction;
use crate::error::BotError;
use crate::state;

pub struct Tailcut;

impl Command for Tailcut {
    fn definition() -> SlashCommand {
        SlashCommand {
            name: "tailcut".to_string(),
            command_type: 1,
            description: "Information about grid image cutting".to_string(),
            options: None,
        }
    }

    async fn handle(interaction: Interaction) -> Result<(), BotError> {
        let client = state::client().await;
        let token = state::token().await;
        let db = state::db().await;

        // Get queue statistics
        let queue_info =
            match crate::workers::tailcut::job::JobTailcut::count_total(&db).await {
                Ok(count) => {
                    if count > 0 {
                        format!(
                            "📊 **Usage Statistics:** {} total grid cuts processed",
                            count
                        )
                    } else {
                        "📊 **Usage Statistics:** No grid cuts yet".to_string()
                    }
                }
                Err(_) => "📊 **Usage Statistics:** Unable to check statistics".to_string(),
            };

        let info_text = format!(
            "✂️ **Grid Image Cutting (Tailcut)**\n\n\
**Usage:**\n\
• Mention the bot with image attachments: `@Raft tailcut [options]`\n\n\
**How it works:**\n\
• Automatically detects the background colour from the image corners\n\
• Finds the grid of equally-sized sub-images\n\
• Handles padding / margins around the grid\n\
• Cuts and returns every cell as a separate PNG\n\n\
**Parameters:**\n\
• `zip` — Bundle all cut images into a ZIP archive\n\n\
**Examples:**\n\
• `@Raft tailcut` — Cut grid and attach each cell as a separate image\n\
• `@Raft tailcut zip` — Cut grid and return a ZIP archive\n\n\
**Supported Formats:** PNG, JPEG, WebP, BMP, GIF, BLP\n\n\
{}",
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

