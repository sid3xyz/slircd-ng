#!/bin/bash
# Run all fuzz targets for a short duration to test basic functionality

set -e

DURATION=${1:-30}  # Default to 30 seconds per target
WORKERS=${2:-1}    # Default to 1 worker

echo "Running fuzz tests for ${DURATION} seconds each with ${WORKERS} worker(s)..."

# Array of fuzz targets
TARGETS=("message_parser" "ctcp_parser" "prefix_parser" "mode_parser")

for target in "${TARGETS[@]}"; do
    echo "Fuzzing ${target}..."
    cargo fuzz run "$target" -- -max_total_time="$DURATION" -workers="$WORKERS"
    echo "Completed ${target}"
    echo
done

echo "All fuzz targets completed!"
echo
echo "To check for any artifacts (crashes), run:"
echo "  ls -la fuzz/artifacts/"
echo
echo "To reproduce a crash, run:"
echo "  cargo fuzz run <target> <crash-file>"