# Phase 3 Implementation Todo

- [ ] **Step 1: State Definitions**
    - [ ] Define `UnregisteredSession` struct (replacing HandshakeState parts)
    - [ ] Define `RegisteredSession` struct
    - [ ] Define `SessionState` enum (or trait)

- [ ] **Step 2: Context Refactor**
    - [ ] Rename `HandshakeState` to `SessionData` (or similar) or split it.
    - [ ] Make `Context` generic: `pub struct Context<'a, S>`.
    - [ ] Update `Handler` traits to accept `Context<'a, S>`.
    - [ ] Fix all handler signatures (50+ files).

- [ ] **Step 3: Registry Refactor**
    - [ ] Update `Registry` to store typed handlers.
    - [ ] Implement `dispatch_pre_reg` and `dispatch_post_reg`.

- [ ] **Step 4: Connection Loop**
    - [ ] Rewrite `handle_connection` in `src/network/connection.rs`.
    - [ ] Implement transition logic.
