#!/bin/bash
#
# Deploy script for Discord Bot
# 
# Prerequisites:
#   1. Install sshpass: brew install hudochenkov/sshpass/sshpass (macOS) or apt install sshpass (Linux)
#   2. Setup systemd service on remote server
#
# Usage:
#   ./deploy.sh
#
set -e

# Run build.sh and check exit code
echo "🔨 Building..."
if ! ./build.sh; then
    echo "❌ Build failed!"
    exit 1
fi

echo ""
echo "📦 Deploying..."
echo ""

# Load sensitive configuration
if [ ! -f "build-sensitive.sh" ]; then
    echo "❌ File 'build-sensitive.sh' not found!"
    exit 1
fi

source build-sensitive.sh

# Check for sshpass
if ! command -v sshpass &> /dev/null; then
    echo "❌ 'sshpass' is required. Install: brew install hudochenkov/sshpass/sshpass (macOS) or sudo apt install sshpass (Linux)"
    exit 1
fi

DIST_DIR="bin"

echo "📁 Preparing directory $REMOTE_PATH on $REMOTE_HOST..."
sshpass -p "$REMOTE_PASS" ssh -p "$REMOTE_PORT" "$REMOTE_USER@$REMOTE_HOST" bash <<EOF
  set -e
  if [ ! -d "$REMOTE_PATH" ]; then
    echo "📁 Directory not found, creating..."
    mkdir -p "$REMOTE_PATH"
    chown $REMOTE_USER:$REMOTE_USER "$REMOTE_PATH"
    chmod 755 "$REMOTE_PATH"
  else
    echo "📁 Directory exists."
  fi
EOF

echo "🛑 Stopping service $SERVICE_NAME..."
sshpass -p "$REMOTE_PASS" ssh -p "$REMOTE_PORT" "$REMOTE_USER@$REMOTE_HOST" "
  systemctl stop $SERVICE_NAME || true
"

echo "📤 Uploading binary to temporary file ($BINARY_NAME.tmp)..."
sshpass -p "$REMOTE_PASS" scp -P "$REMOTE_PORT" "$DIST_DIR/$BINARY_NAME" "$REMOTE_USER@$REMOTE_HOST:$REMOTE_PATH/$BINARY_NAME.tmp"

echo "🔁 Moving $BINARY_NAME.tmp to $BINARY_NAME and setting permissions..."
sshpass -p "$REMOTE_PASS" ssh -p "$REMOTE_PORT" "$REMOTE_USER@$REMOTE_HOST" bash <<EOF
  set -e
  mv -f "$REMOTE_PATH/$BINARY_NAME.tmp" "$REMOTE_PATH/$BINARY_NAME"
  chmod +x "$REMOTE_PATH/$BINARY_NAME"
  echo "✅ Binary replaced and ready to run."
EOF

echo "🚀 Restarting service $SERVICE_NAME..."
sshpass -p "$REMOTE_PASS" ssh -p "$REMOTE_PORT" "$REMOTE_USER@$REMOTE_HOST" "
  systemctl start $SERVICE_NAME
"

echo "📊 Checking service status $SERVICE_NAME..."
sshpass -p "$REMOTE_PASS" ssh -p "$REMOTE_PORT" "$REMOTE_USER@$REMOTE_HOST" "
  systemctl status $SERVICE_NAME --no-pager --full
"
