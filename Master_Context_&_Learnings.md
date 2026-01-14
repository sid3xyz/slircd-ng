# Master Context & Learnings - slircd-ng

> Comprehensive knowledge base for slircd-ng development.
> Last Updated: 2026-01-14 (Session: RELAYMSG labeled-response cleanup)

---

## PROJECT STATUS

**Version**: 1.0.0-alpha.1  
**irctest Compliance**: 92.2% (357/387)  
**Unit Tests**: 642 passing  

### Recent Milestones

| Date | Achievement |
|------|-------------|
| 2026-01-14 | RELAYMSG labeled-response cleanup (labels on FAIL/ACK, no leak to recipients) |
| 2026-01-14 | Bouncer autoreplay polish: channel tracking, nick persistence, device last-seen updates |
| 2026-01-12 | Phase 1 bouncer/multiclient foundation complete |
| 2026-01-12 | v1.0.0-alpha.1 release preparation |
| 2026-01-12 | PRECIS casemapping for UTF-8 nicknames |
| 2026-01-12 | Monorepo: absorbed slirc-proto and slirc-crdt |
| 2026-01-12 | CI/CD pipeline with GitHub Actions |
| 2026-01-11 | METADATA handler complete (9/9 tests passing) |
| 2026-01-11 | NPC/ROLEPLAY handler complete |
| 2026-01-11 | CHATHISTORY TARGETS fix |

---

## CRITICAL CONSTRAINTS & RULES

### PROTO-FIRST RULE (MANDATORY)
**Never create daemon workarounds for proto bugs. ALWAYS fix proto first.**

- If a feature requires a proto change: **STOP implementation and document blocker in PROTO_REQUIREMENTS.md**
- Never use `Command::Raw` as a workaround for missing proto variants
- Never implement daemon logic before verifying complete proto support
- Example: RELAYMSG parameter order was backwards in proto → Fixed proto parser before updating daemon handler

### ARCHITECTURAL PURITY
- Zero dead code (no commented-out blocks, no orphaned TODOs without issue references)
- All TODOs must include context and reference tracking issue numbers
- No legacy artifacts or superseded documentation
- Handlers follow strict typestate patterns (PreRegHandler, PostRegHandler, UniversalHandler<S>)

### HANDLER PATTERNS (DO NOT DEVIATE)
1. **Argument Extraction** → Validation → State Checks → Routing
2. **Use SenderSnapshot** for pre-fetched user data (eliminates redundant lookups)
3. **Route via `route_to_channel_with_snapshot()`** or `route_to_user_with_snapshot()`
4. **Never hold `MessageRef` across `.await` points** - extract data first
5. **DashMap locks released before async calls** (prevents deadlocks)

---

## COMPLETED IMPLEMENTATIONS

### 1. METADATA Handler ✅

**File**: `src/handlers/messaging/metadata.rs`  
**Status**: Complete (9/9 irctest passing)

**Implementation**:
- GET/SET/LIST for user metadata
- GET/SET/LIST for channel metadata  
- Channel metadata stored in ChannelActor
- User metadata stored in User.metadata HashMap
- Binary data support (null bytes allowed)
- ISUPPORT advertising

### 2. NPC/ROLEPLAY Handler ✅

**File**: `src/handlers/messaging/npc.rs`  
**Status**: Complete

**Implementation**:
- Channel mode +E enforcement
- ERR_CANNOTSENDRP (573) on missing +E
- Message relayed with altered nick prefix
- Proper capability advertisement

### 3. PRECIS Casemapping ✅

**Files**: 
- `src/config/types.rs` - Casemapping enum
- `src/handlers/connection/nick.rs` - PRECIS validation
- `src/handlers/connection/welcome_burst.rs` - ISUPPORT

**Implementation**:
- Config-driven casemapping (rfc1459 or precis)
- PRECIS-aware nick validation for Unicode
- ISUPPORT CASEMAPPING token from config
### Current irctest gaps (2026-01-14)

- RELAYMSG labeled-response: handler now labels FAIL replies and emits labeled ACK on success; need to confirm if irctest expects labeled echo instead. Labels are intentionally not forwarded to other recipients.
- READQ enforcement: messages >16KB still return 417; spec/irctest expects disconnect.
- Channel forwarding (+f): mode parsing exists, forwarding logic absent.
- Confusables detection: nick validation lacks homoglyph protection.
- Bouncer resumption: resume tokens/state sync not implemented (7 tests).
- ZNC playback: extension unsupported (1 test).

### RELAYMSG status (daemon)

- Labeled ACK on success plus labeled FAIL/ERR replies; relayed traffic is untagged to avoid leaking labels to other clients.
- Prefix override via `RouteMeta.override_nick` preserved.
- Next: verify irctest expectation; add per-sender labeled echo if ACK is insufficient.

## PROTO DEPENDENCY ANALYSIS

| Feature | Proto Gap | Impact |
|---------|-----------|--------|
| Bouncer resumption | Missing BOUNCER/RESUME-style commands | Blocks 7 irctest bouncer resumption tests |
| ZNC playback | Extension undefined in proto | Blocks 1 ZNC playback test |
| Confusables | None | Daemon-only validation work |
| READQ | None | Daemon-only policy enforcement |
| Channel +f forwarding | None | Daemon-only forwarding logic |

## ARCHITECTURAL DECISIONS & PATTERNS

### SenderSnapshot Pattern
```rust
// Pre-fetch all sender data once at handler entry
let snapshot = SenderSnapshot::build(ctx).await?;

// Pass to routing functions
route_to_channel_with_snapshot(ctx, &channel, msg, &opts, None, None, &snapshot).await
```
**Why**: Eliminates redundant user lookups, improves performance, ensures consistency

### Snapshot Nick Override (RELAYMSG Innovation)
```rust
let mut snapshot = SenderSnapshot::build(ctx).await?;
snapshot.nick = relay_from.to_string();  // Override sender nick
route_to_channel_with_snapshot(..., &snapshot)
```
**Why**: The routing function uses snapshot to build UserContext, which controls the message prefix. Overriding snapshot.nick makes the routed message appear from relay_from while preserving other sender metadata.

### Message Construction Pattern
```rust
let msg = Message {
    tags: None,
    prefix: Some(Prefix::Nickname(nick, user, host)),
    command: Command::PRIVMSG(target, text),
};
```
**Why**: Type-safe message construction, prevents malformed IRC messages

### Routing Return Type Pattern
```rust
// route_to_channel returns ChannelRouteResult, NOT HandlerResult
let _ = route_to_channel_with_snapshot(...).await;
// Drop result with explicit `let _ = ...` pattern
```
**Why**: Routing is fire-and-forget (message already delivered to channel members), handler completes successfully regardless

---

## TEST INFRASTRUCTURE & IRCTEST FRAMEWORK

### Test Invocation
```bash
cd /home/straylight/slircd-ng/slirc-irctest

# Single test
SLIRCD_BIN=/home/straylight/slircd-ng/target/release/slircd \
  pytest --controller=irctest.controllers.slircd \
  irctest/server_tests/metadata.py::MetadataDeprecatedTestCase::testSetGetValid -v

# Full test set
pytest --controller=irctest.controllers.slircd irctest/server_tests/metadata.py -v
```

### Test Framework Capabilities
- **Capability Tracking**: Tests request specific capabilities (labeled-response, batch, etc.)
- **Message Interception**: Each message to/from server is captured and can be asserted
- **Server Config**: Test can specify config via `@staticmethod def config()`
  - Example: `ergo_roleplay=True` enables roleplay mode testing
  - Example: `ergo_metadata=True` enables metadata testing

### Labeled-Response Capability Requirement
- When client sends `@label=x COMMAND ...`, the **first response** must include the same `@label=x` tag
- For commands that produce output, the echo should be the primary output
- For RELAYMSG with echo-message capability, should echo the PRIVMSG back with the label
- Current issue: Middleware sends generic ACK instead of routing the echo properly

---

## KNOWN WORKING PATTERNS

### From Existing PRIVMSG Handler (Reference Implementation)
**Location**: `src/handlers/messaging/privmsg.rs`

Patterns to replicate:
```rust
// 1. Extract and validate arguments
let target = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
let text = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;

// 2. Snapshot for consistent user data
let snapshot = SenderSnapshot::build(ctx).await?;

// 3. Build message with proper prefix
let message = Message {
    tags: None,  // Tags handled separately by framework
    prefix: Some(Prefix::Nickname(snapshot.nick.clone(), snapshot.user.clone(), snapshot.host.clone())),
    command: Command::PRIVMSG(target.to_string(), text.to_string()),
};

// 4. Route with snapshot
route_to_channel_with_snapshot(ctx, &target_lower, message, &opts, None, None, &snapshot).await
```

### Error Response Pattern
```rust
// For ERR_ responses, use server_reply helper
let reply = server_reply(
    &ctx.matrix.server_info.name,
    Response::ERR_NOSUCHCHANNEL,
    vec![ctx.state.nick.clone(), target.to_string(), "No such channel".to_string()],
);
ctx.sender.send(reply).await?;
```

### FAIL Response Pattern (for RELAYMSG)
```rust
// For FAIL responses (Ergo extension), construct manually
let reply = Message {
    tags: None,
    prefix: Some(Prefix::ServerName(ctx.matrix.server_info.name.clone())),
    command: Command::FAIL(
        "RELAYMSG".to_string(),
        "INVALID_NICK".to_string(),
        vec![reason],
    ),
};
ctx.sender.send(reply).await?;
```

---

## CURRENT BUILD & TEST STATUS

- Build/tests: `cargo test -q` on `feat/relaymsg-label-ack` passed.
- irctest: last known 357/387 (92.2%); remaining gaps match "Current irctest gaps" above.
- RELAYMSG labeled-response change pending irctest verification.

---

## CONTINUATION PLAN

1. Verify RELAYMSG labeled-response behavior in irctest; add per-sender labeled echo if ACK-only is insufficient.
2. Implement READQ enforcement: disconnect on >16KB messages (no 417-only behavior).
3. Add channel forwarding (+f) logic and tests.
4. Implement confusables/homoglyph nick detection (mode/config gated).
5. Design/implement bouncer resumption protocol (proto + daemon); document blockers in PROTO_REQUIREMENTS.md.
6. Re-run cargo test + targeted irctest suites; update IRCTEST_SESSION_REPORT.md.

---

## REFERENCE MATERIALS

**Key Files**:
- `/home/straylight/slircd-ng/.github/copilot-instructions.md` - Project constraints & patterns
- `/home/straylight/slircd-ng/PROTO_REQUIREMENTS.md` - Proto blockers & known issues
- `/home/straylight/slircd-ng/ARCHITECTURE.md` - Full architectural deep dive
- `slirc-proto` - Command/Numeric definitions, parsing logic
- `src/handlers/messaging/privmsg.rs` - Reference handler implementation

**Irctest**:
- `/home/straylight/slircd-ng/slirc-irctest/irctest/server_tests/metadata.py` - METADATA test cases
- `/home/straylight/slircd-ng/slirc-irctest/irctest/server_tests/roleplay.py` - NPC/SCENE test cases
- `/home/straylight/slircd-ng/slirc-irctest/irctest/server_tests/relaymsg.py` - RELAYMSG test cases

**Numerics Reference**:
- `ERR_CANNOTSENDRP` = 573 (roleplay disabled)
- `RPL_KEYVALUE` = 761 (metadata key-value)
- `RPL_METADATA_LIST` = 762 (metadata list)
- `RPL_METADATA_NOMATCH` = 763 (no metadata)

---

## BOUNCER/MULTICLIENT ARCHITECTURE (Phase 1 Complete)

### Overview

Bouncer functionality enables multiple IRC clients to connect to the same account, with persistent state across disconnections (always-on mode). This is implemented via a session/client separation pattern.

### Core Data Model

```
Many TCP Connections (Sessions) → One Client (Account State) → One User (Virtual Presence)
```

#### Session (Connection-Level)
- **SessionId**: `uuid::Uuid` generated via `Uuid::new_v4()` on registration
- **DeviceId**: Optional `String` extracted from SASL username (e.g., `alice@phone` → device `"phone"`)
- Stored in `RegisteredState.session_id` and `RegisteredState.device_id`

#### Client (Account-Level)
- **File**: `src/state/client.rs`
- Persists across session disconnects (if always-on enabled)
- Tracks:
  - Active sessions (`HashSet<SessionId>`)
  - Channel memberships with modes
  - Devices with last-seen timestamps
  - Always-on and auto-away settings
  - Dirty bits for selective persistence

#### User (Virtual Presence)
- Existing `User` struct in `src/state/user.rs`
- One-to-one with `Client` for single-account scenarios
- Tracks IRC-visible state (nick, modes, channels)

### Key Files

| File | Purpose | Lines |
|------|---------|-------|
| `src/state/client.rs` | Client struct, dirty bits, devices | ~300 |
| `src/state/managers/client.rs` | ClientManager with attach/detach logic | ~450 |
| `src/config/multiclient.rs` | MulticlientConfig, AlwaysOnPolicy, AutoAwayPolicy | ~250 |
| `src/services/nickserv/commands/sessions.rs` | NickServ SESSIONS command | ~130 |

### Session Lifecycle

#### Attach (SASL Authentication)
```rust
// In src/handlers/cap/sasl.rs
let (account, device_id) = extract_device_id(&username);
attach_session_to_client(ctx, &account, device_id).await;
```

Returns `AttachResult`:
- `Created` - New client created for account
- `Attached { reattach, first_session }` - Session added to existing client
- `MulticlientNotAllowed` - Account doesn't allow multiple sessions
- `TooManySessions` - Session limit reached

#### Detach (Disconnect)
```rust
// In src/state/matrix.rs disconnect_user()
let result = self.client_manager.detach_session(session_id).await;
```

Returns `DetachResult`:
- `Detached { remaining_sessions }` - Other sessions still connected
- `Persisting` - Client persisting in always-on mode
- `Destroyed` - Client destroyed (no always-on)
- `NotFound` - Session wasn't tracked

### Configuration

```toml
[multiclient]
enabled = true                    # Enable multiclient functionality
allowed_by_default = true         # Allow by default (can be per-account)
always_on = "opt-in"             # Disabled, OptIn, OptOut, Mandatory
always_on_expiration = "30d"     # How long to persist disconnected clients
auto_away = "opt-out"            # Disabled, OptIn, OptOut
max_sessions_per_account = 10    # Session limit per account
```

### NickServ SESSIONS Command

```
/msg NickServ SESSIONS           # List your own active sessions
/msg NickServ SESSIONS <account> # List sessions for account (opers only)
```

Output:
```
Active sessions for alice (2 total):
  1. Connected since: 2026-01-12 21:00:00 UTC (device: phone)
  2. Connected since: 2026-01-12 20:30:00 UTC (device: laptop)
End of session list.
```

Oper note: session entries append `id` and `ip` for operator invocations.

### Implementation Status

| Feature | Status | Notes |
|---------|--------|-------|
| Client struct | ✅ Complete | Sessions, channels, devices, dirty bits |
| ClientManager | ✅ Complete | Attach/detach/expiration logic |
| SASL integration | ✅ Complete | Device ID extraction, session attachment |
| Disconnect integration | ✅ Complete | Session detachment, persist check |
| NickServ SESSIONS | ✅ Complete | List active sessions |
| MulticlientConfig | ✅ Complete | Policies, duration parsing |
| Always-on persistence | ✅ Complete | Redb store, restore, and dirty writeback |
| Auto-away | ✅ Basic | Away set/cleared on session detach/attach |
| Channel playback | ✅ Autoreplay | JOIN replay + CHATHISTORY per reattach |
| Channel tracking | ✅ Basic | Join/part/kick updates persisted for autoreplay |
| Client nick sync | ✅ Basic | Nick changes persisted for always-on clients |

### Next Steps (Phase 2)

1. **Read marker integration**: Track per-target read positions for precise playback
2. **Device management**: NickServ commands for device naming/removal
3. **Per-account settings**: Allow per-account multiclient/always-on config

---

## COMMAND SUMMARY

| Command | Status | Tests | Blocker | Next Action |
|---------|--------|-------|---------|---|
| METADATA | Complete | 9/9 pass | None | Regression checks |
| NPC | Complete | Passing | None | Maintain |
| RELAYMSG | ACK+label applied | Verify | Labeled echo semantics | Run irctest relaymsg.py and adjust per-sender labeling if needed |
| READQ | Not implemented | 2 fail | Daemon policy | Disconnect on >16KB inputs |
| Channel +f | Partial | 1 fail | Daemon forwarding | Implement forwarding behavior |
| Confusables | Missing | 1 fail | Daemon validation | Add homoglyph detection |
| Bouncer resumption | Missing | 7 fail | Proto+daemon | Design RESUME/BOUNCER flow |
| ZNC playback | Missing | 1 fail | Proto extension | Decide support or negotiate skip |

