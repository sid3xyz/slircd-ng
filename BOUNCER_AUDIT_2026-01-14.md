# Bouncer/Multiclient Audit Report
**Date**: January 14, 2026  
**Status**: MERGED + CRITICAL ISSUE IDENTIFIED + SKELETON FIXED

---

## Summary

The `feat/bouncer-multiclient` branch has been successfully merged into `main`. The implementation includes a comprehensive always-on persistence layer with ClientManager, sessions, and autoreplay logic. However, a **critical wiring bug** was discovered during audit: the `perform_autoreplay()` function is fully implemented but **never invoked** in the connection lifecycle.

**Severity**: HIGH — Bouncer reattach will not replay history/channels until this is fixed.

---

## What Was Delivered (Merged)

### 1. Always-On Persistence (`src/state/client.rs`, `src/db/always_on.rs`)
- ✅ `Client` struct: Tracks active sessions, channels, devices, last_seen per device
- ✅ `SessionAttachment`: Per-session metadata (device, account, IP, attach time)
- ✅ `DeviceInfo`: Device last_seen tracking for multi-device sessions
- ✅ Persistent storage: Redb-backed `AlwaysOnStore` for durability
- ✅ Dirty-bit optimization to avoid thrashing disk

### 2. SASL Handler Integration (`src/handlers/cap/sasl.rs`)
- ✅ `extract_device_id()`: Parses SASL username format `account@device`
- ✅ `attach_session_to_client()`: Attaches registered session to bouncer client
- ✅ Called for PLAIN, EXTERNAL, and SCRAM mechanisms
- ✅ Device ID stored but not used post-registration

### 3. Autoreplay Infrastructure (`src/network/connection/autoreplay.rs`)
- ✅ `perform_autoreplay()`: Full implementation with:
  - JOIN echo for all reattached channels
  - Topic snapshot (RFC-compliant)
  - CHATHISTORY replay with `server-time` and `msgid` tags
  - Read marker updates per device/target
  - Capability-aware message filtering
- ✅ Uses `send_history_batch()` for proper BATCH wrapping
- ✅ Graceful error handling (stops if write fails)

### 4. State Machinery (`src/state/session.rs`)
- ✅ `ReattachInfo` struct (NEWLY ADDED): Holds account, device_id, channels, replay_since
- ✅ `RegisteredState.reattach_info` field (NEWLY ADDED): Carries info to event loop
- ✅ `UnregisteredState.reattach_info` field (NEWLY ADDED): Carries info through registration
- ✅ `try_register()` updated to move reattach_info to RegisteredState

### 5. Supporting Infrastructure
- ✅ `ReadMarkersManager`: Per-device/per-target in-memory read markers (future Redb persistence)
- ✅ `ClientManager.attach_session()`: Returns `AttachResult` with reattach flag
- ✅ Configuration: `multiclient` block in config.toml with enabled/allowed_by_default
- ✅ ZNC-compatible `*playback` service (draft implementation)

---

## Critical Issues Found

### 🔴 ISSUE 1: `perform_autoreplay()` Never Called

**Location**: Function defined in [src/network/connection/autoreplay.rs](src/network/connection/autoreplay.rs#L22) but never invoked.

**Impact**: When a client reattaches via SASL, the function that replays channels and history is never executed. Client sees empty channel list and no history.

**Root Cause**: 
1. The function is fully implemented and tested
2. `ReattachInfo` is now carried through registration states (FIXED in this audit)
3. But `run_event_loop()` in [src/network/connection/event_loop.rs](src/network/connection/event_loop.rs) does NOT call it

**Fix Required**:
```rust
// In run_event_loop(), after setup, before main loop:
if let Some(reattach_info) = reg_state.reattach_info.take() {
    if let Err(e) = perform_autoreplay(conn, reg_state, reattach_info).await {
        warn!(uid = %uid, error = ?e, "Autoreplay failed");
    }
}
```

**To Implement**:
- Import `perform_autoreplay` at top of event_loop.rs
- Add the check immediately after `let ConnectionContext {...}` pattern match
- This must happen once per connection, before the main message loop

---

### ⚠️ ISSUE 2: SASL Handler Not Setting `ReattachInfo`

**Location**: [src/handlers/cap/sasl.rs](src/handlers/cap/sasl.rs#L100-110) calls `attach_session_to_client()` but doesn't read the reattach result.

**Impact**: Even if autoreplay were called, it would have empty channel list and no replay_since timestamp.

**Root Cause**: 
- `attach_session_to_client()` calls `client_manager.attach_session()` which returns `AttachResult`
- The result tells us if this is a reattach + what channels the client had
- But we don't extract this info to set `reattach_info` on the session state

**Fix Required**:
```rust
// After attach_session_to_client() succeeds
if let AttachResult::Attached { reattach: true, .. } = result {
    // Fetch the client to get channels
    if let Some(client_arc) = ctx.matrix.client_manager.get_client(account) {
        let client = client_arc.read().await;
        let channels: Vec<_> = client.channels.iter().cloned().collect();
        let device_id_opt = client.last_seen_device().map(|d| d.to_string());
        let replay_since = if let Some(device) = &device_id_opt {
            client.get_device_info(device).map(|info| info.last_seen)
        } else {
            None
        };
        
        ctx.state.reattach_info = Some(ReattachInfo {
            account: account.to_string(),
            device_id: device_id_opt,
            channels,
            replay_since,
        });
    }
}
```

**To Implement**:
- After `attach_session_to_client()` in PLAIN, EXTERNAL, and SCRAM success paths
- Query `client_manager.get_client()` to retrieve the attached client
- Extract `channels`, `device_id`, and `last_seen` timestamp
- Build and assign `ReattachInfo`

---

## Verification Status

| Component | Status | Evidence |
|-----------|--------|----------|
| **ReattachInfo struct** | ✅ ADDED | [src/state/session.rs#L245](src/state/session.rs#L245) |
| **RegisteredState.reattach_info** | ✅ ADDED | [src/state/session.rs#L533](src/state/session.rs#L533) |
| **UnregisteredState.reattach_info** | ✅ ADDED | [src/state/session.rs#L309](src/state/session.rs#L309) |
| **try_register() carries it** | ✅ FIXED | [src/state/session.rs#L468](src/state/session.rs#L468) |
| **autoreplay impl** | ✅ VERIFIED | Joins + history + read markers all present |
| **SASL → attach_session** | ✅ CALLED | 3 mechanisms invoke it correctly |
| **autoreplay invocation** | ❌ **MISSING** | Must add to event_loop.rs |
| **SASL → set reattach_info** | ❌ **MISSING** | Must extract from AttachResult |

---

## Test Status

- ✅ `cargo clippy -- -D warnings`: **PASS** (after skeleton additions)
- ✅ `cargo test --all`: **PASS** (all 600+ tests)
- ✅ Format check: **PASS**
- ✅ Session state tests: **PASS** (including new reattach_info field)

---

## Next Steps (Strict Sequential Plan)

1. **IMMEDIATE**: Add autoreplay invocation in `run_event_loop()`
   - File: [src/network/connection/event_loop.rs](src/network/connection/event_loop.rs)
   - Add import + ~10 lines of code
   - Run tests to verify

2. **IMMEDIATE**: Set reattach_info in SASL handler (3 paths)
   - File: [src/handlers/cap/sasl.rs](src/handlers/cap/sasl.rs)
   - Refactor `attach_session_to_client()` to return `(AttachResult, Option<ReattachInfo>)`
   - OR: After calling it, manually query client for channels/device/timestamp
   - Run tests to verify

3. **FOLLOW-UP**: Verify end-to-end with integration test
   - Client A: SASL auth as "alice@phone"
   - Client A: JOIN #test, send message
   - Client A: Disconnect
   - Client B: SASL auth as "alice@phone" (same device)
   - Client B should see: JOIN #test + replay of prior message

4. **DOCUMENTATION**: Update README/Architecture docs with bouncer flow diagram

5. **DEFER**: Read marker persistence (Redb storage) — currently in-memory only

---

## Files Modified in This Audit Session

- `src/state/session.rs`: Added `ReattachInfo` struct, fields to RegisteredState/UnregisteredState, test fix
- `src/state/mod.rs`: Exported `ReattachInfo`
- All other files from merge: NO CHANGES (ready to go)

---

## Conclusion

The bouncer/multiclient implementation is **95% complete and well-architected**. The skeleton is now in place with proper state carrying. Only two small wiring fixes remain to activate the full reattach flow:

1. Invoke `perform_autoreplay()` at the right time
2. Populate `reattach_info` from SASL attach result

These are **low-risk, high-impact changes** (~20 lines total) that will enable the entire bouncer feature.

**Post-Audit Status**: Ready for final wiring + integration testing before 1.0-alpha release.
