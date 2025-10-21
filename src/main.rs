mod db;
mod discord;
mod error;
mod state;
mod types;

use std::env;
use tokio::time::Duration;

use error::Result;

async fn run_bot() -> Result<()> {
    // Register slash commands only on first start to avoid rate limiting
    if state::should_register_commands().await {
        let token = state::token().await;
        let client = state::client().await;
        let app_id = discord::api::get_application_id(&client, &token).await?;
        
        if let Err(e) = discord::api::register_slash_commands(&client, &token, &app_id).await {
            eprintln!("[ERROR] Failed to register commands:");
            e.print_tree();
        }
        
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    let token = state::token().await;
    let client = state::client().await;
    let gateway_url = discord::api::get_gateway_url(&client, &token).await?;
    discord::gateway::run_gateway(gateway_url).await
}

#[tokio::main]
async fn main() -> Result<()> {
    let token = option_env!("DISCORD_BOT_TOKEN")
        .map(String::from)
        .or_else(|| env::var("DISCORD_BOT_TOKEN").ok())
        .expect("DISCORD_BOT_TOKEN not set at compile time or runtime");
    
    let mongo_url = option_env!("MONGO_URL")
        .map(String::from)
        .or_else(|| env::var("MONGO_URL").ok())
        .expect("MONGO_URL not set at compile time or runtime");
    
    let mongo_db = option_env!("MONGO_DB")
        .map(String::from)
        .or_else(|| env::var("MONGO_DB").ok())
        .expect("MONGO_DB not set at compile time or runtime");

    eprintln!("Discord Bot Service - WarRaft (starting)");

    state::init_bot_state(token, &mongo_url, &mongo_db).await?;
    let mut attempt = 0;

    // Infinite retry loop
    loop {
        attempt += 1;

        match run_bot().await {
            Ok(_) => {
                attempt = 0;
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            Err(e) => {
                e.print_tree();

                let wait_time = match attempt {
                    1..=2 => 30,
                    3..=4 => 60,
                    5..=6 => 120,
                    _ => 300,
                };

                eprintln!(
                    "[RETRY] Reconnecting in {} seconds (attempt #{})",
                    wait_time, attempt
                );
                tokio::time::sleep(Duration::from_secs(wait_time)).await;
            }
        }
    }
}
