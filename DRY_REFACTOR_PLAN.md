# DRY Refactor Execution Plan

> Generated: 2025-12-16
> Status: ✅ ALL PHASES COMPLETE

## Overview

Based on the Principal Architect audit, this plan targets ~800 LOC reduction (6.6% of slircd-ng).
Organized into 3 phases by dependency order.

**CRITICAL RULE: Each new helper MUST be immediately followed by migration of ALL legacy usages. No new code without cleanup.**

---

## Phase 1: Context Helper Methods (P0 - Quick Wins) ✅ COMPLETE

**Estimated Impact: ~420 LOC reduction**

### 1.1 Add `server_prefix()` to Context
- [x] **File**: `src/handlers/core/context.rs`
- [x] **Action**: Add method returning `Prefix::ServerName(self.server_name().to_string())`
- [x] **MIGRATE**: Replace direct `.with_prefix(Prefix::ServerName(...))` patterns in handlers
- [x] **Note**: Helper functions in helpers.rs (server_reply, server_notice, labeled_ack) retain `Prefix::ServerName` as they take server_name as parameter for use outside Context

### 1.2 Add `authority()` to Context
- [x] **File**: `src/handlers/core/context.rs`
- [x] **Action**: Add method returning `CapabilityAuthority::new(self.matrix.clone())`
- [x] **MIGRATE**: Replace ALL 21 `CapabilityAuthority::new(ctx.matrix.clone())` calls
- [x] **VERIFY**: `rg "CapabilityAuthority::new\(ctx" src/handlers/` returns 0 matches

### 1.3 Migrate remaining `server_name` usages
- [x] **Files**: 87 handlers using `&ctx.matrix.server_info.name`
- [x] **MIGRATE**: Replace with `ctx.server_name()`
- [x] **VERIFY**: `rg "ctx\.matrix\.server_info\.name" src/handlers/` returns only context.rs

---

## Phase 2: Macro Infrastructure (P1) ✅ COMPLETE

**Actual Impact: 236 net lines removed**

### 2.1 Create `require_arg_or_reply!` macro ✅
- [x] **File**: `src/handlers/helpers.rs`
- [x] **Action**: Macro sends ERR_NEEDMOREPARAMS and records metrics
- [x] **MIGRATE**: Migrated 12 handlers

### 2.2 Create `send_noprivileges!` macro ✅
- [x] **File**: `src/handlers/helpers.rs`
- [x] **Action**: Macro for ERR_NOPRIVILEGES + metrics

### 2.3 Create `require_admin_cap!` macro ✅
- [x] **File**: `src/handlers/helpers.rs`
- [x] **Action**: Macro for admin capability check + error handling
- [x] **Impact**: Eliminated ~120 LOC in admin.rs (4× SA* handler preambles)

### 2.4 Create `require_oper_cap!` macro ✅
- [x] **File**: `src/handlers/helpers.rs`
- [x] **Action**: Generic oper capability check with configurable cap method
- [x] **MIGRATE**: KILL, WALLOPS, TRACE, VHOST, CHGHOST, CHGIDENT, SHUN, OPER handlers

---

## Phase 3: Error Helper Method (P2) ✅ COMPLETE

**Actual Impact: ~28 sites migrated, ~56 LOC reduction**

### 3.1 Add `send_error()` to Context ✅
- [x] **File**: `src/handlers/core/context.rs`
- [x] **Action**: Method combining `ctx.sender.send(err)` + `metrics::record_command_error`
- [x] **Signature**: `pub async fn send_error(&self, command: &str, error_name: &str, message: Message)`

### 3.2 Migrate handler error patterns ✅
- [x] **Files migrated** (28 call sites):
  - `admin.rs`: SAJOIN, SAPART, SANICK, SAMODE (6 sites)
  - `part.rs`, `kill.rs`, `knock.rs`, `vhost.rs`, `trace.rs`, `chghost.rs`, `chgident.rs` (7 sites)
  - `oper/admin.rs`: DIE, REHASH, RESTART (3 sites)
  - `server_query/info.rs`: USERIP (2 sites)
  - `channel/invite.rs`, `channel/kick.rs`, `channel/topic.rs` (5 sites)
  - `messaging/common.rs`, `bans/shun.rs`, `batch/mod.rs`, `chathistory.rs` (8 sites)
  - `mode/channel/mod.rs`, `user_query/whois/whois_cmd.rs`, `bans/xlines/mod.rs` (7 sites)

### 3.3 Remaining patterns (intentionally not migrated)
- `registry.rs` (4): Dispatch-level errors with different semantics
- `helpers.rs` (2): Macro infrastructure, already DRY
- `context.rs` (2): The helper implementation itself
- `ping.rs` (1): Uses `with_label()` wrapper for labeled-response
- `kick.rs` (1): Conditional send pattern (only sends if error is Some)

---

## Commits

1. `bb68b43` - DRY Phase 1: Add server_prefix(), authority(), migrate all usages (52 files, +408/-296)
2. `678dec6` - DRY Phase 2: Add require_arg_or_reply! macro
3. `f9c1c62` - DRY Phase 2: Add send_noprivileges! macro
4. `11cd342` - Update DRY_REFACTOR_PLAN.md with completed items
5. `5f31786` - DRY Phase 2: Add require_admin_cap!/require_oper_cap!, migrate 12 handlers (-236 lines)
6. *(pending)* - DRY Phase 3: Add ctx.send_error(), migrate 28 error sites

---

## Verification Commands

```bash
# Count remaining patterns after each phase:
rg -c "\.with_prefix\(Prefix::ServerName\(" slircd-ng/src/
rg -c "CapabilityAuthority::new\(ctx\.matrix\.clone\(\)\)" slircd-ng/src/
rg -c "let server_name = &ctx\.matrix\.server_info\.name" slircd-ng/src/
rg -c "crate::metrics::record_command_error" slircd-ng/src/
```

---

## Completion Criteria

- [x] All `cargo clippy --workspace -- -D warnings` passes
- [x] All `cargo test --workspace` passes
- [x] Pattern counts reduced to near-zero for migrated patterns
- [ ] No regression in irctest compliance (pending test run)

---

## Notes

- Each phase builds on previous - complete in order
- Commit after each major item for easy rollback
- Delete this file after all items complete
