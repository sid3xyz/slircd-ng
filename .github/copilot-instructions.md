# Copilot Instructions for slircd-ng

High-performance IRC daemon with zero-copy parsing. Rust 2024 edition. Public domain (Unlicense).

## Quick Reference

```bash
cargo build --release           # Build (use for integration tests)
cargo test                      # Unit + integration tests (664+)
cargo clippy -- -D warnings     # Lint (must pass, zero warnings)
cargo fmt -- --check            # Format check
./scripts/irctest_safe.sh       # Run irctest suite (357/387 passing)
```

## Architecture

### The Matrix (`src/state/matrix.rs`)
Central dependency injection container. All state access flows through `Arc<Matrix>`:
- `user_manager`: Users, nicknames, WHOWAS history, UID generation
- `channel_manager`: Channel actors, message broadcasting
- `client_manager`: Bouncer/multiclient session tracking
- `security_manager`: Bans (KLINE/DLINE/GLINE), rate limiting
- `service_manager`: NickServ, ChanServ, history storage
- `monitor_manager`: IRCv3 MONITOR lists
- `lifecycle_manager`: Shutdown coordination
- `sync_manager`: CRDT-based server linking

### Handler Typestate System (`src/handlers/core/traits.rs`)
Compile-time protocol state enforcement—no runtime registration checks:
- `PreRegHandler` → Commands before registration (NICK, USER, CAP, PASS)
- `PostRegHandler` → Registered-only commands (PRIVMSG, JOIN, MODE)
- `UniversalHandler<S>` → Any state (QUIT, PING, PONG)

```rust
// Context<S> type param determines available state fields
async fn handle(&self, ctx: &mut Context<'_, RegisteredState>, msg: &MessageRef<'_>) -> HandlerResult {
    // ctx.state.nick is String (guaranteed), not Option<String>
}
```

### Channel Actors (`src/state/actor/`)
Each channel runs as isolated Tokio task with bounded mailbox:
- State: members, modes, topic, bans owned by actor
- Communication via `ChannelEvent` enum sent to `mpsc::Sender`
- No RwLock contention on message routing hot path

### Service Effects Pattern (`src/services/mod.rs`)
Services return pure effects, handlers apply them to state:
```rust
pub enum ServiceEffect {
    Reply { target_uid: String, msg: Message },
    AccountIdentify { target_uid: String, account: String },
    Kill { target_uid: String, killer: String, reason: String },
    // ...
}
```

## Critical Code Patterns

```rust
// Zero-copy parsing: extract from MessageRef BEFORE any .await
async fn handle(&self, ctx: &mut Context<'_, S>, msg: &MessageRef<'_>) -> HandlerResult {
    let nick = msg.arg(0).map(|s| s.to_string()); // Clone to owned FIRST
    some_async_operation().await; // Safe now
    Ok(())
}

// IRC case-insensitivity: use slirc-proto utilities, NOT std
use slirc_proto::{irc_to_lower, irc_eq};
let nick_lower = irc_to_lower(&nick);     // ✓ Correct
// nick.to_lowercase()                    // ✗ Wrong (ASCII-only)

// DashMap discipline: short locks, clone before await
if let Some(user_arc) = matrix.user_manager.users.get(&uid) {
    let user = user_arc.read().await;
    let nick = user.nick.clone();
}   // Lock released before any async work
```

## Lock Ordering (Deadlock Prevention)
When acquiring multiple locks, always follow this order:
1. DashMap shard lock (during `.get()` / `.iter()`)
2. Channel `RwLock` (read or write)  
3. User `RwLock` (read or write)

Safe patterns: read-only iteration, collect-then-mutate, lock-copy-release.

## Anti-Patterns (DO NOT)
- `Command::Raw` for known commands → Add variant to `slirc-proto`
- `.unwrap()` in handlers → Use `?` propagation
- `std::to_lowercase()` on IRC strings → Use `irc_to_lower()`
- Holding DashMap locks across `.await` → Clone data first
- Empty `Ok(())` stubs → Use `todo!()` to panic if hit
- Traits with one impl → Use struct directly
- New crates for std-solvable problems → Use std first

## Testing

### Integration Tests (`tests/`)
Spawn actual server, connect clients, verify IRC flows:
```rust
let server = TestServer::spawn(19999).await?;
let mut client = TestClient::connect(&server.address(), "nick").await?;
client.register().await?;
```

### irctest (`slirc-irctest/`)
External IRC compliance suite. Run safely with memory limits:
```bash
./scripts/irctest_safe.sh irctest/server_tests/utf8.py
./scripts/run_irctest_safe.py --discover  # All tests
```

## Protocol Proto-First Rule
If `slirc-proto` is missing a command, enum variant, or has a parsing bug:
1. Fix it in `crates/slirc-proto/` first
2. Never work around proto bugs in the daemon
3. Re-export from proto, don't duplicate types

## Development Mode (Active)
- Working logic over abstraction
- Fast iteration over hardening
- `todo!()` panics over silent failures
- Simplicity over enterprise patterns

## Key Files
- `src/state/matrix.rs`: Central state container
- `src/handlers/core/traits.rs`: Handler trait definitions
- `src/state/actor/mod.rs`: Channel actor model
- `src/services/mod.rs`: ServiceEffect enum and dispatch
- `src/state/session.rs`: Typestate session definitions
- `ROADMAP.md`: Release timeline and known gaps
