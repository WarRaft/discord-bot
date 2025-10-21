# Discord Bot in Rust

Discord bot with `/ahoy` command built with tokio, without specialized Discord libraries.

## Features

- Direct WebSocket connection to Discord Gateway
- Thread-safe state with `Arc<Mutex<>>`
- Slash command support
- Auto-reconnect with progressive backoff
- Custom error handling with stack traces

## Quick Start

```bash
# Set token
export DISCORD_BOT_TOKEN="your_token"

# Build and run
cargo build --release
./target/release/discord-bot
```

## Deployment

```bash
# Configure build-sensitive.sh with your server details
cp build-sensitive.default.sh build-sensitive.sh
nano build-sensitive.sh

# Build and deploy
./build.sh
```

## Project Structure

- `src/main.rs` - bot implementation
- `src/error.rs` - error handling
- `build.sh` - build and deploy script
- `build-sensitive.sh` - server credentials (gitignored)

## Architecture

**Thread-safe state** accessible from all tasks:
```rust
struct BotState {
    token: String,
    client: Client,
    sequence: Arc<Mutex<Option<u64>>>,
    session_id: Arc<Mutex<Option<String>>>,
}
```

**Functional design** - state passed to functions:
```rust
async fn get_gateway_url(state: &BotState) -> Result<String>
async fn handle_interaction(state: &BotState, interaction: Interaction) -> Result<()>
```

## Logging

Only errors are logged:
```
Discord Bot Service - WarRaft (starting)

[ERROR] src/main.rs:218 - websocket
└── HTTP error: 400 Bad Request
[RETRY] Reconnecting in 30 seconds (attempt #1)
```
```

## systemd Service

Create `/etc/systemd/system/WarRaftDiscord.service`:

```ini
[Unit]
Description=War Raft Discord Bot
After=network.target

[Service]
ExecStart=/var/www/html/warraft/bin/warraft-discord-linux
Restart=always
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
```

Manage service:
```bash
systemctl start WarRaftDiscord    # Start
systemctl stop WarRaftDiscord     # Stop
systemctl restart WarRaftDiscord  # Restart
systemctl status WarRaftDiscord   # Status
journalctl -u WarRaftDiscord -f   # Logs
```

## Dependencies

- `tokio` - async runtime with Mutex
- `tokio-tungstenite` - WebSocket client  
- `reqwest` - HTTP client
- `serde` + `serde_json` - JSON serialization
- `futures-util` - async utilities
- `futures-util` - utilities for working with Futures