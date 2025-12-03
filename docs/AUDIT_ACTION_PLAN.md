# slircd-ng Audit Action Plan

**Generated:** 2024-12-03  
**Status:** In Progress

## Overview

This document tracks the actionable items identified from the architecture audit.
Each task is prioritized by impact and effort.

---

## 🔴 Critical (High Impact, Security/Correctness)

### C1: Fix Default Cloak Secret
- **Status:** TODO
- **Files:** `src/config.rs:174`
- **Effort:** Low
- **Impact:** HIGH
- **Description:** Current implementation falls back to static `"slircd-default-secret-CHANGE-ME-IN-PRODUCTION"`. 
- **Fix:** Panic at startup if not explicitly set, OR generate random ephemeral secret with warning.

### C2: Run History Prune at Startup
- **Status:** TODO
- **Files:** `src/main.rs:204-220`
- **Effort:** Low
- **Impact:** Medium
- **Description:** Only runs on 24h interval. If server restarts frequently or crashes, pruning never happens.
- **Fix:** Call `db.history().prune_old_messages(30)` once before starting the interval.

---

## 🟠 Architecture Improvements (DRY Violations)

### A1: Refactor Gateway Triplication
- **Status:** TODO
- **Files:** `src/network/gateway.rs`
- **Effort:** Medium
- **Impact:** HIGH
- **Description:** Lines 105-329 contain near-identical logic 3x (TLS/WebSocket/Plaintext).
- **Fix:** Extract to generic `accept_connection<S: Stream>()` function.

### A2: Generic X-line Handler
- **Status:** TODO
- **Files:** `src/handlers/bans/xlines/*.rs`
- **Effort:** Medium
- **Impact:** HIGH
- **Description:** 6 files with identical structure. Create `GenericBanHandler<B: BanStrategy>` trait.

### A3: Unify SAJOIN/SAPART with Core Logic
- **Status:** TODO
- **Files:** `src/handlers/admin.rs`, `src/handlers/channel/join.rs`
- **Effort:** Medium
- **Impact:** Medium
- **Description:** SAJOIN duplicates join logic. Refactor to accept `bypass_checks: bool`.

### A4: Singleton NickServ/ChanServ
- **Status:** TODO
- **Files:** `src/services/mod.rs:110-121`
- **Effort:** Low
- **Impact:** Low
- **Description:** Services instantiated per-message. Pre-instantiate and inject via Context.

---

## 🟡 Code Quality (Overcomplication/Hacks)

### Q1: Activate ChannelModeBuilder
- **Status:** TODO
- **Files:** `src/state/mode_builder.rs`, `src/handlers/mode/channel.rs`
- **Effort:** High
- **Impact:** Medium
- **Description:** `mode_builder.rs` is dead code. Use it to clean up 809-line `channel.rs`.

### Q2: Audit `.to_string()` in Hot Paths
- **Status:** TODO
- **Files:** `src/handlers/**/*.rs`
- **Effort:** Medium
- **Impact:** Low
- **Description:** Keep `&str` for lookups, only allocate when storing.

### Q3: Implement DIE/REHASH/RESTART
- **Status:** TODO
- **Files:** `src/handlers/oper.rs`
- **Effort:** Medium
- **Impact:** Low
- **Description:** Commands exported but appear to be stubs.

### Q4: Complete STATS 'm' Command
- **Status:** TODO
- **Files:** `src/handlers/server_query/stats.rs`
- **Effort:** Low
- **Impact:** Low
- **Description:** Add command counter in Registry, expose via STATS m.

---

## 🟣 Persistence/State (Database Issues)

### P1: Live Ban Reload Mechanism
- **Status:** TODO
- **Files:** `src/security/ip_deny_list.rs`, `src/main.rs`
- **Effort:** Medium
- **Impact:** Medium
- **Description:** IP deny list loaded only at startup. Add SIGHUP/REHASH reload.

### P2: Verify Matrix Locking Patterns
- **Status:** TODO
- **Files:** `src/state/matrix.rs`
- **Effort:** Medium
- **Impact:** HIGH
- **Description:** Nested locking with `DashMap<Uid, Arc<RwLock<User>>>`. Audit for deadlocks.

---

## Execution Order

1. ✅ C1 - Default cloak secret (Security, 15 min)
2. ✅ C2 - Startup prune (Correctness, 10 min)
3. ⬜ A1 - Gateway refactor (Biggest DRY win)
4. ⬜ A2 - X-line generic (Second biggest DRY win)
5. ⬜ P2 - Matrix locking audit (Deadlock risk)
6. ⬜ A3 - SAJOIN/SAPART (State corruption prevention)
7. ⬜ Q1 - Mode builder (Large cleanup)
8. ⬜ P1 - Live ban reload (Nice to have)
9. ⬜ A4/Q2/Q3/Q4 - Lower priority polish

---

## Changelog

- 2024-12-03: Initial plan created from audit findings
