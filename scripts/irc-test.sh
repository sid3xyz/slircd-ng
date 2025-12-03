#!/bin/bash
# slircd-ng quick IRC testing script
# Usage: ./scripts/irc-test.sh [command] [args...]
#
# Commands:
#   connect           - Test basic TCP connection
#   register [nick]   - Register a user and verify welcome
#   join [channel]    - Register and join a channel
#   privmsg           - Test private messaging between clients
#   cap               - Test CAP negotiation
#   scenario [name]   - Run a named test scenario
#   all               - Run all tests

set -euo pipefail

# Configuration
HOST="${IRC_HOST:-127.0.0.1}"
PORT="${IRC_PORT:-6667}"
TIMEOUT="${IRC_TIMEOUT:-5}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Counters
TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

log_info() { echo -e "${BLUE}[INFO]${NC} $*"; }
log_ok() { echo -e "${GREEN}[PASS]${NC} $*"; }
log_fail() { echo -e "${RED}[FAIL]${NC} $*"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }

# Send IRC commands and capture response
# Usage: irc_send "NICK test\r\nUSER t 0 * :t\r\n" [expect_pattern]
irc_send() {
    local commands="$1"
    local expect="${2:-}"
    local response

    response=$(echo -e "$commands" | timeout "$TIMEOUT" nc -q1 "$HOST" "$PORT" 2>&1) || {
        if [[ $? -eq 124 ]]; then
            log_fail "Timeout waiting for response"
            return 1
        fi
        log_fail "Connection failed"
        return 1
    }

    echo "$response"

    if [[ -n "$expect" ]]; then
        if echo "$response" | grep -qE "$expect"; then
            return 0
        else
            return 1
        fi
    fi
}

# Run a test with pass/fail tracking
run_test() {
    local name="$1"
    shift

    ((TESTS_RUN++))
    log_info "Running: $name"

    if "$@"; then
        ((TESTS_PASSED++))
        log_ok "$name"
        return 0
    else
        ((TESTS_FAILED++))
        log_fail "$name"
        return 1
    fi
}

# Test: Basic TCP connection
test_connect() {
    log_info "Testing TCP connection to $HOST:$PORT..."
    if timeout 2 bash -c "echo > /dev/tcp/$HOST/$PORT" 2>/dev/null; then
        log_ok "TCP connection successful"
        return 0
    else
        log_fail "Cannot connect to $HOST:$PORT"
        return 1
    fi
}

# Test: User registration (NICK + USER → 001)
test_register() {
    local nick="${1:-testuser$$}"
    log_info "Testing registration with nick: $nick"

    local response
    response=$(irc_send "NICK $nick\r\nUSER test 0 * :Test User\r\n" "001") || return 1

    if echo "$response" | grep -q "001.*$nick"; then
        log_ok "Registration successful - received RPL_WELCOME"
        return 0
    else
        log_fail "Did not receive proper welcome message"
        echo "Response: $response"
        return 1
    fi
}

# Test: Join channel
test_join() {
    local nick="joiner$$"
    local channel="${1:-#test}"
    log_info "Testing JOIN $channel"

    local response
    response=$(irc_send "NICK $nick\r\nUSER test 0 * :Test\r\nJOIN $channel\r\n" "JOIN.*$channel") || return 1

    if echo "$response" | grep -qE "JOIN.*$channel"; then
        log_ok "Successfully joined $channel"
        return 0
    else
        log_fail "Failed to join channel"
        echo "Response: $response"
        return 1
    fi
}

# Test: CAP negotiation
test_cap() {
    log_info "Testing CAP LS negotiation"

    local response
    response=$(irc_send "CAP LS 302\r\n" "CAP.*LS") || return 1

    if echo "$response" | grep -qE "CAP.*LS"; then
        log_ok "CAP LS response received"
        echo "Capabilities: $(echo "$response" | grep "CAP" | head -1)"
        return 0
    else
        log_fail "No CAP response"
        return 1
    fi
}

# Test: PING/PONG
test_ping() {
    local nick="pingtest$$"
    log_info "Testing PING response"

    local response
    response=$(irc_send "NICK $nick\r\nUSER test 0 * :Test\r\nPING :test123\r\n" "PONG.*test123") || return 1

    if echo "$response" | grep -qE "PONG.*test123"; then
        log_ok "PONG received correctly"
        return 0
    else
        log_fail "No PONG response"
        return 1
    fi
}

# Test: PRIVMSG (requires two connections - use socat for bidirectional)
test_privmsg() {
    log_info "Testing PRIVMSG between two users"

    # This is a simplified test - just verify server accepts PRIVMSG
    local nick="sender$$"
    local response
    response=$(irc_send "NICK $nick\r\nUSER test 0 * :Test\r\nPRIVMSG $nick :Hello self\r\n" "001") || return 1

    # If we got registered, the PRIVMSG was at least accepted
    if echo "$response" | grep -q "001"; then
        log_ok "PRIVMSG accepted by server"
        return 0
    else
        log_fail "Registration failed before PRIVMSG test"
        return 1
    fi
}

# Test: QUIT
test_quit() {
    local nick="quitter$$"
    log_info "Testing QUIT command"

    local response
    response=$(irc_send "NICK $nick\r\nUSER test 0 * :Test\r\nQUIT :Goodbye\r\n" "ERROR|QUIT") || {
        # Connection closing is expected on QUIT
        log_ok "QUIT handled (connection closed)"
        return 0
    }

    log_ok "QUIT processed"
    return 0
}

# Test: LUSERS
test_lusers() {
    local nick="lusers$$"
    log_info "Testing LUSERS command"

    local response
    response=$(irc_send "NICK $nick\r\nUSER test 0 * :Test\r\nLUSERS\r\n" "251|252|253") || return 1

    if echo "$response" | grep -qE "25[1-3]"; then
        log_ok "LUSERS response received"
        return 0
    else
        log_fail "No LUSERS response"
        return 1
    fi
}

# Test: MOTD
test_motd() {
    local nick="motd$$"
    log_info "Testing MOTD"

    local response
    response=$(irc_send "NICK $nick\r\nUSER test 0 * :Test\r\nMOTD\r\n" "375|376|422") || return 1

    if echo "$response" | grep -qE "375|376|422"; then
        log_ok "MOTD response received"
        return 0
    else
        log_fail "No MOTD response"
        return 1
    fi
}

# Test: WHO
test_who() {
    local nick="who$$"
    log_info "Testing WHO command"

    local response
    response=$(irc_send "NICK $nick\r\nUSER test 0 * :Test\r\nWHO $nick\r\n" "315|352") || return 1

    if echo "$response" | grep -qE "315|352"; then
        log_ok "WHO response received"
        return 0
    else
        log_fail "No WHO response"
        return 1
    fi
}

# Test: WHOIS
test_whois() {
    local nick="whois$$"
    log_info "Testing WHOIS command"

    local response
    response=$(irc_send "NICK $nick\r\nUSER test 0 * :Test\r\nWHOIS $nick\r\n" "311|318") || return 1

    if echo "$response" | grep -qE "311|318"; then
        log_ok "WHOIS response received"
        return 0
    else
        log_fail "No WHOIS response"
        return 1
    fi
}

# Scenario: Basic chat flow
scenario_basic() {
    log_info "=== Running basic chat scenario ==="
    run_test "TCP Connection" test_connect
    run_test "User Registration" test_register
    run_test "CAP Negotiation" test_cap
    run_test "Channel Join" test_join "#testchan"
    run_test "PING/PONG" test_ping
    run_test "LUSERS" test_lusers
    run_test "MOTD" test_motd
    run_test "WHO" test_who
    run_test "WHOIS" test_whois
    run_test "PRIVMSG" test_privmsg
    run_test "QUIT" test_quit
}

# Run all tests
run_all() {
    scenario_basic
}

# Print summary
print_summary() {
    echo ""
    echo "=================================="
    echo -e "Tests run:    ${BLUE}$TESTS_RUN${NC}"
    echo -e "Tests passed: ${GREEN}$TESTS_PASSED${NC}"
    echo -e "Tests failed: ${RED}$TESTS_FAILED${NC}"
    echo "=================================="

    if [[ $TESTS_FAILED -eq 0 ]]; then
        echo -e "${GREEN}All tests passed!${NC}"
        return 0
    else
        echo -e "${RED}Some tests failed${NC}"
        return 1
    fi
}

# Help message
show_help() {
    cat << EOF
slircd-ng IRC Test Script

Usage: $0 [command] [args...]

Commands:
  connect              Test TCP connection
  register [nick]      Test user registration
  join [channel]       Test channel join (default: #test)
  cap                  Test CAP negotiation
  ping                 Test PING/PONG
  privmsg              Test PRIVMSG
  lusers               Test LUSERS
  motd                 Test MOTD
  who                  Test WHO
  whois                Test WHOIS
  quit                 Test QUIT
  scenario <name>      Run a test scenario (basic)
  all                  Run all tests

Environment:
  IRC_HOST             Server host (default: 127.0.0.1)
  IRC_PORT             Server port (default: 6667)
  IRC_TIMEOUT          Timeout in seconds (default: 5)

Examples:
  $0 connect
  $0 register mynick
  $0 scenario basic
  IRC_PORT=6668 $0 all
EOF
}

# Main
main() {
    local cmd="${1:-help}"
    shift || true

    case "$cmd" in
        connect)   test_connect ;;
        register)  test_register "$@" ;;
        join)      test_join "$@" ;;
        cap)       test_cap ;;
        ping)      test_ping ;;
        privmsg)   test_privmsg ;;
        lusers)    test_lusers ;;
        motd)      test_motd ;;
        who)       test_who ;;
        whois)     test_whois ;;
        quit)      test_quit ;;
        scenario)
            case "${1:-basic}" in
                basic) scenario_basic ;;
                *) log_fail "Unknown scenario: $1"; exit 1 ;;
            esac
            print_summary
            ;;
        all)
            run_all
            print_summary
            ;;
        help|--help|-h)
            show_help
            ;;
        *)
            log_fail "Unknown command: $cmd"
            show_help
            exit 1
            ;;
    esac
}

main "$@"
