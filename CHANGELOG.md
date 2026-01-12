# Changelog

All notable changes to this project will be documented in this file.

## [1.0.0-alpha.1] - 2026-01-12

First alpha release. Production-ready for testing environments.

### Added

**Protocol Completeness**
- METADATA command with user/channel key-value storage (9/9 irctest passing)
- NPC/ROLEPLAY handler with +E channel mode enforcement
- RELAYMSG handler for message relay between networks
- CHATHISTORY TARGETS with proper timestamp format and staleness filter
- PRECIS casemapping support for Unicode nicknames

**CI/CD Pipeline**
- GitHub Actions workflow for build/test/lint
- Multi-platform release automation (Linux, macOS, Windows)
- Security audit via cargo-audit
- Automated release creation on version tags

**Crate Integration**
- Absorbed slirc-proto (125 files) into monorepo
- Absorbed slirc-crdt (5 files) into monorepo
- Single Cargo.lock for reproducible builds

### Changed

- ISUPPORT CASEMAPPING now config-driven (rfc1459 or precis)
- irctest moved to `slirc-irctest/` directory within repo
- Version bumped from 0.2.0 to 1.0.0-alpha.1

### Fixed

- SCRAM verifiers migration (007) now applied during initialization
- CHATHISTORY DM ordering uses `MAX(ts)` for proper conversation sort
- UTF-8 nickname validation for PRECIS casemapping mode
- InvalidUtf8 error now preserves command metadata for FAIL responses

### Metrics

| Metric | Value |
|--------|-------|
| Unit Tests | 664 passing |
| irctest Compliance | 92.2% (357/387) |
| Clippy Warnings | 0 |
| TODO/FIXME Markers | 0 |

### Notes

**Alpha Release** - Suitable for testing and development. Core IRC protocol
is feature-complete. Monitor for stability before production deployment.

---

## [0.2.0] - 2025-12-21

Distributed System Release.

### Added

**Distributed Core**
- Server-to-Server (S2S) protocol implementation (TS6-like)
- Spanning tree topology with loop detection
- Automatic netsplit handling and state cleanup
- Burst/Sync protocol for initial state exchange

**CRDT Convergence**
- Last-Write-Wins (LWW) conflict resolution for channels and users
- Distributed channel mode synchronization
- Topic convergence with timestamp arbitration

**Distributed Security**
- Global ban propagation (G-lines, Z-lines)
- Distributed account synchronization
- Service visibility across the mesh

**Observability**
- S2S traffic metrics (bytes sent/received per peer)
- Command distribution metrics
- Enhanced `STATS` command (`L` for links, `z` for counts)

## [0.1.0] - 2025-12-18

Initial research preview release.

### Added

**Core Protocol**
- 81 IRC command handlers (RFC 1459/2812 compliant)
- Typestate connection lifecycle (UnregisteredState → RegisteredState)
- Actor model for channel state management
- Zero-copy message parsing via slirc-proto

**IRCv3 Capabilities (21)**
- multi-prefix, userhost-in-names, server-time, echo-message
- batch, message-tags, labeled-response, setname
- away-notify, account-notify, extended-join, invite-notify
- chghost, monitor, cap-notify, account-tag, sasl
- draft/multiline, draft/account-registration
- draft/chathistory, draft/event-playback

**Services**
- NickServ: REGISTER, IDENTIFY, GHOST, INFO, SET, DROP, GROUP, UNGROUP, CERT
- ChanServ: REGISTER, ACCESS, INFO, SET, DROP, OP, DEOP, VOICE, DEVOICE, AKICK, CLEAR

**Security**
- DNSBL integration
- Reputation scoring system
- Connection heuristics
- Spam detection
- X-lines (K/G/D/Z/R-lines, shuns)
- HMAC-SHA256 host cloaking
- Rate limiting
- Ban caching

**Persistence**
- SQLite via sqlx (7 migrations)
- Tables: accounts, channels, channel_access, xlines, shuns, certfp, reputation, bans
- CHATHISTORY with redb backend

**Observability**
- IRC-aware telemetry (IrcTraceContext)
- Prometheus metrics endpoint

### Quality Metrics

| Metric            | Value                 |
| ----------------- | --------------------- |
| Clippy allows     | 19 (reduced from 104) |
| Capacity hints    | 47 in hot paths       |
| Deep nesting      | 0 files >8 levels     |
| TODOs/FIXMEs      | 0                     |
| irctest pass rate | >99% (262+ tests)     |

### Fixed

- INVITE rate limiting only applies after successful delivery (not failed attempts)
- Deep nesting eliminated across codebase
- False dead_code annotations corrected

### Notes

⚠️ **AI RESEARCH EXPERIMENT** - This software is a proof-of-concept developed
using AI agents. It is NOT production ready. Do not deploy for any real network.
