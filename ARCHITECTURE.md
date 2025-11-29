# slircd-ng Architecture & Refactoring Checklist

> Architectural purity enforcement for the zero-copy IRC daemon
> Created: November 29, 2025

---

## Current Architecture (Validated ✅)

```
Client → Gateway → Connection (tokio::select!) → Handler → Matrix (DashMap) → Router
```

| Component | Pattern | Status |
|-----------|---------|--------|
| Hot Loop | `tokio::select!` with `MessageRef<'_>` | ✅ Zero-copy |
| State | `DashMap` for nicks/users/channels | ✅ Lock-free |
| Handlers | `async fn handle(&self, ctx, msg)` trait | ✅ Clean |
| Rate Limit | Token bucket in `limit.rs` | ✅ 2.5 msg/s |
| UID Gen | TS6-compliant `UidGenerator` | ✅ Correct |

---

## Refactoring Checklist

### P0: Critical (DRY Violations)

- [ ] **Extract `user_prefix` to mod.rs**
  - Files: `channel.rs:12`, `messaging.rs:13`, `mode.rs:15`
  - Same 3-line function duplicated 3 times
  - Action: Move to `handlers/mod.rs`, export publicly

- [ ] **Create `err_notregistered()` helper**
  - Pattern repeated 20+ times across handlers:
    ```rust
    if !ctx.handshake.registered {
        let reply = server_reply(...ERR_NOTREGISTERED...);
        ctx.sender.send(reply).await?;
        return Ok(());
    }
    ```
  - Action: Add to `handlers/mod.rs` error helpers section

### P1: Coupling Issues

- [ ] **Remove `MatrixConfig.server_name` duplicate**
  - `ctx.matrix.server_info.name` is canonical source
  - `ctx.matrix.config.server_name` duplicates it
  - Action: Remove from `MatrixConfig`, update all references

- [ ] **Complete error helper migration**
  - Many handlers still inline `server_reply()` for errors
  - Existing helpers: `err_noprivileges`, `err_needmoreparams`, `err_nosuchnick`, 
    `err_nosuchchannel`, `err_notonchannel`, `err_chanoprivsneeded`, `err_usernotinchannel`
  - Action: Audit all handlers, replace inline patterns with helpers

### P2: Service Layer Decoupling

- [ ] **Refactor service routing to return effects**
  - Current: `route_service_message` directly mutates Matrix state
  - Problem: Services shouldn't know about Matrix internals
  - Target:
    ```rust
    pub struct ServiceEffect {
        pub set_registered: Option<String>,  // Account name
        pub kill_uid: Option<String>,
        pub mode_changes: Vec<ModeChange>,
    }
    ```
  - Action: Services return effects, caller applies them

### P3: Cleanup

- [ ] **Audit `#[allow(dead_code)]` markers**
  - 20+ occurrences with vague justifications
  - Categories:
    - "Phase 4+: Server linking" - Keep with TODO
    - "Will be used..." - Verify or remove
    - "Used by X handlers" - Check if actually used, remove annotation
  - Action: Document each decision

---

## Design Principles

1. **Zero-Copy Hot Loop**: No allocations during message dispatch
2. **DashMap State**: Lock-free concurrent access to Matrix
3. **Handler Trait**: Clean async dispatch with borrowed context
4. **Error Helpers**: DRY error responses via helper functions
5. **Service Effects**: Services return effects, don't mutate state directly

---

## File Reference

| File | Purpose |
|------|---------|
| `src/handlers/mod.rs` | Handler trait, Registry, error helpers |
| `src/state/matrix.rs` | Central state: users, channels, nicks |
| `src/network/connection.rs` | Unified `tokio::select!` loop |
| `src/network/limit.rs` | Token bucket rate limiter |
| `src/services/*.rs` | NickServ, ChanServ pseudo-services |
