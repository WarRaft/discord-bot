use crate::commands::{Command, SlashCommand, SlashCommandOption};
use crate::discord::api;
use crate::error::BotError;
use crate::state;
use crate::discord::discord::Interaction;

pub struct I2raw;

impl Command for I2raw {
    fn definition() -> SlashCommand {
        SlashCommand {
            name: "i2raw".to_string(),
            command_type: 1,
            description: "Convert integer to Warcraft III rawcode (1-4 ASCII chars)".to_string(),
            options: Some(vec![
                SlashCommandOption {
                    name: "value".to_string(),
                    description: "Integer value to convert".to_string(),
                    option_type: 3, // STRING type (to support hex input)
                    required: true,
                }
            ]),
        }
    }

    async fn handle(interaction: Interaction) -> Result<(), BotError> {
        let client = state::client().await;
        let token = state::token().await;

        // Extract value parameter
        let value_str = match &interaction.data {
            Some(data) => {
                match &data.options {
                    Some(options) => {
                        options.iter()
                            .find(|opt| opt.name == "value")
                            .and_then(|opt| opt.value.as_ref())
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                    }
                    None => ""
                }
            }
            None => ""
        };

        let response = if value_str.is_empty() {
            "❌ Please provide an integer value".to_string()
        } else {
            match parse_integer(value_str) {
                Ok(value) => format_int_result(value),
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

/// Parse integer from string (supports decimal, hex with 0x prefix, octal with 0o prefix)
fn parse_integer(s: &str) -> Result<u32, String> {
    let s = s.trim();
    
    if s.starts_with("0x") || s.starts_with("0X") {
        u32::from_str_radix(&s[2..], 16)
            .map_err(|e| format!("Invalid hexadecimal: {}", e))
    } else if s.starts_with("0o") || s.starts_with("0O") {
        u32::from_str_radix(&s[2..], 8)
            .map_err(|e| format!("Invalid octal: {}", e))
    } else {
        s.parse::<u32>()
            .map_err(|e| format!("Invalid decimal: {}", e))
    }
}

/// Convert integer to rawcode (1-4 characters, trimming leading zeros)
pub fn int_to_rawcode(value: u32) -> String {
    let bytes = value.to_be_bytes(); // Big-endian for text order
    
    // Find the first non-zero byte (skip leading zeros)
    let start = bytes.iter()
        .position(|&b| b != 0)
        .unwrap_or(3); // If all zeros, start from last byte
    
    String::from_utf8_lossy(&bytes[start..]).to_string()
}

/// Format the conversion result as a Discord message
pub fn format_int_result(value: u32) -> String {
    let rawcode = int_to_rawcode(value);
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

