# Discord Bot in Rust

Discord bot with `/ahoy` command built with tokio, without specialized Discord libraries.

## Features

- Direct WebSocket connection to Discord Gateway
- MongoDB state persistence with session resumption
- Slash command support
- Auto-reconnect with progressive backoff
- Custom error handling with stack traces
- Rate limit tracking
- Session event logging

## Quick Start

```bash
# Configure build-sensitive.sh with your credentials
cp build-sensitive.default.sh build-sensitive.sh
nano build-sensitive.sh

# Build for Linux
./build.sh

# Run locally (if built for your platform)
cargo run --release
```

## Deployment

```bash
# Configure build-sensitive.sh with your server details
cp build-sensitive.default.sh build-sensitive.sh
nano build-sensitive.sh

# Build for Linux
./build.sh

# Deploy to production server
./deploy.sh
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
Type=simple
Environment=RUST_LOG=info
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
```

**Important:** Set environment variables in `build-sensitive.sh` - they are compiled into the binary at build time:
- `DISCORD_BOT_TOKEN` - your Discord bot token
- `MONGO_URL` - MongoDB connection string
- `MONGO_DB` - MongoDB database name
- `DISCORD_GUILD_ID` - (optional) for instant command registration

Manage service:
```bash
systemctl daemon-reload             # Reload systemd after editing service file
systemctl enable WarRaftDiscord     # Enable autostart on boot
systemctl start WarRaftDiscord      # Start service
systemctl stop WarRaftDiscord       # Stop service
systemctl restart WarRaftDiscord    # Restart service
systemctl status WarRaftDiscord     # Check status
journalctl -u WarRaftDiscord -f     # Follow logs in real-time
journalctl -u WarRaftDiscord --since "1 hour ago"  # Last hour logs
```

## nginx Configuration (Optional)

If you need to expose metrics or health endpoints:

```nginx
# /etc/nginx/sites-available/warraft-bot
server {
    listen 80;
    server_name bot.warraft.net;

    location /discord/ {
        proxy_pass http://127.0.0.1:3002/;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_request_buffering off;
    }
}
```

Enable site:
```bash
ln -s /etc/nginx/sites-available/warraft-bot /etc/nginx/sites-enabled/
nginx -t
systemctl reload nginx
```

## MongoDB Collections

The bot creates and uses these collections:

- **discord_state** - Session persistence (session_id, sequence)
- **discord_heartbeat** - Heartbeat counter with timestamp
- **discord_session_events** - Event log (identify, resume, ready, resumed, invalid_session)
- **discord_rate_limits** - HTTP API rate limits per endpoint
- **discord_session_limits** - Gateway session start limits