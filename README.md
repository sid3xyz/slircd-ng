# slircd-ng - Straylight IRC Daemon (Next Generation)

[![Rust 1.85+](https://img.shields.io/badge/Rust-1.85+-orange?logo=rust)](https://www.rust-lang.org)
[![License: Unlicense](https://img.shields.io/badge/License-Unlicense-blue)](LICENSE)
[![Alpha Release](https://img.shields.io/badge/Release-v1.0.0--rc.1-brightgreen)](https://github.com/sid3xyz/slircd-ng/releases/tag/v1.0.0-rc.1)
[![CI/CD](https://github.com/sid3xyz/slircd-ng/actions/workflows/ci.yml/badge.svg)](https://github.com/sid3xyz/slircd-ng/actions)

**Version**: 1.0.0-rc.1
**License**: Unlicense (Public Domain)
**Language**: Rust 2024 Edition (requires rustc 1.85+)

## What This Actually Is

A modern IRC server written in Rust with zero-copy message parsing, actor-based channel state management, and support for IRCv3 extensions. The server is functional for single-server deployments and basic IRCv3 features but has known gaps in multi-server federation and bouncer session management.

## Current Status: Pre-Production (RC1)

### What Works ✅
- **Core IRC Protocol**: Full RFC 1459/2812 compliance for single-server operation
- **120+ IRC Command Handlers**: User commands, channel operations, server queries, operator commands
- **IRCv3 Support**: 26 capabilities including SASL (PLAIN, SCRAM-SHA-256, EXTERNAL), account-notify, labeled-response, batch, CHATHISTORY, message-tags
- **Services**: NickServ (account registration, identification, GHOST) and ChanServ (channel registration, access control, auto-kick)
- **Security**: TLS/SSL support, rate limiting, IP bans (KLINE/DLINE/GLINE), host cloaking, spam detection
- **Persistence**: SQLite for accounts and bans, Redb for message history
- **Monitoring**: Prometheus metrics endpoint, structured logging (JSON or pretty)
- **Build System**: Compiles cleanly with `cargo build --release`
- **Test Suite**: 1400+ tests (unit + integration), including 70+ meaningful integration tests

### What's Incomplete ⚠️
- **Bouncer/Multiclient**: Architecture and commands exist, but session reattachment tracking is incomplete (see `ReattachInfo` in session.rs)
- **Server-to-Server (S2S)**: Basic handshake works, but multi-server federation is beta quality with untested edge cases
- **irctest Compliance**: 357/387 tests passing (92.2%) - 30 tests fail, mostly edge cases in CHATHISTORY and MONITOR

### What Doesn't Exist ❌
- Production deployment documentation beyond basic checklist
- Hot-reload for TLS certificates
- External authentication provider integration (deferred)
- Binary WebSocket frame support
- IRCv3.4 specifications

## Building & Running

### Prerequisites
```bash
# Requires Rust 1.85+ (stable)
rustup update stable
```

### Build
```bash
git clone https://github.com/sid3xyz/slircd-ng.git
cd slircd-ng
cargo build --release
```

**Build time**: ~2-5 minutes on modern hardware
**Binary location**: `target/release/slircd`
**Binary size**: ~30-40 MB (includes all dependencies)

### Configuration

Create `config.toml` (example provided in repository root):

```toml
[server]
name = "irc.example.net"
network = "ExampleNet"
sid = "001"
description = "My IRC Server"
metrics_port = 9090

[listen]
address = "0.0.0.0:6667"

[database]
path = "slircd.db"

[security]
# CRITICAL: Generate with: openssl rand -hex 32
cloak_secret = "CHANGE-ME-or-server-will-refuse-to-start"
cloak_suffix = "ip"

[multiclient]
enabled = true
allowed_by_default = true
always_on = "opt-in"
```

**SECURITY WARNING**: The server will refuse to start if `cloak_secret` is weak or default. You must generate a strong secret.

### Run
```bash
./target/release/slircd config.toml
```

The server starts on port 6667 (plaintext) by default. Connect with any IRC client:
```
/server localhost 6667
/nick YourNick
/join #test
```

## Testing

### Unit & Integration Tests
```bash
cargo test
```
**Expected result**: 1300+ tests pass
**Test time**: ~30 seconds

### Code Quality
```bash
# Format check (must pass)
cargo fmt -- --check

# Linting (must pass with zero warnings)
cargo clippy -- -D warnings
```

### irctest Compliance (Optional)
The repository includes scripts to run the external irctest suite, but the suite itself is not included:
```bash
# Requires Python 3.8+, pytest, and irctest installed separately
./scripts/irctest_safe.sh
```
**Expected result**: 357/387 tests pass (92.2%)

## Architecture Overview

### Core Components
1. **Matrix** (`src/state/matrix.rs`): Central state container with managers for users, channels, clients, security, services
2. **Handler Registry** (`src/handlers/core/`): Typestate-enforced command dispatch (PreReg/PostReg/Universal)
3. **Channel Actors** (`src/state/actor/`): Each channel runs as isolated Tokio task with bounded event queue
4. **Service Effects** (`src/services/`): Pure functions return effects (Reply, Kill, Mode) instead of mutating state
5. **Zero-Copy Parsing** (`crates/slirc-proto/`): `MessageRef<'a>` borrows from buffer, no allocations on hot path

### Directory Structure
```
slircd-ng/
├── src/
│   ├── handlers/        # 126 IRC command implementations (15 categories)
│   │   ├── bans/        # Ban management (KLINE, GLINE, etc.)
│   │   ├── batch/       # Batch message processing (IRCv3)
│   │   ├── cap/         # Capability negotiation + SASL
│   │   ├── channel/     # Channel operations (JOIN, PART, MODE, etc.)
│   │   ├── chathistory/ # Message history queries
│   │   ├── connection/  # Registration (NICK, USER, PASS, QUIT)
│   │   ├── messaging/   # PRIVMSG, NOTICE, TAGMSG
│   │   ├── mode/        # User and channel modes
│   │   ├── oper/        # Operator commands (KILL, WALLOPS, etc.)
│   │   ├── s2s/         # Server-to-server (CONNECT, SQUIT, LINKS)
│   │   ├── server/      # S2S protocol (SID, UID, SJOIN, TMODE)
│   │   ├── server_query/# Server info (ADMIN, INFO, LUSERS, MOTD, STATS)
│   │   ├── services/    # NickServ/ChanServ integration
│   │   └── user/        # User queries (WHO, WHOIS, MONITOR)
│   ├── state/           # State management (users, channels, sessions)
│   ├── services/        # NickServ/ChanServ logic and effects
│   ├── security/        # Crypto, bans, rate limiting
│   ├── db/              # SQLite queries and migrations (7 migrations)
│   ├── history/         # CHATHISTORY with Redb backend
│   ├── sync/            # Server-to-server synchronization
│   ├── network/         # TCP/TLS transport layer
│   └── main.rs          # Server entry point
├── crates/
│   └── slirc-proto/     # IRC protocol parsing library (reusable)
└── tests/               # Integration tests (~60 async tests)
```

## Documentation

- **ARCHITECTURE_AUDIT.md**: Detailed gap analysis and implementation status
- **ROADMAP.md**: Remaining work items (all Phase 1-6 items completed or deferred)
- **STATUS.md**: Module maturity assessment
- **DEPLOYMENT_CHECKLIST.md**: Pre-production verification steps
- **PROTO_REQUIREMENTS.md**: Protocol compliance and known gaps
- **CHANGELOG.md**: Version history

## Performance Characteristics

**Measured on AMD64 Linux (2026-01)**:
- Message routing: <1ms latency (local users)
- User lookup: O(1) via DashMap
- Channel actor queue: Bounded at 1024 events
- Connection throughput: 10K+ messages/second per connection (sustained)
- Memory usage: ~50-100 MB for 1000 users
- Max tested: 1000 concurrent connections

**Limitations**:
- Not tested beyond 1K users
- S2S federation performance unknown
- CHATHISTORY queries on large history (>100K messages) not benchmarked

## Dependencies

**Key dependencies** (see Cargo.toml for versions):
- **tokio**: Async runtime (1.x)
- **sqlx**: Database queries with SQLite (0.8.x)
- **redb**: Embedded key-value store for history (3.x)
- **dashmap**: Concurrent HashMap (6.x)
- **slirc-proto**: Custom IRC parsing library (workspace member)
- **tokio-rustls**: TLS support (0.26.x)
- **argon2**: Password hashing (0.5.x)
- **metrics/metrics-exporter-prometheus**: Observability (0.22.x/0.13.x)

**Total dependency count**: 100+ transitive dependencies
**Audit status**: Not externally audited for security vulnerabilities

## Known Issues & Limitations

### Critical
- **Cloak Secret Validation**: Server refuses to start with weak secrets but does not validate entropy scientifically
- **S2S Split-Brain**: No partition detection or automatic recovery in multi-server setups
- **CHATHISTORY Edge Cases**: Some queries return incorrect results or fail (30 irctest failures)

### Non-Critical
- **No Graceful Shutdown**: Server terminates immediately on SIGTERM (connections dropped)
- **Log Rotation**: Must be handled externally (systemd/journald recommended)
- **WebSocket Binary Frames**: Only text frames supported

## Contributing

1. **Code Quality**: All PRs must pass `cargo fmt`, `cargo clippy -- -D warnings`, and `cargo test`
2. **Tests Required**: New features need integration tests in `tests/` directory
3. **Documentation**: Update relevant .md files with changes
4. **Commit Style**: Clear, atomic commits with descriptive messages
5. **Protocol Compliance**: Check PROTO_REQUIREMENTS.md before adding IRC features

**Current Development Mode**: Fast iteration over hardening. Working logic preferred over abstraction. `todo!()` panics are acceptable for incomplete features (fail fast).

## License

Released to the **public domain** under [The Unlicense](LICENSE). Use freely for any purpose without restriction. No warranty provided.

## Links

- **Repository**: https://github.com/sid3xyz/slircd-ng
- **IRC Protocol RFCs**: RFC 1459, RFC 2812
- **IRCv3 Specifications**: https://ircv3.net
- **irctest Suite**: https://github.com/progval/irctest

---

**Last Updated**: 2026-02-04
**Audit Basis**: Source code inspection of commit HEAD on main branch
