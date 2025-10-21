use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::time::{Duration, interval};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};

use crate::error::Result;
use crate::types::discord::*;
use crate::state;

pub async fn run_gateway(gateway_url: String) -> Result<()> {
    let (ws_stream, _) = connect_async(&gateway_url).await?;
    let (mut write, mut read) = ws_stream.split();

    while let Some(msg) = read.next().await {
        let msg = msg?;

        if let WsMessage::Text(text) = msg {
            let event: DiscordEvent = serde_json::from_str(&text)?;
            state::update_sequence(event.s).await;

            match event.opcode() {
                Opcode::Hello => {
                    // Hello - start heartbeat loop
                    if let Some(d) = event.d {
                        let interval_ms = d["heartbeat_interval"].as_u64().unwrap();

                        // Try RESUME if we have session_id, otherwise IDENTIFY
                        let token = state::token().await;
                        let session_id = state::get_session_id().await;
                        let sequence = state::get_sequence().await;

                        if let Some(sid) = session_id {
                            // RESUME - reconnect with existing session
                            let _ = crate::db::session_events::SessionEvent::log_resume(
                                &*state::db().await,
                                sid.clone(),
                                sequence
                            ).await;

                            let resume = json!({
                                "op": Opcode::Resume as u8,
                                "d": {
                                    "token": token,
                                    "session_id": sid,
                                    "seq": sequence
                                }
                            });
                            
                            write
                                .send(WsMessage::Text(resume.to_string().into()))
                                .await?;
                        } else {
                            // IDENTIFY - new session
                            let _ = crate::db::session_events::SessionEvent::log_identify(
                                &*state::db().await
                            ).await;

                            let payload = json!({
                        "op": 2,
                        "d": {
                            "token": token,
                            "intents": 33280, // GUILDS (1 << 0) + GUILD_MESSAGES (1 << 9) + MESSAGE_CONTENT (1 << 15)
                            "properties": {
                                "os": "linux",
                                "browser": "discord-bot",
                                "device": "discord-bot"
                            }
                        }
                    });

                    write.send(WsMessage::Text(payload.to_string().into())).await?;
                        }

                        // Start heartbeat timer
                        let mut heartbeat_timer = interval(Duration::from_millis(interval_ms));
                        heartbeat_timer.tick().await; // First tick immediately
                        
                        // Handle both heartbeats and messages
                        loop {
                            tokio::select! {
                                _ = heartbeat_timer.tick() => {
                                    let seq = state::get_sequence().await;
                                    let heartbeat = json!({
                                        "op": Opcode::Heartbeat as u8,
                                        "d": seq
                                    });
                                    
                                    if write.send(WsMessage::Text(heartbeat.to_string().into())).await.is_err() {
                                        break;
                                    }
                                    
                                    // Log heartbeat to MongoDB
                                    let _ = state::log_heartbeat().await;
                                }
                                Some(msg_result) = read.next() => {
                                    match msg_result {
                                        Ok(WsMessage::Text(text)) => {
                                            let event: DiscordEvent = serde_json::from_str(&text)?;
                                            state::update_sequence(event.s).await;
                                            
                                            match event.opcode() {
                                                Opcode::Dispatch => {
                                                    // Dispatch
                                                    handle_dispatch_event(event).await?;
                                                }
                                                Opcode::InvalidSession => {
                                                    // Invalid Session - need to re-identify
                                                    let _ = crate::db::session_events::SessionEvent::log_invalid_session(
                                                        &*state::db().await
                                                    ).await;
                                                    state::clear_session().await;
                                                    return Ok(());
                                                }
                                                Opcode::HeartbeatAck => {
                                                    // Heartbeat ACK - silent
                                                }
                                                _ => {}
                                            }
                                        }
                                        Ok(_) => {}
                                        Err(e) => return Err(e.into()),
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    Ok(())
}

async fn handle_dispatch_event(event: DiscordEvent) -> Result<()> {
    match event.event_type() {
        EventType::Ready => {
            if let Some(d) = event.d {
                if let Some(session_id) = d["session_id"].as_str() {
                    state::set_session_id(session_id.to_string()).await;
                    let _ = crate::db::session_events::SessionEvent::log_ready(
                        &*state::db().await,
                        session_id.to_string()
                    ).await;
                }
                // Store bot user ID
                if let Some(user_id) = d["user"]["id"].as_str() {
                    state::set_bot_user_id(user_id.to_string()).await;
                } else {
                    eprintln!("[ERROR] Failed to get bot user ID from READY event");
                }
            }
        }
        EventType::Resumed => {
            // Session resumed successfully
            let _ = crate::db::session_events::SessionEvent::log_resumed(
                &*state::db().await
            ).await;
        }
        EventType::InteractionCreate => {
            if let Some(d) = event.d {
                if let Ok(interaction) = serde_json::from_value::<Interaction>(d) {
                    if let Err(e) = crate::commands::handle_interaction(interaction).await {
                        eprintln!("[ERROR] Failed to handle interaction:");
                        e.print_tree();
                    }
                }
            }
        }
        EventType::MessageCreate => {
            if let Some(d) = event.d {
                if let Ok(message) = serde_json::from_value::<Message>(d.clone()) {
                    if let Err(e) = handle_message(message).await {
                        eprintln!("[ERROR] Failed to handle message:");
                        e.print_tree();
                    }
                } else {
                    eprintln!("[ERROR] Failed to parse MESSAGE_CREATE event");
                }
            }
        }
        EventType::Unknown => {}
    }
    Ok(())
}

async fn handle_message(message: Message) -> Result<()> {
    // Ignore bot messages
    if message.author.bot.unwrap_or(false) {
        return Ok(());
    }

    // Check if bot is mentioned
    let bot_user_id = state::bot_user_id().await;
    
    let bot_mentioned = message.mentions.iter().any(|m| m.id == bot_user_id);
    
    if !bot_mentioned {
        return Ok(());
    }

    // Check if message contains "blp" or "png" command (trim whitespace)
    let content_lower = message.content.trim().to_lowercase();
    
    let (_command, conversion_type) = if content_lower.contains("blp") {
        ("blp", crate::db::blp_queue::ConversionType::ToBLP)
    } else if content_lower.contains("png") {
        ("png", crate::db::blp_queue::ConversionType::ToPNG)
    } else {
        return Ok(());
    };

    // Check if there are attachments
    if message.attachments.is_empty() {
        return Ok(());
    }

    // Parse quality from message (default 80, only used for BLP conversion)
    let quality = parse_quality_from_content(&message.content).unwrap_or(80);

    // Add to queue
    use crate::db::blp_queue::{AttachmentItem, BlpQueueItem};
    
    let attachments: Vec<AttachmentItem> = message.attachments
        .into_iter()
        .map(|att| AttachmentItem {
            url: att.url,
            filename: att.filename,
            converted_path: None,
        })
        .collect();

    // Send confirmation message FIRST to get message ID
    // Acquire rate limit token before sending
    let limiter = state::rate_limiter().await;
    limiter.acquire().await;
    
    let client = state::client().await;
    let token = state::token().await;
    
    let format_desc = match conversion_type {
        crate::db::blp_queue::ConversionType::ToBLP => format!("to BLP (quality: {})", quality),
        crate::db::blp_queue::ConversionType::ToPNG => "to PNG".to_string(),
    };
    
    let response = client
        .post(&format!("https://discord.com/api/v10/channels/{}/messages", message.channel_id))
        .header("Authorization", format!("Bot {}", token))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "content": format!(
                "✅ Added {} image(s) to conversion queue {}\n⏳ Processing...",
                attachments.len(),
                format_desc
            ),
            "message_reference": {
                "message_id": message.id
            }
        }))
        .send()
        .await?;

    // Get status message ID
    let status_message_id = if response.status().is_success() {
        if let Ok(msg_data) = response.json::<serde_json::Value>().await {
            msg_data["id"].as_str().map(|s| s.to_string())
        } else {
            None
        }
    } else {
        None
    };

    // Create queue item with status_message_id already set
    let mut queue_item = BlpQueueItem::new(
        message.author.id.clone(),
        message.channel_id.clone(),
        message.id.clone(),
        String::new(), // No interaction_id for messages
        String::new(), // No interaction_token for messages
        attachments,
        conversion_type,
        quality,
    );
    
    // Set status_message_id before inserting
    queue_item.status_message_id = status_message_id;

    let db = state::db().await;
    let _queue_id = queue_item.insert(&*db).await?;

    // Notify workers that a new task is available
    crate::workers::notify_new_task();

    Ok(())
}

fn parse_quality_from_content(content: &str) -> Option<u8> {
    // Look for "blp 80" or "blp80" pattern
    let words: Vec<&str> = content.split_whitespace().collect();
    
    for (i, word) in words.iter().enumerate() {
        if word.to_lowercase() == "blp" {
            // Check next word
            if let Some(next) = words.get(i + 1) {
                if let Ok(q) = next.parse::<u8>() {
                    if (1..=100).contains(&q) {
                        return Some(q);
                    }
                }
            }
        } else if word.to_lowercase().starts_with("blp") {
            // Check if number follows immediately (e.g., "blp80")
            let num_part = &word[3..];
            if let Ok(q) = num_part.parse::<u8>() {
                if (1..=100).contains(&q) {
                    return Some(q);
                }
            }
        }
    }
    
    None
}
