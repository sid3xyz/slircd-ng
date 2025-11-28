# slircd-ng Implementation Tracking

> Git branch tracking and implementation progress log
> Created: November 28, 2025

---

## Active Branches

| Branch | Phase | Status | Description |
|--------|-------|--------|-------------|
| copilot/start-phase-2-implementation | 2 | In Progress | Database + NickServ |

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

## Phase 2: Database + NickServ (In Progress)

### Branch: `copilot/start-phase-2-implementation`

**Goal:** Add SQLite persistence and NickServ service.

**Tasks Completed:**
- [x] Add `sqlx` dependency with SQLite feature
- [x] Add `argon2` and `rand` dependencies for password hashing
- [x] Create database module with schema migrations
- [x] Implement embedded migration for accounts, nicknames, klines, dlines tables
- [x] Create `Database` struct with connection pool
- [x] Create `AccountRepository` for account management
- [x] Implement NickServ with REGISTER, IDENTIFY, GHOST, INFO, SET commands
- [x] Wire SASL PLAIN to validate against database
- [x] Enable `sasl` capability in CAP LS
- [x] Add service message routing (PRIVMSG NickServ)
- [x] Add NS command alias
- [x] Update User struct creation with +r mode for SASL-authenticated users
- [x] Add database configuration to config.toml

**New Files:**
- `migrations/001_init.sql` - Database schema
- `src/db/mod.rs` - Database module
- `src/db/accounts.rs` - Account repository
- `src/services/mod.rs` - Services module
- `src/services/nickserv.rs` - NickServ implementation

**Modified Files:**
- `Cargo.toml` - Added sqlx, argon2, rand dependencies
- `src/main.rs` - Database initialization
- `src/config.rs` - Database configuration
- `src/handlers/mod.rs` - Context with db, NS handler registration
- `src/handlers/cap.rs` - SASL capability enabled, database validation
- `src/handlers/messaging.rs` - Service message routing
- `src/handlers/misc.rs` - NS alias handler
- `src/handlers/connection.rs` - User creation with +r mode
- `src/network/gateway.rs` - Database passing
- `src/network/connection.rs` - Database in context
- `config.toml` - Database path configuration

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
