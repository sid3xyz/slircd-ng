# Innovation 1 Phase 3: True Typestate Enforcement

**Status:** 🚧 IN PROGRESS
**Owner:** Protocol Architect
**Goal:** Eliminate runtime registration checks entirely by enforcing protocol state in the connection loop and context types.

---

## The Problem

Current "Phase 2" implementation is a hybrid:
1.  `Context` is still a monolithic struct containing `HandshakeState` with a boolean `registered` flag.
2.  `Registry::dispatch` performs a runtime check: `if ctx.handshake.registered { ... }`.
3.  `TypedContext` is just a wrapper created *after* the runtime check.
4.  The connection loop (`network/connection.rs`) is unaware of state transitions.

This violates the "Parse, Don't Validate" principle. We are validating state at every command dispatch instead of parsing the connection into a `Registered` state once.

## The Solution: True Typestate

We will refactor the application to use a state machine at the connection loop level.

### 1. Generic Context
The `Context` struct will become generic over state `S`:

```rust
pub struct Context<'a, S: ProtocolState> {
    pub state: &'a mut S,
    pub matrix: &'a Arc<Matrix>,
    pub sender: ResponseMiddleware<'a>,
    // ... common fields ...
}
```

### 2. State Types
We will define concrete state structs that hold the data relevant to that state:

```rust
pub struct UnregisteredState {
    pub nick: Option<String>,
    pub user: Option<String>,
    pub caps: HashSet<String>,
    // ...
}

pub struct RegisteredState {
    pub uid: String,
    pub nick: String,
    pub user: String,
    // ...
}
```

### 3. Connection Loop State Machine
The connection loop will explicitly handle state transitions:

```rust
enum ConnectionMachine {
    Unregistered(UnregisteredState),
    Registered(RegisteredState),
}

// In loop:
match &mut machine {
    ConnectionMachine::Unregistered(state) => {
        let ctx = Context::new(state, ...);
        registry.dispatch_pre_reg(ctx, msg).await?;
        // Check for transition
        if let Some(new_state) = state.try_transition() {
            machine = ConnectionMachine::Registered(new_state);
        }
    }
    ConnectionMachine::Registered(state) => {
        let ctx = Context::new(state, ...);
        registry.dispatch_post_reg(ctx, msg).await?;
    }
}
```

### 4. Registry Split
The Registry will expose specific dispatch methods:
- `dispatch_pre_reg(ctx: Context<Unregistered>, ...)`
- `dispatch_post_reg(ctx: Context<Registered>, ...)`
- `dispatch_universal(ctx: Context<S>, ...)`

## Implementation Plan

### Step 1: Define State Structs
- [ ] Create `src/state/session.rs` to define `UnregisteredSession` and `RegisteredSession`.
- [ ] Move fields from `HandshakeState` to these new structs.

### Step 2: Refactor Context
- [ ] Modify `Context` to `Context<'a, S>`.
- [ ] Update all 50+ handlers to use `Context<'a, S>` or specific aliases.
- [ ] This is a massive breaking change. We will do it in one go ("No Mercy").

### Step 3: Refactor Registry
- [ ] Split `dispatch` into `dispatch_pre_reg` and `dispatch_post_reg`.
- [ ] Remove the runtime `if registered` check.

### Step 4: Refactor Connection Loop
- [ ] Implement the state machine loop in `src/network/connection.rs`.
- [ ] Implement the transition logic (consuming Unregistered, producing Registered).

## Risks & Mitigation
- **Risk:** Massive compile errors during refactor.
- **Mitigation:** We will comment out the connection loop temporarily while fixing handlers, then rebuild the loop.

---
**"No Users, No Mercy"** - We will not create compatibility shims. We will break the build and fix it.
