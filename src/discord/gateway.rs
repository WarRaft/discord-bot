use crate::discord::handle::message::handle_message;
use crate::error::Result;
use crate::state;
use crate::types::discord::*;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::time::{Duration, interval};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};

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
                                sequence,
                            )
                            .await;

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
                                &*state::db().await,
                            )
                            .await;

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

                            write
                                .send(WsMessage::Text(payload.to_string().into()))
                                .await?;
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
                        session_id.to_string(),
                    )
                    .await;
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
            let _ = crate::db::session_events::SessionEvent::log_resumed(&*state::db().await).await;
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
