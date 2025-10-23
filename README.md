# Discord Bot in Rust

**@Raft** - Discord bot with BLP image conversion and modular slash commands built with tokio, without specialized Discord libraries.

## Features

- Direct WebSocket connection to Discord Gateway
- MongoDB state persistence with session resumption
- **BLP Image Conversion** - Bidirectional conversion between PNG and BLP formats
- **Background Removal** - AI-powered background removal using U2-Net model
- Persistent queue system with event-driven workers
- Modular slash command system (see `src/commands/`)
- Auto-reconnect with progressive backoff
- Custom error handling with stack traces
- Token bucket rate limiting (40 req/sec default)
- Session event logging
- Signal-based command reregistration (SIGUSR1)

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
- **discord_session_limits** - Session start limits tracking
- **discord_command_blp** - BLP conversion queue (pending, processing, completed, failed)
- **discord_command_png** - PNG conversion queue (pending, processing, completed, failed)
- **discord_command_rembg** - Background removal queue (pending, processing, completed, failed)

## Commands

See [src/commands/README.md](src/commands/README.md) for details on adding new commands.

### Download AI Models

Download ONNX Runtime library and models for background removal:

```bash
./signal-download-models.sh
```

This sends SIGUSR2 signal to the bot, triggering:
1. **ONNX Runtime installation** (if not already installed):
   - Downloads libonnxruntime.so 1.16.0 (~16 MB)
   - Installs to `/usr/local/lib/`
   - Updates library cache

2. **Model downloads**:
   - **u2net.onnx** (~176 MB) - Universal background removal model
   - **u2net_human_seg.onnx** (~176 MB) - Optimized for portraits
   - **silueta.onnx** (~43 MB) - Fast and lightweight model

Models are saved to `models/` directory next to the bot binary.

Monitor download progress:
```bash
journalctl -u WarRaftDiscord -f
```

### Background Removal

Remove backgrounds from images using AI:

**Mention command:**
```
@Raft rembg              # Default: threshold 60, soft edges
@Raft rembg 80           # Custom threshold (1-100)
@Raft rembg binary       # Hard edges (binary mode)
@Raft rembg mask         # Include alpha mask as separate file
@Raft rembg 70 binary mask zip   # All options combined
```

**Slash command:**
```
/rembg
```

**Parameters:**
- `threshold` (1-100) - Detection sensitivity (default: 60)
  - Lower = more aggressive removal
  - Higher = preserve more details
- `binary` - Hard edges mode (default: soft/feathered edges)
- `mask` - Include alpha mask as separate PNG file
- `zip` - Force ZIP archive output (auto for multiple files)

**Features:**
- Supports PNG, JPEG, WebP, BMP, GIF input
- Output always in PNG format (with transparency)
- Three concurrent workers for parallel processing
- Persistent queue survives service restarts
- In-memory processing (no temporary files)

### BLP Image Conversion

Convert images between PNG and Warcraft III BLP formats by mentioning the bot with attached images:

**PNG → BLP:**
```
@Raft blp 80           # Convert to BLP with quality 80
@Raft blp              # Convert to BLP with default quality (80)
@Raft blp 95           # Convert to BLP with quality 95
```

**BLP → PNG:**
```
@Raft png              # Convert BLP to PNG
```

**Features:**
- Supports multiple files in one message
- Quality range for BLP: 1-100
- Preserves original filenames (changes extension)
- Persistent queue survives service restarts
- Event-driven workers with automatic rate limiting
- In-memory processing (no temporary files)

### Reregister Commands

Trigger command reregistration without restarting the service:

```bash
./signal-reregister-commands.sh
```

This sends SIGUSR1 signal to the bot, causing immediate command reregistration.

### Download Models

Trigger model download without restarting the service:

```bash
./signal-download-models.sh
```

This sends SIGUSR2 signal to the bot to download AI models.
