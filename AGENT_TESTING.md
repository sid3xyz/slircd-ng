# AI Agent Testing Instructions for slircd-ng

**Goal:** Test the `slircd-ng` IRC server using **third-party tools only**.

## CRITICAL RULE

**DO NOT use custom Python test suites (tests/e2e).** Custom E2E tests waste time debugging the test code itself. Real third-party clients and tools expose real bugs.

**Approved testing tools (third-party only):**
1. `ii` — Filesystem-based IRC client for interactive testing
2. `irctest` — Official RFC compliance suite (used by Ergo, Solanum, InspIRCd)
3. `irssi` / `weechat` — Real IRC clients for manual verification
4. `netcat` — Raw protocol verification (quick checks only)

---

## 1. Interactive Testing with `ii` (Recommended)

The `ii` client maps IRC channels to directories and messages to files. This is the best way for an AI to test the IRC server interactively.

### Setup
```bash
# Install if needed
sudo apt install ii
```

### Connection
```bash
# Connect to local test server (runs in background)
ii -s localhost -p 6667 -n AgentBot -f "AI Test Agent" &
```

### Usage

ii creates files at `~/irc/localhost/`. Write commands to `in`, read responses from `out`.

```bash
# Join a channel
echo "/j #test" > ~/irc/localhost/in

# Send a message (after joining)
echo "Hello World" > ~/irc/localhost/'#test'/in

# Read channel output
cat ~/irc/localhost/'#test'/out

# Leave channel
echo "/l" > ~/irc/localhost/'#test'/in

# Disconnect
echo "/quit" > ~/irc/localhost/in
```

---

## 2. RFC Compliance Testing with `irctest` (Required for Validation)

Use irctest for authoritative protocol compliance testing. This is the same suite used by production IRC servers.

### Setup (one-time)
```bash
cd /tmp && git clone https://github.com/ergochat/irctest.git
cd irctest && python3 -m venv .venv && .venv/bin/pip install -r requirements.txt
```

### Running Tests
```bash
export IRCTEST_SERVER_HOSTNAME=localhost IRCTEST_SERVER_PORT=6667
cd /tmp/irctest

# Single test (preferred for debugging):
timeout 15 .venv/bin/pytest --controller irctest.controllers.external_server \
  -k "testNickCollision" -v 2>&1 | tail -30

# Test suite (e.g., AWAY tests):
timeout 60 .venv/bin/pytest --controller irctest.controllers.external_server \
  irctest/server_tests/away.py --tb=no -q 2>&1 | tail -20
```

**ALWAYS use timeout and limit output.**

---

## 3. Raw Protocol Verification (Netcat)

Use netcat for quick protocol-level checks only.

```bash
# Basic handshake test
echo -e "NICK test$$\r\nUSER t 0 * :Test\r\n" | timeout 3 nc localhost 6667 | head -15

# Test specific command
printf "NICK t$$\r\nUSER t 0 * :T\r\nJOIN #test\r\nQUIT\r" | timeout 5 nc localhost 6667 | head -20
```

---

## 4. Real Client Testing (irssi/weechat)

For manual verification or complex scenarios:

```bash
# Connect with irssi
irssi -c localhost -p 6667 -n testuser
```

---

## Summary Checklist

1. **Is server running?** `nc -z localhost 6667`
2. **Interactive testing?** Use `ii` — write to `in`, read from `out`
3. **RFC compliance?** Use `irctest` — the authoritative third-party suite
4. **Quick protocol check?** Use `netcat` with timeout
5. **Manual verification?** Use `irssi` or `weechat`

**NEVER use custom Python tests (tests/e2e) for server validation.**
