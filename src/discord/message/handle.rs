use crate::discord::message::message::Message;
use crate::error::BotError;
use crate::state;
use serde::Serialize;
use crate::workers::blp::job::ConversionTarget;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CommandKind {
    Blp,
    Png,
    Rembg, // includes "rembg" and "bg" aliases
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandArgs {
    pub kind: CommandKind,
    pub quality: u8,   // 1..=100 for BLP
    pub threshold: u8, // 0..=255 for REMBG
    pub zip: bool,
    pub mode: bool,
    pub mask: bool,
}

impl Default for CommandArgs {
    fn default() -> Self {
        Self {
            kind: CommandKind::Png,
            quality: 80,
            threshold: 160,
            zip: false,
            mode: false,
            mask: false,
        }
    }
}

pub fn parse_command_args(content: &str, _bot_id: &str) -> Option<CommandArgs> {
    let mut args = CommandArgs::default();

    let tokens: Vec<&str> = content
        .trim()
        .split_whitespace()
        .filter(|t| !t.starts_with('<')) // пропускаем упоминания
        .collect();

    if tokens.is_empty() {
        return None;
    }

    args.kind = match tokens[0] {
        "blp" => CommandKind::Blp,
        "png" => CommandKind::Png,
        "rembg" | "bg" => CommandKind::Rembg,
        _ => return None,
    };

    for &tok in &tokens[1..] {
        match tok {
            "zip" => args.zip = true,
            "binary" => args.mode = true,
            "mask" => args.mask = true,
            _ => {
                if let Ok(num) = tok.parse::<u16>() {
                    match args.kind {
                        CommandKind::Blp if (1..=100).contains(&num) => args.quality = num as u8,
                        CommandKind::Rembg if num <= 255 => args.threshold = num as u8,
                        _ => {}
                    }
                }
            }
        }
    }

    Some(args)
}

pub async fn handle_message(message: Message) -> Result<(), BotError> {
    if message.author.bot.unwrap_or(false) {
        return Ok(());
    }

    let bot_user_id = state::bot_user_id().await;
    if !message.mentions.iter().any(|m| m.id == bot_user_id) {
        return Ok(());
    }

    let Some(args) = parse_command_args(&message.content, &bot_user_id) else {
        return Ok(());
    };

    match args.kind {
        CommandKind::Blp => {
            crate::workers::blp::handle::handle(message, ConversionTarget::BLP, args).await
        }
        CommandKind::Png => {
            crate::workers::blp::handle::handle(message, ConversionTarget::PNG, args).await
        }
        CommandKind::Rembg => {
            crate::workers::rembg::handle::handle(message, &args).await // 
        }
    }
}
