mod commands;
mod db;
mod discord;
mod error;
mod state;
mod types;
mod workers;

use std::env;
use std::path::Path;
use tokio::time::Duration;
use crate::error::BotError;

// Number of concurrent BLP processing workers
const BLP_WORKER_COUNT: usize = 3;
// Number of concurrent rembg processing workers
const REMBG_WORKER_COUNT: usize = 3;

/// Download ONNX Runtime and AI models
async fn download_models_and_runtime() -> Result<(), BotError> {
    use std::process::Command;
    use tokio::fs;

    // Check and install ONNX Runtime
    eprintln!("🔍 Checking ONNX Runtime installation...");

    let check_output = Command::new("ldconfig").arg("-p").output();

    let onnx_installed = if let Ok(output) = check_output {
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout.contains("libonnxruntime.so")
    } else {
        false
    };

    if onnx_installed {
        eprintln!("✅ ONNX Runtime already installed");
    } else {
        eprintln!("📦 Installing ONNX Runtime 1.16.0...");

        let client = state::client().await;
        let url = "https://github.com/microsoft/onnxruntime/releases/download/v1.16.0/onnxruntime-linux-x64-1.16.0.tgz";

        eprintln!("  Downloading (~16 MB)...");
        let response = client.get(url).send().await?;
        let bytes = response.bytes().await?;

        eprintln!("  Extracting...");
        let tmp_dir = "/tmp/onnxruntime-download";
        fs::create_dir_all(tmp_dir).await?;

        let tar_path = format!("{}/onnxruntime.tgz", tmp_dir);
        fs::write(&tar_path, bytes).await?;

        // Extract tar.gz
        let extract_result = Command::new("tar")
            .arg("-xzf")
            .arg(&tar_path)
            .arg("-C")
            .arg(tmp_dir)
            .output()?;

        if !extract_result.status.success() {
            return Err(error::BotError::new("onnx_extract_failed")
                .push_str("Failed to extract ONNX Runtime archive".to_string()));
        }

        eprintln!("  Installing to /usr/local/lib...");

        // Copy all .so files with wildcard
        let cp_result = Command::new("bash")
            .arg("-c")
            .arg(format!(
                "sudo cp {}/onnxruntime-linux-x64-1.16.0/lib/libonnxruntime.so* /usr/local/lib/",
                tmp_dir
            ))
            .output()?;

        if !cp_result.status.success() {
            let stderr = String::from_utf8_lossy(&cp_result.stderr);
            eprintln!("  Copy error: {}", stderr);
            return Err(error::BotError::new("onnx_install_failed").push_str(
                "Failed to copy ONNX Runtime library. Run with sudo permissions.".to_string(),
            ));
        }

        eprintln!("  Updating library cache...");
        let ldconfig_result = Command::new("sudo").arg("ldconfig").output()?;

        if !ldconfig_result.status.success() {
            eprintln!("  [WARN] ldconfig failed, but continuing...");
        }

        eprintln!("  Cleaning up...");
        let _ = fs::remove_dir_all(tmp_dir).await;

        eprintln!("✅ ONNX Runtime installed successfully");
    }

    // Download models
    eprintln!("\n📦 Downloading AI models...");

    fs::create_dir_all("models").await?;

    let models = vec![
        (
            "u2net.onnx",
            "https://github.com/danielgatis/rembg/releases/download/v0.0.0/u2net.onnx",
            "~176 MB",
        ),
        (
            "u2net_human_seg.onnx",
            "https://github.com/danielgatis/rembg/releases/download/v0.0.0/u2net_human_seg.onnx",
            "~176 MB",
        ),
        (
            "silueta.onnx",
            "https://github.com/danielgatis/rembg/releases/download/v0.0.0/silueta.onnx",
            "~43 MB",
        ),
    ];

    let client = state::client().await;

    for (filename, url, size) in models {
        let path = format!("models/{}", filename);

        if Path::new(&path).exists() {
            eprintln!("✅ {} already exists", filename);
            continue;
        }

        eprintln!("📥 Downloading {} ({})...", filename, size);

        let response = client.get(url).send().await?;
        let bytes = response.bytes().await?;

        fs::write(&path, bytes).await?;

        eprintln!("✅ {} downloaded", filename);
    }

    eprintln!("\n✅ All models downloaded successfully!");
    eprintln!("🔄 Restart the bot to enable background removal");

    Ok(())
}

async fn register_commands() -> Result<(), BotError> {
    println!("[INFO] Starting slash commands registration...");

    let token = state::token().await;
    let client = state::client().await;

    println!("[INFO] Getting application ID...");
    let app_id = discord::api::get_application_id(&client, &token).await?;
    println!("[INFO] Application ID: {}", app_id);

    // Save application ID to state for invite URL generation
    state::set_application_id(app_id.clone()).await;

    println!("[INFO] Registering slash commands...");
    discord::api::register_slash_commands(&client, &token, &app_id).await?;

    println!(
        "[INFO] Commands registered successfully! Waiting 2 seconds for Discord to process..."
    );
    tokio::time::sleep(Duration::from_secs(2)).await;
    println!("[INFO] Commands registration completed!");
    Ok(())
}

async fn run_bot() -> Result<(), BotError> {
    // Fetch and store bot info with session limits
    let token = state::token().await;
    let client = state::client().await;
    let _ = discord::api::get_gateway_bot_info(&client, &token).await;

    // Commands are only registered manually via SIGUSR1 signal
    // No automatic registration on startup to avoid rate limiting

    let gateway_url = discord::api::get_gateway_url(&client, &token).await?;
    discord::gateway::run_gateway(gateway_url).await
}

#[tokio::main]
async fn main() -> Result<(), BotError> {
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
    
    db::blp_queue::BlpQueueItem::reset_stuck_items(&*db, 10).await?;

    // Start BLP worker pool
    workers::start_blp_workers(BLP_WORKER_COUNT);

    // Start rembg worker pool (only if initialized)
    workers::start_rembg_workers(REMBG_WORKER_COUNT);

    // Setup SIGUSR1 signal handler for command reregistration
    tokio::spawn(async {
        use tokio::signal::unix::{SignalKind, signal};
        let mut stream =
            signal(SignalKind::user_defined1()).expect("Failed to setup SIGUSR1 handler");
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

    // Setup SIGUSR2 signal handler for downloading models
    tokio::spawn(async {
        use tokio::signal::unix::{SignalKind, signal};
        let mut stream =
            signal(SignalKind::user_defined2()).expect("Failed to setup SIGUSR2 handler");
        loop {
            stream.recv().await;
            eprintln!("[SIGNAL] Received SIGUSR2 - downloading models and ONNX Runtime...");

            if let Err(e) = download_models_and_runtime().await {
                eprintln!("[ERROR] Failed to download: {}", e);
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
