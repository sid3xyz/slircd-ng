# Master Context & Learnings - slircd-ng

> Comprehensive knowledge base for slircd-ng development.
> Last Updated: 2026-01-14

---

## PROJECT STATUS

**Version**: 1.0.0-alpha.1  
**irctest Compliance**: ~92% (357/387 estimated)  
**Unit Tests**: 664+ passing  
**Current Branch**: `fix/test-failures-investigation` (2 commits ahead of main)

### Latest Work (2026-01-14)
- **Fixed LUSERS unregistered connection tracking** - Replaced fragile map-based calc with `AtomicUsize` counter
- **Verified message_tags and messages tests** - Both working (false alarms in batch run)
- **Test improvement**: +4 LUSERS tests now passing (4→8/9)
- **Architectural pattern established** - Explicit connection lifecycle tracking

### Committed Features (on main/stable)

| Feature | Status | Branch | Notes |
|---------|--------|--------|-------|
| RELAYMSG | ✅ | main | Full handler, labeled-response support |
| READQ enforcement | ✅ | main | Disconnect on >16KB messages |
| Channel +f infrastructure | ✅ | main | Mode parsing done, forwarding logic pending |
| METADATA | ✅ | main | 9/9 tests passing |
| PRECIS casemapping | ✅ | main | Config-driven, UTF-8 support |
| Bouncer/multiclient | ✅ | main | Core wiring complete, polish pending |

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

---

## COMPLETED IMPLEMENTATIONS (STABLE - ON MAIN)

### 1. RELAYMSG Handler ✅
**File**: `src/handlers/messaging/relaymsg.rs`  
**Status**: Functional on main  
**Features**: Labeled-response support, FAIL replies on validation errors, nick override via SenderSnapshot  

### 2. READQ Enforcement ✅
**File**: `src/network/connection/mod.rs` (line ~150)  
**Status**: Implemented on main (commit 667bc55)  
**Feature**: Disconnect clients on messages >16KB per Ergo spec  

### 3. Channel Forwarding Mode (+f) Infrastructure ✅
**File**: `src/handlers/channel_mode.rs`  
**Status**: Mode parsing complete, forwarding logic **PENDING**  
**Commit**: 0212df2 added `ChannelMode::Forward` variant  

### 4. METADATA Handler ✅
**File**: `src/handlers/messaging/metadata.rs`  
**Status**: 9/9 irctest tests passing on main  
**Features**: GET/SET/LIST for users/channels, binary data support, ISUPPORT advertising  

### 5. PRECIS Casemapping ✅
**Files**: `src/config/types.rs`, `src/handlers/connection/nick.rs`, `welcome_burst.rs`  
**Status**: Implemented on main  
**Features**: Config-driven (rfc1459 or precis), UTF-8 nick validation  

### 6. Bouncer/Multiclient ✅
**Files**: Multiple (see BOUNCER_DESIGN.md)  
**Status**: Core wiring on main, polish **PENDING**  
**Features**: Device tracking, channel persistence, nick resumption  

### 7. LUSERS Unregistered Tracking ✅
**File**: `src/state/managers/user.rs`, `src/handlers/server_query/lusers.rs`  
**Status**: Just implemented (commits 7fd9f31, 739da79)  
**Feature**: Explicit `AtomicUsize` counter instead of derived calculation

---

## CRITICAL RULES

### PROTO-FIRST RULE (MANDATORY)
**Never implement daemon workarounds for proto bugs. ALWAYS fix proto first.**
- If a feature requires proto changes: Document blocker in PROTO_REQUIREMENTS.md and STOP
- Never use `Command::Raw` as workaround for missing variants
- Example: RELAYMSG parameter order fixed in proto before daemon handler updated

### ARCHITECTURAL PURITY
- Zero dead code (no comments, orphaned TODOs)
- All TODOs must reference issue numbers
- Handlers follow strict typestate patterns (PreRegHandler, PostRegHandler, UniversalHandler)
- DashMap locks released before `.await` (deadlock prevention)
- Never hold `MessageRef` across `.await` points - extract data first

---

## KNOWN WORKING PATTERNS

### 1. SenderSnapshot Pattern
```rust
// Pre-fetch all sender data once at handler entry
let snapshot = SenderSnapshot::build(ctx).await?;
route_to_channel_with_snapshot(ctx, &channel, msg, &opts, None, None, &snapshot).await
```

### 2. Snapshot Nick Override (RELAYMSG Pattern)
```rust
let mut snapshot = SenderSnapshot::build(ctx).await?;
snapshot.nick = relay_from.to_string();  // Override sender nick
route_to_channel_with_snapshot(..., &snapshot)
```

### 3. Message Construction
```rust
let msg = Message {
    tags: None,
    prefix: Some(Prefix::Nickname(nick, user, host)),
    command: Command::PRIVMSG(target, text),
};
```

### 4. Routing Return Pattern
```rust
// Routing is fire-and-forget (message already delivered)
let _ = route_to_channel_with_snapshot(...).await;
```

---

## KNOWN IRCTEST GAPS (Still Failing)

| Feature | Tests | Status | Next Step |
|---------|-------|--------|-----------|
| Channel forwarding (+f) | 1 | Infrastructure ready, logic missing | Implement forwarding logic in MODE handler |
| Confusables detection | 1 | Not started | Add homoglyph detection, config option |
| Bouncer resumption | 7 | Core wiring done, polish pending | Resume token support, reconnection logic |
| ZNC playback | 1 | Not in scope for 1.0 | Defer to 1.1 |
| NPC/ROLEPLAY | 0 | ✅ Working (Ergo extension) | Complete |
| METADATA | 0 | ✅ Working (9/9 passing) | Complete |

### Verified Working (This Session)
- lusers.py: 8/9 passing (1 environment issue)
- message_tags.py: 2/2 passing
- messages.py: 11/11 passing

---

## TEST INFRASTRUCTURE

### Safe Test Runner (Commit e43f516)
**Location**: `scripts/run_irctest_safe.py` and `scripts/irctest_safe.sh`  
**Features**:
- Memory limits (default 4GB, override with MEM_MAX)
- Process isolation and guaranteed cleanup
- Prevents RAM exhaustion on multi-test runs
- Auto-kill mechanism for hung daemons

### Test Execution
```bash
# Single test
cd slirc-irctest
SLIRCD_BIN=../target/release/slircd \
  pytest --controller=irctest.controllers.slircd \
  irctest/server_tests/metadata.py::MetadataDeprecatedTestCase::testSetGetValid -v

# Full suite (use safe runner!)
MEM_MAX=4G SWAP_MAX=0 KILL_SLIRCD=1 ./scripts/irctest_safe.sh
```

### Labeled-Response Capability
- When client sends `@label=x COMMAND ...`, first response must echo same `@label=x` tag
- Applies to RELAYMSG ACK/FAIL responses
- Applied to batch processing
- RELAYMSG specifically: Labels intentionally NOT forwarded to message recipients (privacy)

---

## REFERENCE & KEY FILES

**Project Rules**: `.github/copilot-instructions.md`  
**Proto Blockers**: `PROTO_REQUIREMENTS.md`  
**Architecture Deep Dive**: `ARCHITECTURE.md`  
**Release Plan**: `ALPHA_RELEASE_PLAN.md`  
**Deployment**: `DEPLOYMENT_CHECKLIST.md`  
**Session Notes**: `TEST_FAILURES_SESSION_REPORT.md`

**Protocol Libraries**:
- `crates/slirc-proto/` - IRC message parsing, Command/Numeric types
- `crates/slirc-crdt/` - Distributed state synchronization (LWW-CRDT)

**Reference Handlers**:
- `src/handlers/messaging/privmsg.rs` - Template pattern for routed messages
- `src/handlers/messaging/relaymsg.rs` - Advanced pattern with nick override
- `src/handlers/messaging/metadata.rs` - User/channel state storage

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

