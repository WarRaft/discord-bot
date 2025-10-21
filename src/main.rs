mod commands;
mod db;
mod discord;
mod error;
mod state;
mod types;
mod workers;

use std::env;
use tokio::time::Duration;

use error::Result;

// Number of concurrent BLP processing workers
const BLP_WORKER_COUNT: usize = 3;

async fn register_commands() -> Result<()> {
    let token = state::token().await;
    let client = state::client().await;
    let app_id = discord::api::get_application_id(&client, &token).await?;

    discord::api::register_slash_commands(&client, &token, &app_id).await?;
    tokio::time::sleep(Duration::from_secs(2)).await;
    Ok(())
}

async fn run_bot() -> Result<()> {
    // Fetch and store bot info with session limits
    let token = state::token().await;
    let client = state::client().await;
    let _ = discord::api::get_gateway_bot_info(&client, &token).await;

    // Register slash commands only on first start to avoid rate limiting
    if state::should_register_commands().await {
        if let Err(e) = register_commands().await {
            eprintln!("[ERROR] Failed to register commands:");
            e.print_tree();
        }
    }

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

    state::init_bot_state(token, &mongo_url, &mongo_db).await?;
    
    // Reset stuck BLP queue items from previous run
    let db = state::db().await;
    match db::blp_queue::BlpQueueItem::reset_stuck_items(&*db, 10).await {
        Ok(count) if count > 0 => {
            eprintln!("[QUEUE] Reset {} stuck BLP processing items", count);
        }
        Ok(_) => {}
        Err(e) => {
            eprintln!("[QUEUE] Failed to reset stuck items: {:?}", e);
        }
    }
    
    // Start BLP worker pool
    workers::start_blp_workers(BLP_WORKER_COUNT);
    
    // Setup SIGUSR1 signal handler for command reregistration
    tokio::spawn(async {
        use tokio::signal::unix::{signal, SignalKind};
        let mut stream = signal(SignalKind::user_defined1()).expect("Failed to setup SIGUSR1 handler");
        loop {
            stream.recv().await;
            eprintln!("[SIGNAL] Received SIGUSR1 - reregistering commands...");
            if let Err(e) = register_commands().await {
                eprintln!("[ERROR] Failed to reregister commands:");
                e.print_tree();
            } else {
                eprintln!("[SIGNAL] Commands reregistered successfully");
            }
        }
    });
    
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
