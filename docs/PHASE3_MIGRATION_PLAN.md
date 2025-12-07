# Phase 3 Migration Plan: True Typestate Enforcement

## Current Status

The Phase 3 migration attempt revealed significant interdependencies that require
a more careful, staged approach. The build is currently **passing** with the
Phase 2 architecture (TypedContext wrapper).

## Lessons Learned

### 1. The Core Challenge

The Phase 3 goal is to replace `TypedContext<'_, Registered>` wrapper with
`Context<'_, RegisteredState>` directly. However, this requires:

1. **Connection Loop Refactor**: The connection loop must transition from
   `HandshakeState` to `RegisteredState` after registration completes
   
2. **Registry Split**: The Registry needs `dispatch_post_reg()` that takes
   `Context<RegisteredState>` instead of wrapping with TypedContext

3. **Helper Function Updates**: ~15+ helper functions need their signatures
   changed from `Context<'_>` to `Context<'_, RegisteredState>`

4. **Handler Field Access Updates**: Handlers using `ctx.state.nick.unwrap_or()`
   need to change to `ctx.nick()` since nick is `String` not `Option<String>`

### 2. Why It's Complex

The `Context<'a, S>` struct holds `state: &'a mut S`. This means:

- `Context<'_, HandshakeState>` and `Context<'_, RegisteredState>` are 
  **different types** with different memory layouts
- You cannot transmute or cast between them
- The connection loop must actually hold different state types at different
  phases of the connection lifecycle

### 3. Components Already Prepared (in `src/state/session.rs`)

- ✅ `UnregisteredState` - Pre-registration state struct
- ✅ `RegisteredState` - Post-registration state struct  
- ✅ `ConnectionState` enum with `Unregistered`/`Registered` variants
- ✅ `UnregisteredState::try_register()` - Consumes self, returns `RegisteredState`

## Recommended Migration Path

### Step 1: Add `HandshakeState::into_registered()` Method

Add a method to convert HandshakeState to RegisteredState:

```rust
impl HandshakeState {
    pub fn into_registered(self) -> RegisteredState {
        RegisteredState {
            nick: self.nick.expect("nick must be set"),
            user: self.user.expect("user must be set"),
            realname: self.realname.unwrap_or_default(),
            capabilities: self.capabilities,
            // ... copy other fields
        }
    }
}
```

### Step 2: Update Connection Loop

Modify `src/network/connection/mod.rs`:

```rust
// Phase 1: Handshake (as before)
let mut handshake = HandshakeState::default();
loop {
    // ... handshake logic ...
    if handshake.registered { break; }
}

// Transition: Convert state
let mut registered_state = handshake.into_registered();

// Phase 2: Registered operation
loop {
    let mut ctx = Context {
        state: &mut registered_state,
        // ... other fields ...
    };
    registry.dispatch_post_reg(&mut ctx, &msg).await?;
}
```

### Step 3: Add Registry::dispatch_post_reg()

```rust
impl Registry {
    pub async fn dispatch_post_reg(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Direct dispatch to post_reg_handlers
        if let Some(handler) = self.post_reg_handlers.get(cmd_str) {
            handler.handle(ctx, msg).await
        } else {
            Err(HandlerError::Internal("Command not found".into()))
        }
    }
}
```

### Step 4: Update PostRegHandler Trait

```rust
pub trait PostRegHandler: Send + Sync {
    async fn handle(
        &self, 
        ctx: &mut Context<'_, RegisteredState>, 
        msg: &MessageRef<'_>
    ) -> HandlerResult;
}
```

### Step 5: Update All Handler Implementations

For each of the ~37 PostRegHandler implementations:

1. Change signature from `TypedContext<'_, Registered>` to `Context<'_, RegisteredState>`
2. Update imports
3. Change `ctx.inner().matrix` to `ctx.matrix`
4. Change `ctx.state.nick.unwrap()` to `ctx.nick()` (convenience method)

### Step 6: Update Helper Functions

Make helper functions generic or specific to RegisteredState:

```rust
// Generic version (for functions that only need matrix/uid)
pub fn resolve_nick_to_uid<S>(ctx: &Context<'_, S>, nick: &str) -> Option<String> { ... }

// Specific version (for functions that need state fields)
pub async fn is_shunned(ctx: &Context<'_, RegisteredState>) -> bool { ... }
```

### Step 7: Delete Legacy Code

- Remove `TypedContext` struct
- Remove `Registered` marker trait usage from handlers
- Remove `ctx.inner()` pattern

## Files to Modify

### Core Changes
- `src/handlers/core/traits.rs` - PostRegHandler signature
- `src/handlers/core/context.rs` - Add convenience methods, conversion
- `src/handlers/core/registry.rs` - Add dispatch_post_reg
- `src/network/connection/mod.rs` - State transition

### Handler Updates (~37 files)
- All files in `src/handlers/` implementing PostRegHandler

### Helper Function Updates
- `src/handlers/messaging/common.rs`
- `src/handlers/channel/ops.rs`  
- `src/handlers/core/context.rs` (resolve_nick_to_uid, etc.)

## Testing Strategy

1. Build incrementally - ensure each step compiles
2. Run `cargo clippy --workspace -- -D warnings`
3. Run unit tests: `cargo test -p slircd-ng`
4. Run irctest compliance suite

## Estimated Effort

- Steps 1-3: 1-2 hours (connection loop + registry)
- Step 4: 30 minutes (trait change)
- Step 5: 2-3 hours (update ~37 handlers)
- Step 6: 1 hour (helper functions)
- Step 7: 30 minutes (cleanup)
- Testing: 1 hour

**Total: ~6-8 hours of focused work**

## Why This Must Be Done in One Session

The intermediate states don't compile - you can't have some handlers using
the old signature and some using the new. The entire migration must be
completed atomically (or reverted).
