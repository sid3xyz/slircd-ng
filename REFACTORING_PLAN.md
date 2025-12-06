# Refactoring Plan: Breaking Up Monoliths & Extracting Reusable Code

**Date:** December 6, 2025
**Status:** Analysis complete, ready for implementation

## Overview

Analysis identified ~1400 lines of monolithic code and several patterns of code duplication across the codebase that should be refactored for maintainability, testability, and reusability.

---

## Priority 1: Channel Actor Monolith (1406 lines → ~500 lines target)

**File:** `src/state/actor.rs`
**Current:** Single 975-line impl block with 13 handler methods
**Issue:** All channel logic in one massive file, hard to test individual concerns

### Proposed Structure

```
src/state/
  actor/
    mod.rs              # ChannelActor struct, spawn(), run()
    types.rs            # ChannelEvent, ChannelInfo, ChannelMode enums
    handlers/
      mod.rs            # Re-exports
      join.rs           # handle_join (~130 lines)
      part_quit.rs      # handle_part, handle_quit (~50 lines)
      message.rs        # handle_message (~150 lines)
      modes.rs          # handle_apply_modes (~200 lines)
      kick.rs           # handle_kick (~80 lines)
      topic.rs          # handle_set_topic (~60 lines)
      invite_knock.rs   # handle_invite, handle_knock (~100 lines)
      broadcast.rs      # handle_broadcast* (~40 lines)
    validation/
      bans.rs           # Extract ban/except checking logic (DUPLICATED 2x)
      permissions.rs    # member_has_voice_or_higher, etc.
      invites.rs        # Invite list management (4 methods)
    modes/
      flags.rs          # set_flag_mode, replace_param_mode
      lists.rs          # apply_list_mode
      members.rs        # update_member_mode
```

### Benefits
- Each handler testable in isolation
- Ban/except checking logic unified (currently duplicated in handle_join + handle_message)
- Clear separation: event handling vs validation vs state mutation
- Easier to add new channel features (each gets own handler file)

---

## Priority 2: Extract Common Channel Validation Logic

**Files:** Multiple locations
**Issue:** Ban checking, user_mask creation, except checking duplicated 2x in actor.rs alone

### Create `src/state/actor/validation/bans.rs`

```rust
/// Check if user is banned, accounting for ban exceptions.
/// Returns Ok(()) if allowed, Err(reason) if banned.
pub fn check_banned(
    user_context: &UserContext,
    bans: &[ListEntry],
    excepts: &[ListEntry],
) -> Result<(), &'static str> {
    let user_mask = create_user_mask(user_context);

    for ban in bans {
        if matches_ban_or_except(&ban.mask, &user_mask, user_context) {
            // Check exceptions
            let is_excepted = excepts.iter()
                .any(|e| matches_ban_or_except(&e.mask, &user_mask, user_context));

            if !is_excepted {
                return Err("ERR_BANNEDFROMCHAN");
            }
        }
    }
    Ok(())
}

/// Create user mask string (nick!user@host).
pub fn create_user_mask(user_context: &UserContext) -> String {
    format!("{}!{}@{}",
        user_context.nickname,
        user_context.username,
        user_context.hostname)
}
```

### Usage Sites to Refactor
1. `src/state/actor.rs` line 476-481 (handle_join ban check)
2. `src/state/actor.rs` line 748-754 (handle_message ban check)
3. `src/handlers/oper/auth.rs` (user_mask construction)

---

## Priority 3: Connection Handler Consolidation (878 lines)

**File:** `src/network/connection.rs`
**Current:** Single file with handshake, main loop, error classification, batch state
**Issue:** Mixed concerns - protocol handling + error handling + batch management

### Proposed Structure

```
src/network/
  connection/
    mod.rs              # Main Connection struct + public API
    handshake.rs        # Handshake phase logic (~200 lines)
    main_loop.rs        # Unified zero-copy loop (~300 lines)
    error_handling.rs   # classify_read_error, ReadErrorAction (~100 lines)
    batch_state.rs      # Batch tracking (belongs with BATCH handler?)
```

### Alternative: Move Batch State
- Batch management logic (~150 lines) could move to `handlers/batch.rs`
- Connection just stores `HashMap<String, BatchState>` from batch module
- Reduces coupling between network layer and protocol layer

---

## Priority 4: Handler Module Cleanup

**File:** `src/handlers/mod.rs` (504 lines)
**Issue:** Mix of registry, middleware, helpers, and coordinator functions

### Split Into

```
src/handlers/
  mod.rs              # Just re-exports + Handler trait (~100 lines)
  registry.rs         # Handler registration + dispatch (~150 lines)
  middleware.rs       # ResponseMiddleware, labeled_ack, etc. (~100 lines)
  helpers.rs          # Already exists, keep as-is
  context.rs          # Context struct + methods (~100 lines)
```

---

## Priority 5: Large Handler Files

### batch.rs (785 lines)
Already well-structured, mostly match arms. Could extract:
- `validators/` subdir for batch validation logic
- `types.rs` for BatchState, BatchLine structs

### cap.rs (609 lines)
Single monolithic match statement. Extract:
- Each CAP subcommand to own function
- CAP negotiation state machine to separate module

### xlines/mod.rs (597 lines)
Already in own module, but could split:
- KLINE, GLINE, DLINE, ZLINE, SHUN handlers each to own file
- Common X-line logic to `xlines/common.rs`

---

## Priority 6: Duplicate User Mask Construction

**Pattern:** `format!("{}!{}@{}", nick, user, host)` appears 3+ times

### Solution: Add to helpers.rs

```rust
/// Create IRC user mask (nick!user@host).
pub fn format_user_mask(nick: &str, user: &str, host: &str) -> String {
    format!("{}!{}@{}", nick, user, host)
}
```

### Replace At:
- `src/state/actor.rs` line 474
- `src/state/actor.rs` line 746
- `src/handlers/oper/auth.rs` line 67

---

## Implementation Order

1. **Phase 1:** Extract common utilities (user_mask, ban checking) - Low risk, high reuse
2. **Phase 2:** Split actor.rs into modules - High impact, moderate risk
3. **Phase 3:** Split connection.rs - Moderate impact, low risk
4. **Phase 4:** Split handlers/mod.rs - Low impact, low risk
5. **Phase 5:** Refactor large handlers (batch, cap, xlines) - Optional polish

---

## Metrics

**Before Refactoring:**
- Largest file: 1406 lines (actor.rs)
- Total handlers code: 9780 lines in 63 files
- Code duplication: 3+ instances of user_mask, 2+ of ban checking

**After Refactoring (estimated):**
- Largest file: ~500 lines (modes handler)
- Total code: ~9800 lines in ~85 files (slight increase from new modules)
- Code duplication: 0 (all extracted to helpers)
- Testability: Each handler/validator independently testable

---

## Testing Strategy

1. **Before any refactor:** Capture current test output as baseline
2. **After each phase:** Verify all 68 tests still pass
3. **Add new tests:** For extracted utilities (ban checking, user_mask)
4. **Integration test:** Run irctest suite to verify protocol compliance unchanged

---

## Non-Goals

- **NOT refactoring:** Small focused files (< 300 lines) with single responsibility
- **NOT extracting:** Code used only once (unless it improves testability)
- **NOT changing:** Public APIs or protocol behavior
- **NOT adding:** New features (pure refactor, behavior-preserving)

---

## Risk Assessment

**Low Risk:**
- Utility extraction (user_mask, ban checking) - pure functions
- Handler module split - just moving code

**Medium Risk:**
- Actor module split - complex state + async
- Connection split - critical hot path

**Mitigation:**
- One phase at a time with full test runs
- Git commit after each successful phase
- Keep old code in comments during transition
- Run clippy + fmt after each change

---

## Priority 4: Code Duplication Cleanup (10-15% codebase reduction)

**Scope:** Supporting functions and infrastructure across the codebase

### High Priority (5-10% reduction)

#### 1. Ban Query Operations (6 files, ~200 lines of duplication)
**Location:** `src/db/bans/queries/{kline,gline,dline,zline,rline,shun}.rs`

Each ban type implements **identical CRUD operations**:
- `add_{type}()` - Nearly identical implementation across 6 files
- `remove_{type}()` - Same signature/logic duplicated 6 times
- `get_active_{type}s()` - Identical lazy expiration logic
- `matches_{type}()` - Same wildcard/CIDR matching pattern

**Recommendation:** Extract generic `GenericBanQueries<T>` trait with template-based implementations to consolidate repetitive code across K/G/D/Z/R-lines.

---

#### 2. Ban Handler Infrastructure (30-40% of xlines/mod.rs)
**Location:** `src/handlers/bans/xlines/mod.rs` (~597 lines)

Already has good generic structure (`GenericBanAddHandler<C>`, `GenericBanRemoveHandler<C>`) but the `BanConfig` trait implementations for K/G/D/Z/R-lines contain **duplicated ban type logic**:
- Type name strings repeated in error messages
- Duplicate config validation patterns
- Repeated response building for each ban type

**Recommendation:** Create a macro or centralized config registry to avoid repeating type-specific details 6 times.

---

### Medium Priority (3-10% reduction)

#### 3. Error Reply Helper Functions (~50 lines boilerplate)
**Location:** `src/handlers/helpers.rs` and scattered across handlers

Currently ~15 public error reply helpers (`err_nosuchnick`, `err_nosuchchannel`, `err_notonchannel`, etc.) all follow identical pattern:
```rust
pub fn err_X(server_name: &str, nick: &str, ...) -> Message {
    server_reply(server_name, Response::ERR_X, vec![nick, ...])
}
```

**Recommendation:** Create a macro `err_reply!(Response::ERR_X, nick, ...)` to eliminate boilerplate function definitions.

---

#### 4. Message Routing & Validation Patterns (5-10% reduction)
**Location:** `src/handlers/messaging/privmsg.rs` (389 lines) and `src/handlers/messaging/notice.rs` (160 lines)

Both handlers implement **nearly identical pre-send validation**:
- Shun checking (`is_shunned()` called identically)
- Rate limiting (`check_message_rate()` - same pattern)
- Spam detector checks (identical logic)
- CTCP rate limiting (duplicated)

The only real difference is NOTICE silently drops errors while PRIVMSG sends error replies.

**Recommendation:** Extract shared validation pipeline into common function with error handling strategy as parameter (e.g., `validate_message(ctx, text, error_strategy)`).

---

#### 5. Service Command Infrastructure (3-5% reduction)
**Location:** `src/services/nickserv/commands/` (~993 lines) and `src/services/chanserv/commands/` (~1360 lines)

Service commands repeat similar patterns:
- Account ownership checks
- Permission validation
- Error reply building
- Database error handling

**Recommendation:** Extract service command base trait with shared pre/post-hooks for auth and error handling.

---

### Low-Medium Priority (1-3% reduction each)

#### 6. User Lookup & State Extraction Patterns
**Location:** `src/handlers/mod.rs`, `src/handlers/messaging/common.rs`, and scattered across handlers (47+ instances)

Repeated pattern:
```rust
if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
    let user = user_ref.read().await;
    // Extract fields...
}
```

**Recommendation:** Create helper macro like `get_user_fields!(ctx.uid, |user| { ... })` to reduce repetition.

---

#### 7. Database Query Wrapper Methods (~150 lines boilerplate)
**Location:** `src/db/bans/queries/mod.rs` lines 20-289

The `BanRepository` has **28 wrapper methods** that simply delegate to module-level functions with `.await`. Example:
```rust
pub async fn add_kline(...) -> Result<(), DbError> {
    kline::add_kline(...).await
}
```

**Recommendation:** Use generics or macros to generate these thin wrapper functions, or expose module functions directly if no additional logic is needed.

---

#### 8. Ban Disconnection Logic (2-3% reduction)
**Location:** `src/handlers/bans/common.rs` and `src/handlers/bans/xlines/mod.rs`

`disconnect_matching_ban()` function consolidates disconnect logic for all ban types, but each ban type's handler still has some specialized setup around it.

**Recommendation:** Extract common pre/post-disconnect patterns to further consolidate handler code.

---

#### 9. Handler Response Building Patterns
**Location:** Throughout `src/handlers/`, especially `server_query/` and `user_query/whois/`

Handlers repeatedly build responses manually:
```rust
let reply = server_reply(&ctx.matrix.server_info.name, Response::RPL_X, vec![nick, ...]);
ctx.sender.send(reply).await?;
```

**Recommendation:** Create response builder helper like `send_reply!(ctx, Response::RPL_X, nick, ...)` to reduce boilerplate.

---

#### 10. Channel Operation Checks
**Location:** `src/handlers/channel/` (join.rs, ops.rs, kick.rs, topic.rs)

Multiple handlers perform similar checks:
- Channel existence
- User in channel
- User has op privileges
- Channel mode checks

**Recommendation:** Create shared validation helpers like `check_channel_ops_perms()`, `check_user_in_channel()`.

---

### Summary: Code Duplication

| Category | Location | Lines | Reduction Potential |
|----------|----------|-------|---------------------|
| Ban queries | `src/db/bans/queries/` | ~200 | 60-70% |
| Ban handlers | `src/handlers/bans/xlines/mod.rs` | ~150 | 40-50% |
| Error replies | `src/handlers/helpers.rs` | ~50 | 70-80% |
| Message validation | `src/handlers/messaging/` | ~100 | 50-60% |
| Service commands | `src/services/` | ~200 | 30-40% |
| User lookup macros | scattered | ~50 | 60-70% |
| DB wrappers | `src/db/bans/queries/mod.rs` | ~150 | 80-90% |
| **Total identified** | | **~900 lines** | **10-15%** |

---

## Implementation Order

1. **Phase 1 (High Value):** Ban queries generic implementation (reusable pattern)
2. **Phase 2 (High Value):** Ban handler config consolidation
3. **Phase 3 (Medium Value):** Error reply macros
4. **Phase 4 (Medium Value):** Message validation extraction
5. **Phase 5 (Medium Value):** Service command base traits
6. **Phase 6 (Low Value):** User lookup/state extraction helpers
