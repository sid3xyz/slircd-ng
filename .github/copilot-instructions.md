# Copilot Instructions for slircd-ng

High-performance IRC daemon in Rust 2024. You are an AI coding for a human engineer.

## Your Role

You write code. The human reviews and approves. Work autonomously until blocked.

- Make changes directly, don't ask for permission
- If unsure, try the simplest approach first
- When stuck, explain what you tried and why it failed
- Commit often with clear messages

## Quick Commands

```bash
cargo build --release                    # Build (must succeed)
cargo test --test '*'                    # Integration tests
cargo clippy --all-targets               # Lint (55 warnings, some errors in tests)
cargo fmt                                # Auto-format
./scripts/irctest_safe.sh                # IRC protocol compliance (~6 basic tests)
```

## Current State

**Compilation**: ✅ Main binary builds. ⚠️ Two test files have errors (`security_channel_freeze`, `sasl_buffer_overflow`)
**Warnings**: 55 clippy warnings (unused code, collapsible ifs, type casts)
**Tests**: 25 integration test files, 3 rehash tests failing (timing issues)

## Architecture (What You Need to Know)

### State Container: Matrix (`src/state/matrix.rs`)
Central dependency injection. Access everything through `Arc<Matrix>`:
- `user_manager` - Users, nicks, WHOWAS
- `channel_manager` - Channel actors
- `security_manager` - Bans, rate limits
- `service_manager` - NickServ, ChanServ
- `sync_manager` - Server linking (S2S)

### Handlers (`src/handlers/`)
141 files across 25 directories. Traits enforce protocol state:
- `PreRegHandler` - Before registration (NICK, USER, CAP, PASS)
- `PostRegHandler` - After registration (PRIVMSG, JOIN, MODE)
- `ServerHandler` - Server-to-server commands

### Services (`src/services/`)
Pure functions returning effects, not mutations:
```rust
pub enum ServiceEffect {
    Reply { target_uid: String, msg: Message },
    AccountIdentify { target_uid: String, account: String },
    Kill { target_uid: String, killer: String, reason: String },
}
```

### Database (`src/db/`)
Dual persistence: SQLite (accounts, bans) + Redb (history)

## Code Patterns

### Zero-Copy Rule
Extract from `MessageRef` BEFORE any `.await`:
```rust
let nick = msg.arg(0).map(|s| s.to_string()); // Clone first
some_async_operation().await;                  // Safe now
```

### IRC Case-Insensitivity
NEVER use `to_lowercase()`. Use proto utilities:
```rust
use slirc_proto::irc_to_lower;
let nick_lower = irc_to_lower(&nick);
```

### Lock Ordering
DashMap → Channel RwLock → User RwLock. Never hold across `.await`.

## Anti-Patterns (Don't Do These)

- `.unwrap()` in handlers → Use `?` or `let Some(...) else { return }`
- `Command::Raw` for known commands → Add variant to slirc-proto
- New singletons → Add to Matrix instead
- Empty stubs returning `Ok(())` → Use `todo!()` to make failures visible

## File Locations

| What | Where |
|------|-------|
| Entry point | `src/main.rs` |
| Command handlers | `src/handlers/` (25 subdirs) |
| Business logic | `src/services/` |
| State managers | `src/state/managers/` |
| Database | `src/db/` |
| Protocol library | `crates/slirc-proto/` |
| Tests | `tests/` (25 files) |
| Config | `config.toml` |

## When Editing

1. Read the file first
2. Make one change at a time
3. Verify with `cargo check` after each edit
4. Format with `cargo fmt`
5. Run relevant test if exists

## Development Philosophy

- Working code over perfect abstractions
- Fix it now, don't document it for later
- `todo!()` panics are better than silent failures
- Git commits are the documentation
