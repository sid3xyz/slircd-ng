# Protocol Requirements & Server Separation

> **Purpose**: Document the separation between `slirc-proto` and `slircd-ng`, and track protocol-level requirements.

---

## Architecture: Proto vs Server

### slirc-proto (Protocol Library)
**Responsibility**: Pure protocol parsing, zero-copy message handling, transport abstraction.

**Contains**:
- `MessageRef<'a>` - Zero-copy IRC message parser
- Transport layer (read/write with buffer management)
- Protocol error types (`ProtocolError`)
- IRCv3 capability definitions
-CRDTs for distributed state (if used for S2S)

**Must NOT contain**:
- Business logic
- Database access
- NickServ/ChanServ logic
- Configuration
- Network I/O (beyond abstract traits)

### slircd-ng (Server Implementation)
**Responsibility**: IRC daemon business logic, state management, services.

**Contains**:
- Handler registry and command processing
- `Matrix` (shared server state)
- `ClientManager`, `ChannelManager`, etc.
- Database (SQLite/Redb)
- NickServ/ChanServ/OperServ services
- Configuration (`config.toml`)
- Network gateway (TCP/TLS/WebSocket)

**Must NOT contain**:
- Protocol parsing logic (delegate to slirc-proto)
- Network byte-level handling (use proto transport)

---

## Current Compliance Status

### IRCv3 (Verified 2026-01-18)
| Feature | Status | Notes |
|---------|--------|-------|
| Labeled Responses | ✅ | Working in RELAYMSG tests |
| monitor | ⏳ | Basic support, extended-monitor needs verification |
| batch | ✅ | CHATHISTORY batches working |
| READQ | ✅ | 16KB limit enforced correctly |
| Unicode/Confusables | ✅ | PRECIS casemapping handles Cyrillic |
| RELAYMSG | ✅ | draft/relaymsg fully functional |

### Core Protocol
| Feature | Status | Notes |
|---------|--------|-------|
| Account Registration | ✅ | 8/8 tests passing (draft/account-registration) |
| SASL | ✅ | PLAIN, SCRAM-SHA-256, EXTERNAL |
| Bouncer/Multiclient | ✅ | Session resumption working |
| CHATHISTORY | ⏳ | Partial (some queries work, needs deep dive) |

---

## Known Gaps (Beta Blockers)

### 1. CHATHISTORY Edge Cases
**Priority**: Medium  
**Tests Affected**: ~8 tests  
**Action**: Deep dive specific failure modes

### 2. MONITOR Extended
**Priority**: Low  
**Tests Affected**: Unknown  
**Action**: Run monitor.py test suite to verify

---

## Proto/Server Separation Violations (Audit Needed)

**Action Items**:
1. Audit `slirc-proto` for any business logic leakage
2. Ensure all parsing uses `slirc-proto::MessageRef`
3. Verify transport abstraction is properly used
4. Check for direct socket usage in handlers (should use proto layer)

---

## Post-Beta Enhancements

- **S2S (Server-to-Server)**: Full mesh routing with slirc-proto CRDTs
- **CHATHISTORY**: `msgid` lookup optimization
- **WebSockets**: Binary frame support
- **IRCv3.4**: Complete all pending IRCv3 specs
