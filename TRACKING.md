# slircd-ng Implementation Tracking

> Git branch tracking and implementation progress log
> Created: November 28, 2025

---

## Active Branches

| Branch | Phase | Status | Description |
|--------|-------|--------|-------------|
| *none* | - | - | Ready for Phase 2 |

---

## Completed Phases

### ✅ Phase 1: CAP/SASL (feat/p1-caps → main)

**Merged:** November 28, 2025 | **Commit:** c46a666

**Implemented:**
- [x] `CapHandler` - CAP LS/LIST/REQ/ACK/NAK/END subcommands
- [x] `AuthenticateHandler` - SASL PLAIN stub (accepts any credentials)
- [x] `HandshakeState` extended with: `cap_negotiating`, `cap_version`, `capabilities`, `sasl_state`, `account`
- [x] Registration blocked during CAP negotiation (`can_register()` checks `!cap_negotiating`)

**Capabilities Advertised:**
- `multi-prefix`
- `userhost-in-names`  
- `server-time`
- `echo-message`

**Deferred to Phase 2:**
- SASL capability (commented out - needs NickServ for credential validation)
- `away-notify`, `account-notify`, `extended-join` (need services)

**Files Changed:**
- `src/handlers/cap.rs` - NEW (390 lines)
- `src/handlers/mod.rs` - Handler registration + HandshakeState fields
- `src/state/mod.rs` - Allow unused imports
- `src/state/mode_builder.rs` - Allow dead_code (future ChanServ use)

---

## Phase 2: Database + NickServ (Planned)

### Branch: `feat/p2-database` (not started)

**Goal:** Add SQLite persistence and NickServ service.

**Tasks:**
- [ ] Add `sqlx` dependency with SQLite feature
- [ ] Create database schema (accounts, nicknames, channels, klines)
- [ ] Implement `Database` struct with async connection pool
- [ ] NickServ: REGISTER, IDENTIFY, GHOST, INFO, SET
- [ ] Wire SASL PLAIN to validate against accounts table
- [ ] Enable `sasl` capability in CAP LS
- [ ] Persist K-lines/D-lines to database

**Dependencies:**
- `sqlx = { version = "0.7", features = ["runtime-tokio", "sqlite"] }`

---

## Phase 3: ChanServ (Planned)

**Goal:** Channel registration and access control.

**Tasks:**
- [ ] ChanServ: REGISTER, DROP, ACCESS, OP/DEOP, VOICE/DEVOICE
- [ ] Auto-op/voice on JOIN for identified users (uses ChannelModeBuilder)
- [ ] Channel settings (MLOCK, TOPICLOCK, KEEPTOPIC)

---

## Merge Strategy

- **Squash merge** feature branches to main for clean history
- Each phase = 1 squash commit on main
- Format: `feat(phaseN): <description> (#PR)`

---

## Dependencies

| Feature | Requires slirc-proto | Status |
|---------|---------------------|--------|
| CAP | `Command::CAP`, `CapSubCommand` | ✅ Available |
| AUTHENTICATE | `Command::AUTHENTICATE` | ✅ Available |
| Capability enum | `slirc_proto::Capability` | ✅ Available |
| SASL helpers | `slirc_proto::sasl::*` | ✅ Available |

---

*Last updated: November 28, 2025*
