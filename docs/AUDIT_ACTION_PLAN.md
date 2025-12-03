# slircd-ng Audit Action Plan

**Generated:** 2024-12-03
**Updated:** 2024-12-03
**Status:** High-priority items complete

## Overview

This document tracks the actionable items identified from the architecture audit.
Each task is prioritized by impact and effort.

---

## 🔴 Critical (High Impact, Security/Correctness)

### C1: Fix Default Cloak Secret ✅
- **Status:** DONE (commit 5136147)
- **Files:** `src/config.rs`
- **Effort:** Low
- **Impact:** HIGH
- **Change:** Generate random 32-char secret at runtime with warning log.

### C2: Run History Prune at Startup ✅
- **Status:** DONE (commit ec39347)
- **Files:** `src/main.rs`
- **Effort:** Low
- **Impact:** Medium
- **Change:** Added immediate prune before interval timer starts.

---

## 🟠 Architecture Improvements (DRY Violations)

### A1: Refactor Gateway Triplication ✅
- **Status:** DONE (commit 52e5042)
- **Files:** `src/network/gateway.rs`
- **Effort:** Medium
- **Impact:** HIGH
- **Change:** Extracted `validate_and_prepare_connection()` helper, reduced 44 lines.

### A2: Generic X-line Handler ✅
- **Status:** DONE (commit 9c42f2a)
- **Files:** `src/handlers/bans/xlines/`
- **Effort:** Medium
- **Impact:** HIGH
- **Change:** Created `BanConfig` trait system, consolidated 6 files into 1, reduced 113 lines.

### A3: Unify SAJOIN/SAPART with Core Logic ✅
- **Status:** DONE (commit 23867ac)
- **Files:** `src/handlers/admin.rs`, `src/handlers/channel/ops.rs`
- **Effort:** Medium
- **Impact:** Medium
- **Change:** Created reusable `force_join_channel()` and `force_part_channel()` helpers.

### A4: Singleton NickServ/ChanServ ✅

- **Status:** DONE (commit 09a6c51)
- **Files:** `src/services/mod.rs`, `src/state/matrix.rs`
- **Effort:** Low
- **Impact:** Low
- **Change:** Services created once at startup, stored in Matrix, reused for all messages.

---

## 🟡 Code Quality (Overcomplication/Hacks)

### Q1: Activate ChannelModeBuilder ✅

- **Status:** DONE (commit fe48e66)
- **Files:** `src/state/mode_builder.rs`, `src/services/mod.rs`, `src/handlers/channel/join.rs`
- **Effort:** Medium
- **Impact:** Medium
- **Change:** Activated ChannelModeBuilder infrastructure:
  - Added `ServiceEffect::ChannelModes` for batch mode changes
  - Added `parse_mlock()` function with 7 unit tests
  - Implemented MLOCK enforcement on channel creation
  - ChanServ `SET MLOCK +nt-s` now enforced when channel is created

### Q2: Audit `.to_string()` in Hot Paths

- **Status:** TODO
- **Files:** `src/handlers/**/*.rs`
- **Effort:** Medium
- **Impact:** Low
- **Description:** Keep `&str` for lookups, only allocate when storing.

### Q3: Implement DIE/REHASH/RESTART ✅

- **Status:** DONE (already implemented)
- **Files:** `src/handlers/oper.rs`
- **Effort:** N/A
- **Impact:** Low
- **Note:** Commands are fully implemented, not stubs. DIE initiates shutdown,
  REHASH reloads IP deny list from database.

### Q4: Complete STATS 'm' Command ✅

- **Status:** DONE (already implemented)
- **Files:** `src/handlers/server_query/stats.rs`, `src/handlers/mod.rs`
- **Effort:** N/A
- **Impact:** Low
- **Note:** STATS m is fully implemented with command counters in Registry.

---

## 🟣 Persistence/State (Database Issues)

### P1: Live Ban Reload Mechanism

- **Status:** TODO
- **Files:** `src/security/ip_deny_list.rs`, `src/main.rs`
- **Effort:** Medium
- **Impact:** Medium
- **Description:** IP deny list loaded only at startup. Add SIGHUP/REHASH reload.

### P2: Verify Matrix Locking Patterns ✅

- **Status:** DONE (commit 1c74060)
- **Files:** `src/state/matrix.rs`
- **Effort:** Medium
- **Impact:** HIGH
- **Change:** Audited locking patterns - no deadlocks found. Added lock order documentation.

---

## Execution Order

1. ✅ C1 - Default cloak secret (Security)
2. ✅ C2 - Startup prune (Correctness)
3. ✅ A1 - Gateway refactor (Biggest DRY win)
4. ✅ A2 - X-line generic (Second biggest DRY win)
5. ✅ P2 - Matrix locking audit (Verified safe)
6. ✅ A3 - SAJOIN/SAPART (State corruption prevention)
7. ✅ A4 - Service singletons (Minor optimization)
8. ✅ Q1 - Mode builder (MLOCK enforcement)
9. ✅ Q3 - DIE/REHASH/RESTART (Already implemented)
10. ✅ Q4 - STATS m command (Already implemented)
11. ⬜ P1 - Live ban reload (Nice to have)
12. ⬜ Q2 - Audit .to_string() in hot paths
13. ✅ D1 - Documentation Overhaul (Industry parity)

---

## 🔵 Documentation

### D1: Documentation Overhaul

- **Status:** DONE
- **Files:** `docs/COMMANDS.md`, `docs/MODES.md`, `docs/CONFIGURATION.md`
- **Effort:** High
- **Impact:** High
- **Change:** Created comprehensive command reference (70+ commands), mode reference (all user/channel modes), verified CONFIGURATION.md covers all sections.

---

## Changelog

- 2024-12-03: Initial plan created from audit findings
- 2024-12-03: Completed C1, C2, A1, A2, A3, A4, P2 (7 of 11 items)
- 2024-12-03: Completed Q1 (MLOCK enforcement), verified Q3/Q4 already done (10 of 12 items)
- 2024-12-03: Completed D1 (Documentation Overhaul) - created COMMANDS.md, MODES.md (11 of 13 items)
