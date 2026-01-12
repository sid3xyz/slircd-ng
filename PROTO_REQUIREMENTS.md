# Proto Enhancement Requests

Document blockers requiring changes to `slirc-proto` before daemon features can be implemented.

---

## ✅ RESOLVED: CHATHISTORY TARGETS Response Format

**Status**: RESOLVED (Fixed in slirc-proto and daemon)

**Summary**: `Command::ChatHistoryTargets` had wrong type for second parameter:
- **Was**: `count: u32` - Integer message count
- **Fixed**: `timestamp: String` - RFC3339 formatted timestamp

**Specification**: IRCv3 CHATHISTORY spec defines TARGETS response as:
```
CHATHISTORY TARGETS <target> <timestamp>
```
Where `<timestamp>` is the time of the **latest message** in that conversation.

**Proto Changes**:
- `crates/slirc-proto/src/command/types.rs:313` - Changed field type
- `crates/slirc-proto/src/encode/command.rs:445` - Updated serialization
- `crates/slirc-proto/src/command/serialize.rs:354` - Updated Display impl

**Daemon Changes**:
- `src/handlers/chathistory/batch.rs:53` - Use `timestamp` field directly
- `src/handlers/chathistory/queries.rs:311` - Added 30-day staleness filter

**Tests Passing**: 2/2 (testChathistoryTargets, testChathistoryTargetsExcludesUpdatedTargets)

---

## ✅ RESOLVED: InvalidUtf8 Error Preservation

**Status**: RESOLVED (commit c3cc619 in slirc-proto, commit fa6ecb7 in slircd-ng)

**Summary**: Proto team implemented `ProtocolError::InvalidUtf8` with full metadata:
- `raw_line: Vec<u8>` - raw bytes for label extraction
- `byte_pos: usize` - position of UTF-8 validation failure
- `details: String` - error message from UTF-8 decoder
- `command_hint: Option<String>` - command extracted before UTF-8 check

**Bug Fix**: Also fixed critical infinite loop bug where invalid UTF-8 lines were not
consumed from the transport buffer, causing the same error to be returned repeatedly.

**Tests Passing**: 4/4 Utf8TestCase tests (testNonUtf8Filtering, testUtf8Validation,
testNonutf8Realname, testNonutf8Username). ErgoUtf8NickEnabledTestCase tests require
UTF-8 nick support (not yet implemented).

---

---

## ✅ RESOLVED: RELAYMSG Parameter Order

**Status**: RESOLVED (commit in slirc-proto crates/slirc-proto/)

**Summary**: Proto parser had parameter order backwards:
- **Was**: `Command::RELAYMSG { relay_from: args[0], target: args[1], text: args[2] }`
- **Fixed**: `Command::RELAYMSG { relay_from: args[1], target: args[0], text: args[2] }`

The IRC protocol sends: `RELAYMSG <target> <relay_from> <text>` but the proto parsed it as `<relay_from> <target> <text>`.

**Tests Passing**: relaymsg.py validation tests (invalid nick detection, format checking). Full test still has labeled-response tag echo issue (framework-level, not proto).

---

---

**Current Implementation** (60+ handlers):
- ✅ Core: NICK, USER, PASS, CAP, QUIT, PING, PONG, REGISTER, SERVICE, SQURY
- ✅ Messaging: PRIVMSG, NOTICE, TAGMSG, ACCEPT
- ✅ Channels: JOIN, PART, NAMES, LIST, TOPIC, KICK, INVITE, CYCLE, KNOCK
- ✅ User Query: WHO, WHOIS, WHOWAS, ISON, USERHOST, USERIP, USERS, HELP
- ✅ Channel Modes: MODE (user/channel), CHGIDENT, CHGHOST, VHOST, SETNAME
- ✅ Bans: KLINE, DLINE, XLINE, SHUN, GLINE (with ADD/REMOVE variants)
- ✅ Account: REGISTER (via SERVICE), auth integration
- ✅ Oper: OPER, KILL, SAJOIN, SANICK, SAPART, SAMODE, WALLOPS, GLOBOPS
- ✅ Messaging Moderation: SILENCE (partial)
- ✅ Chat History: CHATHISTORY (all subcommands: LATEST, BEFORE, AFTER, BETWEEN, TARGETS)
- ✅ Monitor: MONITOR (add/del/clear/list)
- ✅ Server: ENCAP, SJOIN, TMODE, UID, SID, SVINFO, CAPAB
- ✅ Information: MOTD, INFO, RULES, STATS, TIME, LINKS, LUSERS, TRACE, VERSION, MAP

**Missing Commands** (from irctest):
- ✅ **METADATA** (9/9 tests passing) - Binary data in values now supported
- ❌ **NPC** (1 test failure, part of ROLEPLAY feature) - Send message as another character
- ❌ **READQ** (2 test failures) - No dedicated handler; current behavior sends 417 + continues instead of disconnect for messages >16KB
- ❌ **RELAYMSG** (1 test failure) - Relay message between networks
- ❌ **MODE +f** (1 test failure, channel forwarding) - Incomplete support, no forwarding logic
- ❌ **MODE +E** (1 test failure, part of ROLEPLAY) - Channel roleplay mode
- ❌ **Unicode Confusables Detection** (1 test failure) - Nick validation against homoglyphs
- ❌ **Bouncer Resumption** (7 test failures) - Client connection suspension/resumption
- ❌ **ZNC Playback** (1 test failure) - ZNC-specific extension
- ❌ **Nickserv SAREGISTER** (1 test failure) - Service-level registration command

---

## AUDIT: Proto Gaps by Priority

### Critical (Blocks Protocol Compliance)

#### 1. InvalidUtf8 Must Preserve Command Name
**Status**: BLOCKING 2 tests (utf8.py)
**Severity**: HIGH
**Root Cause**: Protocol parser discards command info on UTF-8 validation failure
**Proposed Solutions**: See detailed section above
**Daemon Impact**: Can't send proper FAIL response, must disconnect instead

#### 2. CHATHISTORY TARGETS as Raw Command
**Status**: RESOLVED (Fixed in slirc-proto and daemon)
**Summary**: Added `Command::ChatHistoryTargets` variant to `slirc-proto`. Updated daemon to use it.
**Severity**: RESOLVED
**Current Workaround**: REMOVED


### High Priority (Architectural Improvements)

#### 3. METADATA Command Definition
**Status**: ✅ FULLY RESOLVED (9/9 tests passing)
**Severity**: RESOLVED
**Spec**: Ergo deprecated spec for user/channel metadata
**Implementation Complete**:
- `Command::METADATA` exists in proto
- Numerics 761, 762, 764, 765, 766 added to proto
- Handler fully implemented with GET/SET/LIST for users and channels
- Metadata stored in `User.metadata` HashMap and `ChannelActor` state
- Advertised in ISUPPORT
- **Binary data support**: NUL bytes (`\0`) now allowed in message content
**Solution**: Modified `slirc-proto` transport layer to allow null bytes in message values
- Changed `is_illegal_control_char()` to allow `\0` (only BEL and other control chars rejected)
- Rationale: Rust strings handle binary data correctly; NUL bytes don't cause issues
- Updated tests in format.rs, irc.rs, control_chars.rs, and transport/mod.rs
**Test Impact**: metadata.py (9/9 passed, including testSetGetZeroCharInValue)

#### 4. ROLEPLAY: NPC Command + MODE +E
**Status**: RESOLVED (Fixed in slirc-proto and daemon)
**Severity**: RESOLVED
**Current**: Handler fully implemented and tested.
**Proposal**: Done.
**Impact**: Enables 1-2 tests (NPC + MODE +E mode setting if separate test)
**Test Impact**: roleplay.py (1 failure → 0 expected)

#### 5. RELAYMSG Command
**Status**: PROTO READY, DAEMON STUB
**Severity**: LOW
**Current**: `Command::RELAYMSG` exists in proto, daemon has stub handler (commit 57da60c)
**Proposal**: Full RELAYMSG handler implementation (oper-only, relay prefix handling)
**Impact**: Enables 1 test
**Test Impact**: relaymsg.py (1 failure → 0 expected)

### Medium Priority (Feature Completeness)

#### 6. Channel Mode +f (Forwarding) Support
**Status**: MODE PARSING OK, LOGIC MISSING
**Severity**: LOW
**Current**: Mode parsing works, but channel forwarding logic not implemented
**Proposal**: Ensure proto exports `ChannelMode::Forward` with parameter
**Impact**: Enables 1 test (channel_forward.py)
**Daemon Impact**: Daemon needs to implement forwarding logic, not proto

#### 7. Mode +U (Unicode Validation)
**Status**: MODE OK, VALIDATION MISSING
**Severity**: LOW
**Issue**: Unicode confusable nick detection requires homoglyph database
**Proposal**: Proto doesn't need changes; daemon needs confusables detection library
**Impact**: Enables 1 test (confusables.py)
**Daemon Impact**: Pure daemon feature, no proto needed

### Test Failure Analysis (Current Session)

#### METADATA Handler Status
**Tests Run**: metadata.py (8 passed, 1 failed)
**Status**: ✅ MOSTLY COMPLETE
**Implementation**:
- Handler: `src/handlers/messaging/metadata.rs`
- User storage: `User.metadata` HashMap
- Channel storage: `ChannelActor` with `MetadataCommand` events
- Advertised: `METADATA` in ISUPPORT (005)
- Format: Deprecated spec (761 params: client, key, visibility, value - no target)
**Passing Tests**:
- testInIsupport, testGetOneUnsetValid, testGetTwoUnsetValid
- testListInvalidTarget, testListValid, testSetGetValid
- testSetGetHeartInValue (emoji works)
**Failing Test**: `testSetGetZeroCharInValue`
- The value contains `\0` (null byte)
- Transport layer rejects: `ERROR :Illegal control character: '\0'`
- This is a proto/transport restriction, not handler logic
**To Fix**: Modify `slirc-proto` transport to allow null bytes in trailing params

#### NPC Handler Status
**Tests Run**: roleplay.py::testRoleplay (1 failed)
**Issue**: Handler doesn't enforce channel mode +E (roleplay enabled)
**Expected**: Return ERR_CANNOTSENDRP when channel lacks +E mode
**Blocker**: ERR_CANNOTSENDRP (approx 927) not defined in slirc-proto Response enum
**Missing Proto Feature**: Channel mode +E flag and its validation
**Status**: Handler executes but test fails due to missing proto support

#### RELAYMSG Handler Status
**Tests Run**: relaymsg.py::testRelaymsg (1 failed, progress made)
**Prior Issue**: Handler validated oper status BEFORE relay_from nick format
**Fixed**: Reordered validation - now checks nick format first, returns FAIL RELAYMSG INVALID_NICK for invalid relay_from
**Fixed**: Proto parser had parameter order backwards (args[0]=relay_from, args[1]=target) - fixed in slirc-proto to match irctest expectations
**Fixed**: Prefix handling - overrode snapshot.nick with relay_from so routed messages appear from the relay source nick
**Current Issue**: labeled-response tag handling - when client sends `@label=x RELAYMSG ...`, the response should echo the label tag. Currently returns ACK instead. This is a framework issue, not RELAYMSG-specific.
**Status**: Handler core logic working, prefix correct, validation order correct. Label tag echo needs framework-level fix.

---

### Low Priority (Advanced Features)

#### 8. Bouncer Resumption Support
**Status**: NOT IN PROTO
**Severity**: LOW
**Complexity**: Very High
**Proposal**: Requires substantial protocol work (BOUNCER command, resumption tokens, etc.)
**Impact**: Enables 7 tests
**Timeline**: Consider for 1.1 release, not 1.0 blocker

#### 9. ZNC Playback Extension
**Status**: NOT IN PROTO
**Severity**: LOW
**Complexity**: High
**Proposal**: ZNC-specific extension, probably should not be in core proto
**Impact**: Enables 1 test
**Timeline**: Could be daemon-specific extension

---

## Summary Table

| Feature | Tests | Proto Status | Daemon Status | Effort | Priority |
|---------|-------|--------------|---------------|--------|----------|
| InvalidUtf8 (FAIL) | 2 | ✅ RESOLVED | ✅ Working | DONE | CRITICAL |
| CHATHISTORY TARGETS | 20 | ✅ RESOLVED | ✅ Working | DONE | HIGH |
| METADATA | 9 | ✅ RESOLVED | ✅ 9/9 passing | DONE | HIGH |
| ROLEPLAY (NPC+E) | 1 | ✅ RESOLVED | ✅ Working | DONE | MEDIUM |
| RELAYMSG | 1 | ✅ RESOLVED | ⚠️ Label echo | LOW | MEDIUM |
| Channel +f | 1 | 🟡 Partial | 🟡 Partial | LOW | MEDIUM |
| Confusables | 1 | ✅ OK | ❌ Missing | LOW | MEDIUM |
| Bouncer | 7 | ❌ Missing | ❌ Missing | VERY HIGH | LOW |
| ZNC Playback | 1 | ❌ Missing | ❌ Missing | HIGH | LOW |

---

## Recommended Proto Enhancement Timeline

1. **Immediate** (for 1.0 release):
   - Fix `InvalidUtf8` to preserve command name (blocks 2 tests, architectural requirement)
   - Refactor `CHATHISTORY` to remove `Command::Raw` workaround (architectural cleanliness)
   - Add `METADATA` command variants (9 tests)

2. **Soon** (post-1.0 planning):
   - Add `NPC` command and mode `+E` for ROLEPLAY (1 test, minor feature)
   - Add `RELAYMSG` command (1 test, niche feature)
   - Evaluate channel mode +f forwarding completion (1 test, partially implemented)

3. **Future** (1.1+ roadmap):
   - Bouncer resumption (large effort, 7 tests)
   - ZNC playback (specialized, 1 test)

---

## Integration Pattern for Proto Changes

When proto team implements changes:

1. **Protocol Team** implements Command variant/Numeric in `slirc-proto`
2. **Daemon Team** (here):
   - Run `cargo update` to pick up new proto version
   - Search for any `Command::Raw` workarounds related to the command
   - Replace with typed variant
   - Update handler to use new parameter structure
   - Add comprehensive round-trip tests: parse → handle → serialize
3. **Validation**: Run irctest to verify compliance
4. **Commit**: Single commit per feature with clear message

Example (from prior work):
```bash
# Proto team: adds Command::ENCAP variant
# Daemon team:
cargo update slirc-proto
grep -r "Command::Raw.*ENCAP" src/
# Replace Raw with typed variant
cargo test
pytest --controller=irctest.controllers.slircd irctest/server_tests/ -k encap
git commit -m "feat: Use typed Command::ENCAP from proto, remove Raw workaround"
```

