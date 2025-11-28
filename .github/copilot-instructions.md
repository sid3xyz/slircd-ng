# slircd-ng Copilot Instructions

## Project Overview

**slircd-ng** is a high-performance, multi-threaded IRC daemon built in Rust with zero-copy parsing. The canonical reference for architecture and implementation details is `IMPLEMENTATION.md`.

## Architecture (Key Mental Model)

```
Client → Gateway → Connection (Tokio task) → Handler → Matrix (DashMap state) → Router → Response
```

- **Matrix** (`Arc<Matrix>`): Central shared state with lock-free `DashMap` collections for users, channels, nicks, servers
- **Handlers**: Implement `Handler` trait, receive `Context` + `MessageRef`, return `Vec<Response>`
- **Router**: Handles unicast/multicast delivery, distinguishes local vs remote users
- **TS6 UIDs**: 9-char identifiers (`SID` 3-char + 6-char client ID) for future server linking

## Core Dependencies

| Crate | Role |
|-------|------|
| `slirc-proto` | Zero-copy IRC parsing ([sid3xyz/slirc-proto](https://github.com/sid3xyz/slirc-proto)) |
| `tokio` | Async runtime |
| `dashmap` | Concurrent state |
| `sqlx` | SQLite for services persistence |

**Cargo.toml dependency:**
```toml
slirc-proto = { git = "https://github.com/sid3xyz/slirc-proto", features = ["tokio"] }
```

## Directory Structure

```
src/
├── state/       # Matrix, User, Channel, Server entities
├── network/     # Gateway, Connection, Handshake
├── handlers/    # IRC command handlers (one file per category)
├── router/      # Message delivery
├── services/    # NickServ, ChanServ
└── db/          # SQLx queries for services
```

## Critical Patterns

### Zero-Copy Lifetime Management
`MessageRef<'a>` borrows from transport buffer. Process immediately or call `.to_owned()`:
```rust
// In handler: msg is MessageRef<'_>
let nick = msg.params().get(0).map(|s| s.to_string()); // Clone if needed
```

### Transport Upgrade Pattern
Use `Transport` during CAP/NICK/USER handshake, then convert to `ZeroCopyTransport` for hot loop. WebSocket transports cannot convert.

### IRC Case Insensitivity
Always use `slirc_proto::irc_to_lower()` and `irc_eq()` for nick/channel comparisons—never `to_lowercase()`.

### Handler Response Pattern
```rust
async fn handle(&self, ctx: &Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
    // ... logic ...
    Ok(vec![
        Response::Reply(reply_msg),
        Response::Broadcast { channel, msg, exclude: Some(ctx.uid.clone()) },
    ])
}
```

### DashMap Access
```rust
// Read: returns RefMulti guard
if let Some(user) = matrix.users.get(&uid) {
    let user = user.read().await;
}
// Lookup via secondary index
if let Some(uid) = matrix.nicks.get(&nick_lower) {
    // ...
}
```

## Gotchas

1. **Message length**: IRC limit is 512 bytes (8191 modern). Tags don't count toward limit.
2. **Mode parameter ordering**: `+ov nick1 nick2` means `+o nick1`, `+v nick2`
3. **Nick collision**: Prefer older timestamp; kill newer or both if equal
4. **Flood protection**: Rate limit per-user (5 msg/2s, then 1/s)

## Services Database

- SQLite via SQLx with migrations in `migrations/`
- Password hashing: Argon2
- Case-insensitive nick/channel matching: `COLLATE NOCASE`

## Testing Approach

- Unit tests: Mock `Context` for handler tests
- Integration: Spawn server, connect real IRC client, verify protocol behavior
- Fuzzing: Protocol parsing is covered by `slirc-proto`

## Implementation Status

This is a new project following the phased plan in `IMPLEMENTATION.md`. Check the phase checkboxes there to understand what's implemented vs. planned.

## AI Agent Workflow

### ⚠️ MANDATORY: slirc-proto First Workflow

**BEFORE implementing any IRC command or feature, you MUST:**

1. **Check slirc-proto FIRST** - Search the [sid3xyz/slirc-proto](https://github.com/sid3xyz/slirc-proto) repository for:
   - `Command` enum variants for the command you're implementing
   - `Response` enum variants for any numeric replies needed
   - Helper functions (parsing, formatting, case handling)
   - Capability definitions in `caps.rs`

2. **If slirc-proto is lacking** - STOP IMMEDIATELY and:
   - Describe what's missing (Command variant, Response code, helper function)
   - Propose the exact addition needed to slirc-proto
   - Wait for confirmation before proceeding
   - Do NOT work around missing features with raw strings or hardcoded values

3. **Only then implement in slircd-ng** - After confirming slirc-proto has what you need

**Example workflow:**
```
Task: Implement SETNAME command

Step 1: Search slirc-proto for "SETNAME" in Command enum
Step 2: Search for any Response codes needed (RPL_*, ERR_*)
Step 3: If missing: STOP, report "slirc-proto needs Command::SETNAME variant"
Step 4: After slirc-proto is updated: implement handler in slircd-ng
```

**Anti-patterns to AVOID:**
- Using `Command::Raw("SETNAME", ...)` when a proper variant should exist
- Hardcoding numeric codes instead of using `Response::RPL_*` variants
- Creating IRC parsing/formatting logic that belongs in slirc-proto
- Ignoring typos in slirc-proto (e.g., `ERR_ALREADYREGISTRED` - fix the library!)

### Progress Tracking
- Use **todo lists** to track multi-step tasks and maintain visibility
- Mark todos in-progress before starting, completed immediately after
- Reference `IMPLEMENTATION.md` phase checkboxes for overall project status

### Git Workflow
- Commit frequently with descriptive messages referencing the phase/feature
- Use conventional commits: `feat:`, `fix:`, `refactor:`, `test:`, `docs:`
- Keep commits atomic—one logical change per commit

### Subagent Delegation
Spawn subagents for focused tasks:
- **Research**: API lookups in `slirc-proto`, IRC RFC details, Tokio patterns
- **Code search**: Finding handler examples, DashMap usage patterns
- **Multi-file refactoring**: When changes span multiple modules

Example prompt for subagent:
> "Research how slirc-proto's MessageRef handles tag parsing. Return the key methods and any lifetime considerations."

### slirc-proto Issues
If you encounter a limitation or missing feature in `slirc-proto` that blocks progress, **stop and describe**:
1. What you're trying to accomplish
2. What API is missing or broken
3. Suggested fix or addition

We maintain `slirc-proto` at [sid3xyz/slirc-proto](https://github.com/sid3xyz/slirc-proto) and can add features as needed.

