#!/bin/bash
# Run E2E tests for slircd-ng
#
# Usage:
#   ./scripts/run-e2e-tests.sh           # Run with auto-started server
#   ./scripts/run-e2e-tests.sh --manual  # Test against running server on 6667
#   ./scripts/run-e2e-tests.sh -k join   # Run only tests matching 'join'
#
# Environment:
#   E2E_HOST    Server host (default: 127.0.0.1)
#   E2E_PORT    Server port (default: 16667 for auto, 6667 for manual)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
E2E_DIR="$PROJECT_DIR/tests/e2e"
VENV_DIR="$E2E_DIR/.venv"

cd "$PROJECT_DIR"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info() { echo -e "${BLUE}[INFO]${NC} $*"; }
log_ok() { echo -e "${GREEN}[OK]${NC} $*"; }
log_fail() { echo -e "${RED}[FAIL]${NC} $*"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }

# Check dependencies
check_deps() {
    if [[ ! -d "$VENV_DIR" ]]; then
        log_warn "Virtual environment not found, creating..."
        python3 -m venv "$VENV_DIR"
        source "$VENV_DIR/bin/activate"
        pip install -r "$E2E_DIR/requirements.txt"
    else
        source "$VENV_DIR/bin/activate"
    fi
}

# Install test dependencies
install_deps() {
    log_info "Creating virtual environment and installing dependencies..."
    python3 -m venv "$VENV_DIR"
    source "$VENV_DIR/bin/activate"
    pip install -r "$E2E_DIR/requirements.txt"
    log_ok "Dependencies installed in $VENV_DIR"
}

# Run tests
run_tests() {
    local pytest_args=("$@")

    log_info "Running E2E tests..."

    cd "$E2E_DIR"
    python3 -m pytest "${pytest_args[@]}" -v
}

# Main
main() {
    local manual=false
    local pytest_args=()

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --manual|-m)
                manual=true
                shift
                ;;
            --install)
                install_deps
                exit 0
                ;;
            --help|-h)
                cat << EOF
Usage: $0 [options] [pytest-args...]

Options:
  --manual, -m    Test against a running server (default port 6667)
  --install       Install test dependencies
  --help, -h      Show this help

Environment:
  E2E_HOST        Server host (default: 127.0.0.1)
  E2E_PORT        Server port (default: 16667 for auto, 6667 for manual)

Examples:
  $0                      # Run all tests with auto-started server
  $0 --manual             # Test against running server on :6667
  $0 -k channel           # Run only channel tests
  $0 --manual -k privmsg  # Test PRIVMSG against running server
EOF
                exit 0
                ;;
            *)
                pytest_args+=("$1")
                shift
                ;;
        esac
    done

    check_deps

    if $manual; then
        export E2E_PORT="${E2E_PORT:-6667}"
        export E2E_MANUAL=1
        log_info "Testing against running server on ${E2E_HOST:-127.0.0.1}:$E2E_PORT"
    fi

    run_tests "${pytest_args[@]}"
}

main "$@"
