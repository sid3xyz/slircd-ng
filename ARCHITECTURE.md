# Architecture - slircd-ng

> A high-performance, distributed IRC daemon written in Rust with modern IRC protocol support and zero-copy message handling.

**Current Version**: 1.0.0-alpha.1  
**Last Updated**: January 15, 2026  
**Status**: Active Development → Beta Planning

---

## System Overview

slircd-ng implements a layered, actor-based architecture optimized for:
- **Concurrency**: Tokio async runtime with per-channel Tokio tasks
- **Safety**: Zero unsafe code, Rust's borrow checker enforces correctness
- **Performance**: Zero-copy message parsing via `MessageRef<'a>` from slirc-proto
- **Distributed**: CRDT-based state synchronization for multi-server linking

### Architecture Diagram

```
┌──────────────────────────────────────────────────────────────┐
│                      Network Layer                           │
│  TcpListener (rustls TLS) → IrcCodec → Transport Frame       │
└──────────────────────────────────────────────────────────────┘
                            ↓
┌──────────────────────────────────────────────────────────────┐
│                   Connection Handler                         │
│  StateMachine(PreReg → PostReg) → Handler Registry           │
└──────────────────────────────────────────────────────────────┘
                            ↓
┌──────────────────────────────────────────────────────────────┐
│                   Matrix (Global State)                      │
│  ├─ UserManager        ├─ ChannelManager (actors)            │
│  ├─ SecurityManager    ├─ ServiceManager                     │
│  ├─ MonitorManager     ├─ LifecycleManager                   │
│  └─ SyncManager ────→ (S2S protocol for linking)             │
└──────────────────────────────────────────────────────────────┘
                            ↓
┌──────────────────────────────────────────────────────────────┐
│               Persistence Layer                              │
│  SQLite (users, bans, history) | Redb (message archive)      │
└──────────────────────────────────────────────────────────────┘
```

---

## Core Design Patterns

### 1. Typestate Handler System

Rust's type system enforces protocol state at compile time:

```rust
// Pre-registration handlers: NICK, USER, PASS, CAP, WEBIRC
pub trait PreRegHandler { ... }

// Post-registration handlers: PRIVMSG, JOIN, MODE, etc.
pub trait PostRegHandler { ... }

// Universal handlers: QUIT, PING, PONG (any state)
pub trait UniversalHandler<S: SessionState> { ... }
```

**Benefit**: No `if !registered` runtime checks; invalid state transitions are compilation errors.

### 2. Actor Model for Channels

Each IRC channel runs as its own Tokio task:

```rust
pub struct ChannelActor {
    name: String,
    members: im::HashMap<Uid, MemberInfo>,  // Persistent data structure
    sender: RwLock<MemberSet>,              // Efficient sender tracking
    topic: Option<Topic>,
    modes: HashSet<ChannelMode>,
    // ... lists (bans, excepts, invites, quiets)
}

pub enum ChannelEvent {
    Join { uid, modes },
    Part { uid, reason },
    Message { from, text },
    Mode { changes },
    Topic { new_topic },
    // ... 20+ event types
}
```

**Benefits**:
- ✅ No global locks on hot path (per-channel bounded queues)
- ✅ Automatic Tokio scheduling across CPU cores
- ✅ Backpressure via bounded channels (capacity 1024)
- ✅ Testable in isolation with mock events

### 3. Zero-Copy Message Handling

Messages are parsed once and borrowed throughout their lifetime:

```rust
pub struct MessageRef<'a> {
    tags: Option<&'a [Tag<'a>]>,
    prefix: Option<&'a str>,
    command: CommandRef<'a>,
    args: &'a [&'a str],
}

// Extracted immediately—MessageRef lifetime ends after handler
async fn handle_privmsg(ctx: &mut Context<'_>, msg: &MessageRef<'_>) {
    let text = msg.arg(2).map(|s| s.to_string());  // Clone only needed data
    // msg dropped here; no lifetime extends across await
}
```

**Benefits**: Minimal allocations, zero copies of message content.

### 4. CRDT-Based Distributed State

Server-to-server linking uses Conflict-free Replicated Data Types (CRDTs) for eventual consistency:

```rust
// Now part of slirc-proto with #[cfg(feature = "sync")]
pub use slirc_proto::sync::{
    clock::{HybridTimestamp, ServerId, VectorClock},
    channel::ChannelCrdt,
    user::UserCrdt,
    traits::{Crdt, LwwRegister, AwSet},
};
```

**Strategies**:
- **Last-Writer-Wins (LWW)**: nick, topic, modes (scalar values)
- **Add-Wins Set (AWSet)**: members, bans, invites (where adds beat removes)
- **Observed-Remove Set (ORSet)**: Alternative for complex collections

**No Coordination**: Changes apply immediately locally; propagate asynchronously to peers.

### 5. Service Effects Pattern

Services (NickServ, ChanServ) return structured effects instead of mutating state:

```rust
pub enum ServiceEffect {
    Reply { target_uid, msg },
    AccountIdentify { target_uid, account },
    Kill { target_uid, killer, reason },
    ChannelMode { channel, target_uid, mode, adding },
    // ...
}

async fn route_service_message(...) -> Vec<ServiceEffect> {
    // Pure function: no mutations, only returns effects
    vec![
        ServiceEffect::Reply { target_uid: uid, msg },
        ServiceEffect::AccountIdentify { target_uid: uid, account },
    ]
}

// Handler applies effects
for effect in effects {
    matrix.apply_effect(effect).await;
}
```

**Benefits**: Testable, composable, easy to audit side effects.

---

## Module Structure

```
src/
├── main.rs                 # Binary entry point, config loading
├── config/                 # Configuration file parsing (TOML)
├── network/
│   └── connection/        # Per-client state machines & message handling
├── handlers/              # IRC command implementations (60+ handlers)
│   ├── messaging/         # PRIVMSG, NOTICE, TAGMSG
│   ├── channels/          # JOIN, PART, MODE, TOPIC, etc.
│   ├── user_query/        # WHO, WHOIS, ISON, etc.
│   ├── server/            # S2S protocol (UID, SID, SJOIN, etc.)
│   └── services/          # NickServ, ChanServ handlers
├── state/                 # Global state management
│   ├── matrix.rs          # Central coordinator (dependency injection)
│   ├── user.rs            # User data structures
│   ├── channel.rs         # Channel data structures
│   ├── session.rs         # Connection session state
│   ├── observer.rs        # CRDT state synchronization
│   ├── managers/          # Domain managers (user, channel, security, etc.)
│   └── actor/             # Channel actor task and event handling
├── db/                    # Database interactions
│   ├── bans/              # Ban/KLINE/DLINE queries
│   ├── history/           # Message history
│   └── migrations/        # SQLx database migrations
├── sync/                  # Server-to-server synchronization
│   ├── mod.rs             # Sync manager and topology
│   ├── observer.rs        # CRDT change notifications
│   ├── split.rs           # Split-horizon algorithm
│   ├── handshake.rs       # Server linking protocol
│   └── topology.rs        # Server mesh management
├── security/              # Security features
│   ├── tls.rs             # TLS/SSL configuration
│   ├── sasl.rs            # SASL authentication handlers
│   └── bans.rs            # Ban enforcement
├── services/              # Service implementations
│   ├── nickserv.rs        # Nickname service
│   ├── chanserv.rs        # Channel service
│   └── helpserv.rs        # Help service
└── history/               # Message history storage
    ├── mod.rs             # History manager
    └── queries.rs         # CHATHISTORY implementation
```

---

## Data Flow

### Incoming Message Flow

```
1. Network → Transport Frame
   └─ TcpListener reads bytes from socket
   └─ IrcCodec parses into Message

2. Message → Handler Registry
   └─ Determine if Pre-Reg / Post-Reg / Universal
   └─ Handler lookup via command

3. Handler Execution
   └─ Acquire locks (user/channel state as needed)
   └─ Validate and process
   └─ Return responses

4. Response → Network
   └─ Responses queued to sender's write channel
   └─ IrcCodec serializes to wire format
   └─ TLS encryption (if enabled)
   └─ Sent to client
```

### Channel Message Flow

```
Client A sends message to channel:
1. PRIVMSG #channel :hello
2. Handler extracts message text
3. ChannelActor event: Message { from_uid, text }
4. Actor broadcasts to all members
5. Each recipient queues response to their sender
6. Sent out in next flush cycle
```

### Server-to-Server Sync

```
Local state change → CRDT update
  ↓
Observer notifies SyncManager
  ↓
SyncManager encodes as SJOIN/TMODE/SJOIN-like messages
  ↓
Messages queued to each peer S2S connection
  ↓
Peer receives and merges CRDT
  ↓
State eventually consistent across all servers
```

---

## Key Innovations

### Innovation 1: Typestate Handlers
Compile-time enforcement of protocol state via trait system eliminates runtime checks.

### Innovation 2: Per-Channel Actors
Each channel is its own task with bounded message queue, eliminating global lock contention.

### Innovation 3: Zero-Copy Parsing
`MessageRef<'a>` borrows from transport buffer; extracted data is cloned, not messages themselves.

### Innovation 4: CRDT-Based Sync
No coordination required for S2S linking; changes apply locally and propagate asynchronously.

### Innovation 5: Service Effects
Service logic is pure and returns effects, decoupling business logic from state mutations.

---

## Error Handling

**Principle**: Use `?` propagation with context; never `unwrap()` in library code.

```rust
// ✅ Good
pub async fn handle_join(...) -> HandlerResult {
    let channel = self.get_or_create_channel(name)?;  // ? propagates
    let mode_chars = mode_str.chars().collect::<Vec<_>>();
    // ...
}

// ❌ Bad
let user = matrix.users.get(uid).unwrap();  // Panics on missing user
```

**Error Types**:
- `HandlerError`: IRC protocol-level errors (ERR_NICKNAMEINUSE, ERR_NOSUCHCHANNEL)
- `CommandError`: Command parsing/validation errors
- `DatabaseError`: SQLx/Redb errors (from `?` operator)

---

## Concurrency Model

### Tokio Async Runtime

slircd-ng uses Tokio's multi-threaded runtime with:
- **Per-core worker threads**: Automatically distributes work
- **Work-stealing scheduler**: Idle threads steal from busy ones
- **Timer precision**: Sub-millisecond timer accuracy for rate limiting

### Lock Discipline

**DashMap Access Pattern**:
```rust
// ✅ Safe: Lock released before async
if let Some(user) = matrix.users.get(&uid) {
    let nick = user.nick.clone();
    // Lock released here
}
// Can now await with nick

// ❌ Unsafe: Lock held across await
let user = matrix.users.get(&uid)?;
some_async_call().await;  // Lock held during IO!
```

**Channel RwLock Pattern**:
```rust
// Multiple concurrent readers
let chan = channel_actor.read().await;  // Sharable

// Exclusive writer
let mut chan = channel_actor.write().await;  // Exclusive
```

---

## Testing Strategy

### Unit Tests (664+)
- Handler logic: Input → Output verification
- State transitions: Valid/invalid state changes
- CRDT convergence: Merge and conflict resolution

### Integration Tests
- Full message flow: Client → Server → Client
- Multi-channel scenarios
- S2S linking behavior
- Persistence (database operations)

### Compliance Testing
- irctest suite: 357/387 tests passing (92.2%)
- Protocol correctness: RFC 1459, RFC 2812, IRCv3

### Load Testing
- Benchmarks: Message throughput, latency
- Memory profiling: Leak detection
- Connection scaling: 1K+ concurrent clients

---

## Performance Characteristics

| Operation | Latency | Memory |
|-----------|---------|--------|
| Message routing | <1ms | O(members) |
| User lookup (nick) | O(1) via HashMap | Small |
| Channel lookup | O(1) via DashMap | Small |
| CRDT merge | O(log n) per entry | Linear in deltas |
| Database write | 1-5ms | Buffered |

---

## Security Model

### Authentication
- ✅ SASL PLAIN, SCRAM-SHA-256
- ✅ CertFP (certificate fingerprint)
- ✅ Account integration

### Authorization
- ✅ Channel modes (+m, +n, +p, +i, etc.)
- ✅ User modes (+o for operators)
- ✅ Ban enforcement (KLINE, DLINE, GLINE, XLINE, SHUN)

### Rate Limiting
- ✅ Per-client message throttling
- ✅ Join/part rate limits
- ✅ Connection rate limits

### Audit
- ✅ Operator action logging
- ✅ Service command tracking
- ✅ Connection/disconnection events

---

## Dependencies

### Core
- `tokio`: Async runtime (1.27+)
- `slirc-proto`: IRC protocol parsing/encoding (feature: `sync`)
- `dashmap`: Concurrent HashMap
- `parking_lot`: Better Mutex/RwLock

### Database
- `sqlx`: Async SQL (SQLite)
- `redb`: Embedded key-value store

### Crypto
- `sha2`, `hmac`, `pbkdf2`: SCRAM hashing
- `tokio-rustls`: TLS/SSL

### Utilities
- `chrono`: Timestamps
- `serde`: Configuration serialization
- `tracing`: Structured logging
- `uuid`: ID generation
- `confusables`: Unicode nick validation

---

## Future Improvements

### Performance
- [ ] Custom memory allocator (mimalloc)
- [ ] SIMD message parsing
- [ ] Parallel batch event processing

### Features
- [ ] Full bouncer mode (session resumption)
- [ ] Multi-line messages (IRCv3.3)
- [ ] Distributed backup/restore

### Operations
- [ ] Prometheus metrics export
- [ ] Structured JSON logging
- [ ] Live configuration hot-swap

---

## References

- [ROADMAP.md](ROADMAP.md) - Release timeline
- [README.md](README.md) - Quick start
- [PROTO_REQUIREMENTS.md](PROTO_REQUIREMENTS.md) - Protocol blockers
- [DEPLOYMENT_CHECKLIST.md](DEPLOYMENT_CHECKLIST.md) - Pre-deployment verification

