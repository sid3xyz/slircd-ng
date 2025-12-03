#!/bin/bash
set -e

# Configuration
WORKSPACE_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SLIRCD_DIR="$WORKSPACE_ROOT/slircd-ng"
IRCTEST_DIR="$WORKSPACE_ROOT/irctest"
CONFIG_FILE="$SLIRCD_DIR/config.test.toml"
SERVER_BIN="$WORKSPACE_ROOT/target/release/slircd"
PORT=6667

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m' # No Color

echo -e "${GREEN}Starting irctest integration run...${NC}"

# 1. Build slircd-ng
echo -e "${GREEN}Building slircd-ng (release)...${NC}"
cd "$SLIRCD_DIR"
cargo build --release

# 2. Setup irctest environment
echo -e "${GREEN}Setting up irctest environment...${NC}"
if [ ! -d "$IRCTEST_DIR" ]; then
    echo -e "${RED}Error: irctest directory not found at $IRCTEST_DIR${NC}"
    echo "Please clone it: git clone https://github.com/ergochat/irctest.git $IRCTEST_DIR"
    exit 1
fi

cd "$IRCTEST_DIR"
if [ ! -d ".venv" ]; then
    python3 -m venv .venv
    .venv/bin/pip install -r requirements.txt
fi

# 3. Start slircd-ng
echo -e "${GREEN}Starting slircd-ng...${NC}"
# Ensure no previous instance is running
# Use exact match or path to avoid killing this script
pkill -f "target/release/slircd-ng" || true
sleep 1

# Start server in background
"$SERVER_BIN" "$CONFIG_FILE" > "$SLIRCD_DIR/slircd.log" 2>&1 &
SERVER_PID=$!
echo "Server PID: $SERVER_PID"

# Wait for server to be ready
echo "Waiting for server to listen on port $PORT..."
for i in {1..30}; do
    if nc -z localhost $PORT; then
        echo -e "${GREEN}Server is up!${NC}"
        break
    fi
    sleep 0.5
done

if ! nc -z localhost $PORT; then
    echo -e "${RED}Server failed to start. Check logs:${NC}"
    cat "$SLIRCD_DIR/slircd.log"
    kill $SERVER_PID || true
    exit 1
fi

# 4. Run irctest
echo -e "${GREEN}Running irctest...${NC}"
export IRCTEST_SERVER_HOSTNAME=localhost
export IRCTEST_SERVER_PORT=$PORT

# Run a subset of tests first to verify integration
# Using timeout to prevent hanging
timeout 300 .venv/bin/pytest --controller irctest.controllers.external_server \
    irctest/server_tests/ \
    -v \
    --tb=short \
    || TEST_EXIT_CODE=$?

# 5. Cleanup
echo -e "${GREEN}Cleaning up...${NC}"
kill $SERVER_PID || true
wait $SERVER_PID 2>/dev/null || true

if [ -z "$TEST_EXIT_CODE" ]; then
    echo -e "${GREEN}Tests passed!${NC}"
    exit 0
else
    echo -e "${RED}Tests failed with exit code $TEST_EXIT_CODE${NC}"
    exit $TEST_EXIT_CODE
fi
