# Changelog

All notable changes to this project will be documented in this file.

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

| Metric | Value |
|--------|-------|
| Clippy allows | 19 (reduced from 104) |
| Capacity hints | 47 in hot paths |
| Deep nesting | 0 files >8 levels |
| TODOs/FIXMEs | 0 |
| irctest pass rate | >99% (262+ tests) |

### Fixed

- INVITE rate limiting only applies after successful delivery (not failed attempts)
- Deep nesting eliminated across codebase
- False dead_code annotations corrected

### Notes

⚠️ **AI RESEARCH EXPERIMENT** - This software is a proof-of-concept developed 
using AI agents. It is NOT production ready. Do not deploy for any real network.
