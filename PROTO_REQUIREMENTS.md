# Proto Enhancement Requests

Document blockers requiring changes to `slirc-proto` before daemon features can be implemented.

---

## BLOCKING: InvalidUtf8 Error Must Preserve Command Name

**Status**: BLOCKING 2 irctest failures (utf8.py)

**Issue**: When a message contains invalid UTF-8, the protocol parser returns `ProtocolError::InvalidUtf8(details)` but loses all information about the partially-parsed message. The daemon needs to respond with a `FAIL` command, which requires knowing the command name that failed.

**Current Behavior**:
```
Input: "@label=xyz PRIVMSG #qux hi\xaa"  (invalid UTF-8 at end)
Parse fails with: ProtocolError::InvalidUtf8("...")
Result: We cannot extract "PRIVMSG" to send: FAIL PRIVMSG INVALID_UTF8
```

**Expected RFC Behavior** (per IRCv3):
```
Input: "@label=xyz PRIVMSG #qux hi\xaa"
Response: :server FAIL PRIVMSG INVALID_UTF8 :Invalid UTF-8 in message
         @label=xyz (echo label back)
Connection: stays open (recoverable error)
```

**Proposal**: Modify `ProtocolError::InvalidUtf8` to include either:
1. **Option A**: The raw line (before UTF-8 validation) + byte position of error
   - Allows daemon to extract command via regex on raw ASCII bytes
   - Example: `InvalidUtf8 { raw_line: Vec<u8>, byte_pos: usize, details: String }`

2. **Option B**: Pre-parsed command name extracted before UTF-8 check
   - Parser extracts command word (first space-delimited token after tags/prefix) in ASCII
   - Then validates UTF-8, returning command if parse failed
   - Example: `InvalidUtf8 { command_hint: Option<String>, details: String }`

3. **Option C**: Return a wrapper error variant like `ProtocolError::PartialParse` that includes all successfully-extracted components
   - Example: `PartialParse { tags: Option<Vec<(String, String)>>, command: Option<String>, reason: String }`

**Impact**: Fixes 2 tests, enables proper RFC 7613 (UTF-8 validation) compliance.

**Timeline**: Needed before UTF-8 error handling implementation can complete.

---

---

## EVALUATION: Commands Implemented vs. Missing

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
- ❌ **METADATA** (9 test failures) - Get/set/list user/channel metadata (deprecated but testable spec)
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
**Status**: WORKS BUT BRITTLE
**Severity**: MEDIUM
**Current Workaround**: `Command::Raw("CHATHISTORY", vec!["TARGETS", ...])`
**Location**: `src/handlers/chathistory/batch.rs:54`
**Issue**: CHATHISTORY TARGETS is a valid subcommand but not typed in proto as `MessageRef` parsing doesn't support it
**Proposal**: Add `ChatHistoryTargets` variant or support `CHATHISTORY` as subcommand enum including TARGETS
**Impact**: Makes protocol stricter, removes workaround

### High Priority (Architectural Improvements)

#### 3. METADATA Command Definition
**Status**: NOT IN PROTO
**Severity**: MEDIUM
**Spec**: Ergo deprecated spec for user/channel metadata
**Proposal**: Add `Command::Metadata` variant with subcommands:
```rust
pub enum MetadataSubcommand {
    Get,      // METADATA GET <target> <key>
    Set,      // METADATA SET <target> <key> [value]
    List,     // METADATA LIST <target>
}
```
**Impact**: Enables 9 tests, supports metadata storage for users/channels
**Test Impact**: metadata.py (9 failures → 0 expected)

#### 4. ROLEPLAY: NPC Command + MODE +E
**Status**: NOT IN PROTO
**Severity**: MEDIUM
**Proposal**: Add two proto features:
- `Command::NPC { channel: String, nick: String, text: String }`
- `ChannelMode::Roleplay` (+E flag)
**Impact**: Enables 2 tests (NPC + MODE +E mode setting)
**Test Impact**: roleplay.py (1 failure → 0 expected)

#### 5. RELAYMSG Command
**Status**: NOT IN PROTO
**Severity**: LOW
**Spec**: Not widely adopted, Ergo extension
**Proposal**: Add `Command::RelayMsg { relay_from: String, target: String, msg: String }`
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
| InvalidUtf8 (FAIL) | 2 | ❌ BLOCKED | 🟡 Ready | PROTO ONLY | CRITICAL |
| CHATHISTORY TARGETS | 20 | 🟡 Workaround | ✅ Working | PROTO REFACTOR | HIGH |
| METADATA | 9 | ❌ Missing | ❌ Missing | MEDIUM | HIGH |
| ROLEPLAY (NPC+E) | 1 | ❌ Missing | ❌ Missing | LOW | MEDIUM |
| RELAYMSG | 1 | ❌ Missing | ❌ Missing | LOW | MEDIUM |
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

