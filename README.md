# slircd-ng

IRC server daemon implementing typestate protocol and actor model.

**Status**: Research prototype testing AI-driven development. Not production ready.

## Features

- **82 IRC commands** (6 universal, 4 pre-registration, 72 post-registration)
- **Typestate protocol** enforces registration requirements at compile time
- **Actor model** for channels — each channel runs in its own Tokio task
- **IRCv3 capabilities** — SASL, CAP, message-tags, account-tag, chathistory
- **Services** — NickServ (10 commands) and ChanServ (12 commands) built-in
- **Multi-layer security** — DNSBL, reputation, heuristics, spam detection, X-lines
- **TLS and WebSocket** support for modern clients
- **Prometheus metrics** — `/metrics` endpoint for observability
- **CHATHISTORY** — Redb-backed message history with IRCv3 playback

## Architecture

Verified from `src/`:

### Typestate Protocol (`src/state/session.rs`, `src/handlers/core/`)

- **Session types**: `UnregisteredState`, `RegisteredState` with guaranteed fields
- **Handler traits**: `PreRegHandler`, `PostRegHandler`, `UniversalHandler<S>`
- **Registry dispatch**: Phase-specific maps in `src/handlers/core/registry.rs`
  - `pre_reg_handlers`: USER, PASS, WEBIRC, AUTHENTICATE
  - `post_reg_handlers`: PRIVMSG, JOIN, KICK, MODE, WHO, WHOIS, etc.
  - `universal_handlers`: QUIT, PING, PONG, NICK, CAP, REGISTER
- **Connection lifecycle**: `src/network/connection/mod.rs` uses typestate transition

### Actor Model (`src/state/actor/`)

- **ChannelActor**: Isolated task per channel, owns all channel state
- **Message passing**: `mpsc::Sender<ChannelEvent>` for channel operations
- **Event types**: JOIN, PART, KICK, MODE, PRIVMSG, TOPIC, bans, invites

### Services (`src/services/`)

- **NickServ**: REGISTER, IDENTIFY, GHOST, INFO, SET, DROP, GROUP, UNGROUP, CERT
- **ChanServ**: REGISTER, ACCESS, INFO, SET, DROP, OP, DEOP, VOICE, DEVOICE, AKICK, CLEAR
- **Enforcement**: Background task enforces nick ownership, AKICK on JOIN

### Security (`src/security/`)

- **DNSBL**: Async DNS blocklist checks at connection time
- **Reputation**: Persistent trust scores (0-100) in SQLite
- **Heuristics**: Velocity, fan-out, repetition detection
- **Spam detection**: Keyword matching, entropy analysis, URL detection
- **X-lines**: K/G/D/Z/R-lines with persistent storage and expiry
- **Cloaking**: HMAC-SHA256 based IP/hostname privacy
- **Rate limiting**: Governor-based token bucket for flood protection

### Observability (`src/telemetry.rs`, `src/metrics.rs`, `src/http.rs`)

- **Structured tracing**: `IrcTraceContext` with command, channel, msgid, uid
- **Prometheus metrics**: Command latency, message throughput, security events
- **HTTP endpoint**: Axum server on configurable port (default 9090)

### Capabilities (`src/caps/`)

- **Unforgeable tokens**: `Cap<T>` proves authorization
- **Authority**: `CapabilityAuthority` mints tokens based on permissions
- **Properties**: `!Clone`, `!Copy`, scoped to resources

### Persistence (`src/db/`, `migrations/`)

- **SQLite**: Via sqlx for async access
- **Repositories**: Accounts, channels, bans, X-lines
- **Migrations**: 7 files (init, shuns, xlines, history, certfp, topics, reputation)

### History (`src/history/`)

- **Provider trait**: Pluggable backends (Redb, NoOp)
- **CHATHISTORY**: IRCv3 message playback with BEFORE/AFTER/AROUND/LATEST
- **Retention**: Configurable per-event-type storage

### Concurrent State (`src/state/matrix.rs`)

- **DashMap**: Shard-based locking for users, channels, nicks
- **Safety**: Never call mutating methods while holding references (deadlock risk)
- **Disconnect worker**: Channel actors request disconnects via unbounded channel

## Configuration

See `config.toml` for all options. Key sections:

- `[server]` — Name, network, SID, admin info, idle timeouts
- `[listen]` / `[tls]` / `[websocket]` — Network bindings
- `[oper]` — Operator blocks with bcrypt password support
- `[webirc]` — Trusted gateway configuration
- `[security]` — Cloaking, spam detection, rate limits
- `[history]` — Backend selection and retention
- `[motd]` — Message of the day

## Build

```bash
cargo build
cargo test
cargo clippy -- -D warnings
```

## Run

```bash
cargo run -- config.toml
```

Default config creates SQLite database and listens on `0.0.0.0:6667`.

## Background Tasks

The server spawns several background tasks:

- **Nick enforcement** — Enforces registered nick ownership
- **WHOWAS cleanup** — Hourly cleanup of old entries
- **Shun expiry** — Minute-by-minute expiry of timed shuns
- **Disconnect worker** — Processes disconnects requested by channel actors

## License

Unlicense
