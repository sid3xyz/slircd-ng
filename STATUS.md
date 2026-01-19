# Project Status

> Last Updated: 2026-01-19

## Core Components

| Component | Status | Maturity | Refactoring Needs | Notes |
|-----------|--------|----------|-------------------|-------|
| **Handlers** | 🟢 **Stable** | High | Low | Recently reorganized into `cap`, `channel`, `user`, `server`, etc. |
| **Sync** | 🟡 **Active** | Medium | Medium | `mod.rs` split into `network`, `link`, `tls`. Further cleanup possible. |
| **State** | 🟢 **Stable** | High | Low | Core logic in `src/state`. |
| **Database** | 🟢 **Stable** | High | Low | Dual-engine (SQLx + Redb) working well. |
| **Security** | 🟡 **Active** | Medium | Low | SASL refactored. Password hashing centralized. |
| **Config** | 🟢 **Stable** | High | Low | TOML based. |

## Module Map

### `src/handlers`
- `cap/`: Capability negotiation (SASL, etc.)
- `channel/`: Channel operations (JOIN, PART, MODE)
- `user/`: User operations (NICK, USER, WHOIS)
- `server/`: Server-to-server commands
- `messaging/`: PRIVMSG, NOTICE
- `op/`: Operator commands

### `src/sync`
- `network.rs`: Low-level TCP/TLS handling
- `handshake.rs`: S2S Handshake state machine
- `link.rs`: Peer link state
- `manager.rs`: (In `mod.rs`) High-level coordination

## Recent Major Changes
1. **Handler Reorganization**: Moved flat handlers into logical subdirectories.
2. **Sync Refactor**: Extracted networking and TLS logic from `sync/mod.rs`.
3. **SASL Refactor**: Split `sasl.rs` into `plain`, `external`, `scram` modules.
4. **Legacy Removal**: Removed `bcrypt` and `rmp-serde`.
