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
            description: "Convert images to BLP format (tag bot with images + 'blp [quality]')".to_string(),
        }
    }

    async fn handle(interaction: Interaction) -> Result<()> {
        let client = state::client().await;
        let token = state::token().await;

        // TODO: Implement full interaction parsing with attachments from mentions
        // For now, just acknowledge the command
        
        api::respond_to_interaction(
            &client,
            &token,
            &interaction.id,
            &interaction.token,
            "⚠️ BLP conversion is under development.\n\n**Usage:** Attach images, mention the bot, and type `blp [quality]`\n**Quality:** 1-100 (default: 80)".to_string(),
        )
        .await?;

        Ok(())
    }
}
