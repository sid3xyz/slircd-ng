# Outstanding Issues - Priority Assessment
**PERMANENT NOTICE: This software is NEVER production ready. All documentation, instructions, and statements herein are for developer reference only.**

**Last Updated:** December 6, 2025
**Status:** 3 open issues, 11 closed issues

## Summary

After completing RFC compliance improvements and architecture audit, the following issues remain open. They are categorized by priority and impact.

---

## 🔴 Critical Priority (Race Conditions)

### #12: Critical Race Condition: Channel Removal causing Split-Brain State
**Impact:** High
**Complexity:** High
**Description:** When last user leaves channel, there's a window where new joins could create duplicate channel actors.

**Root Cause:** TOCTOU between checking if channel is empty and removing it from Matrix.channels

**Proposed Solution:**
```rust
// In ChannelActor, when last member leaves:
1. Mark channel as "closing" state
2. Reject new joins during closing
3. Send final cleanup event to Matrix
4. Matrix removes channel atomically
```

**Blocker:** No current failures observed, but theoretical issue

---

## 🟡 High Priority (Performance)

### #16: Performance Defect: WHOIS handler holds Async Lock
**Impact:** Medium (performance degradation under load)
**Complexity:** Low
**Description:** WHOIS handler holds RwLock while awaiting channel queries.

**Current Issue:**
```rust
let user = matrix.users.get(&uid)?;
let user_read = user.read().await; // Held during channel queries
// ... multiple async operations ...
```

**Proposed Solution:**
```rust
// Clone needed data, drop lock immediately
let (channels, account) = {
    let user = user.read().await;
    (user.channels.clone(), user.account.clone())
}; // Lock dropped
// Now query channels without holding user lock
```

**Blocker:** None - simple refactor

---

## 🟢 Low Priority (Code Quality)

### #10: Refactor handle_join and handle_message in ChannelActor
**Impact:** Low (code maintainability)
**Complexity:** Medium
**Description:** Functions have many parameters (>7), suppressed with #[allow(clippy::too_many_arguments)]

**Proposed Solution:**
```rust
// Group parameters into context structs
struct JoinContext {
    uid: Uid,
    nick: String,
    sender: mpsc::Sender<Message>,
    caps: HashSet<String>,
    user_context: Box<UserContext>,
    key: Option<String>,
    initial_modes: Option<MemberModes>,
}

async fn handle_join(&mut self, ctx: JoinContext, ...) { }
```

**Blocker:** None - pure refactor, no behavior change

---

## Recommended Priority Order

1. **#16** - WHOIS Lock Issue (Quick performance win)
2. **#12** - Split-Brain Channels (Complex, low observed frequency)
3. **#10** - Parameter Refactor (Code quality, non-blocking)

---

## Recently Closed Issues (Reference)

- ✅ #13: TOCTOU nick claiming (Fixed d6d57b5 - atomic entry() API)
- ✅ #15: Ghost members (Fixed d6d57b5 - session_id validation)
- ✅ #19: Resource Exhaustion: Unbounded invite list growth (Fixed with TTL + cap)
- ✅ #20: Memory Leak: user_nicks cleanup (Fixed in PART/QUIT handlers)
- ✅ #18: Stale Data: user_nicks NICK updates (Fixed with actor event)
- ✅ #17: Resource Exhaustion: Duplicate list modes (Fixed with deduplication)
- ✅ #14: Logic Error: Rejoin mode reset (Fixed with mode preservation)
- ✅ #11: Clippy warnings (Fixed, all passing)
- ✅ #9: Refactor ChannelEvent enum size (Completed with Box<T>)

---

## Risk Assessment

| Issue             | Likelihood | Impact | Risk Score |
| ----------------- | ---------- | ------ | ---------- |
| #12 (Split-Brain) | Low        | High   | 🟡 Medium   |
| #16 (WHOIS Lock)  | High       | Low    | 🟡 Medium   |
| #10 (Parameters)  | N/A        | Low    | 🟢 Low      |

---

## Testing Strategy

For each fix:
1. Add unit test demonstrating the race condition
2. Verify fix with concurrent stress test
3. Run full RFC compliance suite
4. Monitor for regressions in production

---

**Notes:**
- All race conditions are theoretical - no production failures observed
- Performance issues are under load only
- Code quality issues don't affect functionality
- Actor model architecture prevents most common concurrency bugs
