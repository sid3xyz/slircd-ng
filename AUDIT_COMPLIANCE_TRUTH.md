# Compliance Truth Audit - December 5, 2025

## Purpose
This document tracks the **actual implementation status** vs. **documented claims** to prevent "documentation as implementation" issues.

---

## ❌ FALSE CLAIMS IDENTIFIED & CORRECTED

### 1. Commit d27f8d8 BATCH 2 - Channel Mode Validation

**FALSE CLAIM:**
> "Proper parameter validation for +k, +l, +f, +j, +J modes"

**ACTUAL STATUS:**
- ✅ `+k` (Key): Basic validation (no spaces, max 23 chars, not empty) - **silently rejects**, does NOT use ERR_INVALIDKEY
- ⚠️ `+l` (Limit): Parse validation only (accepts any valid u32) - **no range validation**
- ❌ `+f` (Flood Protection): **DOES NOT EXIST** - not in protocol, not in server
- ❌ `+j` (Join Throttle): **DOES NOT EXIST** - not in protocol, not in server
- ❌ `+J` (Unknown): **DOES NOT EXIST** - not in protocol, not in server

**CORRECTION:** Commit 874da4c removes false mode advertisement from ISUPPORT

---

### 2. Commit 65111ec (slirc-proto) - ERR_INVALIDKEY/ERR_INVALIDMODEPARAM

**MISLEADING RATIONALE:**
> "These numerics enable the server (slircd-ng) to provide more specific feedback during channel mode validation"

**ACTUAL STATUS:**
- ❌ Server **does NOT use** these numerics
- Server still uses **silent rejection** with `tracing::warn!()`
- Numerics exist in protocol library but are **unused**

**CLARIFICATION:**
The commit message had a "Future Work" section that correctly stated intent, but the "Rationale" section was misleading by using present tense ("enable") instead of future tense ("will enable").

---

### 3. ISUPPORT CHANMODES - Protocol Violation

**FALSE ADVERTISEMENT:**
```rust
"CHANMODES=beIq,k,fLjJl,imnrst"
```

This advertised modes `f`, `L`, `j`, `J` that **do not exist**.

**CORRECTED TO:**
```rust
"CHANMODES=beIq,k,l,imnrst"
```

- A (list modes): `beIq` (ban, exception, invex, quiet)
- B (param always): `k` (key)
- C (param when set): `l` (limit)
- D (no param): `imnrst` (various flags)

---

## ✅ ACTUALLY IMPLEMENTED FEATURES

### Channel Modes (Confirmed Working)
- ✅ `b` - Ban (list mode with extended ban support)
- ✅ `e` - Exception (ban exception)
- ✅ `I` - Invite exception
- ✅ `q` - Quiet (mute users)
- ✅ `k` - Key (with basic validation)
- ✅ `l` - Limit (parse-only validation)
- ✅ `i` - Invite only
- ✅ `m` - Moderated
- ✅ `n` - No external messages
- ✅ `r` - Registered only
- ✅ `s` - Secret
- ✅ `t` - Protected topic
- ✅ `o` - Operator (prefix mode)
- ✅ `v` - Voice (prefix mode)

### IRCv3 Features (Confirmed Working)
- ✅ **labeled-response** - Full implementation with batching
- ✅ **batch** - Complete batch message handling
- ✅ **draft/multiline** - Multiline message support (32 lines, 4096 bytes)
- ✅ **STATUSMSG** - Prefix-based channel messaging (@#channel, +#channel)
- ✅ **SASL** - PLAIN and SCRAM-SHA-256
- ✅ **account-registration** - Draft implementation

### RFC Compliance (Confirmed Working)
- ✅ Password authentication with proper timing
- ✅ WHOWAS count handling (count <= 0 returns all)
- ✅ LINKS command proper format
- ✅ LUSERS unregistered count tracking
- ✅ Channel mode operator validation

---

## ⚠️ PARTIALLY IMPLEMENTED

### Channel Key Validation (+k)
**Current:** Silently rejects invalid keys (spaces, empty, >23 chars)
**Should:** Send ERR_INVALIDKEY (525) or ERR_INVALIDMODEPARAM (696)
**Protocol Support:** ✅ Numerics exist in slirc-proto
**Server Support:** ❌ Not using the numerics yet

**Fix Required:**
```rust
// In handlers/mode/channel.rs, replace silent rejection with:
let reply = server_reply(
    &ctx.matrix.server_info.name,
    Response::ERR_INVALIDKEY,
    vec![nick.clone(), canonical_name.clone(), key.to_string(), "Invalid channel key".to_string()],
);
ctx.sender.send(reply).await?;
```

---

## 📋 FUTURE WORK (NOT IMPLEMENTED)

### Channel Modes to Implement
- ❌ `f` - Flood protection (format: messages:seconds, e.g., `*10:5`)
- ❌ `j` - Join throttle (format: joins:seconds, e.g., `5:10`)
- ❌ `L` - Large list mode / redirect channel
- ❌ `c` - No colors
- ❌ `C` - No CTCP
- ❌ `N` - No nick changes
- ❌ `K` - No KNOCK
- ❌ `V` - No INVITE
- ❌ `T` - No NOTICE
- ❌ `P` - Permanent
- ❌ `O` - Oper only
- ❌ `g` - Free invite
- ❌ `z` - TLS only
- ❌ `a` - Admin (prefix)
- ❌ `h` - Halfop (prefix)
- ❌ `Q` - Founder (prefix)

**Note:** These modes exist in `slirc-proto/src/mode/types.rs` as enum variants but have **no implementation** in the server handlers.

---

## 🔍 VERIFICATION COMMANDS

### Check ISUPPORT
```bash
cd /home/straylight/slircd-ng
cargo run &
sleep 2
echo -e "NICK testuser\r\nUSER test 0 * :Test\r\n" | nc localhost 6667 | grep ISUPPORT
```

### Run Mode Tests
```bash
cd /home/straylight/irctest
timeout 60 .venv/bin/pytest --controller irctest.controllers.slircd -k "mode" -v
```

### Check Numerics Usage
```bash
cd /home/straylight/slircd-ng
rg "ERR_INVALIDKEY|ERR_INVALIDMODEPARAM" src/
# Expected: No results (numerics not used yet)
```

---

## 📊 TEST COMPLIANCE STATUS

**Passing:** 358/481 (74.4%)
**Failing:** 3 (unrelated to mode claims)
**Skipped:** 120 (platform/feature specific)

**Mode-Related Tests:**
- ✅ `KeyTestCase::testKeyNormal` - Basic key functionality
- ✅ `KeyTestCase::testKeyValidation[spaces]` - Rejects keys with spaces
- ✅ `KeyTestCase::testKeyValidation[long]` - Handles long keys
- ✅ `KeyTestCase::testKeyValidation[empty]` - Rejects empty keys
- ✅ `KeyTestCase::testKeyValidation[only-space]` - Rejects space-only keys
- ✅ `ModeTestCase::testKeyInteraction` - Key interaction
- ✅ `ModeTestCase::testOpPrivileges` - Op-only mode changes

**Why Tests Pass Despite Missing Numerics:**
The Modern IRC spec allows multiple acceptable responses for invalid modes:
1. ERR_INVALIDMODEPARAM (696) ← We should send this
2. ERR_INVALIDKEY (525) ← We should send this
3. Silent rejection (no MODE echo) ← **We currently do this**
4. MODE echoed with modified key

Our implementation (#3) is compliant but not ideal for UX.

---

## ✅ COMMIT CORRECTIONS

1. **874da4c** - fix(isupport): remove advertisement of non-existent channel modes
   - Removed `f`, `L`, `j`, `J` from CHANMODES
   - Fixed CHANMODES format to accurately reflect implementation

---

## 🚨 REMAINING ISSUES TO FIX

1. **Update commit d27f8d8 message** (if we rewrite history)
   - Remove mention of `+f, +j, +J` from BATCH 2
   - Clarify only `+k, +l` have basic validation

2. **Update commit 65111ec message** (slirc-proto)
   - Change "enable" to "will enable" in Rationale
   - Emphasize "Future Work" nature more clearly

3. **Implement numeric usage**
   - Update `handlers/mode/channel.rs` to send ERR_INVALIDKEY
   - Add proper error messages instead of silent rejection

4. **Protocol extension** (if desired)
   - Add FloodProtection, JoinThrottle modes to slirc-proto
   - Implement handlers in slircd-ng
   - Update ISUPPORT when actually implemented

---

## 📝 LESSONS LEARNED

1. **Never claim features in commit messages that aren't implemented**
2. **Clearly separate "Future Work" from "Implementation"**
3. **ISUPPORT must match actual code capabilities** (protocol requirement)
4. **Use present tense only for completed work**
5. **Test claims against actual code before committing**

---

**Last Updated:** December 5, 2025
**Auditor:** AI Assistant + User Review
**Status:** Active corrections in progress
