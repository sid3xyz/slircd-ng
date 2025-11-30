# Copilot Instructions for slircd-ng

> High-performance multi-threaded IRC daemon built on zero-copy parsing.
> Released to the public domain under [The Unlicense](../LICENSE).

---

## Quick Reference

```bash
cargo build --release           # Production build
cargo test                      # Run tests
cargo clippy -- -D warnings     # Lint (must pass)
cargo fmt -- --check            # Format check
./target/release/slircd config.toml  # Run daemon
```

---

## Project Constraints

| Constraint | Requirement |
|------------|-------------|
| MSRV | Rust 1.70+ |
| Linting | `#![deny(clippy::all)]` in lib modules — zero warnings |
| Error handling | Use `?` propagation; avoid `unwrap()`/`expect()` except in `main.rs` |
| Allocation discipline | Zero-copy hot loop using `MessageRef<'a>` from slirc-proto |
| RFC compliance | Strict adherence to RFC 1459, RFC 2812, IRCv3.2 specs |

---

## Architecture

| Component | Pattern |
|-----------|---------|
| Protocol | `slirc-proto` — zero-copy parsing with `MessageRef<'a>` |
| Transport | Tokio async: `TcpListener` + `rustls` for TLS |
| Hot Loop | `tokio::select!` in `network/connection.rs` dispatching to handlers |
| State | `Arc<Matrix>` with `DashMap` for lock-free concurrent access |
| Handlers | `async fn handle(&self, ctx: &Context<'_>, msg: &MessageRef<'_>)` trait |
| Router | `unicast(uid, msg)` / `multicast(channel, msg, exclude)` serialization |
| Services | Pure effect functions: NickServ/ChanServ return `ServiceEffect` vectors |
| Persistence | SQLx + SQLite for accounts, bans, channel registrations |

---

## Development Workflow

### 🛡️ PRIME DIRECTIVE: Protocol-First Development

**Never implement daemon logic before verifying protocol support.**

Before writing code:

1. **Check slirc-proto**: Does the `Command` variant exist? Is the numeric reply defined?
   - ✅ If yes: Proceed to handler implementation
   - ❌ If no: **STOP.** Output: `"🛑 Blocking: slirc-proto needs [Command::X / Numeric::RPL_Y]. Do not hack with Command::Raw."`

2. **Check IMPLEMENTATION.md**: Verify phase alignment and architectural constraints.

3. **Check ARCHITECTURE.md**: Review current refactoring status and design principles.

### Request Processing Template

For every IRC feature request:

1. **Goal Analysis**: Map request to RFC command/numeric requirements.
2. **Protocol Check**: Verify slirc-proto support (Command variant, Numeric enum).
3. **State Design**: Identify Matrix/DashMap access patterns.
4. **Handler Logic**: Write async handler with `Context` and `MessageRef`.
5. **Testing**: Round-trip test (parse → handle → serialize).

---

## Critical Patterns

### 1. Zero-Copy Message Handling

```rust
// MessageRef<'a> borrows from transport buffer
async fn handle(&self, ctx: &Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
    // Extract params immediately or .to_string() if needed beyond this scope
    let nick = msg.params().get(0).map(|s| s.to_string());
    
    // Process synchronously in this function
    Ok(vec![Response::Reply(build_response())])
}
```

**Rule**: Never hold `MessageRef` across `.await` points. Extract needed data first.

### 2. DashMap Lock Discipline

```rust
// ✅ Good: Short lock, secondary index
if let Some(uid) = matrix.nicks.get(&irc_to_lower(&nick)) {
    if let Some(user) = matrix.users.get(&*uid) {
        let user_nick = user.nick.clone();
        // Use user_nick after lock is dropped
    }
}

// ❌ Bad: Holding entry across await
let user = matrix.users.get(&uid).unwrap();
some_async_call().await;  // Lock held during IO!
```

**Rule**: Keep DashMap read locks minimal. Clone needed data before async operations.

### 3. Handler Response Pattern

```rust
pub enum Response {
    Reply(Message),                        // To command issuer
    SendTo(Uid, Message),                  // To specific UID
    Broadcast { channel: String, msg: Message, exclude: Option<Uid> },
    WallOps(Message),                      // To all +w users
}

async fn handle(&self, ctx: &Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
    Ok(vec![
        Response::Reply(numeric_reply),
        Response::Broadcast { 
            channel: chan_name, 
            msg: event, 
            exclude: Some(ctx.uid.clone())
        },
    ])
}
```

**Rule**: Handlers return `Vec<Response>`. Router handles serialization and delivery.

### 4. Service Effect Pattern

```rust
// Services don't mutate Matrix directly — they return effects
pub enum ServiceEffect {
    SetRegistered { uid: String, account: String },
    Kill { uid: String, reason: String },
    ModeChange { target: String, change: ModeChange },
}

// Handler applies effects after service processing
let effects = route_service_message(&ctx, &msg, &service_target).await?;
for effect in effects {
    match effect {
        ServiceEffect::SetRegistered { uid, account } => {
            if let Some(mut user) = matrix.users.get_mut(&uid) {
                user.account = Some(account);
            }
        }
        // ... apply other effects
    }
}
```

**Rule**: Services are pure. Handlers apply their effects to Matrix.

### 5. IRC Case-Insensitivity

```rust
use slirc_proto::{irc_to_lower, irc_eq};

// ✅ Correct
let nick_lower = irc_to_lower(&nick);
matrix.nicks.get(&nick_lower);

if irc_eq(&chan1, &chan2) { /* ... */ }

// ❌ Wrong
nick.to_lowercase();  // Doesn't handle RFC1459 {}|~ chars!
```

**Rule**: Always use `slirc_proto` case utilities for IRC string comparison.

---

## Anti-Patterns

| Anti-Pattern | Why | Fix |
|--------------|-----|-----|
| `Command::Raw` for known commands | Bypasses type safety | Add variant to slirc-proto |
| Hardcoded numeric strings | Brittle, error-prone | Use `Numeric::RPL_*` enums |
| `.unwrap()` in handler code | Panics on error | Use `?` or `.ok_or(CommandError::X)?` |
| `std::to_lowercase()` on IRC strings | Incorrect `{}|~` handling | Use `irc_to_lower()` |
| Holding DashMap locks across `.await` | Deadlock risk | Clone data before async |
| Services mutating Matrix | Tight coupling | Return `ServiceEffect` |

---

## Testing Requirements

- **Round-trip tests**: Parse → Handle → Serialize → Parse (verify idempotency)
- **Concurrency stress**: Use `tokio::test` with multi-client scenarios
- **RFC compliance**: Cross-check numerics against RFC 2812 definitions
- **Service isolation**: Unit test service functions without Matrix dependency

---

## Code Review Checklist

Before submitting:

- [ ] slirc-proto has all needed Command/Numeric variants
- [ ] No `unwrap()`/`expect()` in library code
- [ ] DashMap locks released before `.await`
- [ ] Used `irc_to_lower()` / `irc_eq()` for IRC strings
- [ ] Handler returns `Vec<Response>`, not raw I/O
- [ ] Services return effects, not mutate state
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt -- --check` passes

---

## Documentation References

- **ARCHITECTURE.md**: Current refactoring status, design principles
- **IMPLEMENTATION.md**: Phased development plan, data models
- **TODO.md**: Feature parity tracking vs. legacy slircd
- **slirc-proto docs**: Command/Numeric enums, parsing utilities
