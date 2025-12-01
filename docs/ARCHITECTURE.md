# slircd-ng Architecture & Refactoring Checklist

> Architectural purity enforcement for the zero-copy IRC daemon
> Created: November 29, 2025

---

## Current Architecture (Validated ✅)

```
Client → Gateway → Connection (tokio::select!) → Handler → Matrix (DashMap) → Router
                                                     ↓
                                    Security Layer (Spam, ExtBan, XLine, Rate Limit)
```

| Component | Pattern | Status |
|-----------|---------|--------|
| Hot Loop | `tokio::select!` with `MessageRef<'_>` | ✅ Zero-copy |
| State | `DashMap` for nicks/users/channels | ✅ Lock-free |
| Handlers | `async fn handle(&self, ctx, msg)` trait | ✅ Clean |
| Rate Limit | Token bucket in `limit.rs` | ✅ 2.5 msg/s |
| UID Gen | TS6-compliant `UidGenerator` | ✅ Correct |
| **Spam Detection** | **Multi-layer content analysis** | ✅ **Phase 2** |
| **Extended Bans** | **$a/$r/$U pattern matching** | ✅ **Phase 2** |
| **XLines (R-Line)** | **Database-backed bans** | ✅ **Phase 2** |
| **Metrics** | **Prometheus HTTP /metrics** | ✅ **Phase 2** |

---

## Refactoring Checklist

### P0: Critical (DRY Violations) ✅ COMPLETE

- [x] **Extract `user_prefix` to mod.rs** (b0d568c)
- [x] **Create `err_notregistered()` helper** (b0d568c)

### P1: Coupling Issues ✅ COMPLETE

- [x] **Remove `MatrixConfig.server_name` duplicate** (031c772)
  - Canonical: `ctx.matrix.server_info.name`
  - Updated: bans.rs, admin.rs, misc.rs, oper.rs

- [x] **Complete error helper migration** (b0d568c)
  - Helpers: `err_noprivileges`, `err_needmoreparams`, `err_nosuchnick`, 
    `err_nosuchchannel`, `err_notonchannel`, `err_chanoprivsneeded`, 
    `err_usernotinchannel`, `err_notregistered`, `user_prefix`

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
6. **Security First**: Multi-layer spam/abuse protection (Phase 2)
7. **Observability**: Prometheus metrics for production monitoring

---

## Phase 2: Security & Observability (Complete ✅)

### Spam Detection
- **File**: `src/security/spam.rs` (411 lines)
- **Integration**: `handlers/messaging.rs` - Pre-broadcast spam check
- **Mechanisms**: Keyword matching, entropy analysis, URL shorteners, CTCP flood detection
- **Config**: `security.spam_detection_enabled` (default: true)
- **Metrics**: `irc_spam_blocked_total` counter

### Extended Bans (EXTBAN)
- **File**: `src/security/extban.rs` (445 lines)
- **Types**: $a:account, $r:realname, $U (unregistered), $s:server, +5 more
- **Integration**: MODE +b handler, JOIN enforcement, PRIVMSG filtering
- **Storage**: `Channel.extended_bans` list (parallel to regular bans)
- **Pattern**: Wildcard matching via `slirc_proto::util::wildcard_match`

### XLine System (R-Line)
- **Database**: `migrations/003_rlines.sql` (realname/GECOS bans)
- **Methods**: `db/bans.rs` - add/remove/list/matches R-lines
- **Enforcement**: `handlers/connection.rs` - Blocks on USER command
- **Commands**: RLINE/UNRLINE (oper-only)
- **Metrics**: `irc_xlines_enforced_total` counter

### Rate Limiting
- **Integration**: PRIVMSG, NOTICE, JOIN handlers
- **Transport**: Connection-level flood protection (5 strikes)
- **Config**: `security.rate_limits.*` (message/join/connection limits)
- **Error**: ERR_THROTTLE (407) sent to rate-limited clients
- **Metrics**: `irc_rate_limited_total` counter

### Prometheus Metrics
- **File**: `src/metrics.rs` (113 lines)
- **Registry**: lazy_static global registry with 7 metrics
- **Counters**: messages_sent, spam_blocked, bans_triggered, xlines_enforced, rate_limited
- **Gauges**: connected_users, active_channels
- **HTTP Endpoint**: `src/http.rs` - GET /metrics on port 9090
- **Config**: `server.metrics_port` (default: 9090)

---

## File Reference

| File | Purpose |
|------|---------|
| `src/handlers/mod.rs` | Handler trait, Registry, error helpers |
| `src/state/matrix.rs` | Central state: users, channels, nicks |
| `src/network/connection.rs` | Unified `tokio::select!` loop |
| `src/network/limit.rs` | Token bucket rate limiter |
| `src/services/*.rs` | NickServ, ChanServ pseudo-services |
| **`src/security/spam.rs`** | **Spam detection service (Phase 2)** |
| **`src/security/extban.rs`** | **Extended ban types (Phase 2)** |
| **`src/metrics.rs`** | **Prometheus metrics registry (Phase 2)** |
| **`src/http.rs`** | **HTTP /metrics endpoint (Phase 2)** |
