# Innovation 1 Phase 2: Typestate Protocol - Resumption Guide

**Last Updated:** December 7, 2025
**Status:** ✅ COMPLETE - See [TYPESTATE_PHASE2_COMPLETION.md](TYPESTATE_PHASE2_COMPLETION.md)

---

## Context

We are implementing **compile-time protocol state enforcement** for the IRC server. The goal is to make it **impossible at the type level** to call post-registration handlers on unregistered connections.

### What's Already Done (Phase 1)

1. **Registry-based enforcement** - The `Registry::dispatch()` method checks registration state before calling handlers
2. **40+ runtime checks removed** - Handlers no longer need `if !ctx.handshake.registered { return Err(...) }`
3. **State marker types** - `Unregistered`, `Negotiating`, `Registered` in `src/state/machine.rs`
4. **Trait hierarchy** - `ProtocolState`, `IsRegistered`, `PreRegistration` traits

### What's In Progress (Phase 2)

1. **`TypedContext<'a, S>`** - Wrapper that encodes state in type system (in `src/handlers/core/traits.rs`)
2. **`StatefulPostRegHandler`** - New trait receiving `TypedContext<Registered>`
3. **Convenience methods on `Context`** - `ctx.nick()`, `ctx.user_name()` added (workaround, may be removed)
4. **Proof-of-concept handlers** - `TimeHandlerStateful`, `AdminHandlerStateful`, `InfoHandlerStateful`

---

## The Problem We Hit

The `Handler` trait uses anonymous lifetimes that don't work with `TypedContext`:

```rust
// Current Handler trait
#[async_trait]
pub trait Handler: Send + Sync {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult;
}
```

The `'_` lifetime doesn't match what `TypedContext<'a, S>` requires. The workaround was adding `ctx.nick()` methods, but **per project philosophy, we should do it right**:

> "We have zero users — breaking changes are not just OK, they are *encouraged*"
> "No workarounds — if the right solution requires changing 50 files, change 50 files"

---

## The Right Solution

### Option A: Replace Handler Trait (Recommended)

Change the Handler trait signature and migrate ALL handlers:

```rust
// New Handler trait with explicit lifetimes
#[async_trait]
pub trait Handler: Send + Sync {
    async fn handle<'a>(&self, ctx: &mut Context<'a>, msg: &MessageRef<'_>) -> HandlerResult;
}
```

Or split into separate traits:
- `PreRegHandler` - for NICK, USER, PASS, CAP, etc.
- `PostRegHandler` - for PRIVMSG, JOIN, etc. (receives `RegisteredContext`)
- `UniversalHandler` - for QUIT, PING, PONG

### Option B: Keep Handler, Use ctx.nick() (Current State)

The convenience methods `ctx.nick()`, `ctx.user_name()` were added to `Context`:
- They `debug_assert!` registration state
- They panic if nick/user missing
- Provides safety without trait changes

This is technically a workaround but is pragmatic. Decide which path to take.

---

## Files to Understand

| File                                                  | Purpose                                                               |
| ----------------------------------------------------- | --------------------------------------------------------------------- |
| `src/handlers/core/context.rs`                        | `Context` struct, `Handler` trait, new `nick()`/`user_name()` methods |
| `src/handlers/core/traits.rs`                         | `TypedContext<S>`, `StatefulPostRegHandler`, wrapper functions        |
| `src/handlers/core/registry.rs`                       | `Registry::dispatch()` - phase-based handler lookup                   |
| `src/handlers/core/examples.rs`                       | `VersionHandlerStateful` example                                      |
| `src/handlers/server_query/server_info.rs`            | `TimeHandlerStateful`, `AdminHandlerStateful`, `InfoHandlerStateful`  |
| `src/state/machine.rs`                                | State marker types and transitions                                    |
| `docs/innovations/INNOVATION_1_TYPESTATE_PROTOCOL.md` | Documentation                                                         |

---

## Patterns to Migrate

### Old Pattern (scattered throughout handlers)
```rust
let nick = ctx.handshake.nick.as_ref()
    .ok_or(HandlerError::NickOrUserMissing)?;
```

### New Pattern (using ctx.nick())
```rust
let nick = ctx.nick();  // Panics in debug if unregistered
```

### Files with old pattern remaining
Run to find them:
```bash
rg 'ctx\.handshake\.nick\.(as_ref|clone|as_deref)' slircd-ng/src/handlers/ --type rust
```

As of last check:
- `batch.rs` (2)
- `server_query/service.rs` (3)
- `server_query/server_info.rs` (1)
- `connection/nick.rs` (2) - pre-reg, may be intentional
- `mode/channel.rs` (1)
- `connection/ping.rs` (1) - universal, may be intentional
- `messaging/privmsg.rs` (1)
- `oper/auth.rs` (1)

---

## Next Steps

### If Going Full Migration (Option A)

1. **Change Handler trait** in `src/handlers/core/context.rs` to use explicit lifetimes
2. **Create PreRegHandler/PostRegHandler** split in `src/handlers/core/traits.rs`
3. **Update Registry** to dispatch to correct trait
4. **Migrate ALL handlers** (68 total) - use search/replace patterns
5. **Remove TypedContext** - no longer needed if Handler trait itself encodes state
6. **Remove StatefulPostRegHandler** - replaced by the split traits
7. **Update documentation**

### If Keeping Workaround (Option B)

1. **Migrate remaining handlers** to use `ctx.nick()` instead of verbose pattern
2. **Remove TypedContext and StatefulPostRegHandler** - they're unused
3. **Remove dead_code allows** from traits.rs
4. **Update documentation** to reflect actual implementation

---

## Verification Commands

```bash
# Build check
cargo build -p slircd-ng

# Lint check (must pass)
cargo clippy -p slircd-ng -- -D warnings

# Run tests
cargo test -p slircd-ng

# Find old patterns
rg 'ctx\.handshake\.nick\.' slircd-ng/src/handlers/ --type rust

# Find all Handler impls
rg 'impl Handler for' slircd-ng/src/handlers/ --type rust

# Count handlers by file
rg -c 'impl Handler for' slircd-ng/src/handlers/ --type rust
```

---

## Project Philosophy Reminder

From `.github/copilot-instructions.md`:

> **Development Philosophy: NO USERS, NO MERCY**
> - We have zero users — breaking changes are *encouraged*
> - No workarounds — change 50 files if needed
> - No proof-of-concept limbo — fully implement or don't start
> - Delete aggressively — remove old code completely
> - Innovate directly — prefer elegant over safe

---

## Decision Required

Before resuming, decide:

1. **Full migration (Option A)**: Change Handler trait, migrate 68 handlers, delete workarounds
2. **Pragmatic approach (Option B)**: Keep `ctx.nick()` convenience methods, delete TypedContext

Either way, **remove all dead/unused code** - no `#[allow(dead_code)]` for "foundation" code.
