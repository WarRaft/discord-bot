#!/bin/bash
#
# Trigger model download via SIGUSR2 signal on remote server
#
# Usage:
#   ./download-models.sh
#

set -e

# Load SSH credentials
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SENSITIVE_FILE="$SCRIPT_DIR/build-sensitive.sh"

if [ ! -f "$SENSITIVE_FILE" ]; then
    echo "❌ File $SENSITIVE_FILE not found"
    echo "💡 Copy build-sensitive.default.sh to build-sensitive.sh and configure it"
    exit 1
fi

source "$SENSITIVE_FILE"

SERVICE_NAME="WarRaftDiscord"

echo "📦 Triggering model download on $REMOTE_HOST..."

# Check if sshpass is available
if ! command -v sshpass &> /dev/null; then
    echo "❌ sshpass not found. Install it:"
    echo "   brew install hudochenkov/sshpass/sshpass"
    exit 1
fi

# Execute command on remote server
sshpass -p "$REMOTE_PASS" ssh -o StrictHostKeyChecking=no "$REMOTE_USER@$REMOTE_HOST" << 'ENDSSH'
set -e

SERVICE_NAME="WarRaftDiscord"

# Get PID of the service
PID=$(systemctl show -p MainPID --value $SERVICE_NAME 2>/dev/null || echo "0")

if [ -z "$PID" ] || [ "$PID" = "0" ]; then
    echo "❌ Service $SERVICE_NAME is not running or PID not found"
    exit 1
fi

echo "📡 Sending SIGUSR2 to PID $PID..."
kill -SIGUSR2 $PID

echo "✅ Signal sent successfully"
echo "📋 Check logs: journalctl -u $SERVICE_NAME -f"
ENDSSH

echo ""
echo "✅ Model download triggered successfully"
echo "📋 Monitor progress with: journalctl -u $SERVICE_NAME -f"
