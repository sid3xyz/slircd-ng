# Copilot Instructions for slircd-ng

> High-performance distributed IRC daemon built on zero-copy parsing.
> Released to the public domain under [The Unlicense](../LICENSE).

---

## Quick Reference

```bash
cargo build --release           # Production build (stable Rust 1.85+)
cargo test                      # Run 604+ tests
cargo clippy -- -D warnings     # Lint (must pass)
cargo fmt -- --check            # Format check
./target/release/slircd config.toml  # Run daemon

# Memory-safe irctest (never run full suite without a hard cap)
MEM_MAX=4G SWAP_MAX=0 KILL_SLIRCD=1 ./scripts/irctest_safe.sh irctest/server_tests/utf8.py
```

---

## Project Constraints

| Constraint | Requirement |
|------------|-------------|
| Edition | Rust 2024 (stable since Rust 1.85) |
| MSRV | Rust 1.85+ (stable) |
| Linting | `clippy -- -D warnings` — 19 allowed exceptions |
| Error handling | Use `?` propagation; avoid `unwrap()`/`expect()` except in main.rs |
| Allocation discipline | Zero-copy hot loop using `MessageRef<'a>` from slirc-proto |
| RFC compliance | Strict adherence to RFC 1459, RFC 2812, IRCv3 |
| **PROTO-FIRST RULE** | **NEVER create daemon workarounds for proto bugs. ALWAYS fix slirc-proto first, then update daemon. Document proto issues in PROTO_REQUIREMENTS.md before implementing any workaround.** |

---

## Architecture

| Component | Pattern |
|-----------|---------|
| Protocol | `slirc-proto` — zero-copy parsing with `MessageRef<'a>` |
| State Sync | `slirc-crdt` — LWW-based distributed state |
| Transport | Tokio async: `TcpListener` + `rustls` for TLS |
| Hot Loop | `tokio::select!` in `network/connection/` dispatching to handlers |
| State | `Arc<Matrix>` with 7 domain managers |
| Handlers | Typestate: `PreRegHandler`, `PostRegHandler`, `UniversalHandler<S>` |
| Channels | Actor model: per-channel Tokio task with bounded mailbox |
| Services | Pure effect functions: NickServ/ChanServ return `ServiceEffect` vectors |
| Persistence | SQLx + SQLite (7 migrations), Redb for history |

---

## Development Workflow

### 🛡️ PRIME DIRECTIVE: Protocol-First Development

**Never implement daemon logic before verifying protocol support.**

Before writing code:

1. **Check slirc-proto**: Does the `Command` variant exist? Is the numeric reply defined?
   - ✅ If yes: Proceed to handler implementation
   - ❌ If no: **STOP.** Immediately document the blocker in [PROTO_REQUIREMENTS.md](../PROTO_REQUIREMENTS.md) with:
     - What feature is blocked
     - How many tests/features depend on it
     - Proposed solution (with options if applicable)
     - Then pause implementation until proto team responds
   - **Never** hack with `Command::Raw` or work around missing proto support

2. **Check ARCHITECTURE.md**: Verify phase alignment and design principles.

3. **Check ROADMAP_TO_1.0.md**: Review current release blockers.

### Request Processing Template

For every IRC feature request:

1. **Goal Analysis**: Map request to RFC command/numeric requirements.
2. **Protocol Check**: Verify slirc-proto support (Command variant, Numeric enum).
3. **State Design**: Identify Matrix manager and DashMap access patterns.
4. **Handler Logic**: Write typestate handler with `Context<S>` and `MessageRef`.
5. **Testing**: Round-trip test (parse → handle → serialize).

---

## Critical Patterns

### 1. Typestate Handler System (Innovation 1)

```rust
// Handler traits enforce protocol state at compile time
trait PreRegHandler   // NICK, USER, CAP, PASS, WEBIRC
trait PostRegHandler  // PRIVMSG, JOIN, MODE, WHOIS...
trait UniversalHandler<S: SessionState>  // QUIT, PING, PONG

// Context is generic over session state
pub struct Context<'a, S> {
    pub uid: &'a str,
    pub matrix: &'a Arc<Matrix>,
    pub state: &'a mut S,  // UnregisteredState or RegisteredState
    pub sender: ResponseMiddleware<'a>,
    // ...
}

// RegisteredState guarantees nick/user are present (not Option)
impl<'a> Context<'a, RegisteredState> {
    pub fn nick(&self) -> &str { &self.state.nick }  // Always valid
}
```

**Rule**: Never check `if !registered` — the type system enforces it.

### 2. Zero-Copy Message Handling (Innovation 4)

```rust
// MessageRef<'a> borrows from transport buffer
async fn handle(&self, ctx: &mut Context<'_, S>, msg: &MessageRef<'_>) -> HandlerResult {
    // Extract params immediately — msg lifetime ends after this function
    let nick = msg.arg(0).map(|s| s.to_string());
    
    // Process synchronously in this function
    Ok(())
}
```

**Rule**: Never hold `MessageRef` across `.await` points. Extract needed data first.

### 3. Actor Model Channels (Innovation 3)

```rust
// Each channel runs in its own Tokio task
pub struct ChannelActor {
    events: mpsc::Receiver<ChannelEvent>,  // Bounded, capacity 1024
    state: ChannelState,  // Owns all channel state
}

// Interact via events, not locks
pub enum ChannelEvent {
    Join { uid: String, modes: MemberModes, ... },
    Part { uid: String, reason: Option<String>, ... },
    Message { from_uid: String, text: String, ... },
    // ...
}
```

**Rule**: Never directly access channel state. Send `ChannelEvent` to actor.

### 4. Matrix Manager Pattern

```rust
// Matrix delegates to specialized managers (not a god object)
pub struct Matrix {
    pub user_manager: UserManager,       // Users, nicks, WHOWAS
    pub channel_manager: ChannelManager, // Channel actors
    pub security_manager: SecurityManager,
    pub service_manager: ServiceManager,
    pub monitor_manager: MonitorManager,
    pub lifecycle_manager: LifecycleManager,
    pub sync_manager: SyncManager,       // S2S state
}
```

**Rule**: Access state through appropriate manager, not raw DashMaps.

### 5. Service Effect Pattern

```rust
// Services don't mutate Matrix — they return effects
pub enum ServiceEffect {
    Reply { target_uid: String, msg: Message },
    AccountIdentify { target_uid: String, account: String },
    Kill { target_uid: String, killer: String, reason: String },
    ChannelMode { channel: String, target_uid: String, mode_char: char, adding: bool },
    // ...
}

// Handler applies effects after service processing
let handled = route_service_message(&matrix, uid, nick, target, text, &sender).await;
```

**Rule**: Services are pure. `route_service_message` applies effects to Matrix.

### 6. DashMap Lock Discipline

```rust
// ✅ Good: Short lock via manager
if let Some(user) = matrix.user_manager.get_user(&uid) {
    let nick = user.nick.clone();
    // Use nick after lock is dropped
}

// ❌ Bad: Holding entry across await
let user = matrix.user_manager.users.get(&uid).unwrap();
some_async_call().await;  // Lock held during IO!
```

**Lock Order** (to prevent deadlocks):
1. DashMap shard lock
2. Channel RwLock
3. User RwLock

### 7. IRC Case-Insensitivity

```rust
use slirc_proto::{irc_to_lower, irc_eq};

// ✅ Correct
let nick_lower = irc_to_lower(&nick);
matrix.user_manager.nicks.get(&nick_lower);

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
| `.unwrap()` in handler code | Panics on error | Use `?` or `.ok_or(HandlerError::X)?` |
| `std::to_lowercase()` on IRC | Incorrect `{}|~` handling | Use `irc_to_lower()` |
| Direct DashMap access | Bypasses manager | Use `user_manager`, `channel_manager` |
| Holding locks across `.await` | Deadlock risk | Clone data before async |
| Services mutating Matrix | Tight coupling | Return `ServiceEffect` |
| `if !registered` checks | Runtime state check | Use typestate handlers |
| Leaving legacy code/docs | Technical debt accumulates | Delete immediately |
| TODO/FIXME without tracking | Lost context | Create issue or fix now |

---

## 🧹 Codebase Hygiene (MANDATORY)

**Zero-Cruft Policy**: This project maintains strict cleanliness. Legacy artifacts are prohibited.

### Rules

1. **No Legacy Documentation**
   - Delete outdated docs immediately — do not "archive" or rename
   - If a doc is superseded, remove it in the same commit
   - Single source of truth per topic (no ARCHITECTURE.md + DESIGN.md)

2. **No Dead Code**
   - Unused functions, modules, or commented-out code must be deleted
   - `#[allow(dead_code)]` requires justification comment
   - Feature flags for experimental code, not comments

3. **No Orphaned TODOs**
   - Every `TODO`/`FIXME` must reference an issue number: `// TODO(#123): ...`
   - Or fix it immediately — no "I'll get to it later"
   - Current count must stay at 0 (exceptions tracked in ROADMAP)

4. **No Stale Dependencies**
   - Run `cargo update` monthly
   - Remove unused dependencies immediately
   - No version pinning without documented reason

5. **No Duplicate Implementations**
   - One canonical way to do each thing
   - Extract to shared helper, don't copy-paste
   - Generic implementations over repetition (see `db/bans/queries/generic.rs`)

### Housekeeping Commands

```bash
# Find dead code
cargo +nightly udeps  # Unused dependencies
cargo clippy -- -W dead_code

# Find orphaned TODOs
grep -rn "TODO\|FIXME\|HACK\|XXX" src/

# Find large files that might need splitting
find src -name "*.rs" -exec wc -l {} \; | sort -rn | head -20

# Verify no legacy docs
ls -la *.md  # Should only be: README, ARCHITECTURE, CHANGELOG, ROADMAP, DEPLOYMENT_CHECKLIST, LICENSE
```

---

## Testing Requirements

- **Unit tests**: 637+ tests, run with `cargo test`
- **Integration tests**: `tests/` directory (ircv3_features, distributed_sync)
- **irctest compliance**: 328/387 passing (84.8%) - see `slirc-irctest/IRCTEST_RESULTS.md`
- **irctest invocation**: `cd slirc-irctest && SLIRCD_BIN=../target/release/slircd pytest --controller=irctest.controllers.slircd irctest/server_tests/`
- **Round-trip tests**: Parse → Handle → Serialize → Parse
- **No load/fuzz tests yet**: See ROADMAP_TO_1.0.md

---

## Code Review Checklist

Before submitting:

- [ ] slirc-proto has all needed Command/Numeric variants
- [ ] No `unwrap()`/`expect()` in library code
- [ ] DashMap locks released before `.await`
- [ ] Used `irc_to_lower()` / `irc_eq()` for IRC strings
- [ ] Handler uses correct typestate trait (Pre/Post/Universal)
- [ ] Channel interactions via `ChannelEvent`, not direct access
- [ ] Services return effects, not mutate state
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt -- --check` passes
- [ ] No orphaned TODOs/FIXMEs added
- [ ] No legacy docs left behind (delete superseded files)
- [ ] No dead code or commented-out blocks

---

## Documentation References

- **ARCHITECTURE.md**: Complete architectural deep dive (37KB)
- **README.md**: Project overview, installation, configuration
- **ROADMAP_TO_1.0.md**: Release readiness roadmap and blocking issues
- **DEPLOYMENT_CHECKLIST.md**: Production deployment checklist
- **CHANGELOG.md**: Version history
- **slirc-proto**: Command/Numeric enums, parsing utilities (external crate)
- **slirc-crdt**: CRDT state synchronization (external crate)
