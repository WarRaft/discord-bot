use std::env;
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use reqwest::Client;

#[derive(Debug, Deserialize)]
struct GatewayResponse {
    url: String,
}

#[derive(Debug, Deserialize)]
struct DiscordEvent {
    op: u8,
    d: Option<Value>,
    s: Option<u64>,
    t: Option<String>,
}

#[derive(Debug, Serialize)]
struct IdentifyPayload {
    token: String,
    intents: u32,
    properties: ConnectionProperties,
}

#[derive(Debug, Serialize)]
struct ConnectionProperties {
    #[serde(rename = "$os")]
    os: String,
    #[serde(rename = "$browser")]
    browser: String,
    #[serde(rename = "$device")]
    device: String,
}

#[derive(Debug, Serialize)]
struct HeartbeatPayload {
    op: u8,
    d: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct Interaction {
    id: String,
    #[serde(rename = "type")]
    interaction_type: u8,
    data: Option<InteractionData>,
    guild_id: Option<String>,
    channel_id: Option<String>,
    token: String,
}

#[derive(Debug, Deserialize)]
struct InteractionData {
    name: String,
}

#[derive(Debug, Serialize)]
struct InteractionResponse {
    #[serde(rename = "type")]
    response_type: u8,
    data: Option<InteractionResponseData>,
}

#[derive(Debug, Serialize)]
struct InteractionResponseData {
    content: String,
}

#[derive(Debug, Serialize)]
struct SlashCommand {
    name: String,
    description: String,
    #[serde(rename = "type")]
    command_type: u8,
}

struct DiscordBot {
    token: String,
    client: Client,
    sequence: Option<u64>,
    session_id: Option<String>,
}

impl DiscordBot {
    fn new(token: String) -> Self {
        Self {
            token,
            client: Client::new(),
            sequence: None,
            session_id: None,
        }
    }

    async fn get_gateway_url(&self) -> Result<String, Box<dyn std::error::Error>> {
        let response = self
            .client
            .get("https://discord.com/api/v10/gateway")
            .header("Authorization", format!("Bot {}", self.token))
            .send()
            .await?;

        let gateway: GatewayResponse = response.json().await?;
        Ok(format!("{}?v=10&encoding=json", gateway.url))
    }

    async fn register_slash_commands(&self) -> Result<(), Box<dyn std::error::Error>> {
        let commands = vec![
            SlashCommand {
                name: "ahoy".to_string(),
                description: "A pirate greeting".to_string(),
                command_type: 1,
            }
        ];

        // Get application ID from token
        let response = self
            .client
            .get("https://discord.com/api/v10/oauth2/applications/@me")
            .header("Authorization", format!("Bot {}", self.token))
            .send()
            .await?;

        let app_info: Value = response.json().await?;
        let app_id = app_info["id"].as_str().unwrap();

        // Register global commands
        let response = self
            .client
            .put(&format!("https://discord.com/api/v10/applications/{}/commands", app_id))
            .header("Authorization", format!("Bot {}", self.token))
            .header("Content-Type", "application/json")
            .json(&commands)
            .send()
            .await?;

        if response.status().is_success() {
            println!("Successfully registered slash commands");
        } else {
            println!("Failed to register commands: {}", response.status());
        }

        Ok(())
    }

    async fn handle_interaction(&self, interaction: Interaction) -> Result<(), Box<dyn std::error::Error>> {
        if interaction.interaction_type == 2 { // Application Command
            if let Some(data) = &interaction.data {
                match data.name.as_str() {
                    "ahoy" => {
                        let response = InteractionResponse {
                            response_type: 4, // CHANNEL_MESSAGE_WITH_SOURCE
                            data: Some(InteractionResponseData {
                                content: "Aye aye, Captain! Raft's afloat!".to_string(),
                            }),
                        };

                        self.client
                            .post(&format!(
                                "https://discord.com/api/v10/interactions/{}/{}/callback",
                                interaction.id, interaction.token
                            ))
                            .header("Content-Type", "application/json")
                            .json(&response)
                            .send()
                            .await?;

                        println!("Responded to /ahoy command");
                    }
                    _ => {}
                }
            }
        }

        Ok(())
    }

    async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Register slash commands first
        self.register_slash_commands().await?;

        let gateway_url = self.get_gateway_url().await?;
        println!("Connecting to Discord Gateway: {}", gateway_url);

        let (ws_stream, _) = connect_async(&gateway_url).await?;
        let (mut write, mut read) = ws_stream.split();

        let mut heartbeat_interval = None;
        let mut heartbeat_handle = None;

        while let Some(msg) = read.next().await {
            let msg = msg?;

            if let Message::Text(text) = msg {
                let event: DiscordEvent = serde_json::from_str(&text)?;
                
                self.sequence = event.s.or(self.sequence);

                match event.op {
                    10 => { // Hello
                        if let Some(d) = event.d {
                            let interval_ms = d["heartbeat_interval"].as_u64().unwrap();
                            heartbeat_interval = Some(interval_ms);

                            // Send identify
                            let identify = json!({
                                "op": 2,
                                "d": {
                                    "token": self.token,
                                    "intents": 513, // GUILDS + GUILD_MESSAGES
                                    "properties": {
                                        "$os": "linux",
                                        "$browser": "discord-bot",
                                        "$device": "discord-bot"
                                    }
                                }
                            });

                            write.send(Message::Text(identify.to_string())).await?;
                            println!("Sent identify payload");

                            // Start heartbeat task
                            let mut heartbeat_timer = interval(Duration::from_millis(interval_ms));
                            let sequence = Arc::new(tokio::sync::Mutex::new(self.sequence));
                            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

                            // Clone the write half for heartbeat
                            let write_clone = Arc::new(tokio::sync::Mutex::new(write));
                            let write_for_heartbeat = write_clone.clone();

                            heartbeat_handle = Some(tokio::spawn(async move {
                                loop {
                                    tokio::select! {
                                        _ = heartbeat_timer.tick() => {
                                            let seq = *sequence.lock().await;
                                            let heartbeat = json!({
                                                "op": 1,
                                                "d": seq
                                            });
                                            
                                            let mut writer = write_for_heartbeat.lock().await;
                                            if let Err(e) = writer.send(Message::Text(heartbeat.to_string())).await {
                                                println!("Failed to send heartbeat: {}", e);
                                                break;
                                            }
                                            println!("Sent heartbeat");
                                        }
                                        Some(msg) = rx.recv() => {
                                            let mut writer = write_for_heartbeat.lock().await;
                                            if let Err(e) = writer.send(msg).await {
                                                println!("Failed to send message: {}", e);
                                                break;
                                            }
                                        }
                                    }
                                }
                            }));

                            write = Arc::try_unwrap(write_clone).unwrap().into_inner();
                        }
                    }
                    0 => { // Dispatch
                        match event.t.as_deref() {
                            Some("READY") => {
                                if let Some(d) = event.d {
                                    self.session_id = d["session_id"].as_str().map(|s| s.to_string());
                                    println!("Bot is ready!");
                                }
                            }
                            Some("INTERACTION_CREATE") => {
                                if let Some(d) = event.d {
                                    let interaction: Interaction = serde_json::from_value(d)?;
                                    self.handle_interaction(interaction).await?;
                                }
                            }
                            _ => {}
                        }
                    }
                    11 => { // Heartbeat ACK
                        println!("Received heartbeat ACK");
                    }
                    _ => {
                        println!("Received unknown opcode: {}", event.op);
                    }
                }
            }
        }

        if let Some(handle) = heartbeat_handle {
            handle.abort();
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = env::var("DISCORD_BOT_TOKEN")
        .expect("DISCORD_BOT_TOKEN environment variable not set");

    println!("Starting Discord bot...");
    
    let mut bot = DiscordBot::new(token);
    bot.run().await?;

    Ok(())
}