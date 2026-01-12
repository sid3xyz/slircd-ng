---
goal: "Implement Better-Than-Ergo Bouncer Architecture for slircd-ng 2.0"
version: 1.0
date_created: 2026-01-12
last_updated: 2026-01-12
owner: slircd-ng Core Team
status: 'Planned'
tags: [feature, architecture, bouncer, 2.0, multiclient, persistence]
---

# Introduction

![Status: Planned](https://img.shields.io/badge/status-Planned-blue)

This implementation plan defines a comprehensive bouncer architecture for slircd-ng that **exceeds Ergo's capabilities** by leveraging slircd-ng's unique advantages:

1. **CRDT-based state sync** - Distributed conflict-free state across servers
2. **Redb embedded database** - Zero-config persistence, no MySQL required
3. **Actor model channels** - Per-channel isolation for efficient message routing
4. **Rust memory safety** - Aggressive caching and zero-copy efficiency
5. **Federation-first design** - Multi-server session handoff (unique to slircd-ng)

The bouncer feature is targeted for **slircd-ng 2.0** as it requires fundamental architectural changes to how connections and sessions are managed.

### Key Terminology

| Term | Definition |
|------|------------|
| **Account** | Authenticated identity (NickServ account) |
| **Session** | Individual network connection (socket + capabilities) |
| **Client** | Logical user identity, may have 0+ sessions attached |
| **Device ID** | Unique identifier for a device/client (from SASL username) |
| **Always-On** | Client persists in channels even with 0 sessions |
| **Read Marker** | Per-target, per-device cursor for "last read" position |

---

## 1. Requirements & Constraints

### Core Requirements

- **REQ-001**: Multiple concurrent sessions per account (multiclient)
- **REQ-002**: Session attachment via SASL authentication
- **REQ-003**: Always-on presence (0 sessions = still in channels)
- **REQ-004**: Per-device missed message tracking
- **REQ-005**: CHATHISTORY-based replay of missed messages
- **REQ-006**: ZNC playback module compatibility
- **REQ-007**: Cross-server session migration (unique to slircd-ng)
- **REQ-008**: CRDT-based read marker sync (unique to slircd-ng)
- **REQ-009**: Message encryption at rest (optional, exceeds Ergo)

### Security Requirements

- **SEC-001**: Device IDs must be cryptographically bound to accounts
- **SEC-002**: Session limits per account (max 64 devices, configurable)
- **SEC-003**: Read markers must not leak to other accounts
- **SEC-004**: Always-on expiration to prevent zombie accounts

### Constraints

- **CON-001**: Must not break existing single-session behavior
- **CON-002**: Must work with existing SASL infrastructure
- **CON-003**: Must integrate with existing CHATHISTORY implementation
- **CON-004**: Must use Redb for persistence (not MySQL)
- **CON-005**: Must not require external message queue services
- **CON-006**: Proto-first: Any new commands/numerics must be added to slirc-proto first

### Guidelines

- **GUD-001**: Follow typestate pattern for session state transitions
- **GUD-002**: Use actor model for Client state (like channel actors)
- **GUD-003**: Prefer CRDT types for cross-server state
- **GUD-004**: Zero-copy message routing where possible
- **GUD-005**: Explicit device ID extraction from SASL credentials

### Patterns

- **PAT-001**: Session ↔ Client relationship mirrors Ergo's Session ↔ Client
- **PAT-002**: Use `LwwRegister<T>` for scalar state (away, nick)
- **PAT-003**: Use `AwSet<T>` for set state (channels, devices)
- **PAT-004**: Per-client actor task (like per-channel actor)

---

## 2. Implementation Steps

### Phase 1: Foundation - Session/Client Separation (MVP Multiclient)

- GOAL-001: Separate transport-level Session from logical Client identity to enable multiple connections per account

| Task | Description | Completed | Date |
|------|-------------|-----------|------|
| TASK-001 | **Proto**: Add `BOUNCER` command to slirc-proto with subcommands: `LISTDEVICES`, `DEVICE`, `DELDEVICE` | | |
| TASK-002 | **Proto**: Add numeric `RPL_BOUNCERDEVICE` (800), `RPL_BOUNCERDEVICESEND` (801), `ERR_DEVICELIMIT` (900) | | |
| TASK-003 | Create `src/state/client.rs` with `Client` struct (account-level state: nick, channels, modes, devices) | | |
| TASK-004 | Create `src/state/session_v2.rs` with `Session` struct (socket, caps, device_id, send queue) | | |
| TASK-005 | Add `ClientManager` to Matrix (`src/state/managers/client.rs`) | | |
| TASK-006 | Implement `ClientActor` task pattern in `src/state/actor/client.rs` | | |
| TASK-007 | Modify SASL authentication to extract device ID from username (`alice@phone` → device=phone) | | |
| TASK-008 | Implement session attachment: SASL success attaches session to existing Client if same account | | |
| TASK-009 | Implement `NickServ SESSIONS` command to list active sessions | | |
| TASK-010 | Add config option `accounts.multiclient.enabled` (default: false for 1.x compatibility) | | |
| TASK-011 | Implement session message fanout: messages to Client are sent to all attached Sessions | | |
| TASK-012 | Handle session detach: when socket closes, detach session but keep Client alive | | |

#### Session/Client State Separation Design

```
┌─────────────────────────────────────────────────────────────┐
│                         Client                               │
│ (Account-level, persists across connections)                 │
│                                                              │
│  • account_name: String                                      │
│  • nick: String                                              │
│  • user: String                                              │
│  • realname: String                                          │
│  • visible_host: String                                      │
│  • channels: HashSet<String>                                 │
│  • modes: UserModes                                          │
│  • away: Option<String>                                      │
│  • devices: HashMap<DeviceId, DeviceInfo>                    │
│  • last_seen: HashMap<DeviceId, Timestamp>                   │
│  • always_on: bool                                           │
│  • created_at: Timestamp                                     │
│                                                              │
│  ┌──────────────────────────────────────────────────────┐   │
│  │ Sessions (attached transports)                        │   │
│  │                                                       │   │
│  │  Session 1 (phone)      Session 2 (laptop)           │   │
│  │  ├─ socket              ├─ socket                    │   │
│  │  ├─ capabilities        ├─ capabilities              │   │
│  │  ├─ device_id           ├─ device_id                 │   │
│  │  ├─ send_tx             ├─ send_tx                   │   │
│  │  └─ last_active         └─ last_active               │   │
│  └──────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

---

### Phase 2: Persistence - Always-On Clients

- GOAL-002: Enable clients to remain present in channels with zero attached sessions

| Task | Description | Completed | Date |
|------|-------------|-----------|------|
| TASK-013 | Create Redb table `always_on_clients` with schema: account, nick, channels, modes, realname | | |
| TASK-014 | Create Redb table `device_state` with schema: account, device_id, last_seen, caps, read_markers | | |
| TASK-015 | Add migration `008_bouncer_state.sql` for SQLite (account settings) | | |
| TASK-016 | Implement `AddAlwaysOnClient()` - create virtual client from database on startup | | |
| TASK-017 | Implement `PersistClient()` - save client state to Redb when last session detaches | | |
| TASK-018 | Add config option `accounts.multiclient.always-on` (default: false) | | |
| TASK-019 | Add config option `accounts.multiclient.always-on-expiration` (default: 90 days) | | |
| TASK-020 | Implement expiration reaper task: remove always-on clients not seen in N days | | |
| TASK-021 | Implement auto-away: set AWAY message when all sessions detach | | |
| TASK-022 | Implement session restore: when first session attaches, restore from persistence | | |
| TASK-023 | Add `NickServ SET ALWAYS-ON {ON|OFF}` command | | |
| TASK-024 | Modify quit handling: no QUIT broadcast when sessions=0 if always-on | | |
| TASK-025 | Modify join/part: work with virtual (no-session) clients | | |

#### Persistence Schema Design

```
Redb Tables:
┌─────────────────────────────────────────────────────────────┐
│ Table: always_on_clients                                     │
│ Key: account_name (String)                                   │
│ Value: AlwaysOnState (CBOR)                                 │
│   • nick: String                                            │
│   • user: String                                            │
│   • realname: String                                        │
│   • visible_host: String                                    │
│   • channels: Vec<(String, ChannelMemberModes)>             │
│   • user_modes: String                                      │
│   • away: Option<String>                                    │
│   • last_quit_at: i64                                       │
│   • crdt_timestamp: HybridTimestamp                         │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│ Table: device_state                                          │
│ Key: (account_name, device_id) (String, String)             │
│ Value: DeviceState (CBOR)                                   │
│   • caps: Vec<String>                                       │
│   • last_seen: i64                                          │
│   • read_markers: Vec<ReadMarker>                           │
│   • push_endpoint: Option<String>                           │
│   • crdt_timestamp: HybridTimestamp                         │
└─────────────────────────────────────────────────────────────┘
```

---

### Phase 3: History Playback - Better Than Ergo

- GOAL-003: Implement per-device missed message replay that exceeds Ergo's capabilities

| Task | Description | Completed | Date |
|------|-------------|-----------|------|
| TASK-026 | Add `read_markers` table to Redb: (account, device_id, target) → (msgid, timestamp) | | |
| TASK-027 | **Proto**: Add `MARKREAD` command: `MARKREAD <target> [timestamp=<time>]` | | |
| TASK-028 | **Proto**: Add `RPL_MARKREAD` (802) and `RPL_MARKREADEND` (803) numerics | | |
| TASK-029 | Implement read marker storage: update on every message sent by device | | |
| TASK-030 | Implement `autoreplayMissedSince`: replay messages since device's last read marker | | |
| TASK-031 | Implement CHATHISTORY `AFTER` with device-specific start point | | |
| TASK-032 | Add ZNC playback module emulation (`znc.in.playback`) | | |
| TASK-033 | Implement ZNC `*playback` commands: `PLAY <target> [timestamp]` | | |
| TASK-034 | Add config option `history.auto-replay-on-join` (default: true) | | |
| TASK-035 | Add config option `history.max-auto-replay` (default: 100 messages) | | |
| TASK-036 | Implement `draft/read-marker` IRCv3 capability | | |
| TASK-037 | CRDT-ify read markers: `ReadMarkerCrdt` with LWW semantics per target | | |
| TASK-038 | Test: verify missed message count is correct across reconnects | | |

#### Read Marker CRDT Design (Unique Advantage)

```rust
/// CRDT-enabled read markers for distributed sync.
/// 
/// Unlike Ergo's simple HashMap (max 256 entries), slircd-ng uses:
/// - LWW semantics per (account, target) pair
/// - No arbitrary limit on targets tracked
/// - CRDT sync across federated servers
pub struct ReadMarkerCrdt {
    /// Per-target read position.
    /// Key: lowercase target name (channel or nick)
    /// Value: (msgid, timestamp) with LWW semantics
    markers: HashMap<String, LwwRegister<ReadPosition>>,
    
    /// Device-specific markers for multi-device sync.
    /// Key: (device_id, target)
    device_markers: HashMap<(String, String), LwwRegister<ReadPosition>>,
}

pub struct ReadPosition {
    /// Last read message ID.
    pub msgid: String,
    /// Timestamp of last read message.
    pub timestamp: i64,
    /// Hybrid timestamp for CRDT ordering.
    pub crdt_ts: HybridTimestamp,
}
```

**Advantages over Ergo:**
- No 256 marker limit (HashMap scales with targets)
- Per-device markers for accurate multi-device sync
- CRDT merge for cross-server consistency
- Federated read marker sync (unique to slircd-ng)

---

### Phase 4: Distributed Bouncer - Unique to slircd-ng

- GOAL-004: Enable session migration and state sync across federated servers (Ergo cannot do this)

| Task | Description | Completed | Date |
|------|-------------|-----------|------|
| TASK-039 | Create `ClientCrdt` in slirc-crdt: CRDT wrapper for Client state | | |
| TASK-040 | Add CRDT sync messages: `BNCSTATE <account> <serialized_crdt>` | | |
| TASK-041 | Implement cross-server session handoff protocol | | |
| TASK-042 | Add `sync_manager` methods for bouncer state: `broadcast_client_state()` | | |
| TASK-043 | Implement history aggregation: merge history from multiple servers | | |
| TASK-044 | Add federated read marker sync: replicate markers to all linked servers | | |
| TASK-045 | Implement session migration: move session from server A to server B | | |
| TASK-046 | Add `BOUNCER MIGRATE <target-server>` command for session migration | | |
| TASK-047 | Test: verify client state survives server restart | | |
| TASK-048 | Test: verify cross-server message delivery to all sessions | | |

#### Distributed Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    slircd-ng Cluster                         │
│                                                              │
│  ┌──────────────┐  CRDT Sync   ┌──────────────┐             │
│  │  Server A    │◄────────────►│  Server B    │             │
│  │              │              │              │             │
│  │  Client:     │              │  Client:     │             │
│  │  alice (ao)  │              │  alice (ao)  │ ← replicated│
│  │              │              │              │             │
│  │  Sessions:   │              │  Sessions:   │             │
│  │  • phone     │              │  • laptop    │             │
│  │  • tablet    │              │              │             │
│  └──────────────┘              └──────────────┘             │
│                                                              │
│  Messages to alice are delivered to ALL sessions            │
│  on ALL servers via CRDT-aware routing                      │
└─────────────────────────────────────────────────────────────┘

Session Migration Flow:
1. User connects to Server B with account "alice"
2. Server B queries CRDT state for "alice"
3. Server B attaches new session to existing Client state
4. Messages start routing to new session immediately
5. History replay uses federated CHATHISTORY aggregation
```

---

### Phase 5: Enhanced Features - Exceeding Ergo

- GOAL-005: Implement advanced features that Ergo doesn't have

| Task | Description | Completed | Date |
|------|-------------|-----------|------|
| TASK-049 | Add push notification support (webpush spec) | | |
| TASK-050 | **Proto**: Add `WEBPUSH` command for push endpoint registration | | |
| TASK-051 | Implement per-device certificate authentication (certfp bound to device) | | |
| TASK-052 | Add message encryption at rest (AES-256-GCM with account-derived key) | | |
| TASK-053 | Implement rich device management UI via NickServ | | |
| TASK-054 | Add `BOUNCER LISTDEVICES` with detailed device info | | |
| TASK-055 | Add `BOUNCER DELDEVICE <device-id>` to revoke a device | | |
| TASK-056 | Add `BOUNCER RENAME <old-device> <new-device>` | | |
| TASK-057 | Implement device activity notifications (notify when new device added) | | |
| TASK-058 | Add session priority: prefer "primary" device for nick enforcement | | |
| TASK-059 | Add per-device notification settings (mute/unmute) | | |
| TASK-060 | Implement client-side session list (`BOUNCER SESSIONS`) | | |

#### Push Notification Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                  Push Notification Flow                      │
│                                                              │
│  1. Client registers push endpoint:                         │
│     WEBPUSH REGISTER <endpoint> <p256dh> <auth>             │
│                                                              │
│  2. Server stores in device_state:                          │
│     push_endpoint: Some("https://push.example.com/...")     │
│     push_keys: { p256dh, auth }                             │
│                                                              │
│  3. When message arrives and device is offline:             │
│     - Encrypt message summary with device keys              │
│     - POST to push_endpoint                                 │
│                                                              │
│  4. Mobile client receives push:                            │
│     - Decrypt summary                                       │
│     - Show notification                                     │
│     - User taps → app connects → CHATHISTORY replay         │
└─────────────────────────────────────────────────────────────┘
```

---

## 3. Alternatives

- **ALT-001**: Store bouncer state in SQLite instead of Redb
  - Rejected: Redb provides embedded key-value semantics better suited for CRDT state
  - SQLite already used for accounts; mixing storage concerns

- **ALT-002**: Use Redis for session state
  - Rejected: Adds external dependency, slircd-ng philosophy is zero-config embedded

- **ALT-003**: Implement bouncer as external proxy (like ZNC)
  - Rejected: Misses unique CRDT/federation advantage, worse UX

- **ALT-004**: Use Ergo's exact data model
  - Rejected: We can do better with CRDTs and federation

- **ALT-005**: Simple "last session wins" instead of multiclient
  - Rejected: Ergo already has multiclient, we must match or exceed

---

## 4. Dependencies

- **DEP-001**: slirc-proto (add BOUNCER, MARKREAD, WEBPUSH commands)
- **DEP-002**: slirc-crdt (add ClientCrdt, ReadMarkerCrdt types)
- **DEP-003**: redb >= 2.0 (already in use for history)
- **DEP-004**: ciborium (CBOR serialization for Redb values)
- **DEP-005**: web-push crate (for push notifications in Phase 5)
- **DEP-006**: aes-gcm crate (for encryption at rest in Phase 5)

---

## 5. Files

### New Files

- **FILE-001**: `src/state/client.rs` - Client struct (account-level state)
- **FILE-002**: `src/state/session_v2.rs` - Session struct (transport-level state)
- **FILE-003**: `src/state/managers/client.rs` - ClientManager implementation
- **FILE-004**: `src/state/actor/client.rs` - ClientActor task pattern
- **FILE-005**: `src/bouncer/mod.rs` - Bouncer module root
- **FILE-006**: `src/bouncer/persistence.rs` - Redb persistence layer
- **FILE-007**: `src/bouncer/read_markers.rs` - Read marker implementation
- **FILE-008**: `src/bouncer/replay.rs` - History replay logic
- **FILE-009**: `src/bouncer/znc.rs` - ZNC playback compatibility
- **FILE-010**: `src/bouncer/device.rs` - Device management
- **FILE-011**: `src/handlers/bouncer/mod.rs` - BOUNCER command handlers
- **FILE-012**: `src/handlers/bouncer/listdevices.rs` - LISTDEVICES subcommand
- **FILE-013**: `src/handlers/bouncer/markread.rs` - MARKREAD command handler
- **FILE-014**: `crates/slirc-crdt/src/client.rs` - ClientCrdt CRDT
- **FILE-015**: `crates/slirc-crdt/src/read_marker.rs` - ReadMarkerCrdt
- **FILE-016**: `crates/slirc-proto/src/command/bouncer.rs` - BOUNCER command types
- **FILE-017**: `migrations/008_bouncer_state.sql` - Account bouncer settings

### Modified Files

- **FILE-018**: `src/state/matrix.rs` - Add ClientManager, bouncer config
- **FILE-019**: `src/state/mod.rs` - Export new types
- **FILE-020**: `src/state/session.rs` - Extract Session from RegisteredState
- **FILE-021**: `src/handlers/cap/sasl.rs` - Device ID extraction from SASL
- **FILE-022**: `src/handlers/connection/quit.rs` - Always-on handling
- **FILE-023**: `src/handlers/chathistory/queries.rs` - Device-specific replay
- **FILE-024**: `src/network/connection/mod.rs` - Session lifecycle changes
- **FILE-025**: `src/services/nickserv/commands.rs` - Add SESSIONS, SET ALWAYS-ON
- **FILE-026**: `crates/slirc-proto/src/command/types.rs` - Add BOUNCER, MARKREAD
- **FILE-027**: `crates/slirc-proto/src/response/types.rs` - Add bouncer numerics
- **FILE-028**: `src/config/mod.rs` - Add bouncer configuration types
- **FILE-029**: `crates/slirc-crdt/src/lib.rs` - Export new CRDT types

---

## 6. Testing

### Unit Tests

- **TEST-001**: Client creation and session attachment
- **TEST-002**: Multiple sessions on same Client
- **TEST-003**: Session detach preserves Client state
- **TEST-004**: Always-on client persistence to Redb
- **TEST-005**: Always-on client restoration on startup
- **TEST-006**: Device ID extraction from SASL username
- **TEST-007**: Read marker updates and queries
- **TEST-008**: Read marker CRDT merge (concurrent updates)
- **TEST-009**: ZNC playback command parsing
- **TEST-010**: BOUNCER command parsing (all subcommands)

### Integration Tests

- **TEST-011**: `tests/bouncer_multiclient.rs` - Multiple connections same account
- **TEST-012**: `tests/bouncer_always_on.rs` - Disconnect/reconnect cycle
- **TEST-013**: `tests/bouncer_replay.rs` - Missed message replay
- **TEST-014**: `tests/bouncer_federation.rs` - Cross-server session
- **TEST-015**: `tests/bouncer_read_markers.rs` - Read marker sync

### irctest Compliance

- **TEST-016**: Pass all 7 bouncer-related irctest failures (currently failing)
- **TEST-017**: Multiclient session listing
- **TEST-018**: ZNC playback compatibility
- **TEST-019**: CHATHISTORY device-specific replay

---

## 7. Risks & Assumptions

### Risks

- **RISK-001**: Major refactor of session/connection architecture
  - Mitigation: Feature flag for gradual rollout, keep 1.x behavior as default

- **RISK-002**: Performance impact of per-client actor tasks
  - Mitigation: Benchmark before/after, optimize hot paths

- **RISK-003**: CRDT state size growth for always-on clients
  - Mitigation: Expiration policy, state compaction

- **RISK-004**: Breaking change for clients expecting single-session behavior
  - Mitigation: Opt-in via `multiclient.enabled` config

- **RISK-005**: ZNC compatibility may be incomplete
  - Mitigation: Test with real ZNC clients, document limitations

### Assumptions

- **ASSUMPTION-001**: Redb can handle expected state size (<100MB per server)
- **ASSUMPTION-002**: CRDT sync overhead is acceptable (<1% of message traffic)
- **ASSUMPTION-003**: Device ID extraction from SASL username is acceptable UX
- **ASSUMPTION-004**: Push notifications are optional/nice-to-have for 2.0
- **ASSUMPTION-005**: Ergo's ZNC implementation is the compatibility target

---

## 8. Related Specifications / Further Reading

### IRC Specifications
- [IRCv3 CHATHISTORY](https://ircv3.net/specs/extensions/chathistory)
- [IRCv3 draft/read-marker](https://github.com/ircv3/ircv3-specifications/pull/489)
- [ZNC Playback Module](https://wiki.znc.in/Playback)

### Ergo Implementation
- [Ergo client.go](https://github.com/ergochat/ergo/blob/master/irc/client.go) - Client/Session model
- [Ergo accounts.go](https://github.com/ergochat/ergo/blob/master/irc/accounts.go) - Always-on
- [Ergo znc.go](https://github.com/ergochat/ergo/blob/master/irc/znc.go) - ZNC compatibility

### slircd-ng Documentation
- [ARCHITECTURE.md](../ARCHITECTURE.md) - Current architecture
- [COMPETITIVE_ANALYSIS.md](../COMPETITIVE_ANALYSIS.md) - Ergo comparison
- [ALPHA_RELEASE_PLAN.md](../ALPHA_RELEASE_PLAN.md) - 2.0 deferral decision

### CRDT References
- [CRDTs: An Update](https://crdt.tech/) - CRDT theory
- [slirc-crdt crate](../crates/slirc-crdt/) - Existing CRDT implementation

---

## Appendix A: Effort Estimates

| Phase | Effort (Hours) | Dependencies |
|-------|----------------|--------------|
| Phase 1: Session/Client Separation | 80 | Proto changes |
| Phase 2: Always-On Persistence | 60 | Phase 1 |
| Phase 3: History Playback | 40 | Phase 1, 2 |
| Phase 4: Distributed Bouncer | 80 | Phase 1, 2, 3 |
| Phase 5: Enhanced Features | 60 | Phase 1, 2 |
| **Total** | **320 hours** | |

---

## Appendix B: Configuration Schema

```toml
# config.toml additions for bouncer features

[accounts.multiclient]
# Enable multiple concurrent connections per account
enabled = true

# Allow clients to stay present with 0 sessions
always-on = true

# Maximum devices per account
max-devices = 64

# How long to keep always-on clients (0 = forever)
always-on-expiration = "90d"

# Auto-away message when all sessions disconnect
auto-away-message = "Client reconnecting"

[history]
# Automatically replay missed messages on channel join
auto-replay-on-join = true

# Maximum messages to auto-replay
max-auto-replay = 100

# Enable read marker tracking
read-markers = true

# Enable ZNC playback compatibility
znc-playback = true

[push]
# Enable push notifications (Phase 5)
enabled = false

# VAPID public key for web push
vapid-public-key = ""
vapid-private-key = ""

[encryption]
# Encrypt messages at rest (Phase 5)
messages-at-rest = false
```

---

## Appendix C: Proto Requirements Summary

### slirc-proto Command Additions

```rust
// New commands needed in slirc-proto

/// BOUNCER command with subcommands
enum BouncerSubCommand {
    LISTDEVICES,
    DEVICE { device_id: String },
    DELDEVICE { device_id: String },
    RENAME { old_id: String, new_id: String },
    MIGRATE { target_server: String },
    SESSIONS,
}

/// MARKREAD command for read marker updates
/// MARKREAD <target> [timestamp=<time>]
Command::MARKREAD { target: String, timestamp: Option<String> }

/// WEBPUSH command for push notification registration (Phase 5)
Command::WEBPUSH { subcommand: WebPushSubCommand }
```

### slirc-proto Numeric Additions

```rust
// New numerics needed in slirc-proto

RPL_BOUNCERDEVICE = 800,     // :server 800 nick device_id last_seen :device_info
RPL_BOUNCERDEVICESEND = 801, // :server 801 nick :End of device list
RPL_MARKREAD = 802,          // :server 802 nick target msgid :timestamp
RPL_MARKREADEND = 803,       // :server 803 nick :End of read markers
ERR_DEVICELIMIT = 900,       // :server 900 nick :Device limit reached
ERR_UNKNOWNDEVICE = 901,     // :server 901 nick device_id :Unknown device
ERR_NOTSUPPORTED = 902,      // :server 902 nick :Multiclient not enabled
```
