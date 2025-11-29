# slircd-ng: Implementation Blueprint

> Straylight IRC Daemon — Next Generation
> A modern, multi-threaded IRC server built on zero-copy parsing.

## 1. Executive Summary

**Goal:** Build a high-performance IRC server that:
- Utilizes all CPU cores via Tokio async runtime
- Achieves near-zero allocation in the message hot loop
- Supports future multi-server linking via TS6-style addressing
- Provides NickServ/ChanServ services with SQLite persistence

**Core Dependencies:**
| Crate | Purpose |
|-------|---------|
| `slirc-proto` | Zero-copy IRC parsing, IRCv3, CAP/SASL |
| `tokio` | Async runtime, networking |
| `dashmap` | Lock-free concurrent hash maps |
| `sqlx` | Async SQLite |
| `rustls` | TLS without OpenSSL |
| `tracing` | Structured logging |

---

## 2. Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                         slircd-ng                                │
├─────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐          │
│  │  Gateway    │───▶│ Connection  │───▶│  Handler    │          │
│  │  (TCP/TLS)  │    │  (Tokio     │    │  (Command   │          │
│  │             │    │   Task)     │    │   Dispatch) │          │
│  └─────────────┘    └─────────────┘    └─────────────┘          │
│         │                  │                  │                  │
│         │                  ▼                  ▼                  │
│         │          ┌─────────────────────────────────┐          │
│         │          │         Matrix (Arc)            │          │
│         │          │  ┌─────────┐  ┌─────────────┐   │          │
│         │          │  │ Users   │  │  Channels   │   │          │
│         │          │  │(DashMap)│  │  (DashMap)  │   │          │
│         │          │  └─────────┘  └─────────────┘   │          │
│         │          │  ┌─────────┐  ┌─────────────┐   │          │
│         │          │  │ Nicks   │  │  Servers    │   │          │
│         │          │  │(DashMap)│  │  (DashMap)  │   │          │
│         │          │  └─────────┘  └─────────────┘   │          │
│         │          └─────────────────────────────────┘          │
│         │                          │                            │
│         │                          ▼                            │
│         │                  ┌─────────────┐                      │
│         │                  │   SQLite    │                      │
│         │                  │  (SQLx)     │                      │
│         │                  └─────────────┘                      │
│         │                                                       │
│         ▼                                                       │
│  ┌─────────────────────────────────────────────────────┐        │
│  │                    Router                            │        │
│  │  unicast(uid, msg) / multicast(channel, msg)         │        │
│  │  Determines Local vs Remote, serializes once         │        │
│  └─────────────────────────────────────────────────────┘        │
└─────────────────────────────────────────────────────────────────┘
```

### Data Flow

1. **Accept**: Listener accepts TCP/TLS connection
2. **Handshake**: `Transport` handles CAP, SASL, NICK, USER
3. **Upgrade**: Convert to `ZeroCopyTransport` for hot loop
4. **Loop**: Read `MessageRef` → Dispatch to handler → Mutate state → Route responses
5. **Write**: Responses queued to per-client `mpsc::Sender<Bytes>`

---

## 3. Data Models

### 3.1 Unique Identifiers (TS6)

```rust
/// Server ID: 3 characters, e.g., "001"
pub type Sid = String;

/// User ID: SID + 6 chars, e.g., "001AAAAAB"
pub type Uid = String;

/// Generate next UID for this server
pub struct UidGenerator {
    sid: Sid,
    counter: AtomicU64,
}

impl UidGenerator {
    pub fn next(&self) -> Uid {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        format!("{}{}", self.sid, base36_encode_6(n))
    }
}
```

### 3.2 User Entity

```rust
pub struct User {
    // Identity
    pub uid: Uid,
    pub nick: String,
    pub user: String,           // ident/username
    pub realname: String,
    pub host: String,           // actual hostname
    pub cloak: Option<String>,  // virtual host
    pub ip: IpAddr,
    
    // State
    pub modes: HashSet<UserMode>,
    pub away: Option<String>,
    pub account: Option<String>,  // NickServ account
    pub oper_type: Option<String>,
    
    // Timing
    pub signon: i64,              // Unix timestamp
    pub last_active: Instant,
    
    // IRCv3
    pub caps: HashSet<Capability>,
    
    // Routing
    pub route: Route,
}

pub enum Route {
    Local {
        sink: mpsc::Sender<Bytes>,
    },
    Remote {
        via: Sid,  // Next-hop server
    },
}

pub enum UserMode {
    Invisible,      // +i
    Wallops,        // +w
    Oper,           // +o
    LocalOper,      // +O
    Bot,            // +B
    RegisteredNick, // +r (identified to NickServ)
    SecureConn,     // +Z (TLS)
    ServerNotices,  // +s
    Unknown(char),
}
```

### 3.3 Channel Entity

```rust
pub struct Channel {
    pub name: String,
    pub topic: Option<Topic>,
    pub modes: ChannelModes,
    pub created: i64,
    
    // Membership: UID -> prefix modes (o, v, h, etc.)
    pub members: HashMap<Uid, MemberModes>,
    
    // Lists
    pub bans: Vec<ListEntry>,
    pub excepts: Vec<ListEntry>,
    pub invex: Vec<ListEntry>,
    pub quiets: Vec<ListEntry>,
    
    // ChanServ registration (if any)
    pub registered: Option<ChannelRegistration>,
}

pub struct Topic {
    pub text: String,
    pub set_by: String,
    pub set_at: i64,
}

pub struct ChannelModes {
    // Flags
    pub invite_only: bool,      // +i
    pub moderated: bool,        // +m
    pub no_external: bool,      // +n
    pub secret: bool,           // +s
    pub topic_lock: bool,       // +t
    pub registered_only: bool,  // +r
    
    // Parameters
    pub key: Option<String>,    // +k
    pub limit: Option<u32>,     // +l
}

pub struct MemberModes {
    pub founder: bool,   // +q (if supported)
    pub admin: bool,     // +a
    pub op: bool,        // +o
    pub halfop: bool,    // +h
    pub voice: bool,     // +v
}

pub struct ListEntry {
    pub mask: String,
    pub set_by: String,
    pub set_at: i64,
}
```

### 3.4 Server Entity (for linking)

```rust
pub struct Server {
    pub sid: Sid,
    pub name: String,
    pub description: String,
    pub hop_count: u32,
    pub uplink: Option<Sid>,
    pub link: mpsc::Sender<Bytes>,
}
```

### 3.5 Matrix (Shared State)

```rust
pub struct Matrix {
    // Primary indices
    pub users: DashMap<Uid, Arc<RwLock<User>>>,
    pub channels: DashMap<String, Arc<RwLock<Channel>>>,
    pub servers: DashMap<Sid, Arc<Server>>,
    
    // Secondary indices (for fast lookup)
    pub nicks: DashMap<String, Uid>,  // nick -> uid
    
    // Configuration
    pub config: Arc<RwLock<Config>>,
    
    // This server's identity
    pub me: ServerInfo,
    
    // UID generator
    pub uid_gen: UidGenerator,
}

pub struct ServerInfo {
    pub sid: Sid,
    pub name: String,
    pub network: String,
    pub created: i64,
}
```

---

## 4. Command Handlers

### 4.1 Handler Trait

```rust
pub struct Context<'a> {
    pub uid: &'a Uid,
    pub matrix: &'a Arc<Matrix>,
    pub db: &'a SqlitePool,
}

pub type HandlerResult = Result<Vec<Response>, CommandError>;

#[async_trait]
pub trait Handler: Send + Sync {
    async fn handle(&self, ctx: &Context<'_>, msg: &MessageRef<'_>) -> HandlerResult;
}

pub enum Response {
    /// Send to the command issuer
    Reply(Message),
    /// Send to a specific UID
    SendTo(Uid, Message),
    /// Broadcast to a channel (with optional exclusions)
    Broadcast { channel: String, msg: Message, exclude: Option<Uid> },
    /// Send to all users with a specific mode (e.g., opers)
    WallOps(Message),
}
```

### 4.2 Command Registry

```rust
pub struct Registry {
    handlers: HashMap<&'static str, Box<dyn Handler>>,
}

impl Registry {
    pub fn new() -> Self {
        let mut r = Self { handlers: HashMap::new() };
        
        // Connection
        r.register("NICK", NickHandler);
        r.register("USER", UserHandler);
        r.register("PING", PingHandler);
        r.register("PONG", PongHandler);
        r.register("QUIT", QuitHandler);
        r.register("CAP", CapHandler);
        r.register("AUTHENTICATE", AuthenticateHandler);
        
        // Channels
        r.register("JOIN", JoinHandler);
        r.register("PART", PartHandler);
        r.register("KICK", KickHandler);
        r.register("TOPIC", TopicHandler);
        r.register("NAMES", NamesHandler);
        r.register("LIST", ListHandler);
        r.register("MODE", ModeHandler);
        r.register("INVITE", InviteHandler);
        
        // Messaging
        r.register("PRIVMSG", PrivmsgHandler);
        r.register("NOTICE", NoticeHandler);
        
        // Queries
        r.register("WHO", WhoHandler);
        r.register("WHOIS", WhoisHandler);
        r.register("MOTD", MotdHandler);
        r.register("LUSERS", LusersHandler);
        
        // Oper
        r.register("OPER", OperHandler);
        r.register("KILL", KillHandler);
        r.register("WALLOPS", WallopsHandler);
        r.register("REHASH", RehashHandler);
        
        // Services aliases
        r.register("NICKSERV", NickServAliasHandler);
        r.register("NS", NickServAliasHandler);
        r.register("CHANSERV", ChanServAliasHandler);
        r.register("CS", ChanServAliasHandler);
        
        r
    }
    
    pub async fn dispatch(&self, ctx: &Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let cmd = msg.command_name().to_ascii_uppercase();
        match self.handlers.get(cmd.as_str()) {
            Some(h) => h.handle(ctx, msg).await,
            None => Err(CommandError::UnknownCommand(cmd)),
        }
    }
}
```

### 4.3 Command Implementation Priority

#### Phase 0: Boot (Must have to connect)
| Command | Implementation Notes |
|---------|---------------------|
| `NICK` | Set/change nick, check collision in `matrix.nicks` |
| `USER` | Set ident/realname, trigger welcome burst (001-005) |
| `PING` | Echo back as PONG |
| `PONG` | Reset idle timer |
| `QUIT` | Clean up user from all channels, remove from world |
| `CAP` | Negotiate IRCv3 capabilities |

#### Phase 1: Core IRC
| Command | Implementation Notes |
|---------|---------------------|
| `JOIN` | Add to channel.members, broadcast JOIN to channel |
| `PART` | Remove from channel, broadcast PART |
| `PRIVMSG` | Route to user or channel |
| `NOTICE` | Same as PRIVMSG but no auto-reply allowed |
| `MODE` | User modes and channel modes |
| `TOPIC` | Get/set topic with permission check |
| `KICK` | Remove user from channel (requires +o) |
| `NAMES` | List channel members with prefixes |
| `WHO` | User query |
| `WHOIS` | Detailed user info |
| `MOTD` | Send message of the day |

#### Phase 2: Services
| Command | Implementation Notes |
|---------|---------------------|
| `AUTHENTICATE` | SASL (PLAIN mechanism first) |
| NickServ | REGISTER, IDENTIFY, GHOST, INFO, SET |
| ChanServ | REGISTER, DROP, ACCESS, FLAGS, OP, VOICE, TOPIC |

#### Phase 3: Operator
| Command | Implementation Notes |
|---------|---------------------|
| `OPER` | Authenticate as IRC operator |
| `KILL` | Disconnect user with reason |
| `KLINE` | Ban user@host pattern |
| `WALLOPS` | Send to all +w users |
| `REHASH` | Reload config |

#### Phase 4: Extended
| Command | Implementation Notes |
|---------|---------------------|
| `AWAY` | Set/clear away message |
| `INVITE` | Invite user to +i channel |
| `LIST` | List channels |
| `LUSERS` | Network statistics |
| `MONITOR` | IRCv3 presence tracking |
| `BATCH` | Message batching |

---

## 5. Services Implementation

### 5.1 NickServ

**Database Schema:**
```sql
CREATE TABLE accounts (
    id INTEGER PRIMARY KEY,
    name TEXT UNIQUE NOT NULL COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    email TEXT,
    registered_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    
    -- Settings
    enforce BOOLEAN DEFAULT FALSE,
    hide_email BOOLEAN DEFAULT TRUE
);

CREATE TABLE account_access (
    account_id INTEGER REFERENCES accounts(id) ON DELETE CASCADE,
    fingerprint TEXT,  -- TLS cert fingerprint
    mask TEXT,         -- user@host mask
    PRIMARY KEY (account_id, fingerprint, mask)
);

CREATE TABLE nicknames (
    name TEXT PRIMARY KEY COLLATE NOCASE,
    account_id INTEGER REFERENCES accounts(id) ON DELETE CASCADE
);
```

**Commands:**
| Command | Handler |
|---------|---------|
| `REGISTER <password> [email]` | Create account, hash password, link current nick |
| `IDENTIFY <password>` | Verify password, set user.account, grant +r |
| `GHOST <nick>` | Kill session using your registered nick |
| `INFO <nick>` | Show registration info |
| `SET <option> <value>` | Configure account options |

### 5.2 ChanServ

**Database Schema:**
```sql
CREATE TABLE channels (
    id INTEGER PRIMARY KEY,
    name TEXT UNIQUE NOT NULL COLLATE NOCASE,
    founder_account INTEGER REFERENCES accounts(id),
    registered_at INTEGER NOT NULL,
    last_used_at INTEGER NOT NULL,
    
    -- Settings
    description TEXT,
    mlock TEXT,        -- +nt-l (forced modes)
    keeptopic BOOLEAN DEFAULT TRUE,
    guard BOOLEAN DEFAULT FALSE  -- ChanServ joins channel
);

CREATE TABLE channel_access (
    channel_id INTEGER REFERENCES channels(id) ON DELETE CASCADE,
    account_id INTEGER REFERENCES accounts(id) ON DELETE CASCADE,
    flags TEXT NOT NULL,  -- e.g., "+votsriRfAOF"
    PRIMARY KEY (channel_id, account_id)
);

CREATE TABLE channel_akick (
    channel_id INTEGER REFERENCES channels(id) ON DELETE CASCADE,
    mask TEXT NOT NULL,
    reason TEXT,
    set_by TEXT,
    set_at INTEGER,
    PRIMARY KEY (channel_id, mask)
);
```

**Access Flags:**
| Flag | Meaning |
|------|---------|
| `+v` | Auto-voice |
| `+o` | Auto-op |
| `+t` | Can set topic |
| `+r` | Can RECOVER/SYNC |
| `+s` | Can use SET |
| `+i` | Can use INVITE |
| `+A` | Admin (can modify access) |
| `+F` | Founder (full control) |

### 5.4 Service Architecture

**Pattern:** Services (NickServ/ChanServ) are pure functions that return `Vec<ServiceEffect>`. They do not mutate state directly. The Router applies these effects to the Matrix.

**Design Rationale:**
- Services remain pure decision-making functions with no direct Matrix access
- All state mutations flow through the central Router layer
- Effects can be logged, audited, or replayed for debugging
- Future multi-server environments can replicate effects across network

**ServiceEffect Enum:**
```rust
pub enum ServiceEffect {
    /// Send a reply to the requesting user
    Reply { target_uid: Uid, message: Message },
    
    /// Terminate a user's connection (GHOST, enforcement)
    Kill { target_uid: Uid, reason: String },
    
    /// Apply mode changes (auto-op, +r on identify)
    Mode { target: ModeTarget, modes: Vec<Mode> },
    
    /// Broadcast account identification (account-notify)
    AccountIdentify { target_uid: Uid, account: String },
    
    /// Broadcast account logout (account-notify)
    AccountClear { target_uid: Uid },
}
```

**Service Handler Flow:**
1. User sends: `PRIVMSG NickServ :IDENTIFY password`
2. NickServ handler receives message, validates credentials
3. Returns `Vec<ServiceEffect>` with Reply + AccountIdentify + Mode(+r)
4. Router applies effects:
   - Updates `user.account` in Matrix
   - Sets `user.modes.registered = true`
   - Broadcasts `ACCOUNT accountname` to shared channels (account-notify)
   - Sends `MODE nick +r` to all channel members
5. User sees confirmation message

**Benefits:**
- Zero direct coupling between services and Matrix
- Testable: Mock effects, verify logic without database
- Auditable: Log all service actions for security analysis
- Network-ready: Effects can be serialized for server-to-server sync

---

## 6. Network Protocol (Router)

### 6.1 Local Delivery

```rust
impl Router {
    /// Send to a single user (by UID)
    pub async fn unicast(&self, uid: &Uid, msg: &Message) {
        if let Some(user) = self.matrix.users.get(uid) {
            let user = user.read().await;
            match &user.route {
                Route::Local { sink } => {
                    let bytes = msg.to_string().into_bytes();
                    let _ = sink.send(Bytes::from(bytes)).await;
                }
                Route::Remote { via } => {
                    self.forward_to_server(via, msg).await;
                }
            }
        }
    }
    
    /// Multicast to channel members
    pub async fn broadcast(&self, channel: &str, msg: &Message, exclude: Option<&Uid>) {
        if let Some(chan) = self.matrix.channels.get(channel) {
            let chan = chan.read().await;
            
            // Collect unique next-hops for remote users
            let mut remote_servers: HashSet<Sid> = HashSet::new();
            
            for member_uid in chan.members.keys() {
                if exclude.map_or(false, |e| e == member_uid) {
                    continue;
                }
                
                if let Some(user) = self.matrix.users.get(member_uid) {
                    let user = user.read().await;
                    match &user.route {
                        Route::Local { sink } => {
                            let bytes = msg.to_string().into_bytes();
                            let _ = sink.send(Bytes::from(bytes)).await;
                        }
                        Route::Remote { via } => {
                            remote_servers.insert(via.clone());
                        }
                    }
                }
            }
            
            // Send ONE copy to each interested server
            for sid in remote_servers {
                self.forward_to_server(&sid, msg).await;
            }
        }
    }
}
```

### 6.2 Server-to-Server (Future)

When linking is implemented:
- `PASS`, `CAPAB`, `SERVER` for handshake
- `UID` to introduce users
- `SJOIN` to sync channel membership
- `PRIVMSG`/`NOTICE` routed by target

---

## 7. Directory Structure

```
slircd-ng/
├── Cargo.toml
├── config.toml              # Default config
├── migrations/              # SQLx migrations
│   └── 001_init.sql
├── src/
│   ├── main.rs              # Entry point
│   ├── config.rs            # Config parsing
│   ├── server.rs            # Server lifecycle
│   │
│   ├── state/
│   │   ├── mod.rs
│   │   ├── matrix.rs        # The shared Matrix
│   │   ├── user.rs          # User entity
│   │   ├── channel.rs       # Channel entity
│   │   ├── server.rs        # Server entity (linking)
│   │   └── uid.rs           # UID generation
│   │
│   ├── network/
│   │   ├── mod.rs
│   │   ├── gateway.rs       # TCP/TLS listener
│   │   ├── connection.rs    # Client connection loop
│   │   └── handshake.rs     # CAP/NICK/USER flow
│   │
│   ├── router/
│   │   ├── mod.rs
│   │   └── delivery.rs      # unicast/multicast
│   │
│   ├── handlers/
│   │   ├── mod.rs           # Registry
│   │   ├── connection.rs    # NICK, USER, PING, PONG, QUIT
│   │   ├── channel.rs       # JOIN, PART, KICK, TOPIC, NAMES
│   │   ├── messaging.rs     # PRIVMSG, NOTICE
│   │   ├── mode.rs          # MODE (complex)
│   │   ├── query.rs         # WHO, WHOIS, MOTD, LUSERS
│   │   ├── oper.rs          # OPER, KILL, WALLOPS
│   │   └── cap.rs           # CAP, AUTHENTICATE
│   │
│   ├── services/
│   │   ├── mod.rs
│   │   ├── nickserv.rs
│   │   └── chanserv.rs
│   │
│   └── db/
│       ├── mod.rs
│       ├── accounts.rs      # NickServ queries
│       └── channels.rs      # ChanServ queries
```

---

## 8. Cargo.toml

```toml
[package]
name = "slircd-ng"
version = "0.1.0"
edition = "2024"
license = "Unlicense"
description = "Straylight IRC Daemon - Next Generation"

[[bin]]
name = "slircd"
path = "src/main.rs"

[dependencies]
# Protocol core
slirc-proto = { path = "../slirc-proto", features = ["tokio"] }

# Async runtime
tokio = { version = "1.42", features = ["full"] }
futures = "0.3"
bytes = "1.9"

# Concurrent state
dashmap = "6.1"
parking_lot = "0.12"

# Database
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite"] }

# TLS
tokio-rustls = "0.26"
rustls-pemfile = "2.1"

# Crypto (password hashing)
argon2 = "0.5"
rand = "0.8"
base64 = "0.22"

# Config
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"

# Observability
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# Error handling
thiserror = "2.0"
anyhow = "1.0"

# Time
chrono = { version = "0.4", features = ["serde"] }

[dev-dependencies]
tokio-test = "0.4"
```

---

## 9. Implementation Phases

### Phase 0: Skeleton (Week 1)
- [ ] Initialize project with Cargo.toml
- [ ] Implement `Config` struct and TOML loading
- [ ] Implement `Matrix` with empty DashMaps
- [ ] Implement `Gateway` (TCP listener)
- [ ] Implement `Connection` shell (accept, spawn task, read loop)
- [ ] Wire up tracing

**Milestone:** Server starts, accepts TCP, logs connections

### Phase 1: Handshake (Week 2)
- [ ] Implement `NICK` handler (nick validation, collision check)
- [ ] Implement `USER` handler (complete registration)
- [ ] Send welcome burst (001-005, MOTD)
- [ ] Implement `PING`/`PONG`
- [ ] Implement `QUIT`
- [ ] Upgrade to `ZeroCopyTransport` after registration

**Milestone:** Connect with irssi/weechat, see welcome, ping/pong works

### Phase 2: Core Messaging (Week 3)
- [ ] Implement `PRIVMSG` (user-to-user)
- [ ] Implement `JOIN` (create channel, add member)
- [ ] Implement `PART`
- [ ] Implement `PRIVMSG` (to channel)
- [ ] Implement `TOPIC`
- [ ] Implement `NAMES`
- [ ] Implement `KICK`
- [ ] Implement `Router.broadcast()`

**Milestone:** Two users can join #test and chat

### Phase 3: Modes (Week 4)
- [ ] Implement user mode parsing (`MODE nick +i`)
- [ ] Implement channel mode parsing (`MODE #chan +o nick`)
- [ ] Implement ban list (+b)
- [ ] Implement +k, +l, +i, +m, +n, +s, +t

**Milestone:** Ops can set modes, bans work

### Phase 4: Services Foundation (Week 5)
- [ ] Set up SQLx with migrations
- [ ] Implement `CAP` handler (multi-line, REQ/ACK)
- [ ] Implement `AUTHENTICATE` (SASL PLAIN)
- [ ] Implement NickServ REGISTER/IDENTIFY
- [ ] Auto-grant +r on identify

**Milestone:** Users can register nicks, login persists across restart

### Phase 5: ChanServ (Week 6)
- [ ] Implement channel registration
- [ ] Implement ACCESS list
- [ ] Auto-op on join for registered users
- [ ] Implement AKICK

**Milestone:** Registered channels have persistent access control

### Phase 6: Operators (Week 7)
- [ ] Implement `OPER` (config-based)
- [ ] Implement `KILL`
- [ ] Implement `KLINE` (with DB persistence)
- [ ] Implement `WALLOPS`
- [ ] Implement `REHASH`

**Milestone:** Opers can manage the network

### Phase 7: Polish (Week 8)
- [ ] Implement `WHO`/`WHOIS`/`WHOWAS`
- [ ] Implement `LIST`
- [ ] Implement `LUSERS`
- [ ] Implement `AWAY`
- [ ] Implement `INVITE`
- [ ] Flood protection / rate limiting
- [ ] TLS support

**Milestone:** Feature parity with basic IRCd

---

## 10. Testing Strategy

### Unit Tests
- Each handler has tests with mock `Context`
- Mode parser edge cases
- UID generation

### Integration Tests
- Spawn server, connect real client, verify behavior
- Multi-client scenarios (channel broadcast)

### Fuzzing
- Feed random bytes to `MessageRef::parse()` (already done in slirc-proto)
- Feed malformed commands to handlers

---

## 11. slirc-proto API Reference

### Core Types

| Type | Description |
|------|-------------|
| `Message` | Owned IRC message with `tags`, `prefix`, `command` fields |
| `MessageRef<'a>` | Zero-copy borrowed message from buffer |
| `Command` | Enum of all IRC commands with typed parameters |
| `CommandRef<'a>` | Borrowed command reference |

### Transport Layer

```rust
// Framed transport for handshake phase
pub enum Transport {
    Tcp { ... },
    Tls { ... },
    WebSocket { ... },
    WebSocketTls { ... },
}

impl Transport {
    async fn read_message(&mut self) -> Result<Option<Message>, TransportReadError>;
    async fn write_message(&mut self, message: &Message) -> Result<()>;
    fn is_tls(&self) -> bool;
    fn is_websocket(&self) -> bool;
}

// Zero-copy transport for hot loop
pub struct ZeroCopyTransport<S> { ... }

impl<S: AsyncRead + Unpin> ZeroCopyTransport<S> {
    async fn next(&mut self) -> Option<Result<MessageRef<'_>, TransportReadError>>;
}

// Conversion: Transport -> ZeroCopyTransportEnum via TryInto
// Note: WebSocket transports cannot convert
```

### Key Utilities

```rust
// Case mapping (RFC 1459 style)
fn irc_to_lower(s: &str) -> String;
fn irc_eq(a: &str, b: &str) -> bool;

// Channel validation
trait ChannelExt {
    fn is_channel_name(&self) -> bool;
}

// ISUPPORT parsing
pub struct Isupport<'a> { ... }
impl Isupport<'_> {
    fn casemapping(&self) -> Option<&str>;
    fn prefix(&self) -> Option<PrefixSpec<'_>>;
    fn chanmodes(&self) -> Option<ChanModes<'_>>;
}

// IRCv3 helpers
fn generate_msgid() -> String;
fn format_server_time(time: DateTime<Utc>) -> String;
```

---

## 12. Open Questions

1. **WebSocket Support**: Do we need WS from day one, or add later?
   - *Recommendation:* Phase 8, after core is stable

2. **Server Linking Protocol**: TS6 vs custom?
   - *Recommendation:* TS6-compatible for interop with existing networks

3. **IPv6**: Full support from start?
   - *Recommendation:* Yes, Tokio handles it transparently

4. **Cloaking Algorithm**: Use same as UnrealIRCd or custom?
   - *Recommendation:* Simple HMAC-based cloak, document algorithm

---

## 13. Gotchas & Notes

1. **Zero-copy lifetime management**: `MessageRef` borrows from the transport buffer. Process immediately or call `.to_owned()`.

2. **Transport upgrade pattern**: Use `Transport` for handshake, convert to `ZeroCopyTransport` for hot loop. WebSocket transports cannot be converted.

3. **IRC case insensitivity**: Use `irc_to_lower()` and `irc_eq()` for nick/channel comparisons.

4. **Mode parameter ordering**: Arguments appear in order of mode letters. `+ov nick1 nick2` means `+o nick1` and `+v nick2`.

5. **Message length**: 512 bytes total (RFC), modern servers allow 8191. Tags don't count toward 512 limit.

6. **Flood protection**: Implement rate limiting per-user. Typical: 5 messages/2 seconds, then 1/second.

7. **TS6 UID format**: 9 characters = 3-char SID + 6-char client ID (base36).

8. **Nick collision handling**: Prefer older timestamp. Kill newer user or both if timestamps equal.
