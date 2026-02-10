# slircd-ng Project Status

> Generated from source code audit and cargo build/test on 2026-02-10.

## Build Status

| Check | Status | Details |
|-------|--------|---------|
| `cargo build --release` | ✅ PASS | 12 warnings (unused imports, dead code) |
| `cargo test` (all 26 files) | ✅ PASS | 91 tests, 0 failures |
| `cargo clippy` (release) | ⚠️ WARNINGS | 12 warnings, no errors |
| `cargo clippy --all-targets` | ⚠️ WARNINGS | 81 warnings (16 duplicates), 2 errors in test file |
| `cargo doc` | ✅ PASS | 25 warnings (doc formatting) |

### Build Warnings (12)

| Warning | Location | Severity |
|---------|----------|----------|
| Unused imports: `hash_password`, `verify_password` | password.rs | Low |
| Unused import: `PasswordHash` | password.rs | Low |
| Unused import: `MAX_HISTORY_LIMIT` | chathistory | Low |
| Unused import: `channel_has_mode` | handlers | Low |
| Unused fields: `whowas_maxgroups`, `whowas_groupsize`, `whowas_maxkeep_days` | user manager | Low |
| Unused method: `get_metadata` | state | Low |
| Unused function: `channel_has_mode` | handlers | Low |
| Unused function: `join_message_args` | channel | Low |
| Unused function: `inc_channel_messages_dropped` | metrics | Low |
| Unused method: `configure_whowas` | user manager | Low |
| Unused variant: `GetModes` | ChannelEvent | Low |

### Clippy Categories

| Category | Count | Autofix |
|----------|-------|---------|
| Collapsible `if` statements | 27 | Yes |
| `assert_eq!` with literal bool | 20 | Yes (tests only) |
| Field assignment outside initializer | 20 | Yes |
| `clone` replaceable with `from_ref` | 12 | Yes |
| Expression always evaluates to false | 9 | No (investigate) |
| Collapsible `if let` | 8 | Yes |
| Unit struct via `default` | 4 | Yes |
| MutexGuard held across await | 1 | No (investigate) |
| Too many arguments (8/7) | 1 | No (refactor) |
| **Errors**: read amount not handled | 2 | No (test file only) |

---

## Test Results (2026-02-10)

All **26** integration test files pass. **91 total tests**, **0 failures**.

| Test File | Tests | Pass | Time |
|-----------|-------|------|------|
| channel_flow | 1 | ✅ | 0.23s |
| channel_ops | 5 | ✅ | 0.62s |
| channel_queries | 5 | ✅ | 0.35s |
| chathistory | 1 | ✅ | 0.77s |
| chrono_check | 1 | ✅ | 0.00s |
| compliance | 3 | ✅ | 0.31s |
| connection_lifecycle | 4 | ✅ | 0.32s |
| distributed_channel_sync | 5 | ✅ | 0.00s |
| integration_bouncer | 3 | ✅ | 0.79s |
| integration_partition | 4 | ✅ | 0.11s |
| integration_rehash | 3 | ✅ | 1.06s |
| ircv3_features | 3 | ✅ | 0.00s |
| ircv3_gaps | 5 | ✅ | 0.54s |
| operator_commands | 4 | ✅ | 1.07s |
| operator_moderation | 10 | ✅ | 2.08s |
| s2s_two_server | 4 | ✅ | 4.27s |
| sasl_buffer_overflow | 1 | ✅ | 0.15s |
| sasl_external | 2 | ✅ | 0.36s |
| security_channel_freeze | 2 | ✅ | 0.41s |
| security_channel_key | 1 | ✅ | 0.33s |
| security_flood_dos | 4 | ✅ | 0.00s |
| security_slow_handshake | 1 | ✅ | 0.21s |
| server_queries | 8 | ✅ | 0.28s |
| services_chanserv | 1 | ✅ | 0.24s |
| stress_sasl | 2 | ✅ | 2.13s |
| unified_read_state | 1 | ✅ | 0.21s |
| user_commands | 8 | ✅ | 0.40s |

---

## Codebase Metrics

| Metric | Value |
|--------|-------|
| Rust edition | 2024 |
| Version | 1.0.0-rc.1 |
| Source files (src/) | 286 |
| Source lines (src/) | ~62,800 |
| Protocol library files | 108 |
| Protocol library lines | ~26,400 |
| Test files | 31 |
| Test lines | ~6,700 |
| Handler files | ~141 |
| Handler directories | 17 |
| IRC commands handled | ~95+ |
| IRCv3 capabilities | 27 |
| Database migrations | 10 |
| Direct dependencies | ~40 |

---

## Feature Completeness

### Core IRC (RFC 2812)

| Feature | Status | Notes |
|---------|--------|-------|
| Connection registration (NICK/USER/PASS) | ✅ | With typestate enforcement |
| Channel operations (JOIN/PART/TOPIC/KICK/INVITE) | ✅ | Full mode support |
| Messaging (PRIVMSG/NOTICE) | ✅ | With bouncer echo |
| User modes | ✅ | +i, +w, +o, +r, +s, etc. |
| Channel modes | ✅ | +b/e/I/q, +k/l/o/v, +n/t/m/s/p/i |
| Server queries (LUSERS/STATS/VERSION/etc.) | ✅ | All standard queries |
| Operator commands | ✅ | OPER, KILL, WALLOPS, etc. |
| Ban commands (K/D/G/Z-lines) | ✅ | With persistence |
| WHOIS/WHO/WHOWAS | ✅ | WHO with WHOX support |

### IRCv3 Extensions

| Feature | Status | Notes |
|---------|--------|-------|
| CAP negotiation | ✅ | LS 302, LIST, REQ, END |
| SASL (PLAIN/EXTERNAL/SCRAM-SHA-256) | ✅ | Pre-registration |
| Message tags + server-time + msgid | ✅ | Full support |
| Batch + labeled-response | ✅ | |
| CHATHISTORY | ✅ | Redb backend, all subcommands |
| MONITOR | ✅ | Online/offline notifications |
| Extended JOIN + account-notify | ✅ | |
| away-notify + chghost | ✅ | |
| echo-message | ✅ | Bouncer-aware |
| setname | ✅ | |
| draft/multiline | ✅ | 40KB, 100 lines |
| draft/read-marker | ✅ | Max-forward semantics |
| draft/account-registration | ✅ | REGISTER command |
| draft/relaymsg | ✅ | Oper-only relay |
| STARTTLS | ✅ | Plaintext connections |
| STS | ✅ | Dynamic based on TLS config |
| standard-replies (FAIL/WARN/NOTE) | ✅ | |

### Server Linking (S2S)

| Feature | Status | Notes |
|---------|--------|-------|
| TS6-like protocol | ✅ | PASS/CAPAB/SERVER/SVINFO |
| State burst (UID/SJOIN/TB/SID) | ✅ | Correct ordering |
| Netsplit handling | ✅ | Mass-quit, topology cleanup |
| Message routing | ✅ | UID prefix → SID → peer |
| CRDT state sync | ✅ | LWW + AWSet |
| Heartbeat (PING/PONG) | ✅ | 30s/90s |
| TLS for S2S | ✅ | Separate config |
| Ban propagation (G-lines, Z-lines) | ✅ | Via ENCAP |

### Services

| Feature | Status | Notes |
|---------|--------|-------|
| NickServ | ✅ | 11 commands |
| ChanServ | ✅ | 12 commands |
| Playback (ZNC-compat) | ✅ | PLAY/LIST/CLEAR |
| Service pseudoclients | ✅ | Mode +S, deterministic UIDs |
| Effect pattern | ✅ | Services never mutate state |

### Bouncer/Multiclient

| Feature | Status | Notes |
|---------|--------|-------|
| Multi-session per account | ✅ | Configurable max |
| Session attach/detach | ✅ | |
| Always-on persistence | ✅ | Redb, 30s writeback |
| Auto-away | ✅ | Configurable policy |
| Per-session capability tracking | ✅ | |
| Message echo across sessions | ✅ | |

### Security

| Feature | Status | Notes |
|---------|--------|-------|
| IP cloaking (HMAC-SHA256) | ✅ | |
| Rate limiting (Governor) | ✅ | Message/connection/join |
| IP deny list (Roaring Bitmap) | ✅ | Nanosecond rejection |
| Spam detection | ✅ | Multi-layer heuristics |
| Extended bans ($a:/$r:/$j:/$x:/$z) | ✅ | |
| RBL integration | ✅ | HTTP + DNS |
| Reputation system | ✅ | Per-user scoring |
| Argon2 password hashing | ✅ | |
| Capability tokens (unforgeable) | ✅ | Compile-time authorization |
| STARTTLS + STS | ✅ | |

---

## Known Issues

1. **Clippy errors in test**: `security_channel_freeze` test has 2 "read amount not handled" errors (compilation succeeds but clippy errors)
2. **Dead code**: 12 unused functions/fields/variants in production code
3. **MutexGuard across await**: 1 instance detected by clippy (potential deadlock under contention)
4. **Expressions always false**: 9 instances (potential logic issues or dead branches)

---

## External Test Suite

The project includes an `slirc-irctest/` directory containing a fork of the [irctest](https://github.com/progval/irctest) conformance test suite for IRC protocol compliance testing.
