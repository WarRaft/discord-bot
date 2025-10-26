use crate::commands::{Command, SlashCommand};
use crate::discord::api;
use crate::error::BotError;
use crate::state;
use crate::discord::discord::Interaction;

pub struct Ahoy;

impl Command for Ahoy {
    fn definition() -> SlashCommand {
        SlashCommand {
            name: "ahoy".to_string(),
            command_type: 1,
            description: "A pirate greeting".to_string(),
        }
    }

    async fn handle(interaction: Interaction) -> Result<(), BotError> {
        let client = state::client().await;
        let token = state::token().await;

        api::respond_to_interaction(
            &client,
            &token,
            &interaction.id,
            &interaction.token,
            "Aye aye, Captain! Raft's afloat!".to_string(),
        )
        .await
    }
}
