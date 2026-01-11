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

## Not Yet Required

(Reserve section for future proto needs identified during development)

