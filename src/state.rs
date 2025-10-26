use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, OnceCell};
use tokio::time::Instant;

use crate::db::{mongo::mongo_pool, state::DiscordState};
use crate::error::BotError;

static BOT_STATE: OnceCell<Arc<BotStateInner>> = OnceCell::const_new();

/// Rate limiter using token bucket algorithm
pub(crate) struct RateLimiter {
    tokens: Mutex<f64>,
    max_tokens: f64,
    refill_rate: f64, // tokens per second
    last_refill: Mutex<Instant>,
}

impl RateLimiter {
    fn new(requests_per_second: f64) -> Self {
        Self {
            tokens: Mutex::new(requests_per_second),
            max_tokens: requests_per_second,
            refill_rate: requests_per_second,
            last_refill: Mutex::new(Instant::now()),
        }
    }

    pub async fn acquire(&self) {
        loop {
            // Refill tokens based on elapsed time
            let now = Instant::now();
            let mut last_refill = self.last_refill.lock().await;
            let elapsed = now.duration_since(*last_refill).as_secs_f64();
            
            let mut tokens = self.tokens.lock().await;
            let new_tokens = (*tokens + elapsed * self.refill_rate).min(self.max_tokens);
            *tokens = new_tokens;
            *last_refill = now;

            // Try to consume one token
            if *tokens >= 1.0 {
                *tokens -= 1.0;
                return;
            }

            // Not enough tokens, wait for refill
            drop(tokens);
            drop(last_refill);
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

pub(crate) struct BotStateInner {
    token: String,
    client: Client,
    sequence: Mutex<Option<u64>>,
    session_id: Mutex<Option<String>>,
    db: Arc<mongodb::Database>,
    bot_user_id: Mutex<Option<String>>,
    application_id: Mutex<Option<String>>,
    // Rate limiter: Discord allows ~50 requests per second globally
    // We use 45/sec to have safety margin
    rate_limiter: Arc<RateLimiter>,
}

pub(crate) async fn bot_state() -> Arc<BotStateInner> {
    BOT_STATE
        .get()
        .expect("BotState not initialized. Call init_bot_state() first.")
        .clone()
}

pub async fn init_bot_state(token: String, mongo_url: &str, mongo_db: &str) -> Result<(), BotError> {
    let db = mongo_pool(mongo_url, mongo_db).await;
    let saved_state = DiscordState::load(&db).await?;
    
    // Use rate limit from DB or default to 40 req/sec (safe margin from Discord's ~50)
    let rate_limit = saved_state.rate_limit.unwrap_or(40.0);
    
    BOT_STATE.get_or_init(|| async {
        Arc::new(BotStateInner {
            token,
            client: Client::new(),
            sequence: Mutex::new(saved_state.sequence),
            session_id: Mutex::new(saved_state.session_id),
            db,
            bot_user_id: Mutex::new(saved_state.bot_user_id),
            application_id: Mutex::new(None),
            rate_limiter: Arc::new(RateLimiter::new(rate_limit)),
        })
    }).await;
    
    Ok(())
}

pub async fn update_sequence(seq: Option<u64>) {
    if let Some(s) = seq {
        let state = bot_state().await;
        *state.sequence.lock().await = Some(s);
        let _ = save_state().await;
    }
}

pub async fn get_sequence() -> Option<u64> {
    let state = bot_state().await;
    *state.sequence.lock().await
}

pub async fn get_session_id() -> Option<String> {
    let state = bot_state().await;
    state.session_id.lock().await.clone()
}

pub async fn set_session_id(id: String) {
    let state = bot_state().await;
    *state.session_id.lock().await = Some(id.clone());
    let _ = save_state().await;
}

pub async fn clear_session() {
    let state = bot_state().await;
    *state.session_id.lock().await = None;
    *state.sequence.lock().await = None;
    let _ = save_state().await;
}

async fn save_state() -> Result<(), BotError> {
    let state = bot_state().await;
    let session_id = state.session_id.lock().await.clone();
    let sequence = *state.sequence.lock().await;
    let bot_user_id = state.bot_user_id.lock().await.clone();
    
    let discord_state = DiscordState {
        id: "bot_state".to_string(),
        session_id,
        sequence,
        bot_user_id,
        rate_limit: None, // Don't override DB value when saving session state
    };
    
    discord_state.save(&state.db).await
}

pub async fn token() -> String {
    bot_state().await.token.clone()
}

pub async fn client() -> Client {
    bot_state().await.client.clone()
}

pub async fn log_heartbeat() -> Result<i64, BotError> {
    let state = bot_state().await;
    crate::db::heartbeat::Heartbeat::increment(&state.db).await
}

pub async fn db() -> Arc<mongodb::Database> {
    bot_state().await.db.clone()
}

pub async fn set_bot_user_id(user_id: String) {
    let state = bot_state().await;
    *state.bot_user_id.lock().await = Some(user_id);
}

pub async fn bot_user_id() -> String {
    let state = bot_state().await;
    state.bot_user_id.lock().await.clone().unwrap_or_default()
}

pub async fn set_application_id(app_id: String) {
    let state = bot_state().await;
    *state.application_id.lock().await = Some(app_id);
}

pub async fn application_id() -> String {
    let state = bot_state().await;
    state.application_id.lock().await.clone().unwrap_or_default()
}

/// Generate bot invite URL with required permissions
pub async fn get_invite_url() -> String {
    let app_id = application_id().await;
    if app_id.is_empty() {
        return String::new();
    }
    
    // Permissions: VIEW_CHANNEL (1024) + SEND_MESSAGES (2048) + ATTACH_FILES (32768) + READ_MESSAGE_HISTORY (65536)
    // Total: 101376
    let permissions = 0x400 | 0x800 | 0x8000 | 0x10000; // 101376
    
    format!(
        "https://discord.com/api/oauth2/authorize?client_id={}&permissions={}&scope=bot%20applications.commands",
        app_id, permissions
    )
}

/// Get the rate limiter for Discord API requests
pub async fn rate_limiter() -> Arc<RateLimiter> {
    let state = bot_state().await;
    state.rate_limiter.clone()
}
