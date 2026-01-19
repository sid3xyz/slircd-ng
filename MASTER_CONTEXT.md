# MASTER_CONTEXT.md
> **Single Source of Truth** for slircd-ng architecture, systems, and current state.
> Updated: 2026-01-19 17:25 | Pre-release | Zero users
> Last Session: Phase 4 cleanup (dnsbl removed), dispatch.rs extraction

---

## 1. Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              slircd-ng                                       │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │   Network   │  │   Handlers  │  │    State    │  │      Services       │ │
│  │  (Gateway)  │──│  (Command   │──│  (Matrix +  │──│  (NickServ/ChanServ)│ │
│  │             │  │   Routing)  │  │   Managers) │  │                     │ │
│  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────────────┘ │
│        │                │                │                    │             │
│        ▼                ▼                ▼                    ▼             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │   Security  │  │    Sync     │  │   Database  │  │      History        │ │
│  │ (RBL/Spam)  │  │   (S2S)     │  │  (SQLx+Redb)│  │  (Message Storage)  │ │
│  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Core Systems

### 2.1 Network Layer (`src/network/`)
| Component | Purpose |
|-----------|---------|
| `gateway.rs` | TCP/TLS listener, accepts connections |
| `connection/` | Per-connection lifecycle (registration + event loop) |
| `event_loop.rs` | **MONOLITH** - 480 lines, needs pipeline refactor |

### 2.2 State Layer (`src/state/`)
| Component | Purpose |
|-----------|---------|
| `matrix.rs` | Central DI container + user disconnect logic |
| `managers/` | User, Channel, Stats, Client managers |
| `actor/` | Channel actors (async message handlers) |
| `persistence.rs` | Channel state save/restore |

> **MONOLITH**: `matrix.rs::disconnect_user_session()` is 195 lines.

### 2.3 Handlers (`src/handlers/`)
Organized by domain:
- `cap/` - IRCv3 capability negotiation (SASL, etc.)
- `channel/` - JOIN, PART, MODE, KICK, TOPIC
- `user/` - NICK, USER, WHOIS, MODE
- `messaging/` - PRIVMSG, NOTICE, NPC, SCENE
- `server/` - S2S commands, KILL, STATS
- `op/` - OPER, REHASH, DIE

### 2.4 Services (`src/services/`)
| Service | Purpose |
|---------|---------|
| `nickserv.rs` | Account management (REGISTER, IDENTIFY) |
| `chanserv.rs` | Channel management (OP, AKICK) |
| `effect.rs` | Unified effect application (state mutations) |
| `playback.rs` | ZNC-compatible history replay |

### 2.5 Security (`src/security/`)
| Module | Purpose | Status |
|--------|---------|--------|
| `rbl.rs` | HTTP-based blocklists | ✅ Active |
| `spam.rs` | Multi-layer spam detection | ✅ Active |
| `cloaking.rs` | HMAC-SHA256 hostname cloaking | ✅ Active |
| `rate_limit.rs` | Governor-based flood protection | ✅ Active |
| `password.rs` | Argon2 hashing | ✅ Active |
| ~~`dnsbl.rs`~~ | ~~DNS blocklists~~ | ❌ DELETED |

### 2.6 Sync (`src/sync/`)
Server-to-server linking (TS6-compatible):
- `handshake.rs` - S2S handshake state machine
- `topology.rs` - Network graph
- `routing.rs` - Message routing
- `crdt.rs` - Conflict-free channel state merging

### 2.7 Database (`src/db/`)
Dual-engine persistence:
- **SQLx** (SQLite): Accounts, channel registrations, bans
- **Redb**: High-speed history storage

---

## 3. IRCv3 Capabilities

| Capability | Status | Notes |
|------------|--------|-------|
| `account-notify` | ✅ | |
| `away-notify` | ✅ | |
| `batch` | ✅ | |
| `cap-notify` | ✅ | |
| `chghost` | ✅ | |
| `echo-message` | ✅ | |
| `extended-join` | ✅ | |
| `extended-monitor` | ✅ | |
| `invite-notify` | ✅ | |
| `labeled-response` | ✅ | |
| `message-tags` | ✅ | |
| `multi-prefix` | ✅ | |
| `sasl` (PLAIN, EXTERNAL, SCRAM-SHA-256) | ✅ | |
| `server-time` | ✅ | |
| `setname` | ✅ | |
| `standard-replies` | ✅ | FAIL/WARN/NOTE |
| `userhost-in-names` | ✅ | |
| `draft/chathistory` | ✅ | |
| `draft/event-playback` | ✅ | |
| `slirc.chat/bouncer` | ✅ | Multi-session support |

---

## 4. Known Monoliths (Refactoring Targets)

| File | Function | Lines | Plan |
|------|----------|-------|------|
| `event_loop.rs` | `run_event_loop()` | 412 | Pipeline: Read→Decode→Dispatch→Respond |
| `matrix.rs` | `disconnect_user_session()` | 195 | State machine: PENDING→NOTIFIED→REMOVED |
| `main.rs` | Task spawning | ~300 | Extract `LifecycleManager` |

---

## 5. Current Roadmap Status

| Phase | Name | Status |
|-------|------|--------|
| 1 | Production Visibility | ✅ Complete |
| 2 | IRCv3.3 Compliance | ✅ Complete |
| 3 | Data Safety | ✅ Complete |
| 4 | Configuration Mastery | ✅ Complete |
| 5 | Ecosystem (S2S, External Auth) | 🔄 In Progress |
| 6 | Advanced Protection | ✅ Complete |
| 7 | Next-Gen Architecture | 📋 Planned |

---

## 6. Conventions

### Naming
- `Uid`: User identifier (e.g., `001AAAAAA`)
- `SessionId`: Bouncer session identifier
- `ServerId`: 3-digit server ID (e.g., `001`)

### Error Handling
- Handlers return `Result<(), HandlerError>`
- Services return `Vec<ServiceEffect>` (deferred application)
- Panics are forbidden in production paths

### Concurrency
- `DashMap` for concurrent collections
- `RwLock` for individual user/channel state
- Channel actors for channel-specific operations

---

## 7. Dependencies (Key Crates)

| Crate | Purpose |
|-------|---------|
| `tokio` | Async runtime |
| `slirc-proto` | IRC protocol parsing |
| `sqlx` | Database (SQLite) |
| `redb` | Embedded KV store |
| `argon2` | Password hashing |
| `rustls` | TLS |
| `governor` | Rate limiting |
| `tracing` | Structured logging |
| `prometheus` | Metrics |
