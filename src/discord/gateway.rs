use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::time::{Duration, interval};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

use crate::error::Result;
use crate::types::discord::*;
use crate::discord::api;
use crate::state;

pub async fn run_gateway(gateway_url: String) -> Result<()> {
    let (ws_stream, _) = connect_async(&gateway_url).await?;
    let (mut write, mut read) = ws_stream.split();

    while let Some(msg) = read.next().await {
        let msg = msg?;

        if let Message::Text(text) = msg {
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
                            let resume = json!({
                                "op": Opcode::Resume as u8,
                                "d": {
                                    "token": token,
                                    "session_id": sid,
                                    "seq": sequence
                                }
                            });
                            
                            eprintln!("[GATEWAY] Resuming session {} with seq {:?}", sid, sequence);
                            
                            write
                                .send(Message::Text(resume.to_string().into()))
                                .await?;
                        } else {
                            // IDENTIFY - new session
                            let identify = json!({
                                "op": Opcode::Identify as u8,
                                "d": {
                                    "token": token,
                                    "intents": 513, // GUILDS + GUILD_MESSAGES
                                    "properties": {
                                        "$os": "linux",
                                        "$browser": "discord-bot",
                                        "$device": "discord-bot"
                                    }
                                }
                            });

                            eprintln!("[GATEWAY] Starting new session (IDENTIFY)");

                            write
                                .send(Message::Text(identify.to_string().into()))
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
                                    
                                    if write.send(Message::Text(heartbeat.to_string().into())).await.is_err() {
                                        break;
                                    }
                                }
                                Some(msg_result) = read.next() => {
                                    match msg_result {
                                        Ok(Message::Text(text)) => {
                                            let event: DiscordEvent = serde_json::from_str(&text)?;
                                            state::update_sequence(event.s).await;
                                            
                                            match event.opcode() {
                                                Opcode::Dispatch => {
                                                    // Dispatch
                                                    handle_dispatch_event(event).await?;
                                                }
                                                Opcode::InvalidSession => {
                                                    // Invalid Session - need to re-identify
                                                    eprintln!("[GATEWAY] Invalid session, clearing state for re-identify");
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
                    eprintln!("[GATEWAY] New session established: {}", session_id);
                    state::set_session_id(session_id.to_string()).await;
                }
            }
        }
        EventType::Resumed => {
            eprintln!("[GATEWAY] Session resumed successfully");
        }
        EventType::InteractionCreate => {
            if let Some(d) = event.d {
                if let Ok(interaction) = serde_json::from_value::<Interaction>(d) {
                    if let Err(e) = handle_interaction(interaction).await {
                        eprintln!("[ERROR] Failed to handle interaction:");
                        e.print_tree();
                    }
                }
            }
        }
        EventType::Unknown => {}
    }
    Ok(())
}

async fn handle_interaction(interaction: Interaction) -> Result<()> {
    if interaction.interaction_type == 2 {
        // Application command
        if let Some(data) = interaction.data {
            if data.name == "ahoy" {
                let client = state::client().await;
                let token = state::token().await;
                api::respond_to_interaction(
                    &client,
                    &token,
                    &interaction.id,
                    &interaction.token,
                    "Aye aye, Captain! Raft's afloat!".to_string(),
                )
                .await?;
            }
        }
    }
    Ok(())
}
