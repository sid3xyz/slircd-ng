# RFC Compliance Audit Plan

**Created**: 2025-12-13  
**Status**: CRITICAL - MODE broadcast violation discovered

## Problem Statement

We discovered a fundamental RFC 2812 violation:
- **Violation**: Broadcasting user MODE changes to channels
- **Correct behavior**: User mode changes are sent ONLY to the user
- **Impact**: Protocol non-compliance, breaks IRC client expectations
- **Root cause**: Inadequate RFC verification during implementation

## Compliance Verification System

### Phase 1: Baseline Establishment (Week 1)

1. **Run full irctest suite** (in progress)
   ```bash
   cd slirc-irctest
   SLIRCD_BIN=/path/to/slircd timeout 600 \
     pytest --controller irctest.controllers.slircd \
     irctest/server_tests/ -v --json-report --json-report-file=baseline.json
   ```

2. **Document current pass/fail state**
   - Generate compliance matrix
   - Categorize failures by RFC section
   - Prioritize by severity (P0: protocol violations, P1: feature gaps, P2: edge cases)

3. **Create RFC section checklist**
   - RFC 1459 (deprecated features marked)
   - RFC 2812 (modern spec)
   - IRCv3 capabilities we claim to support

### Phase 2: RFC Cross-Reference (Week 1-2)

For each implemented command, create verification checklist:

#### Example: MODE Command
- [ ] RFC 2812 Section 3.1.5 - User mode syntax
- [ ] RFC 2812 Section 3.2.3 - Channel mode syntax  
- [ ] User modes sent ONLY to user (not broadcast)
- [ ] Channel modes broadcast to channel members
- [ ] Oper-only modes reject non-opers
- [ ] +r can only be set by server
- [ ] IRCv3: multi-prefix in NAMES/WHO
- [ ] Test: `irctest/server_tests/umodes/`

#### Example: JOIN Command
- [ ] RFC 2812 Section 3.2.1 - JOIN syntax
- [ ] Key handling (RFC 2812 Section 3.2.1)
- [ ] Ban exceptions (+e mode)
- [ ] Invite exceptions (+I mode)
- [ ] IRCv3: extended-join capability
- [ ] Test: `irctest/server_tests/join.py`

### Phase 3: Systematic Testing (Week 2-3)

1. **Create test coverage map**
   ```
   Command -> RFC Section -> irctest file -> Pass/Fail
   ```

2. **Priority repair order**
   - P0: Protocol violations (like MODE broadcast bug)
   - P1: Core commands (JOIN, PART, PRIVMSG, NICK, etc.)
   - P2: Operator commands
   - P3: IRCv3 extensions

3. **Verification workflow**
   ```
   For each failed test:
   1. Read RFC section
   2. Understand expected behavior
   3. Check implementation
   4. Fix bug
   5. Re-run test
   6. Update compliance matrix
   ```

### Phase 4: Continuous Compliance (Ongoing)

1. **Pre-commit hook**
   ```bash
   # Run relevant irctest subset for changed files
   scripts/test-rfc-compliance.sh <changed-files>
   ```

2. **CI integration**
   - Full irctest suite on every PR
   - Block merge if P0/P1 failures introduced
   - Compliance percentage must not decrease

3. **Documentation requirement**
   - Every handler must document RFC section
   - Code comments must cite RFC for non-obvious behavior
   - Example:
     ```rust
     /// MODE command handler.
     ///
     /// # RFC 2812 Compliance
     /// - Section 3.1.5: User mode changes sent ONLY to user (not broadcast)
     /// - Section 3.2.3: Channel mode changes broadcast to channel members
     /// - User cannot set +o on themselves (server-only)
     ```

## Compliance Matrix Template

| Command | RFC Section | Behavior | Test File | Status | Notes |
|---------|-------------|----------|-----------|--------|-------|
| MODE (user) | RFC 2812 §3.1.5 | Changes sent to user only | umodes/*.py | ✅ FIXED | Was broadcasting to channels |
| MODE (chan) | RFC 2812 §3.2.3 | Broadcast to channel | chmodes/*.py | ❓ TESTING | |
| JOIN | RFC 2812 §3.2.1 | Standard join flow | join.py | ❓ TESTING | |
| PART | RFC 2812 §3.2.2 | Part with optional message | part.py | ❓ TESTING | |
| PRIVMSG | RFC 2812 §3.3.1 | Message to user/channel | messages.py | ❓ TESTING | |
| NOTICE | RFC 2812 §3.3.2 | No auto-reply allowed | messages.py | ❓ TESTING | |
| NICK | RFC 2812 §3.1.2 | Broadcast nick change | connection_registration.py | ❓ TESTING | |
| QUIT | RFC 2812 §3.1.7 | Broadcast to shared channels | quit.py | ❓ TESTING | |
| WHO | RFC 2812 §3.6.1 | Query visible users | who.py | ❓ TESTING | Just added result limits |
| WHOIS | RFC 2812 §3.6.2 | User information | whois.py | ❓ TESTING | |
| NAMES | RFC 2812 §3.2.5 | Channel member list | names.py | ❓ TESTING | Just added result limits |
| LIST | RFC 2812 §3.2.6 | Channel list | list.py | ❓ TESTING | Just added result limits |
| TOPIC | RFC 2812 §3.2.4 | Channel topic | topic.py | ❓ TESTING | |
| KICK | RFC 2812 §3.2.8 | Remove user from channel | kick.py | ❓ TESTING | |
| INVITE | RFC 2812 §3.2.7 | Invite to channel | invite.py | ❓ TESTING | |
| KILL | RFC 2812 §3.7.1 | Disconnect user (oper) | kill.py | ❓ TESTING | |

## Known Issues to Investigate

Based on MODE bug pattern, check these potential issues:

1. **NICK changes** - Are they broadcast correctly?
   - Should broadcast to all shared channels
   - User sees their own NICK change
   
2. **QUIT messages** - Are they sent to the right users?
   - Should send to all shared channels
   - User sees their own QUIT
   
3. **AWAY status** - IRCv3 away-notify behavior
   - Should broadcast to shared channels (if capability enabled)
   - Not to the user themselves
   
4. **ACCOUNT messages** - IRCv3 account-notify
   - Currently fixed: broadcasts to shared channels (exclude user)
   - User receives direct copy
   
5. **User visibility** - WHO/NAMES/LIST respect +i?
   - Invisible users only visible if:
     - Requester is oper
     - Requester shares channel
     - Exact nick query

## Action Items

- [ ] Wait for full irctest baseline run to complete
- [ ] Generate compliance matrix from results
- [ ] Prioritize failures by severity
- [ ] Create handler-by-handler RFC audit checklist
- [ ] Fix P0 violations (protocol breaks) immediately
- [ ] Schedule P1 repairs (missing features)
- [ ] Add RFC section comments to all handlers
- [ ] Create pre-commit irctest hook
- [ ] Update CI to enforce compliance

## Success Criteria

1. **90%+ irctest pass rate** (excluding deprecated tests)
2. **Zero P0 violations** (protocol correctness)
3. **All handlers document RFC compliance** in code
4. **CI enforces RFC compliance** (tests required for merge)
5. **Compliance percentage tracked** in README

## Timeline

- Week 1: Baseline + audit plan
- Week 2: P0 fixes + P1 triage
- Week 3: P1 repairs + documentation
- Week 4: CI integration + monitoring

## Notes

- This audit was triggered by discovering MODE broadcast bug
- Similar systematic issues may exist elsewhere
- Cannot trust implementation without RFC cross-reference
- irctest is authoritative but we need to understand WHY tests fail
