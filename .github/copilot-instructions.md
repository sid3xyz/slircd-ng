# Copilot Instructions for slircd-ng

> Generated from source code audit on 2026-02-10.

## Project Overview

slircd-ng is a high-performance IRC daemon (ircd) written in Rust (edition 2024). Binary name: `slircd`. Version: 1.0.0-rc.1.

- **Runtime**: Tokio async
- **Protocol library**: `crates/slirc-proto/` (zero-copy parsing, ~100 IRC command variants, CRDT types)
- **Architecture**: Central `Matrix` state container → per-channel actor isolation → typestate handler dispatch
- **Database**: SQLite (sqlx, async) for accounts/bans/channels + Redb for message history/always-on state
- **Tests**: 26 integration test files, 91+ tests, all passing

## Critical Invariants

### Lock Ordering
```
DashMap shard → Channel RwLock → User RwLock
```
**NEVER reverse. NEVER hold across `.await`.**

Safe patterns:
- Read-only iteration: iterate DashMap, acquire read locks inside
- Collect-then-mutate: collect to Vec, release DashMap, then mutate
- Lock-copy-release: acquire lock, copy data, release before next operation

### Case Folding
Use `slirc_proto::irc_to_lower()` for ALL IRC name comparisons. **Never use `.to_lowercase()`** — IRC case folding maps `[]{}|~` differently.

### Channel Actor Isolation
Each channel is an isolated Tokio task processing `ChannelEvent` via mpsc. Never access channel state directly — send events through the channel's `mpsc::Sender<ChannelEvent>`. The actor uses `im::HashMap` for members (persistent/snapshot-safe).

### Service Effect Pattern
Services (NickServ, ChanServ) return `Vec<ServiceEffect>` — they NEVER mutate state directly. Callers apply effects via `apply_effects()`. This enables S2S forwarding and testability.

### Capability Token Authorization
Privileged operations require `Cap<T>` tokens from `CapabilityAuthority`. `Cap<T>` is non-Clone, non-Copy with `pub(super)` constructor. Don't bypass this with direct permission checks.

### No `unsafe`
`unsafe_code = "forbid"` is set workspace-wide. Do not use unsafe.

## Code Architecture

### Central State: Matrix (`src/state/matrix.rs`)
All managers are public fields on `Arc<Matrix>`:
- `user_manager`, `channel_manager`, `client_manager`, `security_manager`
- `service_manager`, `monitor_manager`, `lifecycle_manager`, `sync_manager`
- `stats_manager`, `read_marker_manager`
- `server_info`, `config`, `hot_config`, `db`

### Handler System (`src/handlers/`)
141 files, 17 directories, ~95+ IRC commands.

Four handler traits (typestate dispatch):
- `PreRegHandler` — Before NICK+USER (USER, PASS, WEBIRC, STARTTLS)
- `PostRegHandler` — After registration (PRIVMSG, JOIN, MODE, etc.)
- `UniversalHandler<S>` — Any state (CAP, NICK, PING, PONG, QUIT)
- `ServerHandler` — S2S protocol (UID, SID, SJOIN, TMODE, etc.)

Handlers receive `Context` with `matrix: Arc<Matrix>`, `uid: &str`, `session_state`, `db`.
Responses go through `ResponseMiddleware` (supports BATCH/label wrapping).

### Key Types
- `Uid` = `String` (TS6: 3-char SID + 6-char base36)
- `SessionId` = `u64` (unique per connection)
- `Message` = owned IRC message, `MessageRef<'_>` = zero-copy borrowed
- `Command` = ~100 variant enum, `CommandRef<'_>` = zero-copy variant
- `ChannelEvent` = 23-variant enum sent to channel actors

### Module Layout
| Module | Purpose |
|--------|---------|
| `src/config/` | TOML config loading, validation |
| `src/state/` | Matrix, managers, actors, user/channel models |
| `src/handlers/` | IRC command handlers (141 files) |
| `src/network/` | Gateway (TCP), Connection (per-conn task) |
| `src/security/` | Rate limiting, spam, cloaking, bans, IP deny |
| `src/services/` | NickServ, ChanServ, Playback |
| `src/sync/` | S2S linking, CRDT propagation, netsplit |
| `src/history/` | Message history (Redb backend) |
| `src/caps/` | Capability token authorization |
| `src/db/` | SQLite + Redb persistence |

## Coding Conventions

### Adding a New IRC Command Handler
1. Create handler file in appropriate `src/handlers/` subdirectory
2. Implement `PreRegHandler`, `PostRegHandler`, or `UniversalHandler<S>` trait
3. Register in the subdirectory's `mod.rs` (which inserts into Registry)
4. Add `Command` variant in `crates/slirc-proto/src/command.rs` if new
5. Add parsing in `crates/slirc-proto/src/parser.rs` if new

### Adding a Service Command
1. Add handler in `src/services/nickserv/` or `src/services/chanserv/`
2. Return `Vec<ServiceEffect>` — never mutate state
3. Effects are applied by `apply_effects()` in `src/services/effect.rs`

### Modifying Channel Behavior
1. Add new `ChannelEvent` variant in `src/state/actor/types.rs`
2. Handle it in the actor's event loop in `src/state/actor/`
3. Send events through the channel's `mpsc::Sender<ChannelEvent>`
4. Never access actor state directly from outside

### Database Changes
1. Add new migration file in `migrations/` (next sequential number)
2. Add repository methods in `src/db/` (accounts.rs, bans/, channels/)
3. Migrations run automatically on startup

### Testing
- Integration tests in `tests/` use a `TestServer` helper from `tests/common/`
- Each test spins up a real server instance with `:memory:` database
- Rate limits are relaxed in test config (`config.test.toml`)
- Run: `cargo test --test <name>`

## Build Commands
```bash
cargo build --release                    # Build optimized binary
cargo test                               # All tests
cargo test --test channel_ops            # Specific test file
cargo clippy --all-targets               # Lint
cargo doc --no-deps --document-private-items  # Internal docs
```

## Configuration
- Main config: `config.toml` (production), `config.test.toml` (testing)
- Config supports `include` directives with glob patterns
- Hot-reloadable fields (REHASH): description, MOTD, oper blocks, admin info
- Security: cloak_secret must be strong (16+ chars, 3+ char classes, 8+ unique)

## Dependencies (key)
| Crate | Purpose |
|-------|---------|
| tokio | Async runtime |
| slirc-proto | Protocol library (workspace member) |
| dashmap | Concurrent hash maps |
| sqlx | Async SQLite |
| redb | Embedded key-value store |
| tokio-rustls | TLS |
| argon2 | Password hashing |
| governor | Rate limiting |
| roaring | IP deny bitmap |
| im | Persistent collections (channel actor) |
| metrics + axum | Prometheus metrics |
