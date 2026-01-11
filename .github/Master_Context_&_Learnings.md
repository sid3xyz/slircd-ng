# Master Context & Learnings (slircd-ng)

## Current Focus (Integration Testing Framework - Tier 1.2)

**Status**: 🟡 **IN PROGRESS** — Building comprehensive integration test suite

**Branch**: `test/integration-framework`

**Objective**: Implement Tier 1.2 from ROADMAP (Integration Testing Framework)
- **Current Phase**: 1.2.2.2 — Command integration tests (continuing expansion)
- **Foundation Complete**: TestServer + TestClient infrastructure operational

**Why This Matters**: 
- Only 3 integration tests existed (chrono + CRDT + IRCv3 features)
- Zero end-to-end connection/command tests before this work
- Unknown failure modes in production scenarios
- BLOCKING for Alpha release

**Implementation Plan**:
1. ✅ Merge protocol-first work to main
2. ✅ Create connection lifecycle integration tests (5 tests, infrastructure complete)
3. ✅ Channel operation tests (5 tests: PART, TOPIC, INVITE, KICK, NAMES/WHOIS)
4. 🟡 Command integration tests (11 tests added: AWAY, NICK, MODE, USERHOST, QUIT, LIST, WHO, WHOWAS) — **IN PROGRESS**
5. ⏳ Service integration tests
6. ⏳ Load testing infrastructure
7. ⏳ Chaos engineering tests
8. ⏳ Fuzz testing setup

**Latest Milestone** (Jan 11, 2026):
- User command tests: 6 tests (AWAY, NICK changes, MODE self, USERHOST, QUIT with reason)
- Channel query tests: 5 tests (LIST, LIST with pattern, WHO channel, WHO nick, WHOWAS)
- Test suite: 641 total tests passing (611 unit + 30 integration)

## Truth Timeline (key commits)

- `2962079`: Integration test infrastructure complete (TestServer + TestClient + 5 connection lifecycle tests).
- `e33133c`: S2S rate limiting implemented (Tier 1.3.1.2).
- `265cc6b`: S2S TLS support implemented (Tier 1.3.1.1).
- `ff79d31` / `7db5701`: Privacy-preserving RBL service landed (Tier 1.3.1.3).
- `6495664`: Initial sweep fixing a set of DashMap+await hazards (later discovered to be incomplete).
- `008f370`: Expanded codebase sweep (architectural enforcement):
  - Introduced `DashMapExt` helpers to make safe access easy/consistent.
  - Refactored handlers/services/managers to clone underlying values (`.value().clone()` / `get_cloned`) before awaiting.
- `aeb022d`: Corrected roadmap note to reflect the expanded sweep and avoid under-reporting scope.
- `20121b9`: Added this Master Context file.

## Current Session (2026-01-10)

- Branch: `audit/preflight-sanitization` (continued work on `feat/s2s-feature-complete`)
- Objective: pre-flight sanitation to ensure we do not regress into:
  - `#[allow(dead_code)]` / `#[allow(unused_*)]` as warning silencers
  - “Legacy/Stub” placeholder methods or dead APIs
  - commented-out legacy blocks or vestigial debug logging
  - unused imports left behind by mechanical refactors
- Standard: delete dead code or wire it up; do not suppress warnings.

Key outcomes (high level):

- S2S topology semantics hardened: treat `via` as the introducer/parent (uplink) to preserve spanning-tree meaning; derive “next hop” by walking parent pointers to reach a directly-connected peer.
- S2S handler completeness: added server-side handling for `TOPIC` and `KICK` and wired them into the command registry.
- Service routing activation: routed messages targeting local service UIDs now execute the service command server-side with a S2S-safe “no sender” effect application path.
- Dead-code cleanup: removed/activated dead fields/APIs to eliminate `#[allow(dead_code)]` in `src/**`.
- Protocol alignment: added typed `Command::ENCAP` support in `slirc-proto` (parse + serialize + encode + test) and switched S2S ENCAP propagation in `slircd-ng` to use the typed variant (no `Command::Raw("ENCAP", ...)`).
- Protocol alignment (TS6 PASS): standardized S2S handshakes on typed `Command::PassTs6 { password, sid }` (canonical `PASS <password> TS 6 :<sid>`), removed the remaining `TS=6`/`Command::Raw("PASS", ...)` usages, and made the handshake machine reject non-TS6 `PASS` while enforcing PASS/SERVER SID consistency.
- Quality gates verified on this branch: `cargo fmt -- --check`, `cargo clippy -- -D warnings`, `cargo test`.

## Learnings / Rules

- **DashMap guards are locks**: `DashMap::get()` / `iter()` return guard types that hold a shard lock.
- **Never await with a guard live**:
  - Avoid patterns like `let x = map.get(..); ... await ...`.
  - Avoid `Option<Ref>::cloned()` / `map(|r| r.clone())` on DashMap results.
- **Preferred patterns**:
  - Use `DashMapExt::get_cloned()` when available.
  - Otherwise: `map.get(key).map(|r| r.value().clone())`.
  - For fanout: collect cloned senders/Arcs into a `Vec<_>` first, then `await` sends/locks.

### Integration Testing Patterns (Jan 11, 2026)

**IRC Server Behavior Constraints**:
- **Auto-op behavior**: First user to JOIN a channel automatically receives `+o` (operator privileges).
  - Tests must account for this: sequence JOINs to control who gets auto-op.
- **Async broadcast propagation**: IRC commands (JOIN, PART, TOPIC, MODE) broadcast to all channel members asynchronously.
  - Requires drain sleeps (50-150ms) to ensure all responses received before assertions.
- **Response ordering**: Welcome bursts, automatic NAMES responses, and command replies can interleave.
  - Use `recv_until()` predicates for flexible matching instead of exact message ordering.

**Deterministic Testing Patterns**:
1. **Sequential JOINs with delays**: Use `tokio::time::sleep(Duration::from_millis(50))` between JOINs to establish consistent auto-op behavior.
2. **Drain welcome bursts**: After registration, drain all welcome messages (001-376) before starting tests.
3. **Drain JOIN responses**: After JOIN, drain automatic responses (JOIN echo, NAMES, mode, topic) before expecting specific replies.
4. **Flexible response matching**: Use `params.iter().any(|p| ...)` and `params.last()` for checking response content instead of fixed param indices.
5. **Timing**: 50ms delays for sequential operations, 150ms drain sleeps for async broadcasts.

**TestClient Helper Patterns**:
- All helpers marked with `#[allow(dead_code)]` with rationale comments: "per-binary clippy requires dead_code annotation for test helpers".
- Raw command helpers (`send_raw()`, `mode_channel_op()`) for protocol-level operations not yet abstracted.
- High-level helpers (`join()`, `privmsg()`, `topic()`) for common test scenarios.
- Drain patterns: `recv_until()` with predicates for consuming automatic responses.

## Open Work (next reasonable steps)

- Pre-flight sanitation: identify and remove vestigial/legacy code paths where safe.
- Continue scanning for any subtle guard-lifetime leaks (e.g., guards stored in locals spanning control-flow that later awaits).
- Keep quality gates green: `cargo fmt -- --check`, `cargo clippy -- -D warnings`, `cargo test`.

## Session: Known Command De-Stringification (Jan 10, 2026)

- **Protocol alignment (REGISTER + remaining known commands)**: 
  - Added `Command::REGISTER { account: String, message: Option<String> }` to slirc-proto with full serialization/encoding support and tests.
  - Audited remaining 7 `Command::Raw(` usages in slircd-ng (account/user_status/batch/sync modules).
  - Converted 6 known-command Raw usages to typed variants:
    - `Command::FAIL(...)` (3x: account.rs, user_status.rs, batch/mod.rs)
    - `Command::REGISTER { ... }` (1x: account.rs)
    - `Command::GLINE/ZLINE/RLINE/SHUN` and their UNXXX variants (2x: sync/observer.rs), removing dynamic string-based command construction in favor of match statements on `GlobalBanType` enum.
  - Removed dead methods `GlobalBanType::command_name()` and `GlobalBanType::unset_command_name()` (8 associated tests deleted) as they are no longer needed with typed commands.
  - Only 1 Raw remains: `CHATHISTORY` batch response in chathistory/batch.rs—this is a synthetic/dynamic server response (not a standard IRC command), so Raw is appropriate here.
- **Zero-cruft hygiene**: Deleted obsolete test methods immediately upon removal of their tested functions.
- **Quality gates**: All pass—format check, clippy with `-D warnings`, and test suite (611 unit + integration tests, down from 619 due to removal of obsolete tests).
- **Status**: Known command de-stringification complete. Daemon now uses typed protocol commands exclusively for all standard IRC/S2S operations.

## Session: Channel Operation Integration Tests (Jan 11, 2026)

**Branch**: `test/integration-framework` (continued)

**Objective**: Expand integration test coverage to validate channel operations with multiple concurrent clients.

**Key Changes**:
1. **TestClient enhancements** (tests/common/client.rs):
   - Added `mode_channel_op(channel, nick)` helper to grant +o privileges using raw MODE command.
   - Established pattern: all helpers marked `#[allow(dead_code)]` with rationale comments for per-binary clippy.

2. **Channel operation tests** (tests/channel_ops.rs) — 5/5 passing:
   - `test_part_broadcast`: Validates PART message broadcast to channel members with reason.
   - `test_topic_broadcast`: Validates TOPIC change propagation; fixed race condition via sequential JOINs (alice first, 50ms delay, bob second, 150ms drain).
   - `test_invite_flow`: Validates INVITE message delivery and successful JOIN of invited user.
   - `test_kick_requires_op_and_succeeds_with_op`: Validates KICK privilege enforcement (bob JOINs first gets +o, alice JOINs second gets none, alice KICK fails with 482, bob grants alice +o, alice KICKs bob successfully).
   - `test_names_and_whois`: Validates NAMES numeric (353) and WHOIS numeric (311) with flexible param matching (last param contains both nicks).

3. **Bug fixes**:
   - **KICK test race condition**: Original test had alice JOIN first (auto +o), couldn't test unprivileged KICK. Fixed by having bob JOIN first (gets +o), alice JOIN second (no +o), bob grants alice +o after initial failure.
   - **NAMES test parsing**: Fixed param indexing; used `params.iter().any(|p| p == "#ops")` and `params.last()` for flexible matching of RPL_NAMREPLY (353) format.
   - **TOPIC test race condition**: Both clients JOINing simultaneously caused bob to miss TOPIC message. Fixed by sequential JOINs (alice first, 50ms delay, bob second) and 150ms drain sleep before TOPIC command.

4. **Deterministic testing patterns established**:
   - **Sequential JOINs**: Use `tokio::time::sleep(50ms)` between JOINs to control auto-op behavior.
   - **Drain timing**: 150ms drain sleeps to accommodate async broadcast propagation.
   - **Flexible response matching**: Use `recv_until()` predicates and param iteration instead of fixed indexing.
   - **Auto-op handling**: Document and account for first-joiner-gets-op behavior in test design.

5. **Test suite status**:
   - **Total tests**: 630 (611 unit + 19 integration)
   - **Integration suites**: connection_lifecycle (4), channel_flow (1), channel_ops (5), chrono_check (1), distributed_channel_sync (5), ircv3_features (3)
   - **All suites passing**: channel_ops 5/5 in 0.59s, full suite in ~1.5s

6. **Quality gates**: 
   - ✅ `cargo test --tests`: 630 passed
   - ✅ `cargo clippy --test channel_ops -- -D warnings`: No warnings in test code
   - ⚠️ Upstream clippy warnings in main codebase (51 errors from handlers/state/sync modules) — not introduced by this session

**Outcomes**:
- Validated MODE/KICK/INVITE/NAMES/WHOIS command flows via end-to-end integration tests.
- Established robust testing patterns for async IRC operations with concurrent clients.
- Documented IRC server behavior constraints (auto-op, async broadcasts, response ordering).
- Advanced Tier 1.2 Integration Testing from connection lifecycle to channel operations.

**Next Steps**:
- Continue expanding command coverage (remaining 76 commands).
- Service integration tests (NickServ/ChanServ).
- Load/chaos/fuzz testing infrastructure.
