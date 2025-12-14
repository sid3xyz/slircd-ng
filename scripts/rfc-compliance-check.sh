#!/usr/bin/env bash
# RFC Compliance Verification Script
# Runs irctest suite and generates compliance report

set -euo pipefail

WORKSPACE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IRCTEST_DIR="${WORKSPACE_ROOT}/slirc-irctest"
SLIRCD_BIN="${WORKSPACE_ROOT}/target/debug/slircd"
REPORT_DIR="${WORKSPACE_ROOT}/compliance-reports"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

usage() {
    cat << EOF
Usage: $0 [options] [test-pattern]

Run RFC compliance tests and generate reports.

Options:
    -r, --release       Use release build
    -t, --timeout SEC   Test timeout (default: 600)
    -o, --output FILE   Report output file
    -q, --quick         Quick test (connection_registration only)
    -h, --help          Show this help

Examples:
    $0                           # Run full suite
    $0 -q                        # Quick smoke test
    $0 connection_registration   # Test specific module
    $0 -r                        # Test release build

EOF
    exit 0
}

# Parse arguments
BUILD_TYPE="debug"
TIMEOUT=600
OUTPUT_FILE=""
QUICK_MODE=0
TEST_PATTERN=""

while [[ $# -gt 0 ]]; do
    case $1 in
        -r|--release)
            BUILD_TYPE="release"
            shift
            ;;
        -t|--timeout)
            TIMEOUT="$2"
            shift 2
            ;;
        -o|--output)
            OUTPUT_FILE="$2"
            shift 2
            ;;
        -q|--quick)
            QUICK_MODE=1
            shift
            ;;
        -h|--help)
            usage
            ;;
        *)
            TEST_PATTERN="$1"
            shift
            ;;
    esac
done

# Build if needed
SLIRCD_BIN="${WORKSPACE_ROOT}/target/${BUILD_TYPE}/slircd"
if [[ ! -f "$SLIRCD_BIN" ]]; then
    echo -e "${YELLOW}Building ${BUILD_TYPE} binary...${NC}"
    cargo build $([ "$BUILD_TYPE" = "release" ] && echo "--release") -p slircd-ng
fi

# Ensure irctest environment
if [[ ! -d "${IRCTEST_DIR}/.venv" ]]; then
    echo -e "${RED}irctest virtualenv not found. Run: cd slirc-irctest && python3 -m venv .venv && .venv/bin/pip install -e .${NC}"
    exit 1
fi

# Create report directory
mkdir -p "$REPORT_DIR"

# Determine test path
if [[ $QUICK_MODE -eq 1 ]]; then
    TEST_PATH="irctest/server_tests/connection_registration.py"
    echo -e "${BLUE}Running quick smoke test (connection_registration only)${NC}"
elif [[ -n "$TEST_PATTERN" ]]; then
    TEST_PATH="irctest/server_tests/${TEST_PATTERN}"
    echo -e "${BLUE}Running tests matching: ${TEST_PATTERN}${NC}"
else
    TEST_PATH="irctest/server_tests/"
    echo -e "${BLUE}Running full RFC compliance suite${NC}"
fi

# Set output file
if [[ -z "$OUTPUT_FILE" ]]; then
    TIMESTAMP=$(date +%Y%m%d-%H%M%S)
    OUTPUT_FILE="${REPORT_DIR}/compliance-${TIMESTAMP}.txt"
fi

JSON_REPORT="${REPORT_DIR}/compliance-latest.json"

echo -e "${BLUE}Binary:      ${SLIRCD_BIN}${NC}"
echo -e "${BLUE}Test path:   ${TEST_PATH}${NC}"
echo -e "${BLUE}Timeout:     ${TIMEOUT}s${NC}"
echo -e "${BLUE}Report:      ${OUTPUT_FILE}${NC}"
echo ""

# Run tests
cd "$IRCTEST_DIR"

echo -e "${YELLOW}Starting test run...${NC}"
set +e
SLIRCD_BIN="$SLIRCD_BIN" timeout "$TIMEOUT" \
    .venv/bin/pytest \
    --controller irctest.controllers.slircd \
    "$TEST_PATH" \
    -v \
    --tb=short \
    --json-report \
    --json-report-file="$JSON_REPORT" \
    2>&1 | tee "$OUTPUT_FILE"

TEST_EXIT_CODE=$?
set -e

# Parse results
echo ""
echo -e "${BLUE}================================================${NC}"
echo -e "${BLUE}         RFC COMPLIANCE TEST SUMMARY${NC}"
echo -e "${BLUE}================================================${NC}"

if [[ -f "$JSON_REPORT" ]]; then
    TOTAL=$(jq -r '.summary.total // 0' "$JSON_REPORT")
    PASSED=$(jq -r '.summary.passed // 0' "$JSON_REPORT")
    FAILED=$(jq -r '.summary.failed // 0' "$JSON_REPORT")
    SKIPPED=$(jq -r '.summary.skipped // 0' "$JSON_REPORT")
    
    PASS_RATE=0
    if [[ $TOTAL -gt 0 ]]; then
        PASS_RATE=$(awk "BEGIN {printf \"%.1f\", ($PASSED / $TOTAL) * 100}")
    fi
    
    echo -e "Total tests:    $TOTAL"
    echo -e "${GREEN}Passed:         $PASSED${NC}"
    echo -e "${RED}Failed:         $FAILED${NC}"
    echo -e "${YELLOW}Skipped:        $SKIPPED${NC}"
    echo -e ""
    echo -e "Pass rate:      ${PASS_RATE}%"
    
    if [[ $FAILED -gt 0 ]]; then
        echo -e ""
        echo -e "${RED}Failed tests:${NC}"
        jq -r '.tests[] | select(.outcome == "failed") | "  - \(.nodeid)"' "$JSON_REPORT" | head -20
        
        FAILED_COUNT=$(jq -r '[.tests[] | select(.outcome == "failed")] | length' "$JSON_REPORT")
        if [[ $FAILED_COUNT -gt 20 ]]; then
            echo -e "${YELLOW}  ... and $(($FAILED_COUNT - 20)) more${NC}"
        fi
    fi
else
    echo -e "${RED}JSON report not generated${NC}"
fi

echo -e ""
echo -e "${BLUE}Full report: ${OUTPUT_FILE}${NC}"
echo -e "${BLUE}JSON report: ${JSON_REPORT}${NC}"

# Exit with test result
if [[ $TEST_EXIT_CODE -eq 124 ]]; then
    echo -e ""
    echo -e "${RED}Tests timed out after ${TIMEOUT}s${NC}"
    exit 124
elif [[ $FAILED -gt 0 ]]; then
    exit 1
else
    echo -e ""
    echo -e "${GREEN}All tests passed!${NC}"
    exit 0
fi
