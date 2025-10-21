use serde::{Deserialize, Serialize};
use serde_json::Value;

// Discord Gateway opcodes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opcode {
    Dispatch = 0,
    Heartbeat = 1,
    Identify = 2,
    Resume = 6,
    InvalidSession = 9,
    Hello = 10,
    HeartbeatAck = 11,
    Unknown,
}

impl Opcode {
    pub fn from_u8(op: u8) -> Self {
        match op {
            0 => Self::Dispatch,
            1 => Self::Heartbeat,
            2 => Self::Identify,
            6 => Self::Resume,
            9 => Self::InvalidSession,
            10 => Self::Hello,
            11 => Self::HeartbeatAck,
            _ => Self::Unknown,
        }
    }
}

// Discord Gateway event types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    Ready,
    Resumed,
    InteractionCreate,
    Unknown,
}

impl EventType {
    pub fn from_str(s: &str) -> Self {
        match s {
            "READY" => Self::Ready,
            "RESUMED" => Self::Resumed,
            "INTERACTION_CREATE" => Self::InteractionCreate,
            _ => Self::Unknown,
        }
    }
}

// Gateway response
#[derive(Debug, Deserialize)]
pub struct GatewayResponse {
    pub url: String,
}

// Discord event from WebSocket
#[derive(Debug, Deserialize)]
pub struct DiscordEvent {
    pub op: u8,
    pub d: Option<Value>,
    pub s: Option<u64>,
    pub t: Option<String>,
}

impl DiscordEvent {
    pub fn opcode(&self) -> Opcode {
        Opcode::from_u8(self.op)
    }
    
    pub fn event_type(&self) -> EventType {
        self.t.as_deref()
            .map(EventType::from_str)
            .unwrap_or(EventType::Unknown)
    }
}

// Interaction from Discord
#[derive(Debug, Deserialize)]
pub struct Interaction {
    pub id: String,
    #[serde(rename = "type")]
    pub interaction_type: u8,
    pub data: Option<InteractionData>,
    pub token: String,
}

#[derive(Debug, Deserialize)]
pub struct InteractionData {
    pub name: String,
}

// Interaction response to Discord
#[derive(Debug, Serialize)]
pub struct InteractionResponse {
    #[serde(rename = "type")]
    pub response_type: u8,
    pub data: Option<InteractionResponseData>,
}

#[derive(Debug, Serialize)]
pub struct InteractionResponseData {
    pub content: String,
}

// Discord API error response
#[derive(Debug, Deserialize)]
pub struct DiscordErrorResponse {
    pub message: String,
    #[serde(default)]
    pub code: Option<i32>,
    #[serde(default)]
    pub retry_after: Option<f64>,
    #[serde(default)]
    pub global: Option<bool>,
    #[serde(default)]
    pub errors: Option<Value>,
}

impl std::fmt::Display for DiscordErrorResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", 
            self.code.unwrap_or(0), 
            self.message
        )?;
        
        if let Some(retry) = self.retry_after {
            write!(f, " (retry after {:.3}s)", retry)?;
        }
        
        if let Some(global) = self.global {
            if global {
                write!(f, " [GLOBAL]")?;
            }
        }
        
        if let Some(errors) = &self.errors {
            write!(f, "\nDetails: {}", serde_json::to_string_pretty(errors).unwrap_or_default())?;
        }
        
        Ok(())
    }
}

// Application info
#[derive(Debug, Deserialize)]
pub struct ApplicationInfo {
    pub id: String,
}

// Gateway Bot info with session limits
#[derive(Debug, Deserialize)]
pub struct GatewayBotInfo {
    #[allow(dead_code)]
    pub url: String,
    pub shards: i32,
    pub session_start_limit: SessionStartLimit,
}

#[derive(Debug, Deserialize)]
pub struct SessionStartLimit {
    pub total: i32,
    pub remaining: i32,
    pub reset_after: i64,  // milliseconds
    pub max_concurrency: i32,
}
