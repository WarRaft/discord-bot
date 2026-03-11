use crate::commands::{Command, SlashCommand};
use crate::discord::api;
use crate::error::BotError;
use crate::state;
use crate::discord::discord::Interaction;

pub struct Link;

impl Command for Link {
    fn definition() -> SlashCommand {
        SlashCommand {
            name: "link".to_string(),
            command_type: 1,
            description: "Get the bot invite link".to_string(),
            options: None,
        }
    }

    async fn handle(interaction: Interaction) -> Result<(), BotError> {
        let client = state::client().await;
        let token = state::token().await;
        let invite_url = state::get_invite_url().await;

        let message = format!(
            "```\n{}\n```\n\
**Required permissions:**\n\
• **View Channels** — needed to receive slash commands and mentions\n\
• **Send Messages** — needed to reply with results and error messages\n\
• **Attach Files** — needed to send converted BLP, PNG and ZIP files\n\
• **Read Message History** — needed to access images from replied-to messages",
            invite_url
        );

        api::respond_to_interaction(
            &client,
            &token,
            &interaction.id,
            &interaction.token,
            message,
        )
        .await
    }
}
