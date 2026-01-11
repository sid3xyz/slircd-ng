# irctest Compliance Fix Strategy

**Session Start**: 2026-01-11  
**Goal**: Achieve 100% irctest pass rate (currently 328/387 = 84.8%)  
**Methodology**: Protocol-first, architectural enforcement, systematic bottom-up fixing

## Current Baseline

```
Total:    436 tests
Passed:   328 (75.2%)
Failed:   47  (10.8%)
Skipped:  49  (11.2%)
XFailed:  5   (1.1%)
Errors:   7   (1.6%)

Eligible: 387 tests (436 - 49 skipped)
Pass Rate: 328/387 = 84.8%
```

## Failure Categories

### Priority 1: Core Protocol (Tier 1 Failures)

These are essential IRC commands where **we should be passing 100%**. Failures indicate implementation gaps, not optional features.

#### 1.1 Account Registration (4 failures)
- `testRegisterDefaultName`
- `testRegisterSameName`
- `testRegisterDifferentName`
- `testBeforeConnect`

**Root Cause Analysis**:
- REGISTER command handler missing or incomplete
- Likely in `src/handlers/account.rs` or needs new handler
- Check: does slirc-proto have Command::Register variant?

**Architecture Impact**:
- Part of account state machine
- Needs Matrix.account_manager integration
- Service effect handling required

---

#### 1.2 Core Channel Operations (✅ PASSING)
- JOIN: 8/8
- PART: 5/5
- TOPIC: 6/6
- KICK: 7/7
- INVITE: 15/16 (1 skipped)

**Status**: These validate correctly. Core channel mechanics work.

---

### Priority 2: Extended Capabilities (Tier 2 Failures)

Advanced features with specification complexity. Optional but required for modern IRCv3 compliance.

#### 2.1 CHATHISTORY (12 failures)
- testInvalidTargets
- testMessagesToSelf
- testChathistoryDMs[LATEST/BEFORE/AFTER/BETWEEN/AROUND]
- testChathistoryTagmsg
- testChathistoryDMClientOnlyTags
- testChathistoryTargets
- testChathistoryTargetsExcludesUpdatedTargets

**Root Cause**:
- Known missing feature (verified in ROADMAP_TO_1.0.md)
- Requires message history storage + query engine
- Conflicts with in-memory database design?

**Architecture**:
- Needs History backend integration (redb/sqlite)
- Query parsing and execution
- Timestamp and message-id correlation

---

#### 2.2 MONITOR Extended (8 failures)
- testExtendedMonitorAccountNotify (4 variants × 2 modes = 8)

**Root Cause**:
- Extended-monitor capability partially implemented
- Missing account-notify integration
- Check handlers/monitor.rs

**Architecture**:
- Account state transitions must emit MONITOR updates
- Requires lifecycle_manager hook

---

#### 2.3 METADATA (8 failures)
- Deprecated METADATA command (set/get/list)
- testInIsupport, testGetOneUnsetValid, testGetTwoUnsetValid, testListNoSet, testListInvalidTarget, testSetGetValid, testSetGetZeroCharInValue, testSetGetHeartInValue

**Root Cause**:
- METADATA command handler missing (deprecated spec)
- If not in ROADMAP, likely intentional omission

**Decision Point**: Is METADATA on the 1.0 roadmap?

---

### Priority 3: Tertiary Features (Tier 3 Failures)

Specialized or less-common protocol features. Lower priority but important for full compliance.

#### 3.1 Channel Forwarding (1 failure)
- testChannelForwarding

**Root Cause**: Channel +f mode handling incomplete

---

#### 3.2 Confusables (1 failure)
- testConfusableNicks

**Root Cause**: Unicode confusable detection not implemented

---

#### 3.3 Readq/Buffering (2 failures)
- testReadqTags
- testReadqNoTags

**Root Cause**: READQ command or message buffering incomplete

---

#### 3.4 RELAYMSG (1 failure)
- testRelaymsg

**Root Cause**: RELAYMSG handler incomplete

---

#### 3.5 Registered-Only Modes (3 failures)
- testRegisteredOnlySpeakMode (chmodes/ergo.py)
- testRegisteredOnlyUserMode (umodes/registeredonly.py)
- testRegisteredOnlyUserModeAcceptCommand
- testRegisteredOnlyUserModeAutoAcceptOnDM

**Root Cause**: +M channel mode and user mode not fully implemented

---

#### 3.6 UTF-8 Filtering (2 failures)
- testNonUtf8Filtering (utf8.py)
- testUtf8NonAsciiNick

**Root Cause**: UTF-8 validation or normalization incomplete

---

#### 3.7 WHOX Account (1 failure)
- testWhoxAccount (who.py::WhoServicesTestCase)

**Root Cause**: WHO extended response missing account field

---

#### 3.8 ZNC Playback (1 failure)
- testZncPlayback

**Root Cause**: ZNC-specific playback capability incomplete

---

#### 3.9 ROLEPLAY (1 failure)
- testRoleplay

**Root Cause**: ROLEPLAY capability handler missing

---

### Priority 4: Bouncer/Resume (7 errors)

All 7 are from bouncer.py - server resumption capability not implemented.

```
ERROR irctest/server_tests/bouncer.py::BouncerTestCase::testAutomaticResumption
ERROR irctest/server_tests/bouncer.py::BouncerTestCase::testChannelMessageFromOther
ERROR irctest/server_tests/bouncer.py::BouncerTestCase::testChannelMessageFromSelf
ERROR irctest/server_tests/bouncer.py::BouncerTestCase::testDirectMessageFromOther
ERROR irctest/server_tests/bouncer.py::BouncerTestCase::testDirectMessageFromSelf
ERROR irctest/server_tests/bouncer.py::BouncerTestCase::testQuit
ERROR irctest/server_tests/bouncer.py::BouncerTestCase::testDisableAutomaticResumption
```

**Root Cause**: Likely assert failures in test framework, or capability not advertised

**Decision Point**: Is bouncer resumption on 1.0 roadmap?

---

## Fix Strategy: Bottom-Up Systematic Approach

### Phase 1: Quick Wins (Est. 30 min - 1 hour)
1. Identify which failures are **missing protocol support** vs **implementation bugs**
2. Check slirc-proto for missing Command/Numeric variants
3. Run individual test files to isolate error patterns
4. Document root causes

### Phase 2: Priority 1 (Account Registration)
1. Check if REGISTER command exists in slirc-proto
2. Implement/fix handler in handlers/account.rs
3. Test REGISTER flow: parse → validate → store account
4. Verify: all 4 tests pass

### Phase 3: Priority 2 Features
1. MONITOR Extended: Add account-notify hooks
2. CHATHISTORY: Implement query engine (if on roadmap)
3. METADATA: Decide inclusion; implement if needed

### Phase 4: Priority 3 Tertiary Features
- Systematically implement each missing handler/mode

### Phase 5: Full Suite Validation
- Run complete irctest
- Fix any regressions
- Document final pass rate

---

## Assumptions & Constraints

1. **Architecture adherence**: All fixes must follow existing patterns (typestate handlers, Matrix managers, service effects)
2. **Zero-copy preservation**: Don't break zero-copy parsing guarantees
3. **DashMap discipline**: No locks held across await points
4. **Protocol-first**: Don't add logic without verifying slirc-proto support
5. **Test-driven**: Each fix validated by irctest before moving to next

---

## Success Criteria

- [ ] 100% pass rate on 387 eligible tests (or document why xfails/skips are necessary)
- [ ] No regressions in existing 328 passing tests
- [ ] All commits documented with rationale
- [ ] Code reviewed for architecture compliance
- [ ] Master Context updated with learnings

