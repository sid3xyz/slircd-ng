# slircd-ng

> **Straylight IRC Daemon — Next Generation**
> A high-performance, zero-copy IRC server written in Rust.

![Status](https://img.shields.io/badge/status-active-green)
![License](https://img.shields.io/badge/license-Unlicense-blue)
![Rust](https://img.shields.io/badge/rust-2024_edition-orange)

`slircd-ng` is a modern IRC daemon built on the [Tokio](https://tokio.rs) asynchronous runtime. It uses the `slirc-proto` library for zero-allocation message parsing in the hot path.

## Features

### Core

- **Zero-Copy Parsing**: `MessageRef<'a>` borrows directly from the transport buffer
- **Lock-Free State**: DashMap-based concurrent user/channel storage ("The Matrix")
- **RFC Compliance**: Full RFC 1459/2812 support
- **Prometheus Metrics**: `/metrics` endpoint on configurable port (default: 9090)

### IRCv3 Capabilities

Extensive IRCv3 support including:

| Capability          | Description                             |
| ------------------- | --------------------------------------- |
| `multi-prefix`      | Multiple prefix characters in NAMES/WHO |
| `userhost-in-names` | Full user@host in NAMES replies         |
| `server-time`       | Message timestamps                      |
| `echo-message`      | Echo sent messages back to client       |
| `sasl`              | PLAIN/SCRAM-SHA-256 authentication      |
| `batch`             | Message batching                        |
| `message-tags`      | Arbitrary message metadata              |
| `labeled-response`  | Request/response correlation            |
| `setname`           | Change realname without reconnect       |
| `away-notify`       | AWAY status notifications               |
| `account-notify`    | Account login/logout notifications      |
| `extended-join`     | Account info in JOIN                    |
| `invite-notify`     | INVITE notifications to channel         |
| `chghost`           | Username/hostname change notifications  |
| `monitor`           | Presence monitoring                     |
| `cap-notify`        | Capability change notifications         |
| `chathistory`       | Message history retrieval (draft)       |

### Connectivity

- **TCP**: Standard plaintext connections (default: port 6667)
- **TLS**: Secure connections via tokio-rustls (typically port 6697)
- **WebSocket**: Web client support via tokio-tungstenite
- **WEBIRC**: Trusted gateway IP forwarding

### Integrated Services

Built-in service bots with SQLite persistence:

**NickServ** — Account management
- `REGISTER`, `IDENTIFY`, `GHOST`, `INFO`, `SET`
- Nick enforcement with configurable grace period
- Email verification support

**ChanServ** — Channel management
- `REGISTER`, `OP`, `DEOP`, `VOICE`, `DEVOICE`
- Access lists with flags system
- AKICK (auto-kick) management
- Topic preservation

### Security

| Feature            | Description                                             |
| ------------------ | ------------------------------------------------------- |
| **Host Cloaking**  | HMAC-SHA256 IP/hostname masking                         |
| **Rate Limiting**  | Per-client message/connection/join flood protection     |
| **Spam Detection** | Multi-layer content analysis (entropy, patterns, URLs)  |
| **Ban Cache**      | In-memory K/D/G/Z-line cache for fast connection checks |
| **Extended Bans**  | `$a:account`, `$r:realname`, `$U` (unregistered), etc.  |

### Server Bans (X-Lines)

| Type   | Target                 | Persistence  |
| ------ | ---------------------- | ------------ |
| K-Line | user@host (local)      | SQLite       |
| G-Line | user@host (global)     | SQLite       |
| D-Line | IP address             | SQLite       |
| Z-Line | IP address (no DNS)    | SQLite       |
| R-Line | Realname pattern       | SQLite       |
| SHUN   | Silent ban (in-memory) | Runtime only |

## Architecture

### The Matrix

The core state container (`src/state/matrix.rs`) holds all users, channels, and server state in concurrent DashMap collections:

```rust
pub struct Matrix {
    pub users: DashMap<Uid, Arc<RwLock<User>>>,
    pub channels: DashMap<String, Arc<RwLock<Channel>>>,
    pub nicks: DashMap<String, Uid>,
    pub senders: DashMap<Uid, mpsc::Sender<Message>>,
    pub monitors: DashMap<Uid, DashSet<String>>,
    pub ban_cache: BanCache,
    // ...
}
```

### Service Effects Pattern

Services return `ServiceEffect` vectors instead of mutating state directly:

```rust
pub enum ServiceEffect {
    Reply { target_uid: String, msg: Message },
    AccountIdentify { target_uid: String, account: String },
    Kill { target_uid: String, killer: String, reason: String },
    Kick { channel: String, target_uid: String, ... },
    ChannelMode { channel: String, target_uid: String, ... },
    ForceNick { target_uid: String, old_nick: String, new_nick: String },
    // ...
}
```

This decouples business logic from state management, improving testability.

### Connection Architecture

```
Phase 1: Handshake (ZeroCopyTransport + FramedWrite)
   ↓
Phase 2: Unified Zero-Copy Loop (tokio::select!)
   ┌─────────────────────────────────────────────────────┐
   │              Unified Connection Task                │
   │                                                     │
   │  ┌─────────────────┐       ┌──────────────────┐    │
   │  │ ZeroCopyReader  │       │   FramedWrite    │    │
   │  │   (Borrow)      │       │                  │    │
   │  └────────┬────────┘       └────────▲─────────┘    │
   │           │                         │              │
   │           ▼                         │              │
   │    tokio::select! ──────────────────┼──────────────┤
   │    │                                │              │
   │    ▼                                │              │
   │  [Handlers] ─────────────▶ [Outgoing Queue]        │
   │  (Zero Alloc)                                      │
   └─────────────────────────────────────────────────────┘
```

## Configuration

Configuration via `config.toml`:

```toml
[server]
name = "irc.example.net"
network = "ExampleNet"
sid = "001"
description = "My IRC Server"
metrics_port = 9090

[listen]
address = "0.0.0.0:6667"

# Optional TLS
[tls]
address = "0.0.0.0:6697"
cert_path = "server.crt"
key_path = "server.key"

# Optional WebSocket
[websocket]
address = "0.0.0.0:8080"

[database]
path = "slircd.db"

[security]
cloak_secret = "CHANGE_THIS_IN_PRODUCTION"
cloak_suffix = "ip"
spam_detection_enabled = true

[security.rate_limits]
message_rate_per_second = 2
connection_burst_per_ip = 3
join_burst_per_client = 5

[limits]
rate = 2.5
burst = 5.0

# IRC operators
[[oper]]
name = "admin"
password_hash = "$argon2id$..."
host = "*@trusted.host"
```

## Build & Run

**Requirements**: Rust 2024 edition

```bash
# Build
cargo build -p slircd-ng --release

# Run
cargo run -p slircd-ng -- config.toml

# Or directly
./target/release/slircd config.toml
```

**Environment Variables**:
- `RUST_LOG`: Logging level (default: `info`)

## Project Structure

```
slircd-ng/
├── src/
│   ├── main.rs           # Entry point, background tasks
│   ├── config.rs         # Configuration parsing
│   ├── metrics.rs        # Prometheus metrics
│   ├── http.rs           # Metrics HTTP server
│   ├── db/               # SQLite persistence
│   │   ├── accounts.rs   # NickServ accounts
│   │   ├── channels/     # ChanServ data
│   │   ├── bans/         # X-line persistence
│   │   └── history.rs    # CHATHISTORY storage
│   ├── handlers/         # IRC command handlers
│   │   ├── admin.rs      # SA* commands (SAJOIN, etc.)
│   │   ├── bans/         # X-line commands
│   │   ├── cap.rs        # CAP + SASL
│   │   ├── channel/      # JOIN, PART, KICK, etc.
│   │   ├── chathistory.rs
│   │   ├── messaging/    # PRIVMSG, NOTICE, TAGMSG
│   │   ├── mode/         # MODE handling
│   │   ├── monitor.rs    # MONITOR command
│   │   ├── oper.rs       # OPER, KILL, DIE, etc.
│   │   └── ...
│   ├── network/          # Transport layer
│   │   ├── gateway.rs    # TCP/TLS/WebSocket listeners
│   │   └── connection.rs # Per-client handler
│   ├── security/         # Security subsystem
│   │   ├── ban_cache.rs  # In-memory ban cache
│   │   ├── cloaking.rs   # Host cloaking
│   │   ├── rate_limit.rs # Flood protection
│   │   ├── spam.rs       # Spam detection
│   │   └── xlines.rs     # Extended bans
│   ├── services/         # NickServ/ChanServ
│   │   ├── nickserv/
│   │   ├── chanserv/
│   │   └── enforce.rs    # Nick enforcement
│   └── state/            # Shared state
│       ├── matrix.rs     # The Matrix
│       ├── user.rs       # User state
│       ├── channel.rs    # Channel state
│       └── uid.rs        # UID generation
├── migrations/           # SQLite migrations
├── config.toml           # Example configuration
└── Cargo.toml
```

## Background Tasks

The server runs several background maintenance tasks:

| Task                 | Interval  | Purpose                                   |
| -------------------- | --------- | ----------------------------------------- |
| Nick enforcement     | 100ms     | Force nick changes for unidentified users |
| WHOWAS cleanup       | 1 hour    | Remove entries older than 7 days          |
| Shun expiry          | 1 minute  | Remove expired shuns                      |
| Ban cache prune      | 5 minutes | Remove expired K/D/G/Z-lines              |
| Rate limiter cleanup | 5 minutes | Clean up old rate limit buckets           |
| History prune        | 24 hours  | Remove messages older than 7 days         |

## Channel Modes

| Mode                | Description                     |
| ------------------- | ------------------------------- |
| `+i`                | Invite only                     |
| `+m`                | Moderated                       |
| `+n`                | No external messages            |
| `+s`                | Secret                          |
| `+t`                | Topic lock (ops only)           |
| `+r`                | Registered users only           |
| `+k <key>`          | Channel key                     |
| `+l <limit>`        | User limit                      |
| `+f <lines>:<secs>` | Flood protection                |
| `+L <#channel>`     | Redirect on limit               |
| `+j <joins>:<secs>` | Join throttle                   |
| `+J <secs>`         | Join delay (quiet period)       |
| `+c`                | Strip colors                    |
| `+C`                | No CTCP (except ACTION)         |
| `+N`                | No nick changes                 |
| `+K`                | No KNOCK                        |
| `+V`                | No INVITE                       |
| `+T`                | No channel NOTICE               |
| `+u`                | No kicks (peace mode)           |
| `+P`                | Permanent (persists empty)      |
| `+O`                | Oper-only                       |
| `+g`                | Free INVITE (anyone can invite) |

## User Modes

| Mode | Description             |
| ---- | ----------------------- |
| `+i` | Invisible               |
| `+w` | Receive wallops         |
| `+o` | IRC operator            |
| `+r` | Registered (identified) |
| `+Z` | TLS connection          |
| `+R` | Registered-only PMs     |

## Testing

### Quick Start

```bash
# Start test server with relaxed rate limits
cargo run -p slircd-ng -- config.test.toml

# In another terminal, connect with any IRC client
irssi -c localhost -p 6667
```

### Test Configurations

| Config | Purpose | Rate Limits |
|--------|---------|-------------|
| `config.toml` | Production | Normal (2 msg/s) |
| `config.test.toml` | Manual testing | Relaxed (1000 msg/s) |
| `tests/e2e/test_config.toml` | Automated tests | Unlimited |

### irctest (RFC Compliance)

Test against the official IRC protocol test suite:

```bash
# Install irctest (one-time)
cd /tmp && git clone https://github.com/ergochat/irctest.git
cd irctest && python3 -m venv .venv && .venv/bin/pip install -r requirements.txt

# Start slircd-ng in one terminal
cargo run -p slircd-ng -- tests/e2e/test_config.toml

# Run irctest in another terminal
export IRCTEST_SERVER_HOSTNAME=localhost IRCTEST_SERVER_PORT=6667
cd /tmp/irctest
.venv/bin/pytest --controller irctest.controllers.external_server \
  -k "not deprecated and not strict and not Ergo" -v
```

### E2E Tests (Python)

```bash
cd tests/e2e
python3 -m venv .venv && .venv/bin/pip install -r requirements.txt
.venv/bin/pytest -v
```

### Unit Tests

```bash
cargo test -p slircd-ng
```

## License

This project is released into the public domain under the [Unlicense](LICENSE).
