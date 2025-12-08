# slircd-ng

IRC server daemon implementing typestate protocol and actor model.

**Status**: Research prototype testing AI-driven development. Not production ready.

## Architecture

Verified from `src/`:

### Typestate Protocol (`src/state/session.rs`, `src/handlers/core/`)

- **Session types**: `UnregisteredState`, `RegisteredState` with guaranteed fields
- **Handler traits**: `PreRegHandler`, `PostRegHandler`, `UniversalHandler<S>` in `src/handlers/core/traits.rs`
- **Registry dispatch**: Phase-specific maps in `src/handlers/core/registry.rs`
  - `pre_reg_handlers`: USER, PASS, WEBIRC, AUTHENTICATE
  - `post_reg_handlers`: PRIVMSG, JOIN, etc.
  - `universal_handlers`: QUIT, PING, PONG, NICK, CAP
- **Connection lifecycle**: `src/network/connection/mod.rs` uses typestate transition

### Actor Model (`src/state/actor/`)

- **ChannelActor**: Isolated task per channel, owns all channel state
- **Message passing**: `mpsc::Sender<ChannelEvent>` for channel operations
- **Event types**: JOIN, PART, KICK, MODE, PRIVMSG, etc.

### Concurrent State (`src/state/matrix.rs`)

- **DashMap**: Shard-based locking for users, channels, nicks
- **Safety**: Never call mutating methods while holding references (deadlock risk)

### Observability (`src/telemetry.rs`, `src/metrics.rs`)

- **Structured tracing**: `IrcTraceContext` with command, channel, msgid, uid
- **Prometheus metrics**: Command latency, message throughput, security events
- **HTTP endpoint**: `/metrics` for Prometheus scraping

### Capabilities (`src/caps/`)

- **Unforgeable tokens**: `Cap<T>` proves authorization
- **Authority**: `CapabilityAuthority` mints tokens based on permissions
- **Properties**: `!Clone`, `!Copy`, scoped to resources

### Persistence (`src/db/`, `migrations/`)

- **SQLite**: Via sqlx for async access
- **Repositories**: Accounts, channels, bans, message history
- **Migrations**: 5 files in `migrations/`

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

## License

Unlicense
