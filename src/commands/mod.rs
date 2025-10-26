mod ahoy;
mod blp;
mod png;
mod rembg;

use crate::error::{BotError};
use crate::types::discord::Interaction;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct SlashCommand {
    pub name: String,
    #[serde(rename = "type")]
    pub command_type: u8,
    pub description: String,
}

/// Trait for Discord slash commands
pub trait Command {
    /// Get command definition for registration
    fn definition() -> SlashCommand;
    
    /// Handle command execution
    fn handle(interaction: Interaction) -> impl std::future::Future<Output = Result<(), BotError>> + Send;
}

/// Get all registered commands for Discord API registration
pub fn all_commands() -> Vec<SlashCommand> {
    vec![
        ahoy::Ahoy::definition(),
        blp::Blp::definition(),
        png::Png::definition(),
        rembg::Rembg::definition(),
    ]
}

/// Route interaction to appropriate command handler
pub async fn handle_interaction(interaction: Interaction) -> Result<(), BotError> {
    if interaction.interaction_type != 2 {
        // Not an application command
        return Ok(());
    }

    let Some(data) = &interaction.data else {
        return Ok(());
    };

    match data.name.as_str() {
        "ahoy" => ahoy::Ahoy::handle(interaction).await,
        "blp" => blp::Blp::handle(interaction).await,
        "png" => png::Png::handle(interaction).await,
        "rembg" => rembg::Rembg::handle(interaction).await,
        _ => Ok(()), // Unknown command, ignore
    }
}
