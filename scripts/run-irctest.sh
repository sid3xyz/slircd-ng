#!/bin/bash
# Run irctest against slircd-ng
#
# Usage:
#   ./scripts/run-irctest.sh [pytest-args...]
#
# Examples:
#   ./scripts/run-irctest.sh                          # Run all tests
#   ./scripts/run-irctest.sh -k "Ping"                # Run ping tests
#   ./scripts/run-irctest.sh -k "JOIN" -v             # Run JOIN tests verbosely
#   ./scripts/run-irctest.sh --maxfail=5              # Stop after 5 failures
#
# Prerequisites:
#   - slircd-ng must be running on localhost:6667
#   - irctest must be cloned to /tmp/irctest

set -e

IRCTEST_DIR="/tmp/irctest"

# Check if irctest is installed
if [[ ! -d "$IRCTEST_DIR" ]]; then
    echo "irctest not found. Installing..."
    cd /tmp
    git clone --depth 1 https://github.com/ergochat/irctest.git
    cd irctest
    python3 -m venv .venv
    .venv/bin/pip install -r requirements.txt
fi

# Check if server is running
if ! nc -z localhost 6667 2>/dev/null; then
    echo "ERROR: No server running on localhost:6667"
    echo ""
    echo "Start the test server first:"
    echo "  ./scripts/start-test-server.sh"
    exit 1
fi

# Set environment
export IRCTEST_SERVER_HOSTNAME=localhost
export IRCTEST_SERVER_PORT=6667

# Default filter: exclude deprecated, strict, and Ergo-specific tests
DEFAULT_FILTER="-k not deprecated and not strict and not Ergo"

# Run irctest
cd "$IRCTEST_DIR"
echo "Running irctest against localhost:6667..."
echo ""

if [[ $# -eq 0 ]]; then
    # No args - use default filter
    exec .venv/bin/pytest --controller irctest.controllers.external_server \
        $DEFAULT_FILTER -v --tb=short
else
    # User provided args
    exec .venv/bin/pytest --controller irctest.controllers.external_server "$@"
fi
