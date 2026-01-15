# slircd-ng

**Straylight IRC Daemon - Next Generation**

[![Rust 1.85+](https://img.shields.io/badge/Rust-1.85+-orange?logo=rust)](https://www.rust-lang.org)
[![License: Unlicense](https://img.shields.io/badge/License-Unlicense-blue)](LICENSE)
[![Alpha Release](https://img.shields.io/badge/Release-v1.0.0--alpha.1-brightgreen)](https://github.com/sid3xyz/slircd-ng/releases/tag/v1.0.0-alpha.1)
[![CI/CD](https://github.com/sid3xyz/slircd-ng/actions/workflows/ci.yml/badge.svg)](https://github.com/sid3xyz/slircd-ng/actions)

> **A high-performance, distributed IRC daemon** written in Rust with modern architecture: zero-copy parsing, actor-based channels, and CRDT state synchronization.

---

## Quick Start

### Installation

**Requirements**: Rust 1.85+ (stable)

```bash
git clone https://github.com/sid3xyz/slircd-ng.git
cd slircd-ng
cargo build --release
./target/release/slircd config.toml
```

### Configuration

Create `config.toml`:

```toml
[server]
name = "irc.example.com"
sid = "001"
description = "slircd-ng IRC Server"
port = 6667
tls_port = 6697
password = "your-secret-key"  # Random 32 chars

[server.admin]
line1 = "Admin Name"
line2 = "Admin Email"
line3 = "Admin URL"

[database]
path = "data/irc.db"
```

Then run:

```bash
./target/release/slircd config.toml
```

Connect with your IRC client:

```
/server irc.example.com 6667
/join #test
```

---

## Status: v1.0.0-alpha.1 ✅

### Release Highlights

| Metric | Value | Status |
|--------|-------|--------|
| **Rust Tests** | 664 passing | ✅ |
| **irctest Compliance** | 357/387 (92.2%) | ✅ |
| **Code Quality** | Clippy 0 warnings | ✅ |
| **Format** | 100% compliant | ✅ |
| **CI/CD** | GitHub Actions | ✅ |
| **Documentation** | Complete | ✅ |

### What's Included

- ✅ **60+ IRC Handlers**: PRIVMSG, JOIN, MODE, WHO, WHOIS, and more
- ✅ **21 IRCv3 Capabilities**: Modern protocol features (SASL, account tracking, etc.)
- ✅ **Server Linking**: CRDT-based distributed state synchronization
- ✅ **Session Management**: Bouncer architecture for connection resumption
- ✅ **Message History**: CHATHISTORY with Redb persistence
- ✅ **Security**: SCRAM-SHA-256, CertFP, ban enforcement
- ✅ **Unicode Support**: Confusables detection, UTF-8 validation
- ✅ **TLS/WebSocket**: encrypted connections, web client support

### Known Limitations

- ⚠️ **Bouncer**: Core architecture present, session tracking incomplete
- ⚠️ **S2S Linking**: Single-server mode works; multi-server features in beta
- ⚠️ **irctest**: 92.2% passing (357/387 tests) — see [ROADMAP.md](ROADMAP.md) for remaining gaps
- ⚠️ **Production**: Not recommended for public deployments yet; suitable for testing

---

## Key Features

### 🏗️ Architecture Innovations

1. **Zero-Copy Message Parsing**  
   Direct buffer borrowing via `MessageRef<'a>` eliminates allocation overhead.

2. **Actor Model Channels**  
   Each channel runs as its own Tokio task with bounded message queues—no global locks on hot path.

3. **Typestate Protocol Enforcement**  
   Compile-time state machine via trait system prevents invalid state transitions.

4. **CRDT-Based Sync**  
   Conflict-free replicated data types enable multi-server linking without coordination.

5. **Service Effects**  
   Pure service logic returns effects instead of mutating state—testable and auditable.

### 📡 Protocol Support

**81 IRC Commands**:
- **Channels**: JOIN, PART, MODE, TOPIC, KICK, INVITE, LIST, CYCLE, KNOCK
- **Messaging**: PRIVMSG, NOTICE, TAGMSG, BATCH
- **Queries**: WHO, WHOIS, WHOWAS, USERHOST, ISON, USERS
- **Services**: NICKSERV (REGISTER, IDENTIFY, GHOST, etc.), CHANSERV
- **Server Ops**: OPER, KILL, WALLOPS, GLOBOPS
- **Moderation**: KLINE, DLINE, GLINE, SHUN, SILENCE, MONITOR
- **History**: CHATHISTORY (LATEST, BEFORE, AFTER, BETWEEN, TARGETS)
- **Roleplay**: NPC command, MODE +E support

**IRCv3 Capabilities** (21 total):
- Core: `multi-prefix`, `userhost-in-names`, `server-time`, `echo-message`
- Batching: `batch`, `message-tags`, `labeled-response`
- Presence: `away-notify`, `account-notify`, `monitor`, `chghost`
- Accounts: `account-tag`, `sasl` (TLS-only)
- Drafts: `multiline`, `account-registration`, `chathistory`, `event-playback`

### 🔒 Security

- **SASL Authentication**: PLAIN and SCRAM-SHA-256
- **TLS/SSL**: rustls with modern cipher support
- **CertFP**: Certificate fingerprint authentication
- **Ban Management**: KLINE, DLINE, GLINE, XLINE, SHUN with CIDR support
- **Rate Limiting**: Per-client message throttling and join/part limits
- **Audit Logging**: Operator actions and service commands

### 💾 Persistence

- **SQLite**: User accounts, ban lists, SCRAM verifiers
- **Redb**: Message history (CHATHISTORY) with efficient queries
- **7 Migrations**: Schema evolution with no data loss

---

## Development

### Building

```bash
# Development (debug build)
cargo build

# Release (optimized)
cargo build --release

# Run tests
cargo test

# Run irctest suite
cd slirc-irctest
SLIRCD_BIN=../target/release/slircd pytest --controller=irctest.controllers.slircd irctest/server_tests/
```

### Code Quality

```bash
# Format check
cargo fmt -- --check

# Linting (19 allowed exceptions documented)
cargo clippy -- -D warnings

# Documentation
cargo doc --no-deps --open
```

### Project Structure

```
slircd-ng/
├── src/                           # Main daemon code
│   ├── handlers/                  # 60+ IRC command handlers
│   ├── state/                     # User/channel state management
│   ├── sync/                      # Server-to-server synchronization
│   ├── security/                  # TLS, SASL, bans
│   ├── services/                  # NickServ, ChanServ
│   └── db/                        # Database queries and migrations
├── crates/
│   └── slirc-proto/               # IRC protocol parsing (reusable)
└── tests/                         # Integration tests
```

---

## Documentation

- **[ARCHITECTURE.md](ARCHITECTURE.md)** - Deep dive into system design
- **[ROADMAP.md](ROADMAP.md)** - Release timeline and strategic direction
- **[PROTO_REQUIREMENTS.md](PROTO_REQUIREMENTS.md)** - Protocol blockers and enhancements
- **[DEPLOYMENT_CHECKLIST.md](DEPLOYMENT_CHECKLIST.md)** - Pre-deployment verification
- **[CHANGELOG.md](CHANGELOG.md)** - Release notes and version history

---

## Performance

slircd-ng is designed for high performance:

| Benchmark | Result |
|-----------|--------|
| Message Routing | <1ms latency |
| User Lookup | O(1) via HashMap |
| Channel Actor | Bounded queue (1024 events) |
| Connection Throughput | 10K+ msg/sec per connection |
| Memory (1K users) | ~50-100 MB |

Load tested with up to 1000 concurrent users.

---

## Compliance

### IRC Standards

- ✅ **RFC 1459**: Core IRC protocol
- ✅ **RFC 2812**: Updated specifications
- ✅ **IRCv3 Specifications**: Modern extensions
- ✅ **irctest Suite**: 92.2% passing (357/387 tests)

### Code Quality

- ✅ `cargo fmt`: 100% formatting compliance
- ✅ `cargo clippy -- -D warnings`: 0 warnings (19 documented exceptions)
- ✅ `cargo test`: 664 tests passing
- ✅ Zero unsafe code in library code
- ✅ Zero TODO/FIXME markers

---

## Technology Stack

### Runtime & Async
- **Tokio**: Multi-threaded async runtime
- **Bytes**: Zero-copy buffer handling
- **Futures**: Composable async utilities

### Crypto & Security
- **rustls**: TLS/SSL connections
- **sha2, hmac, pbkdf2**: SCRAM-SHA-256 hashing
- **uuid**: Unique identifiers

### Database
- **sqlx**: Async SQL with SQLite
- **redb**: Embedded key-value store

### Utilities
- **serde**: Configuration serialization (TOML)
- **chrono**: Timestamp handling
- **tracing**: Structured logging
- **dashmap**: Concurrent HashMap
- **parking_lot**: Optimized locks
- **confusables**: Unicode nick validation

---

## Contributing

slircd-ng welcomes contributions! Guidelines:

1. **Code Quality**: Pass `cargo fmt` and `cargo clippy -- -D warnings`
2. **Tests**: Add tests for new features; all tests must pass
3. **Documentation**: Update relevant docs with changes
4. **Commits**: Clear, atomic commits with descriptive messages
5. **Issues**: Reference issue numbers in PRs

See [PROTO_REQUIREMENTS.md](PROTO_REQUIREMENTS.md) for known blockers before implementing features.

---

## License

Released to the public domain under **[The Unlicense](LICENSE)**. Use freely for any purpose without restriction.

---

## References

- [GitHub Repository](https://github.com/sid3xyz/slircd-ng)
- [irctest Compliance Suite](https://github.com/progval/irctest)
- [IRCv3 Specifications](https://ircv3.net)
- [RFC 1459 - Internet Relay Chat Protocol](https://tools.ietf.org/html/rfc1459)
- [RFC 2812 - Internet Relay Chat: Client Protocol](https://tools.ietf.org/html/rfc2812)

