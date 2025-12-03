#!/bin/bash
# Start slircd-ng test server with fresh build
#
# Usage:
#   ./scripts/start-test-server.sh [config]
#
# Examples:
#   ./scripts/start-test-server.sh                    # Uses config.test.toml
#   ./scripts/start-test-server.sh tests/e2e/test_config.toml
#
# This script:
#   1. Kills any existing slircd instances
#   2. Rebuilds the server
#   3. Starts fresh with the specified config

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
WORKSPACE_DIR="$(dirname "$PROJECT_DIR")"

CONFIG="${1:-config.test.toml}"

# Resolve config path
if [[ ! "$CONFIG" = /* ]]; then
    CONFIG="$PROJECT_DIR/$CONFIG"
fi

echo "=== slircd-ng Test Server ==="
echo "Project: $PROJECT_DIR"
echo "Config:  $CONFIG"
echo ""

# Kill any existing instances
echo "[1/3] Stopping existing instances..."
pkill -f "target/debug/slircd" 2>/dev/null || true
pkill -f "target/release/slircd" 2>/dev/null || true
sleep 1

# Verify port is free
if lsof -i :6667 >/dev/null 2>&1; then
    echo "ERROR: Port 6667 still in use after killing slircd"
    lsof -i :6667
    exit 1
fi

# Rebuild
echo "[2/3] Building slircd-ng..."
cd "$WORKSPACE_DIR"
cargo build -p slircd-ng 2>&1 | tail -3

# Start server
echo "[3/3] Starting server..."
echo ""
exec "$WORKSPACE_DIR/target/debug/slircd" "$CONFIG"
