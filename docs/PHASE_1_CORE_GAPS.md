# Phase 1: Core Feature Gaps

> **Target Duration:** 3-4 weeks
> **Primary Agent:** server-engineer
> **Exit Criteria:** WebSocket operational, extended modes complete, hot reload working, irctest ≥ 80%

---

## Overview

Phase 1 closes the critical feature gaps between slircd-ng and production SLIRCd. These are table-stakes features for any modern IRC deployment.

---

## 1.1 WebSocket Transport

**Priority:** Critical
**Agent:** server-engineer (supported by: protocol-architect)
**Estimated Effort:** 1 week

### Objective

Add WebSocket transport support (ws:// and wss://) alongside existing TCP connections. This enables browser-based clients and modern web integrations.

### Technical Requirements

1. **Transport Abstraction**
   - Create `src/transport/mod.rs` with `Transport` trait
   - Implement `TcpTransport` wrapping existing `TcpStream`
   - Implement `WsTransport` using `tokio-tungstenite`
   - Both must implement `AsyncRead + AsyncWrite`

2. **Configuration**
   - Add to `config.toml`:
     ```toml
     [websocket]
     enabled = true
     bind = "0.0.0.0:6697"
     tls = true
     cert_file = "/path/to/cert.pem"
     key_file = "/path/to/key.pem"
     ```

3. **Connection Handling**
   - Modify `src/server.rs` to accept WebSocket connections
   - WebSocket upgrade happens before IRC handshake
   - After upgrade, treat as normal IRC connection

4. **Message Framing**
   - WebSocket messages are already framed (no `\r\n` parsing needed)
   - Text frames only; binary frames rejected
   - Each WebSocket message = one IRC line

### Implementation Steps

```
STEP 1: Create transport abstraction
  FILE: src/transport/mod.rs (new)
  - Define Transport trait with read_line/write_line methods
  - Define TransportKind enum { Tcp, WebSocket }

STEP 2: Implement TcpTransport
  FILE: src/transport/tcp.rs (new)
  - Wrap existing BufReader<TcpStream> logic
  - Implement Transport trait

STEP 3: Add tokio-tungstenite dependency
  FILE: Cargo.toml
  - Add: tokio-tungstenite = "0.21"

STEP 4: Implement WsTransport
  FILE: src/transport/websocket.rs (new)
  - Use tokio_tungstenite::accept_async()
  - Implement Transport trait
  - Handle close frames gracefully

STEP 5: Modify connection handler
  FILE: src/connection/mod.rs
  - Accept Transport instead of TcpStream
  - All existing code works unchanged

STEP 6: Add WebSocket listener
  FILE: src/server.rs
  - Add WebSocket accept loop
  - Upgrade HTTP to WebSocket
  - Create WsTransport, pass to connection handler

STEP 7: Configuration parsing
  FILE: src/config.rs
  - Add WebsocketConfig struct
  - Parse [websocket] section

STEP 8: TLS support for WebSocket
  FILE: src/transport/websocket.rs
  - Use tokio-rustls for wss://
  - Reuse existing TLS configuration
```

### Verification

```bash
# Test with wscat
npm install -g wscat
wscat -c ws://localhost:6697
# Send: NICK test\r\nUSER test 0 * :Test\r\n
# Expect: RPL_WELCOME

# Test TLS
wscat -c wss://localhost:6697 --no-check
```

### Files to Create/Modify

| File | Action | Lines (est.) |
|------|--------|--------------|
| src/transport/mod.rs | Create | 50 |
| src/transport/tcp.rs | Create | 80 |
| src/transport/websocket.rs | Create | 120 |
| src/server.rs | Modify | +40 |
| src/connection/mod.rs | Modify | +20 |
| src/config.rs | Modify | +30 |
| Cargo.toml | Modify | +2 |

---

## 1.2 Extended Channel Modes

**Priority:** High
**Agent:** server-engineer (supported by: qa-compliance-lead)
**Estimated Effort:** 1 week

### Objective

Implement extended channel modes that SLIRCd supports for channel management and anti-spam.

### Mode Specifications

| Mode | Name | Behavior |
|------|------|----------|
| +f | Flood Protection | `+f [5j]:10` = 5 joins in 10 seconds triggers +i |
| +L | Link Channel | `+L #overflow` = redirect when +l limit reached |
| +q | Quiet | `+q nick!*@*` = can join but cannot speak |
| +R | Registered Only | Only SASL-authenticated users can join |
| +M | Moderated Registered | Only registered users can speak (+m for others) |
| +N | No Nick Change | Cannot change nick while in channel |
| +c | No Colors | Strip mIRC color codes from messages |

### Implementation Steps

```
STEP 1: Define mode types
  FILE: src/state/actor/modes.rs
  ACTION: Extend ModeType enum
  - Add: FloodProtection, LinkChannel, Quiet, RegisteredOnly, ModeratedRegistered, NoNickChange, NoColors

STEP 2: Implement +f (Flood Protection)
  FILE: src/state/actor/handlers/modes.rs
  - Parse +f parameter: [joins#][texts#][knocks#]:seconds
  - Track per-action counts in ChannelActor
  - Trigger +i on threshold breach
  - Reset counters periodically

STEP 3: Implement +L (Link Channel)
  FILE: src/handlers/channel/join.rs
  FILE: src/state/actor/handlers/join.rs
  - If +l limit reached and +L set, send 471 with link
  - Client auto-joins link channel (or manual redirect)

STEP 4: Implement +q (Quiet)
  FILE: src/state/actor/validation/bans.rs (add QuietEntry)
  FILE: src/state/actor/handlers/message.rs
  - QuietList: Vec<(mask, setter, timestamp)>
  - Check message sender against quiet list
  - Quiet users can join/see but not speak

STEP 5: Implement +R (Registered Only)
  FILE: src/state/actor/handlers/join.rs
  - Check if joining user has account set (from SASL)
  - If not registered and +R, send 477 ERR_NEEDREGGEDNICK

STEP 6: Implement +M (Moderated Registered)
  FILE: src/state/actor/handlers/message.rs
  - If +M and user not registered: silent drop or 404
  - Registered users always can speak

STEP 7: Implement +N (No Nick Change)
  FILE: src/handlers/user_state/nick.rs
  - Before nick change, iterate user's channels
  - If any channel has +N, return 447 ERR_NONICKCHANGE

STEP 8: Implement +c (No Colors)
  FILE: src/state/actor/handlers/message.rs
  - Strip \x03, \x02, \x1F, \x1D, \x16, \x0F from messages
  - Strip before broadcast

STEP 9: Update MODE display
  FILE: src/handlers/channel/mode.rs
  - Include new modes in 324 RPL_CHANNELMODEIS
  - Handle parameters correctly for +fLq
```

### Verification

```bash
# Test +R
/mode #test +R
# Unregistered user tries to join -> 477

# Test +f
/mode #test +f [5j]:10
# Flood 6 joins quickly -> channel goes +i

# Test +q
/mode #test +q *!*@spammer.net
# Matching user joins, tries to speak -> message dropped
```

### Files to Modify

| File | Action | Lines (est.) |
|------|--------|--------------|
| src/state/actor/modes.rs | Modify | +100 |
| src/state/actor/handlers/modes.rs | Modify | +150 |
| src/state/actor/handlers/join.rs | Modify | +50 |
| src/state/actor/handlers/message.rs | Modify | +80 |
| src/handlers/channel/mode.rs | Modify | +40 |
| src/handlers/user_state/nick.rs | Modify | +30 |

---

## 1.3 Hot Reload (SIGHUP)

**Priority:** High
**Agent:** server-engineer (supported by: security-ops)
**Estimated Effort:** 3 days

### Objective

Allow runtime configuration reload via SIGHUP without disconnecting clients. Essential for production operations.

### Reloadable Configuration

| Config Section | Reloadable | Notes |
|----------------|------------|-------|
| [server] listen | ❌ No | Requires restart |
| [server] motd | ✅ Yes | Reload file |
| [server] name | ❌ No | Protocol identity |
| [tls] cert/key | ✅ Yes | New connections use new certs |
| [operators] | ✅ Yes | Add/remove opers |
| [olines] | ✅ Yes | Oper auth rules |
| [klines] | ✅ Yes | Ban patterns (also in DB) |

### Implementation Steps

```
STEP 1: Create signal handler
  FILE: src/signals.rs (new)
  - Use tokio::signal::unix::signal(SignalKind::hangup())
  - Spawn task watching for SIGHUP
  - On signal, send ReloadConfig event to main

STEP 2: Define ReloadableConfig
  FILE: src/config.rs
  - Create ReloadableConfig struct (subset of Config)
  - Implement Config::reload() -> Result<ReloadableConfig>
  - Validate before applying

STEP 3: Apply reloaded config
  FILE: src/server.rs
  - Receive ReloadConfig event
  - Call config.reload()
  - Update shared state:
    - Arc<RwLock<MotdLines>>
    - Arc<RwLock<OperConfig>>
    - TLS acceptor (Arc<tokio_rustls::TlsAcceptor>)

STEP 4: Broadcast REHASH notice
  FILE: src/handlers/oper/rehash.rs
  - Existing REHASH command triggers same reload path
  - SIGHUP = automatic REHASH
  - Send NOTICE to all opers: "Configuration reloaded"

STEP 5: Graceful degradation
  FILE: src/signals.rs
  - If reload fails, log error, keep old config
  - Send NOTICE to opers with error details
```

### Verification

```bash
# Modify config.toml
echo 'New MOTD line' >> /etc/slircd/motd.txt

# Send SIGHUP
kill -HUP $(pgrep slircd)

# Check logs for "Configuration reloaded"
# /motd shows new content
```

### Files to Create/Modify

| File | Action | Lines (est.) |
|------|--------|--------------|
| src/signals.rs | Create | 60 |
| src/config.rs | Modify | +40 |
| src/server.rs | Modify | +30 |
| src/handlers/oper/rehash.rs | Modify | +10 |

---

## 1.4 irctest Compliance

**Priority:** High
**Agent:** qa-compliance-lead (supported by: protocol-architect)
**Estimated Effort:** 1 week

### Objective

Achieve ≥80% pass rate on the irctest compliance suite. Document all intentional deviations.

### Current Status

```bash
cd /home/straylight/slirc-irctest
SLIRCD_BIN=/home/straylight/target/debug/slircd \
  timeout 90 .venv/bin/pytest --controller irctest.controllers.slircd \
  -v 2>&1 | tail -20
```

### Focus Areas

| Test Category | Priority | Files |
|---------------|----------|-------|
| Connection Registration | Critical | connection_registration.py |
| Channel Operations | High | channel.py, channel_*.py |
| User Modes | Medium | modes.py |
| NAMES/WHO/WHOIS | Medium | names.py, who.py, whois.py |
| Services Integration | Low | services.py |

### Implementation Steps

```
STEP 1: Run full test suite, capture results
  COMMAND:
    SLIRCD_BIN=/home/straylight/target/debug/slircd \
    timeout 300 .venv/bin/pytest --controller irctest.controllers.slircd \
    -v --tb=short 2>&1 | tee irctest-baseline.log

STEP 2: Categorize failures
  ACTION: Parse irctest-baseline.log
  - FAIL: Implementation bug (fix)
  - XFAIL: Known deviation (document)
  - ERROR: Test infrastructure issue (investigate)

STEP 3: Fix connection_registration.py failures
  FILES: src/handlers/core/*, src/connection/*
  PRIORITY: These are blocking all other tests

STEP 4: Fix channel.py failures
  FILES: src/handlers/channel/*, src/state/actor/*
  - Focus on JOIN, PART, KICK, MODE correctness

STEP 5: Fix numeric response issues
  FILE: src/handlers/core/numerics.rs
  - Ensure correct parameter order
  - Ensure trailing parameter handling

STEP 6: Document intentional deviations
  FILE: slircd-ng/docs/IRCTEST_DEVIATIONS.md (new)
  - List each XFAIL with rationale
  - Reference RFC sections

STEP 7: Add irctest to CI
  FILE: .github/workflows/test.yml
  - Run subset of critical tests on each PR
  - Full suite on main branch
```

### Verification

```bash
# Target: ≥80% pass rate
SLIRCD_BIN=/home/straylight/target/debug/slircd \
  .venv/bin/pytest --controller irctest.controllers.slircd \
  -v 2>&1 | grep -E "passed|failed|error"

# Expected: ~80+ passed, <20 failed/xfail
```

### Files to Create/Modify

| File | Action | Lines (est.) |
|------|--------|--------------|
| docs/IRCTEST_DEVIATIONS.md | Create | 100 |
| Various handlers | Modify | +200 total |
| .github/workflows/test.yml | Modify | +30 |

---

## Phase 1 Completion Checklist

- [ ] 1.1 WebSocket Transport
  - [ ] Transport trait abstraction
  - [ ] TcpTransport implementation
  - [ ] WsTransport implementation
  - [ ] Configuration parsing
  - [ ] TLS support (wss://)
  - [ ] Integration tests

- [ ] 1.2 Extended Channel Modes
  - [ ] +f Flood Protection
  - [ ] +L Link Channel
  - [ ] +q Quiet List
  - [ ] +R Registered Only
  - [ ] +M Moderated Registered
  - [ ] +N No Nick Change
  - [ ] +c No Colors
  - [ ] MODE display updates
  - [ ] Unit tests for each mode

- [ ] 1.3 Hot Reload
  - [ ] SIGHUP handler
  - [ ] ReloadableConfig struct
  - [ ] Config validation on reload
  - [ ] TLS cert reload
  - [ ] Oper notification
  - [ ] Error handling

- [ ] 1.4 irctest Compliance
  - [ ] Baseline test run
  - [ ] Connection registration fixes
  - [ ] Channel operation fixes
  - [ ] Numeric response fixes
  - [ ] Deviation documentation
  - [ ] CI integration
  - [ ] ≥80% pass rate achieved

---

## Agent Handoff Notes

When assigning this phase to AI agents:

1. **Start with 1.1 WebSocket** - Creates foundation for other work
2. **Run `cargo clippy --workspace -- -D warnings`** after each step
3. **Commit atomically** - One feature per commit, descriptive messages
4. **Read existing code first** - `src/server.rs`, `src/connection/mod.rs`
5. **Follow existing patterns** - DashMap for registries, mpsc for actors
6. **No `unwrap()`** - Use `?` or explicit error handling
7. **Update tests** - Add tests for each new feature

### Recommended Prompts for GPT-5.1-codex-max

```
TASK: Implement WebSocket transport for slircd-ng
CONTEXT: Read PHASE_1_CORE_GAPS.md section 1.1
FILES TO READ FIRST: src/server.rs, src/connection/mod.rs, Cargo.toml
CONSTRAINTS: Follow existing patterns, no unwrap(), atomic commits
OUTPUT: Implementation with tests, ready for cargo clippy
```
