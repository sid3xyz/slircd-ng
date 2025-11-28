# slircd-ng Implementation Tracking

> Git branch tracking and implementation progress log
> Created: November 28, 2025

---

## Active Branches

| Branch | Phase | Status | Description |
|--------|-------|--------|-------------|
| `feat/p1-caps` | P1 | 🚧 In Progress | CAP negotiation + SASL authentication |

---

## Phase 1: Core Protocol Completeness

### Branch: `feat/p1-caps`

**Goal:** Implement IRCv3 CAP negotiation and SASL PLAIN authentication.

**Commits:**
- [ ] `feat(cap): add CAP handler with LS/REQ/ACK/END support`
- [ ] `feat(state): add client capabilities to User and HandshakeState`
- [ ] `feat(auth): add AUTHENTICATE handler for SASL PLAIN`
- [ ] `feat(connection): integrate CAP into registration flow`
- [ ] `test(cap): add CAP negotiation integration test`

**Files Changed:**
- `src/handlers/mod.rs` - Register CAP/AUTHENTICATE handlers
- `src/handlers/cap.rs` - NEW: CAP command handler
- `src/handlers/auth.rs` - NEW: AUTHENTICATE command handler
- `src/state/matrix.rs` - Add capabilities to User struct
- `src/network/connection.rs` - CAP negotiation integration

**Testing:**
```bash
# Test CAP LS
printf "CAP LS 302\r\n" | nc localhost 6667

# Test full CAP negotiation
printf "CAP LS 302\r\nNICK test\r\nUSER test 0 * :Test\r\nCAP REQ :multi-prefix\r\nCAP END\r\n" | nc localhost 6667
```

---

## Completed Phases

*None yet*

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
