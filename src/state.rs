use reqwest::Client;
use std::sync::Arc;
use tokio::sync::{Mutex, OnceCell};

use crate::db::{mongo::mongo_pool, state::DiscordState};
use crate::error::Result;

static BOT_STATE: OnceCell<Arc<BotStateInner>> = OnceCell::const_new();

pub(crate) struct BotStateInner {
    token: String,
    client: Client,
    sequence: Mutex<Option<u64>>,
    session_id: Mutex<Option<String>>,
    commands_registered: Mutex<bool>,
    db: Arc<mongodb::Database>,
}

pub(crate) async fn bot_state() -> Arc<BotStateInner> {
    BOT_STATE
        .get()
        .expect("BotState not initialized. Call init_bot_state() first.")
        .clone()
}

pub async fn init_bot_state(token: String, mongo_url: &str, mongo_db: &str) -> Result<()> {
    let db = mongo_pool(mongo_url, mongo_db).await;
    let saved_state = DiscordState::load(&db).await?;
    
    BOT_STATE.get_or_init(|| async {
        Arc::new(BotStateInner {
            token,
            client: Client::new(),
            sequence: Mutex::new(saved_state.sequence),
            session_id: Mutex::new(saved_state.session_id),
            commands_registered: Mutex::new(false),
            db,
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

async fn save_state() -> Result<()> {
    let state = bot_state().await;
    let session_id = state.session_id.lock().await.clone();
    let sequence = *state.sequence.lock().await;
    
    let discord_state = DiscordState {
        id: "bot_state".to_string(),
        session_id,
        sequence,
    };
    
    discord_state.save(&state.db).await
}

pub async fn should_register_commands() -> bool {
    let state = bot_state().await;
    let mut registered = state.commands_registered.lock().await;
    if !*registered {
        *registered = true;
        true
    } else {
        false
    }
}

pub async fn token() -> String {
    bot_state().await.token.clone()
}

pub async fn client() -> Client {
    bot_state().await.client.clone()
}

pub async fn log_heartbeat() -> Result<i64> {
    let state = bot_state().await;
    crate::db::heartbeat::Heartbeat::increment(&state.db).await
}

pub async fn db() -> Arc<mongodb::Database> {
    bot_state().await.db.clone()
}
