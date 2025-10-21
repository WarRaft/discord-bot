# Discord Bot in Rust

A simple Discord bot built with Rust using tokio, without specialized Discord libraries. The bot implements the `/ahoy` command with a pirate greeting.

## Features

- Direct connection to Discord Gateway via WebSocket
- Slash command processing
- `/ahoy` command that responds with "Aye aye, Captain! Raft's afloat!"
- Automated build and deployment to remote server

## Project Structure

- `build.sh` - main build and deployment script
- `build-sensitive.sh` - configuration with sensitive data (not in git)
- `build-sensitive.default.sh` - configuration template with comments
- `src/main.rs` - bot source code

## Installation and Setup

### 1. Create Discord Application

1. Go to [Discord Developer Portal](https://discord.com/developers/applications)
2. Click "New Application" and name your bot
3. Navigate to "Bot" section in the left menu
4. Click "Add Bot"
5. Copy the bot token

### 2. Configure Permissions

1. In "OAuth2" → "URL Generator" select:
   - Scopes: `bot`, `applications.commands`
   - Bot Permissions: `Send Messages`, `Use Slash Commands`
2. Use the generated URL to add the bot to your server

### 3. Install Dependencies

Make sure you have Rust installed, then run:

```bash
cd discord-bot
cargo build --release
```

### 4. Local Development Setup

Set the environment variable with your bot token:

```bash
export DISCORD_BOT_TOKEN="your_bot_token_here"
```

### 5. Run Bot Locally

```bash
cargo run
```

Or run the compiled version:

```bash
./target/release/discord-bot
```

## Server Deployment

### 1. Configure Deployment Settings

Copy the configuration template and fill in your credentials:

```bash
cp build-sensitive.default.sh build-sensitive.sh
```

Edit `build-sensitive.sh` and specify:
- `REMOTE_USER` - SSH user
- `REMOTE_HOST` - server IP or domain
- `REMOTE_PORT` - SSH port (usually 22)
- `REMOTE_PATH` - path on server for the binary
- `REMOTE_PASS` - SSH password
- `SERVICE_NAME` - systemd service name
- `DISCORD_TOKEN` - Discord bot token

### 2. Setup systemd Service

Create a systemd service file on the remote server at `/etc/systemd/system/warraft-discord.service`:

```ini
[Unit]
Description=War Raft Discord Bot
After=network.target

[Service]
# Path to the compiled binary (update REMOTE_PATH in build-sensitive.sh accordingly)
ExecStart=/var/www/html/warraft/bin/warraft-discord-linux
Restart=always
Type=simple

# Environment variables
Environment=RUST_LOG=info

# Logging
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
```

Create the service file on the server:

```bash
# On your local machine
cat > warraft-discord.service << 'EOF'
# ... paste the content above ...
EOF

# Copy to server
scp warraft-discord.service root@your-server:/etc/systemd/system/

# On the server, reload systemd and enable the service
ssh root@your-server << 'EOF'
systemctl daemon-reload
systemctl enable warraft-discord
systemctl start warraft-discord
EOF
```

### 3. Systemd Service Management

Manage the service with these commands:

```bash
# Start the service
systemctl start warraft-discord

# Stop the service
systemctl stop warraft-discord

# Restart the service
systemctl restart warraft-discord

# Check service status
systemctl status warraft-discord

# View logs
journalctl -u warraft-discord -f

# Enable auto-start on boot
systemctl enable warraft-discord

# Disable auto-start on boot
systemctl disable warraft-discord
```

### 4. Automated Build and Deploy

Run the build script to automatically compile and deploy:

```bash
./build.sh
```

The script will automatically:
1. Build the Linux binary
2. Stop the service on the server
3. Upload the new binary
4. Start the service again
5. Show the service status

### 5. Nginx Reverse Proxy Setup (Optional)

If you want to expose the bot through nginx, add this configuration to your nginx site config:

```nginx
location /discord/ {
    proxy_pass http://127.0.0.1:3002/;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
    proxy_request_buffering off;
}
```

Apply the configuration:

```bash
# Edit your nginx configuration
sudo nano /etc/nginx/sites-available/your-site

# Add the location block above to your server block

# Test nginx configuration
sudo nginx -t

# Reload nginx
sudo systemctl reload nginx

# Check nginx status
sudo systemctl status nginx
```

## Usage

After starting, the bot will automatically register the `/ahoy` command. Use it in any channel where the bot is present:

```
/ahoy
```

The bot will respond: "Aye aye, Captain! Raft's afloat!"

## Technical Details

The bot is implemented without using specialized Discord libraries and includes:

- Direct connection to Discord Gateway API via WebSocket
- Real-time Discord event processing
- Slash command registration via REST API
- Heartbeat messages to maintain connection
- User interaction handling

## Dependencies

- `tokio` - asynchronous runtime
- `tokio-tungstenite` - WebSocket client
- `reqwest` - HTTP client
- `serde` - JSON serialization
- `futures-util` - utilities for working with Futures