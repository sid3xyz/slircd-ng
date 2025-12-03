# AI Agent Testing Instructions for slircd-ng

**Goal:** Test the `slircd-ng` IRC server using native tools that an AI agent can easily control.

**Tools:**
1.  `ii` (Interactive/Manual Testing) - Treated as a filesystem interface.
2.  `python + pytest` (Automated/Regression Testing) - Using the existing `tests/e2e` suite.
3.  `netcat` (Protocol Fuzzing) - For raw socket injection.

---

## 1. Interactive Testing with `ii` (Recommended)

The `ii` client maps IRC channels to directories and messages to files. This is the best way for an AI to "use" the IRC server interactively.

### Setup (Run once)
If `ii` is not installed, ask the user to install it (`sudo apt install ii` or build from source).

### Connection
To connect to the local development server:
```bash
# Create a temporary directory for this session
mkdir -p /tmp/irc_test_session
cd /tmp/irc_test_session

# Connect (runs in background)
ii -s localhost -p 6667 -n AgentBot -f "AI Test Agent" &
```

### How to Control the Client

The client creates a directory tree. The server control file is at `~/irc/localhost/in`.

**Join a Channel:**

```bash
echo "/j #test" > /tmp/irc_test_session/localhost/in
```

**Send a Message:**

```bash
# Wait for join to complete, then:
echo "Hello World" > /tmp/irc_test_session/localhost/#test/in
```

**Read Chat Logs (Verify Output):**

```bash
cat /tmp/irc_test_session/localhost/#test/out
```

**Leave/Part:**

```bash
echo "/l" > /tmp/irc_test_session/localhost/#test/in
```

### Example Agent Prompt

> *"Join channel \#dev, say 'connection test', and check the output file to verify the server echoed the message back."*

-----

## 2\. Automated Testing (Python E2E Suite)

Your project already contains a robust E2E test suite. Use this for regression testing or checking specific logic.

### Setup

```bash
cd tests/e2e
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
```

### Running Tests

Run the full suite to ensure no regressions:

```bash
# Auto-starts the server and runs tests
pytest -v
```

### Writing New Tests on the Fly

To test a specific bug or feature, write a temporary test file using the `client` fixture.

**Pattern:**
Create `tests/e2e/temp_agent_test.py`:

```python
import pytest

@pytest.mark.asyncio
async def test_agent_scenario(client):
    # Scenario: Check if NICK change is enforced
    await client.send("NICK new_nick")
    response = await client.expect("NICK")
    assert "new_nick" in response.raw
```

Run it:

```bash
pytest tests/e2e/temp_agent_test.py
```

-----

## 3\. Raw Protocol Testing (Netcat)

Use this to test handshake edge cases or invalid UTF-8 handling.

**Handshake Test:**

```bash
echo -e "NICK rawbot\r\nUSER raw 0 * :Raw Bot\r\n" | nc -C localhost 6667
```

**Crash Test (Fuzzing):**

```bash
# Send garbage data to see if server panics
echo -e ":invalid_prefix COMMAND parameter\r\n" | nc -C localhost 6667
```

-----

## 4. Server Control (serverctl.sh)

Use the workspace-provided helper to safely start/stop/restart the test server without affecting unrelated processes.

- Script: `slircd-ng/scripts/serverctl.sh`
- Commands: `start`, `stop`, `restart`, `status`, `verify`, `tail`
- Defaults: `PORT=6667`, `PIDFILE=/tmp/slircd-test.pid`, `LOGFILE=/tmp/slircd-test.log`, `CONFIG=slircd-ng/config.test.toml`

Quick start:

```bash
# From workspace root
./slircd-ng/scripts/serverctl.sh restart
./slircd-ng/scripts/serverctl.sh verify   # Expect 001 welcome
./slircd-ng/scripts/serverctl.sh status   # Show PID listening on :6667
```

Follow the safe flow for agent sessions:

```bash
# Stop only the managed server (PID+binary checked)
./slircd-ng/scripts/serverctl.sh stop

# Start fresh build with test config
./slircd-ng/scripts/serverctl.sh start

# Tail logs live
./slircd-ng/scripts/serverctl.sh tail
```

Environment overrides (optional):

```bash
PORT=6667 PIDFILE=/tmp/slircd-test.pid LOGFILE=/tmp/slircd-test.log \
    CONFIG=/home/straylight/slircd-ng/config.test.toml \
    ./slircd-ng/scripts/serverctl.sh restart
```

### Real-World Scenarios (copy/paste)

CAP + SETNAME + WHOIS:

```bash
(printf "CAP LS 302\r\nNICK realtest\r\nUSER real 0 * :Real User\r\n";
 printf "CAP REQ :setname\r\nCAP END\r\nSETNAME :New Realname\r\nWHOIS realtest\r\nQUIT\r\n") |
    timeout 6 nc -C localhost 6667
```

MODE +kl and query (expect 324 with separate params):

```bash
chan="#test$(tr -dc a-z0-9 </dev/urandom | head -c 6)";
(printf "NICK modecase\r\nUSER modecase 0 * :Mode Case\r\nJOIN ${chan}\r\n";
 printf "MODE ${chan} +kl key123 21\r\nMODE ${chan}\r\nQUIT\r\n") |
    timeout 7 nc -C localhost 6667
```

PING and TIME:

```bash
(printf "NICK echoer\r\nUSER echoer 0 * :Echoer\r\nPING token123\r\nTIME\r\nQUIT\r\n") |
    timeout 6 nc -C localhost 6667
```

LIST channels (may be empty on fresh server):

```bash
(printf "NICK lister\r\nUSER lister 0 * :Lister\r\nLIST\r\nQUIT\r\n") |
    timeout 6 nc -C localhost 6667
```


## Summary Checklist for Agent

1. **Is the server running?** Check with `nc -z localhost 6667`.
2. **Interactive?** Use `ii`. Write to `in` files, read from `out` files.
3. **Regression?** Use `pytest` in `tests/e2e`.
4. **Debug?** Check server logs: `./slircd-ng/scripts/serverctl.sh tail` (or set `RUST_LOG=debug`).
