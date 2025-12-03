#!/usr/bin/env bash
set -euo pipefail

# slircd-ng test server control script
# - Safe start/stop/restart/status/verify/tail
# - Only manages the slircd built from this workspace
#
# Defaults (override via env):
#   PORT=6667
#   PIDFILE=/tmp/slircd-test.pid
#   LOGFILE=/tmp/slircd-test.log

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_DIR=$(cd "${SCRIPT_DIR}/.." && pwd)
WORKSPACE_DIR=$(cd "${REPO_DIR}/.." && pwd)
BIN="${WORKSPACE_DIR}/target/debug/slircd"
CONFIG="${CONFIG:-${REPO_DIR}/config.test.toml}"
PORT="${PORT:-6667}"
PIDFILE="${PIDFILE:-/tmp/slircd-test.pid}"
LOGFILE="${LOGFILE:-/tmp/slircd-test.log}"

cmd_exists() { command -v "$1" >/dev/null 2>&1; }

listening_pid() {
  # Return PID listening on :$PORT (tcp), or empty
  if cmd_exists ss; then
    ss -lntp 2>/dev/null | awk -v p=":${PORT} " '$0 ~ p {print $NF}' | sed 's/.*pid=\([0-9]*\).*/\1/' | head -n1
  else
    # Fallback: lsof
    lsof -iTCP:"${PORT}" -sTCP:LISTEN -t 2>/dev/null | head -n1 || true
  fi
}

is_our_binary() {
  local pid="$1"
  [[ -n "$pid" ]] || return 1
  # Compare the real path of the pid's exe to our BIN
  if [[ -e "/proc/${pid}/exe" ]]; then
    local exe
    exe=$(readlink -f "/proc/${pid}/exe" || true)
    [[ "${exe}" == "${BIN}" ]]
  else
    return 1
  fi
}

kill_pid() {
  local pid="$1"
  [[ -n "$pid" ]] || return 0
  kill "$pid" 2>/dev/null || true
  for _ in {1..20}; do
    if ! kill -0 "$pid" 2>/dev/null; then
      return 0
    fi
    sleep 0.15
  done
  kill -9 "$pid" 2>/dev/null || true
}

wait_for_port_up() {
  for _ in {1..50}; do
    if [[ -n "$(listening_pid)" ]]; then
      return 0
    fi
    sleep 0.12
  done
  echo "Timeout waiting for port :${PORT} to listen" >&2
  return 1
}

wait_for_port_down() {
  for _ in {1..40}; do
    if [[ -z "$(listening_pid)" ]]; then
      return 0
    fi
    sleep 0.1
  done
  echo "Timeout waiting for port :${PORT} to close" >&2
  return 1
}

build_server() {
  (cd "${WORKSPACE_DIR}" && cargo build -p slircd-ng >/dev/null)
}

start() {
  mkdir -p "$(dirname "${LOGFILE}")"
  build_server

  # Refuse to start if another non-our process is bound to the port
  local pid
  pid="$(listening_pid || true)"
  if [[ -n "$pid" ]]; then
    if is_our_binary "$pid"; then
      echo "Server already running (pid=$pid)"
      return 0
    else
      echo "Refusing to start: another process (pid=$pid) is listening on :${PORT}" >&2
      return 1
    fi
  fi

  echo "Starting slircd-ng with ${CONFIG} → ${LOGFILE}"
  "${BIN}" "${CONFIG}" >"${LOGFILE}" 2>&1 &
  echo $! >"${PIDFILE}"
  wait_for_port_up
  echo "Started pid=$(cat "${PIDFILE}")"
}

stop() {
  # Prefer pidfile if it points to our binary
  if [[ -f "${PIDFILE}" ]]; then
    local pf
    pf=$(cat "${PIDFILE}" || true)
    if [[ -n "$pf" ]] && is_our_binary "$pf" && kill -0 "$pf" 2>/dev/null; then
      echo "Stopping (pidfile) pid=$pf"
      kill_pid "$pf"
      rm -f "${PIDFILE}"
      wait_for_port_down || true
      echo "Stopped"
      return 0
    fi
  fi

  # Fallback: detect by port and ensure it's our binary
  local pid
  pid="$(listening_pid || true)"
  if [[ -n "$pid" ]] && is_our_binary "$pid"; then
    echo "Stopping (port) pid=$pid"
    kill_pid "$pid"
    rm -f "${PIDFILE}" 2>/dev/null || true
    wait_for_port_down || true
    echo "Stopped"
  else
    echo "No managed server running"
  fi
}

status() {
  local pid
  pid="$(listening_pid || true)"
  if [[ -n "$pid" ]]; then
    local exe
    exe=$(readlink -f "/proc/${pid}/exe" 2>/dev/null || echo "<unknown>")
    echo "Listening on :${PORT} pid=${pid} exe=${exe}"
    exit 0
  fi
  echo "Not listening on :${PORT}"
  exit 1
}

verify() {
  # Minimal handshake; expect 001 welcome
  if ! cmd_exists nc; then
    echo "nc not found" >&2; exit 2
  fi
  local out
  out=$( (printf "NICK verify$$\r\nUSER verify 0 * :Test\r\nQUIT\r\n"; sleep 0.1) | timeout 5 nc -C localhost "${PORT}" 2>&1 | sed -n '1,10p')
  echo "$out"
  if echo "$out" | rg -q ' 001 '; then
    echo "Verify: OK"
    exit 0
  else
    echo "Verify: FAILED" >&2
    exit 1
  fi
}

tail_log() {
  exec tail -n 100 -f "${LOGFILE}"
}

usage() {
  cat <<EOF
Usage: $(basename "$0") <start|stop|restart|status|verify|tail>
Environment:
  PORT=${PORT} PIDFILE=${PIDFILE} LOGFILE=${LOGFILE}
  CONFIG=${CONFIG}
EOF
}

case "${1:-}" in
  start) start ;;
  stop) stop ;;
  restart) stop; start ;;
  status) status ;;
  verify) verify ;;
  tail) tail_log ;;
  *) usage; exit 2 ;;
esac
