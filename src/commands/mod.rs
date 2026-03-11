mod ahoy;
mod blp;
mod icon;
mod link;
mod png;
mod rembg;
mod raw2i;
mod i2raw;

use crate::error::{BotError};
use crate::discord::discord::Interaction;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct SlashCommand {
    pub name: String,
    #[serde(rename = "type")]
    pub command_type: u8,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<SlashCommandOption>>,
}

#[derive(Debug, Serialize)]
pub struct SlashCommandOption {
    pub name: String,
    pub description: String,
    #[serde(rename = "type")]
    pub option_type: u8,
    pub required: bool,
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
        icon::Icon::definition(),
        link::Link::definition(),
        png::Png::definition(),
        rembg::Rembg::definition(),
        raw2i::Raw2i::definition(),
        i2raw::I2raw::definition(),
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
        "icon" => icon::Icon::handle(interaction).await,
        "png" => png::Png::handle(interaction).await,
        "rembg" => rembg::Rembg::handle(interaction).await,
        "raw2i" => raw2i::Raw2i::handle(interaction).await,
        "i2raw" => i2raw::I2raw::handle(interaction).await,
        "link" => link::Link::handle(interaction).await,
        _ => Ok(()), // Unknown command, ignore
    }
}
