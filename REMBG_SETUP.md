# Background Removal Setup

## Prerequisites

### Download ONNX Runtime and Models

After deploying the bot, trigger download via signal:

```bash
./signal-download-models.sh
```

This will:
1. Check if ONNX Runtime is installed, install if missing (~16 MB)
2. Download models to `models/` directory:
   - `u2net.onnx` (~176 MB) - Universal model (default)
   - `u2net_human_seg.onnx` (~176 MB) - For portraits
   - `silueta.onnx` (~43 MB) - Fast lightweight model

Monitor progress:
```bash
journalctl -u WarRaftDiscord -f
```

## Usage

### Basic Usage

Mention the bot with attached images:

```
@Raft rembg
```

### Advanced Options

```
@Raft rembg 80              # Custom threshold (1-100, default: 60)
@Raft rembg binary          # Hard edges instead of soft/feathered
@Raft rembg mask            # Include alpha mask as separate PNG
@Raft rembg 70 binary mask  # Combine options
@Raft rembg zip             # Force ZIP output
```

### Parameters

- **threshold** (1-100): Detection sensitivity
  - Lower = more aggressive background removal
  - Higher = preserve more edge details
  - Default: 60
  
- **binary**: Output mode
  - Without: Soft/feathered edges (default)
  - With: Hard binary edges (fully transparent or opaque)
  
- **mask**: Include mask output
  - Adds separate PNG file with alpha channel mask
  - Useful for manual editing
  
- **zip**: Archive output
  - Force ZIP archive
  - Auto-enabled for multiple files

### Slash Command

View statistics and documentation:

```
/rembg
```

## Technical Details

- **Model**: U2-Net (Universal background removal)
- **Input formats**: PNG, JPEG, WebP, BMP, GIF
- **Output format**: PNG with transparency
- **Workers**: 3 concurrent processors
- **Queue**: Persistent (survives restarts)
- **Processing**: In-memory (no temporary files)

## File Structure

```
.
├── bin/
│   ├── warraft-discord-linux    # Bot binary (27 MB)
│   └── libonnxruntime.so        # ONNX Runtime library (16 MB)
├── models/
│   ├── u2net.onnx              # Default model (~176 MB)
│   ├── u2net_human_seg.onnx    # Portrait model (~176 MB)
│   └── silueta.onnx            # Fast model (~43 MB)
└── install-onnxruntime.sh      # Library installer
```

## Troubleshooting

### Library not found

```bash
# Check if ONNX Runtime is installed
ldconfig -p | grep onnxruntime

# If not found, run signal script to install
./signal-download-models.sh
```

### Models not found

Bot will show error if models are missing. Download via:

```bash
./signal-download-models.sh
```

Or manually:
```bash
# Install ONNX Runtime
cd /tmp
wget https://github.com/microsoft/onnxruntime/releases/download/v1.16.0/onnxruntime-linux-x64-1.16.0.tgz
tar -xzf onnxruntime-linux-x64-1.16.0.tgz
sudo cp onnxruntime-linux-x64-1.16.0/lib/libonnxruntime.so* /usr/local/lib/
sudo ldconfig
rm -rf onnxruntime-linux-x64-1.16.0*

# Download model
cd models
curl -L -O https://github.com/danielgatis/rembg/releases/download/v0.0.0/u2net.onnx
```

### Permission denied

Ensure bot has write access to `models/` directory:

```bash
chown -R www-data:www-data /var/www/html/warraft/
```

## Systemd integration (required for ONNX Runtime)

If you run the bot as a systemd service, add this line to your service file (e.g. `/etc/systemd/system/WarRaftDiscord.service`) under the `[Service]` section:

```
Environment=LD_LIBRARY_PATH=/usr/local/lib
```

This is required for the bot to find `libonnxruntime.so` when running as a service.
After editing, reload systemd and restart the bot:

```
sudo systemctl daemon-reload
sudo systemctl restart WarRaftDiscord
```
