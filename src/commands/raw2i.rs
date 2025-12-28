use crate::commands::{Command, SlashCommand, SlashCommandOption};
use crate::discord::api;
use crate::error::BotError;
use crate::state;
use crate::discord::discord::Interaction;

pub struct Raw2i;

impl Command for Raw2i {
    fn definition() -> SlashCommand {
        SlashCommand {
            name: "raw2i".to_string(),
            command_type: 1,
            description: "Convert Warcraft III rawcode (1-4 ASCII chars) to integer".to_string(),
            options: Some(vec![
                SlashCommandOption {
                    name: "rawcode".to_string(),
                    description: "1-4 character rawcode (e.g., A, Hpal)".to_string(),
                    option_type: 3, // STRING type
                    required: true,
                }
            ]),
        }
    }

    async fn handle(interaction: Interaction) -> Result<(), BotError> {
        let client = state::client().await;
        let token = state::token().await;

        // Extract rawcode parameter
        let rawcode = match &interaction.data {
            Some(data) => {
                match &data.options {
                    Some(options) => {
                        options.iter()
                            .find(|opt| opt.name == "rawcode")
                            .and_then(|opt| opt.value.as_ref())
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                    }
                    None => ""
                }
            }
            None => ""
        };

        let response = if rawcode.is_empty() {
            "❌ Please provide a rawcode".to_string()
        } else {
            match rawcode_to_int(rawcode) {
                Ok(value) => format_rawcode_result(rawcode, value),
                Err(e) => format!("❌ Error: {}", e),
            }
        };

        api::respond_to_interaction(
            &client,
            &token,
            &interaction.id,
            &interaction.token,
            response,
        )
        .await
    }
}

/// Convert a 1-4 character rawcode to integer
pub fn rawcode_to_int(rawcode: &str) -> Result<u32, String> {
    let bytes = rawcode.as_bytes();
    if bytes.is_empty() || bytes.len() > 4 {
        return Err(format!("Rawcode must be 1-4 characters, got {}", bytes.len()));
    }

    // Pad to 4 bytes with zeros at the END (big-endian style)
    let mut padded = [0u8; 4];
    for (i, &b) in bytes.iter().enumerate() {
        padded[i] = b;
    }

    // Convert 4 bytes to u32 (big-endian - text order)
    let result = u32::from_be_bytes(padded);
    Ok(result)
}

/// Format the conversion result as a Discord message
pub fn format_rawcode_result(rawcode: &str, value: u32) -> String {
    let signed_value = value as i32;
    format!(
        "**rawcode**\n```\n{}\n```\n\
**radix 8**\n```\n0{:o}\n```\n\
**radix 10: i32**\n```\n{}\n```\n\
**radix 10: u32**\n```\n{}\n```\n\
**radix 16**\n```\n0x{:08X}\n```",
        rawcode,
        value,
        signed_value,
        value,
        value
    )
}

