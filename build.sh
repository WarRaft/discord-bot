#!/bin/bash
#
# Build and deploy script for Discord Bot
# 
# Prerequisites:
#   1. Copy build-sensitive.default.sh to build-sensitive.sh
#   2. Fill in your credentials in build-sensitive.sh
#   3. Install jq: brew install jq (macOS) or apt install jq (Linux)
#   4. Install sshpass: brew install hudochenkov/sshpass/sshpass (macOS) or apt install sshpass (Linux)
#   5. Setup systemd service on remote server (see warraft-discord.service)
#
# Usage:
#   ./build.sh
#
set -e

# Load sensitive configuration
if [ ! -f "build-sensitive.sh" ]; then
    echo "❌ File 'build-sensitive.sh' not found!"
    echo "📝 Copy 'build-sensitive.default.sh' to 'build-sensitive.sh' and fill in your credentials."
    exit 1
fi

source build-sensitive.sh

if ! command -v jq &> /dev/null; then
    echo "❌ Требуется 'jq'. Установи: brew install jq"
    exit 1
fi

DIST_DIR="bin"

mkdir -p "$DIST_DIR"

echo "🐧 Building for Linux (x86_64-unknown-linux-musl)..."
rustup target add x86_64-unknown-linux-musl &>/dev/null || true
cargo build --release --target x86_64-unknown-linux-musl
cp "target/x86_64-unknown-linux-musl/release/$PROJECT_NAME" "$DIST_DIR/$BINARY_NAME"

echo ""
echo "✅ Build complete:"
ls -lh "$DIST_DIR"

# Проверка наличия sshpass
if ! command -v sshpass &> /dev/null; then
    echo "❌ Требуется 'sshpass'. Установи: brew install hudochenkov/sshpass/sshpass или sudo apt install sshpass"
    exit 1
fi

TMP_FILE="$BINARY_NAME.tmp"
FINAL_FILE="$BINARY_NAME"

echo "📁 Подготовка директории $REMOTE_PATH на $REMOTE_HOST..."
sshpass -p "$REMOTE_PASS" ssh -p "$REMOTE_PORT" "$REMOTE_USER@$REMOTE_HOST" bash <<EOF
  set -e
  if [ ! -d "$REMOTE_PATH" ]; then
    echo "📁 Директория не найдена, создаю..."
    mkdir -p "$REMOTE_PATH"
    chown $REMOTE_USER:$REMOTE_USER "$REMOTE_PATH"
    chmod 755 "$REMOTE_PATH"
  else
    echo "📁 Директория существует."
  fi
EOF

echo "🛑 Остановка службы $SERVICE_NAME..."
sshpass -p "$REMOTE_PASS" ssh -p "$REMOTE_PORT" "$REMOTE_USER@$REMOTE_HOST" "
  systemctl stop $SERVICE_NAME || true
"

echo "📤 Загрузка бинарника во временный файл ($TMP_FILE)..."
sshpass -p "$REMOTE_PASS" scp -P "$REMOTE_PORT" "$DIST_DIR/$FINAL_FILE" "$REMOTE_USER@$REMOTE_HOST:$REMOTE_PATH/$TMP_FILE"

echo "🔁 Перемещение $TMP_FILE в $FINAL_FILE и установка прав..."
sshpass -p "$REMOTE_PASS" ssh -p "$REMOTE_PORT" "$REMOTE_USER@$REMOTE_HOST" bash <<EOF
  set -e
  mv -f "$REMOTE_PATH/$TMP_FILE" "$REMOTE_PATH/$FINAL_FILE"
  chmod +x "$REMOTE_PATH/$FINAL_FILE"
  echo "✅ Бинарник заменён и готов к запуску."
EOF

echo "🚀 Перезапуск службы $SERVICE_NAME..."
sshpass -p "$REMOTE_PASS" ssh -p "$REMOTE_PORT" "$REMOTE_USER@$REMOTE_HOST" "
  systemctl start $SERVICE_NAME
"

echo "📊 Проверка статуса службы $SERVICE_NAME..."
sshpass -p "$REMOTE_PASS" ssh -p "$REMOTE_PORT" "$REMOTE_USER@$REMOTE_HOST" "
  systemctl status $SERVICE_NAME --no-pager --full
"