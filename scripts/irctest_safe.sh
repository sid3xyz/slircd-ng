#!/usr/bin/env bash
set -euo pipefail

# Memory-safe irctest runner.
# - Runs a single test module (or a specific test node) at a time.
# - Disables pytest output capture to avoid large in-memory buffers.
# - Runs under a systemd user scope with a hard memory cap to prevent OOM reboots.

IRCTEST_ROOT=${IRCTEST_ROOT:-/home/straylight/slirc-irctest}
SLIRCD_BIN=${SLIRCD_BIN:-/home/straylight/target/release/slircd}
MEM_MAX=${MEM_MAX:-4G}
SWAP_MAX=${SWAP_MAX:-0}
KILL_SLIRCD=${KILL_SLIRCD:-1}

if [[ ! -d "$IRCTEST_ROOT" ]]; then
  echo "IRCTEST_ROOT not found: $IRCTEST_ROOT" >&2
  exit 2
fi

if [[ ! -x "$SLIRCD_BIN" ]]; then
  echo "SLIRCD_BIN not found or not executable: $SLIRCD_BIN" >&2
  echo "Build with: (cd /home/straylight/slircd-ng && cargo build --release)" >&2
  exit 2
fi

# Optional cleanup: kill any leftover slircd from prior runs.
# Default on, because orphaned servers can accumulate and cause OOM.
if [[ "$KILL_SLIRCD" == "1" ]]; then
  while read -r pid; do
    [[ -z "$pid" ]] && continue
    cmdline=$(tr '\0' ' ' < "/proc/$pid/cmdline" 2>/dev/null || true)
    # Only kill the test-launched form: slircd <.../config.toml>
    if [[ "$cmdline" == *" slircd "*" config.toml"* ]]; then
      kill -TERM "$pid" 2>/dev/null || true
    fi
  done < <(pgrep -u "${USER}" -x slircd 2>/dev/null || true)

  # Give them a moment to exit cleanly
  sleep 0.2

  # Hard kill anything still around
  pgrep -u "${USER}" -x slircd >/dev/null 2>&1 && pkill -KILL -u "${USER}" -x slircd || true
fi

TEST_TARGET=${1:-irctest/server_tests/utf8.py}
shift || true

cd "$IRCTEST_ROOT"

PYTEST_BASE_ARGS=(
  --controller=irctest.controllers.slircd
  -x
  --maxfail=1
  --tb=no
  --capture=no
)

# Run in a memory-capped scope.
exec systemd-run --user --scope --quiet \
  -p "MemoryMax=$MEM_MAX" \
  -p "MemorySwapMax=$SWAP_MAX" \
  env SLIRCD_BIN="$SLIRCD_BIN" \
  pytest "${PYTEST_BASE_ARGS[@]}" "$TEST_TARGET" "$@"
