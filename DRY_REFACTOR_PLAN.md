# DRY Refactor Execution Plan

> Generated: 2025-12-16
> Status: IN PROGRESS - Phase 1 COMPLETE

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
- [x] **MIGRATE**: Replace ALL 100+ `.with_prefix(Prefix::ServerName(...))` patterns
- [x] **VERIFY**: `rg "Prefix::ServerName" src/handlers/` returns only context.rs implementation

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

## Phase 2: Macro Infrastructure (P1)

**Estimated Impact: ~250 LOC reduction**

### 2.1 Create `require_arg_or_reply!` macro
- [ ] **File**: `src/handlers/helpers.rs`
- [ ] **Action**: Extend `require_arg!` to send ERR_NEEDMOREPARAMS and record metrics
- [ ] **Impact**: Consolidates 30 `Response::err_needmoreparams` + metrics patterns

### 2.2 Create `require_admin_cap!` macro
- [ ] **File**: `src/handlers/helpers.rs`
- [ ] **Action**: Macro for admin capability check + error handling
- [ ] **Impact**: Eliminates ~120 LOC in admin.rs (4× 30-line preambles)

### 2.3 Create `require_oper_cap!` macro
- [ ] **File**: `src/handlers/helpers.rs`
- [ ] **Action**: Similar to admin, for general oper commands
- [ ] **Impact**: Consolidates oper handler preambles across 15+ files

---

## Phase 3: Trait Unification (P2)

**Estimated Impact: ~130 LOC reduction**

### 3.1 Implement `IrcErrorReply` trait
- [ ] **File**: `src/handlers/core/context.rs` or new `src/handlers/error_reply.rs`
- [ ] **Action**: Trait combining `ctx.sender.send(err)` + `metrics::record_command_error`
- [ ] **Impact**: Consolidates 80 error+metrics patterns

### 3.2 Refactor high-frequency error patterns
- [ ] **Files**: Multiple handlers
- [ ] **Action**: Migrate `ERR_NOSUCHNICK` (17), `ERR_NOPRIVILEGES` (20), etc.
- [ ] **Method**: Use new trait methods

---

## Execution Checklist

### Phase 1 Execution
```bash
# After each change:
cargo clippy -p slircd-ng -- -D warnings
cargo test -p slircd-ng
```

### Phase 2 Execution
```bash
# Test macro expansion:
cargo expand -p slircd-ng --lib 2>&1 | head -100
```

### Phase 3 Execution
```bash
# Full validation:
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

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

- [ ] All `cargo clippy --workspace -- -D warnings` passes
- [ ] All `cargo test --workspace` passes
- [ ] Pattern counts reduced to near-zero for migrated patterns
- [ ] No regression in irctest compliance

---

## Notes

- Each phase builds on previous - complete in order
- Commit after each major item for easy rollback
- Delete this file after all items complete
