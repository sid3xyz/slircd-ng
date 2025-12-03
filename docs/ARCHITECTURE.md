# slircd-ng Architecture

This document describes the internal architecture of slircd-ng.

## Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                            slircd-ng                                    │
├─────────────────────────────────────────────────────────────────────────┤
│  ┌─────────┐  ┌─────────┐  ┌───────────┐                               │
│  │   TCP   │  │   TLS   │  │ WebSocket │   ← Network Listeners         │
│  └────┬────┘  └────┬────┘  └─────┬─────┘                               │
│       │            │             │                                      │
│       └────────────┴─────────────┘                                      │
│                    │                                                    │
│       ┌────────────▼────────────┐                                       │
│       │        Gateway          │   ← Connection Acceptance             │
│       └────────────┬────────────┘                                       │
│                    │                                                    │
│       ┌────────────▼────────────┐                                       │
│       │  Connection (per-user)  │   ← Zero-Copy Transport               │
│       │    ZeroCopyTransport    │                                       │
│       └────────────┬────────────┘                                       │
│                    │                                                    │
│       ┌────────────▼────────────┐                                       │
│       │   Handler Registry      │   ← Command Dispatch                  │
│       │  (JOIN, PRIVMSG, etc.)  │                                       │
│       └────────────┬────────────┘                                       │
│                    │                                                    │
│       ┌────────────▼────────────┐                                       │
│       │       The Matrix        │   ← Shared State (DashMap)            │
│       │  (Users, Channels, etc) │                                       │
│       └─────────────────────────┘                                       │
│                                                                         │
│  ┌───────────┐  ┌───────────┐  ┌──────────────────────┐                │
│  │  NickServ │  │  ChanServ │  │  Security Subsystem  │                │
│  │           │  │           │  │  (Cloaking, Bans,    │                │
│  │           │  │           │  │   Rate Limiting)     │                │
│  └─────┬─────┘  └─────┬─────┘  └──────────┬───────────┘                │
│        │              │                    │                            │
│        └──────────────┴────────────────────┘                            │
│                       │                                                 │
│       ┌───────────────▼───────────────┐                                 │
│       │          Database             │   ← SQLite Persistence          │
│       │  (Accounts, Channels, Bans)   │                                 │
│       └───────────────────────────────┘                                 │
└─────────────────────────────────────────────────────────────────────────┘
```

## Core Components

### The Matrix (`src/state/matrix.rs`)

The Matrix is the central state container. It uses `DashMap` for lock-free concurrent access:

```rust
pub struct Matrix {
    // User state
    pub users: DashMap<Uid, Arc<RwLock<User>>>,
    pub nicks: DashMap<String, Uid>,
    pub senders: DashMap<Uid, mpsc::Sender<Message>>,

    // Channel state
    pub channels: DashMap<String, Arc<RwLock<Channel>>>,

    // Presence monitoring (MONITOR)
    pub monitors: DashMap<Uid, DashSet<String>>,
    pub monitoring: DashMap<String, DashSet<Uid>>,

    // Security
    pub ban_cache: BanCache,
    pub rate_limiter: RateLimitManager,
    pub shuns: DashMap<String, Shun>,

    // History
    pub whowas: DashMap<String, VecDeque<WhowasEntry>>,
}
```

**Why DashMap?**
- Lock-free reads for the common case
- Sharded writes to reduce contention
- Safe concurrent access from multiple tokio tasks

### Zero-Copy Transport

Messages are parsed without allocating memory using `MessageRef<'a>`:

```rust
// From slirc-proto
pub struct MessageRef<'a> {
    pub tags: Option<&'a str>,
    pub prefix: Option<&'a str>,
    pub command: &'a str,
    pub params: &'a str,
}
```

The transport borrows directly from the read buffer:

```
┌─────────────────────────────────────────────┐
│           TCP Read Buffer                   │
│  @time=... :nick!user@host PRIVMSG #ch :hi  │
│  ▲         ▲                ▲           ▲   │
│  │         │                │           │   │
│  tags    prefix          command      params│
└─────────────────────────────────────────────┘
```

### Handler System

Handlers implement the `Handler` trait:

```rust
#[async_trait]
pub trait Handler: Send + Sync {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult;
}
```

The `Registry` dispatches commands to handlers:

```rust
pub struct Registry {
    handlers: HashMap<&'static str, Box<dyn Handler>>,
}

impl Registry {
    pub fn dispatch(&self, command: &str) -> Option<&dyn Handler> {
        self.handlers.get(command.to_uppercase().as_str())
    }
}
```

### Service Effects Pattern

Services (NickServ, ChanServ) don't mutate state directly. They return effects:

```rust
pub enum ServiceEffect {
    Reply { target_uid: String, msg: Message },
    AccountIdentify { target_uid: String, account: String },
    AccountClear { target_uid: String },
    ClearEnforceTimer { target_uid: String },
    Kill { target_uid: String, killer: String, reason: String },
    Kick { channel: String, target_uid: String, kicker: String, reason: String },
    ChannelMode { channel: String, target_uid: String, mode_char: char, adding: bool },
    ChannelModes { channel: String, modes: Vec<Mode<ChannelMode>> },
    ForceNick { target_uid: String, old_nick: String, new_nick: String },
    BroadcastAccount { target_uid: String, new_account: String },
    BroadcastChghost { target_uid: String, new_user: String, new_host: String },
}
```

Effects are applied by the caller:

```rust
let effects = nickserv.handle(matrix, uid, nick, text).await;
apply_effects(matrix, nick, sender, effects).await;
```

**Benefits:**
- Testability: Services are pure functions
- Separation of concerns: Business logic vs. state mutation
- Future server-linking: Effects can be serialized and forwarded

### Connection Lifecycle

```
1. TCP Accept → Gateway spawns Connection task
                     │
2. Handshake Phase   │
   ├── CAP negotiation (IRCv3)
   ├── SASL authentication (optional)
   ├── NICK + USER commands
   └── Complete registration
                     │
3. Main Loop         │
   ├── Read message (zero-copy)
   ├── Dispatch to handler
   ├── Handler processes message
   ├── Handler sends replies
   └── Loop
                     │
4. Disconnect        │
   ├── Remove from Matrix
   ├── Part all channels
   ├── Notify MONITORs
   └── Clean up
```

### Security Subsystem

```
┌─────────────────────────────────────────────────────────────────────┐
│                       Security Module                               │
├──────────┬─────────────┬────────────────┬──────────────┬───────────┤
│ BanCache │  Cloaking   │ Rate Limiting  │ ExtendedBans │   Spam    │
│ DashMap  │ HMAC-SHA256 │   Governor     │ $a:/$r:/$U   │  Entropy  │
│ K/D/G/Z  │ IP+Hostname │ Token Bucket   │ Channel +b   │  URL/Rep  │
└──────────┴─────────────┴────────────────┴──────────────┴───────────┘
```

**BanCache**: In-memory cache of active X-lines for fast connection-time checks.

**Cloaking**: HMAC-SHA256 based IP/hostname masking:
```rust
pub fn cloak_ip_hmac(ip: &str, secret: &str, suffix: &str) -> String {
    // 192.168.1.100 → abc123.def456.ghi789.ip
}
```

**Rate Limiting**: Token bucket algorithm via `governor` crate:
- Per-client message rate
- Per-IP connection rate
- Per-client join rate

**Spam Detection**: Multi-layer content analysis:
- Entropy detection (random strings)
- Pattern matching (spam phrases)
- URL shortener detection
- Character repetition

### Database Layer

SQLite persistence via `sqlx`:

```
┌─────────────────────────────────────────────────────────┐
│                      Database                           │
├─────────────────────────────────────────────────────────┤
│  accounts        │ NickServ accounts                   │
│  nicknames       │ Registered nicks → accounts         │
│  channels        │ ChanServ channels                   │
│  channel_access  │ Access lists                        │
│  channel_akick   │ Auto-kick lists                     │
│  klines/dlines/  │ Server bans                         │
│  glines/zlines   │                                     │
│  message_history │ CHATHISTORY storage                 │
└─────────────────────────────────────────────────────────┘
```

Migrations run automatically on startup from `migrations/`.

### Background Tasks

Spawned in `main.rs`:

| Task                     | Interval  | Description          |
| ------------------------ | --------- | -------------------- |
| `spawn_enforcement_task` | 100ms     | Nick enforcement     |
| WHOWAS cleanup           | 1 hour    | Prune old entries    |
| Shun expiry              | 1 minute  | Remove expired shuns |
| Ban cache prune          | 5 minutes | Remove expired bans  |
| Rate limiter cleanup     | 5 minutes | Clean old buckets    |
| History prune            | 24 hours  | Remove old messages  |

### Metrics

Prometheus metrics via `prometheus` crate:

```rust
lazy_static! {
    pub static ref CONNECTED_USERS: IntGauge = ...;
    pub static ref MESSAGES_SENT: IntCounter = ...;
    pub static ref SPAM_BLOCKED: IntCounter = ...;
    // etc.
}
```

Exposed via HTTP on metrics port (default 9090).

## Data Flow Examples

### PRIVMSG to Channel

```
1. Client sends: PRIVMSG #channel :Hello
2. Connection reads message (zero-copy)
3. Registry dispatches to PrivmsgHandler
4. Handler:
   a. Looks up sender in Matrix.users
   b. Looks up #channel in Matrix.channels
   c. Checks: is sender in channel? is sender +q?
   d. For each member:
      - Get sender from Matrix.senders
      - Send PRIVMSG message
5. If echo-message cap: send copy to sender
```

### NickServ IDENTIFY

```
1. Client sends: PRIVMSG NickServ :IDENTIFY password
2. PrivmsgHandler detects service target
3. Routes to nickserv::route_service_message()
4. NickServ::handle():
   a. Verify password against database
   b. Returns ServiceEffect::AccountIdentify
5. apply_effect():
   a. Sets user.account in Matrix
   b. Sets user.modes.registered = true
   c. Broadcasts MODE +r to user
```

### Connection with Ban Check

```
1. TCP accept in Gateway
2. Check rate limiter (connection_burst_per_ip)
3. Check BanCache:
   - Z-line by IP (no DNS)
   - D-line by IP
4. Create Connection task
5. During handshake, check:
   - G-line by user@host
   - K-line by user@host
   - R-line by realname
6. If any match: send ERROR and close
```

## Thread Safety

- `Matrix` is wrapped in `Arc` for sharing across tasks
- Users/Channels are `Arc<RwLock<T>>` for fine-grained locking
- DashMap provides lock-free reads, sharded writes
- Message senders (`mpsc::Sender`) are cloned per-handler call

## Performance Considerations

1. **Zero-copy parsing**: No allocations in the hot path
2. **DashMap**: Lock-free reads, sharded writes
3. **Lazy ban expiration**: Checked at lookup time, not on a timer
4. **Batched broadcasts**: Channel messages use iterator, not Vec allocation
5. **WHOWAS bounded**: Max entries per nick to prevent unbounded growth
