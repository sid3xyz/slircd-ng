# slircd-ng

**Straylight IRC Daemon - Next Generation**

[![Rust](https://img.shields.io/badge/rust-1.85+-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Unlicense-blue.svg)](LICENSE)
[![CI](https://github.com/sid3xyz/slircd-ng/actions/workflows/ci.yml/badge.svg)](https://github.com/sid3xyz/slircd-ng/actions)

> **Status**: v1.0.0-alpha.1 — Feature-complete, suitable for testing.

A high-performance, distributed IRC server written in Rust, featuring modern architecture with zero-copy parsing, actor-based channel management, and CRDT-based state synchronization.

## 📊 Metrics

### Codebase

| Metric        | Value                                    |
| ------------- | ---------------------------------------- |
| Source files  | 412                                      |
| Lines of Rust | 50,000+                                  |
| Commands      | 81 (6 universal, 4 pre-reg, 71 post-reg) |
| IRCv3 Caps    | 21                                       |
| Migrations    | 7                                        |
| Test Coverage | 664 unit tests                           |

### Quality

| Metric         | Value             |
| -------------- | ----------------- |
| Clippy allows  | 19 (from 104)     |
| Capacity hints | 47                |
| Deep nesting   | 0 files >8 levels |
| TODOs/FIXMEs   | 0                 |

### Protocol Compliance

| Metric         | Value                                        |
| -------------- | -------------------------------------------- |
| irctest passed | 357/387 (92.2%)                              |
| Pass rate      | 92.2%                                        |
| Test suite     | irctest @ `./slirc-irctest`                  |
| Last run       | 2026-01-12                                   |
| Status         | ✅ Core protocols pass                       |

## 🚀 Features

### Architecture Innovations

1. **Zero-Copy Parsing**: Direct buffer borrowing eliminates allocation overhead
2. **Distributed Server Linking**: CRDT-based state synchronization with hybrid timestamps
3. **Actor Model Channels**: Lock-free per-channel task isolation
4. **Typestate Handlers**: Compile-time protocol state enforcement
5. **Event-Sourced History**: Pluggable message history backends

### IRC Protocol Support

**81 IRC Commands** organized by state:

- **Universal (6)**: QUIT, PING, PONG, NICK, CAP, REGISTER
- **Pre-Registration (4)**: USER, PASS, WEBIRC, AUTHENTICATE
- **Post-Registration (71)**: Complete IRC command set including:
  - **Channel Operations**: JOIN, PART, CYCLE, TOPIC, NAMES, MODE, KICK, LIST, INVITE, KNOCK
  - **Messaging**: PRIVMSG, NOTICE, TAGMSG, BATCH
  - **User Queries**: WHO, WHOIS, WHOWAS, USERHOST, ISON
  - **Server Queries**: VERSION, TIME, ADMIN, INFO, LUSERS, STATS, MOTD, MAP, RULES, LINKS, HELP
  - **Services**: NICKSERV/NS, CHANSERV/CS (9+11 commands)
  - **Operator Commands**: OPER, KILL, WALLOPS, GLOBOPS, DIE, REHASH, RESTART, CHGHOST, CHGIDENT, VHOST, TRACE
  - **Ban Management**: KLINE, DLINE, GLINE, ZLINE, RLINE, SHUN (+ UN* variants)
  - **Forced Actions**: SAJOIN, SAPART, SANICK, SAMODE
  - **Other**: AWAY, SETNAME, MONITOR, CHATHISTORY

### IRCv3 Capabilities (21)

| Category | Capabilities |
|----------|-------------|
| **Core** | multi-prefix, userhost-in-names, server-time, echo-message |
| **Batching** | batch, message-tags, labeled-response |
| **Presence** | away-notify, account-notify, extended-join, invite-notify, chghost, monitor, cap-notify |
| **Accounts** | account-tag, sasl (TLS-only) |
| **Drafts** | draft/multiline, draft/account-registration, draft/chathistory, draft/event-playback |

> 🔒 **Security Note**: SASL is only advertised over TLS connections to prevent plaintext password exposure.

### ISUPPORT Tokens

```
NETWORK, CASEMAPPING=rfc1459, CHANTYPES=#&+!,
PREFIX=(qaohv)~&@%+, CHANMODES=beIq,k,l,imnrst,
NICKLEN=30, CHANNELLEN=50, TOPICLEN=390, KICKLEN=390,
AWAYLEN=200, MODES=6, MAXTARGETS=4, MONITOR=100,
EXCEPTS=e, INVEX=I, ELIST=MNU, STATUSMSG=~&@%+,
BOT=B, WHOX
```

### Internal Services

**NickServ** (9 commands):  
REGISTER, IDENTIFY, GHOST, INFO, SET, DROP, GROUP, UNGROUP, CERT

**ChanServ** (11 commands):  
REGISTER, ACCESS, INFO, SET, DROP, OP, DEOP, VOICE, DEVOICE, AKICK, CLEAR

## 🛡️ Security Features

### Multi-Layer Defense

| Layer | Technology | Purpose |
|-------|-----------|---------|
| **Layer 1** | IP Deny List (Roaring Bitmap) | Nanosecond-scale instant rejection |
| **Layer 2** | Rate Limiting (Token Bucket) | Connection throttling per IP |
| **Layer 3** | DNSBL (DNS Blacklists) | Reputation checking |
| **Layer 4** | Heuristics (Pattern Matching) | Behavioral analysis |
| **Layer 5** | Spam Detection | Content filtering with reputation |
| **Layer 6** | X-lines (K/G/D/Z/R/Shun) | User/host/IP bans |

### Security Modules

| Module | File | Features |
|--------|------|----------|
| **DNSBL** | `src/security/dnsbl.rs` | DNS blacklist checking |
| **Reputation** | `src/security/reputation.rs` | User reputation scoring |
| **Heuristics** | `src/security/heuristics.rs` | Pattern-based abuse detection |
| **Spam** | `src/security/spam.rs` | Multi-stage spam pipeline |
| **X-lines** | `src/security/xlines.rs` | Ban types and matching |
| **Cloaking** | `src/security/cloaking.rs` | HMAC-based IP obfuscation |
| **Rate Limit** | `src/security/rate_limit.rs` | Token bucket limiter |
| **Ban Cache** | `src/security/ban_cache.rs` | In-memory LRU cache |
| **IP Deny** | `src/security/ip_deny/` | Dual-engine (Bitmap + Redb) |

### Authentication & Encryption

- **Password Hashing**: Argon2 (memory-hard, GPU-resistant)
- **TLS Support**: Optional TLS with client certificate validation (CERTFP)
- **SASL**: PLAIN and EXTERNAL mechanisms (TLS-only advertisement)
- **IP Cloaking**: Deterministic HMAC-based cloaking with configurable secret

## 📦 Installation

### Prerequisites

- **Rust**: 1.85+ (stable) — Edition 2024 is now stable
- **Platform**: Linux, macOS, or Windows

### Building from Source

```bash
# Clone the repository
git clone https://github.com/sid3xyz/slircd-ng.git
cd slircd-ng

# Build release binary
cargo build --release

# Run tests
cargo test

# Lint
cargo clippy -- -D warnings

# Run server
./target/release/slircd config.toml
```

### Pre-built Binaries

Download from [GitHub Releases](https://github.com/sid3xyz/slircd-ng/releases):
- Linux x86_64
- macOS x86_64 / ARM64
- Windows x86_64

## ⚙️ Configuration

### Basic Configuration

Create a `config.toml` file:

```toml
[server]
name = "irc.example.com"       # Server hostname
network = "ExampleNet"          # Network name
sid = "001"                     # Server ID (3 chars)
password = "linkpassword"       # S2S link password
metrics_port = 9090             # Prometheus metrics port (0 to disable)

[[listen]]
addr = "0.0.0.0:6667"          # Listen address:port
tls = false                     # Enable TLS
websocket = false               # Enable WebSocket

[[listen]]
addr = "0.0.0.0:6697"          # TLS listener
tls = true
websocket = false
[listen.tls]
cert_path = "cert.pem"          # TLS certificate
key_path = "key.pem"            # TLS private key

[database]
path = "slircd.db"              # SQLite database path

[security]
cloak_secret = "CHANGE_THIS"    # ⚠️ CHANGE IN PRODUCTION!
max_connections_per_ip = 3      # Connection limit per IP
connection_timeout_secs = 60    # Connection timeout

[history]
enabled = true                  # Enable message history
backend = "redb"                # Backend: "redb" or "none"
path = "history.db"             # History database path
retention_days = 30             # Message retention period

[[oper]]
name = "admin"                  # Operator name
password = "$argon2..."         # Hashed password (use argon2)

[[links]]                       # Server-to-server links
name = "hub.example.com"
addr = "hub.example.com:7000"
password = "linkpassword"
autoconnect = true
```

### Configuration Sections

| Section | Purpose |
|---------|---------|
| `[server]` | Server identity and settings |
| `[[listen]]` | Network listeners (TCP/TLS/WebSocket) |
| `[database]` | SQLite database configuration |
| `[security]` | Security settings (cloaking, rate limits) |
| `[history]` | Message history backend |
| `[[oper]]` | Operator accounts (multiple blocks allowed) |
| `[[links]]` | Server-to-server links (multiple blocks allowed) |
| `[webirc]` | WEBIRC trusted hosts |
| `[account_registration]` | NickServ registration settings |
| `[limits]` | Output limits (WHO, LIST, etc.) |

For complete configuration reference, see examples in the repository.

## 🗄️ Database

### Automatic Migrations

slircd-ng uses SQLx with embedded migrations that run automatically on startup:

| Migration | Purpose |
|-----------|---------|
| `001_init.sql` | Core schema (accounts, channels, bans, access) |
| `002_shuns.sql` | Shuns table |
| `002_xlines.sql` | X-lines table (K/G/D/Z/R-lines) |
| `003_history.sql` | Message history metadata |
| `004_certfp.sql` | Certificate fingerprint storage |
| `005_channel_topics.sql` | Persistent channel topics |
| `006_reputation.sql` | User reputation tracking |

### Database Backends

- **SQLite** (via SQLx): Primary database for accounts, channels, and bans
- **Redb** (optional): Embedded database for message history
- **In-Memory**: Test mode (`:memory:` path)

### Backup & Maintenance

```bash
# Backup database (SQLite)
cp slircd.db slircd.db.backup

# Vacuum database (periodic maintenance)
sqlite3 slircd.db "VACUUM;"

# Check integrity
sqlite3 slircd.db "PRAGMA integrity_check;"
```

## 📊 Monitoring & Metrics

### Prometheus Metrics

Metrics are exposed on `/metrics` endpoint (default port 9090):

**Connection Metrics**:
- `slircd_connections_total` - Total connections accepted
- `slircd_connections_active` - Currently active connections
- `slircd_connections_rejected` - Rejected connections (rate limit, IP deny)

**User Metrics**:
- `slircd_users_registered` - Currently registered users
- `slircd_users_unregistered` - Users in pre-registration state

**Channel Metrics**:
- `slircd_channels_total` - Total active channels

**Security Metrics**:
- `slircd_bans_active{type}` - Active ban count by type
- `slircd_rate_limit_hits` - Rate limit violations

**S2S Metrics** (distributed mode):
- `slircd_s2s_bytes_sent` - Bytes sent to peer servers
- `slircd_s2s_bytes_received` - Bytes received from peers
- `slircd_s2s_commands{type}` - Commands sent/received by type

**Performance**:
- `slircd_command_duration_seconds` - Command latency histogram

### Logging

Configure logging via `RUST_LOG` environment variable:

```bash
# Info level for all modules
RUST_LOG=info cargo run -- config.toml

# Debug specific modules
RUST_LOG=info,slircd_ng::handlers=debug cargo run -- config.toml

# Trace everything (very verbose)
RUST_LOG=trace cargo run -- config.toml
```

## 🔧 Development

### Project Structure

```
slircd-ng/
├── src/
│   ├── caps/               # IRCv3 capability negotiation
│   ├── config/             # Configuration loading
│   ├── db/                 # Database layer
│   ├── handlers/           # IRC command handlers (18k lines)
│   ├── history/            # Message history abstraction
│   ├── network/            # Network layer (gateway, connections)
│   ├── security/           # Security modules
│   ├── services/           # NickServ, ChanServ
│   ├── state/              # Server state (matrix, actors, managers)
│   ├── sync/               # Server-to-server synchronization
│   └── main.rs             # Entry point
├── migrations/             # SQL migrations
├── tests/                  # Integration tests
├── config.toml             # Example configuration
└── Cargo.toml              # Dependencies
```

### Architecture Overview

For a complete architectural deep dive, see [ARCHITECTURE.md](ARCHITECTURE.md).

**Key Components**:

1. **Matrix**: Central state hub coordinating 7 domain managers
   - UserManager, ChannelManager, SecurityManager
   - ServiceManager, MonitorManager, LifecycleManager
   - SyncManager (S2S)

2. **Handler Registry**: Typestate dispatch system
   - PreRegHandler (before registration)
   - PostRegHandler (after registration)
   - UniversalHandler (any state)

3. **Channel Actors**: Per-channel isolated tasks
   - Lock-free message broadcasting
   - Bounded mailboxes with backpressure
   - CRDT-based distributed state

4. **Security Pipeline**: 6-layer defense
   - IP deny list → Rate limiting → DNSBL → Heuristics → Spam → X-lines

### Testing

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run integration tests
cargo test --test distributed_channel_sync

# Run with output
cargo test -- --nocapture

# Run clippy
cargo clippy -- -D warnings
```

### Code Quality

The project follows strict code quality standards:

- **Clippy**: 19 allows (down from 104)
- **No deep nesting**: 0 files >8 levels
- **No TODOs**: All addressed
- **Capacity hints**: 47 pre-allocations
- **Documentation**: Inline module and function docs

### Contributing

Contributions are welcome! To contribute:

1. Fork the repository
2. Create a feature branch
3. Make your changes with tests
4. Run `cargo clippy -- -D warnings` and `cargo test`
5. Submit a pull request

Please follow the project's coding conventions documented in ARCHITECTURE.md.

## 🚢 Deployment

> ⚠️ **WARNING**: This software is NOT production-ready. Deploy at your own risk.

See [DEPLOYMENT_CHECKLIST.md](DEPLOYMENT_CHECKLIST.md) for comprehensive deployment guide.

### Quick Start (Development/Testing Only)

```bash
# 1. Build release binary
cargo build --release

# 2. Create data directory
mkdir -p data

# 3. Copy and edit config
cp config.toml config.production.toml
# IMPORTANT: Change cloak_secret, oper passwords, etc.

# 4. Test migrations
./target/release/slircd config.production.toml
# Watch logs for "Database migrations applied"

# 5. Run server
./target/release/slircd config.production.toml
```

### Production Checklist

Before deploying to production (NOT RECOMMENDED):

- [ ] Change `cloak_secret` from default
- [ ] Hash oper passwords (use Argon2)
- [ ] Configure TLS certificates
- [ ] Set up log rotation
- [ ] Configure firewall rules
- [ ] Set up monitoring (Prometheus)
- [ ] Test database migrations
- [ ] Configure backups
- [ ] Review security settings
- [ ] Test failover scenarios

## 🔍 Troubleshooting

### Common Issues

**Issue**: Server refuses to start with "cloak_secret" error  
**Solution**: Set a strong cloak secret: `openssl rand -hex 32` and add to `[security].cloak_secret`.

**Issue**: TLS handshake failed  
**Solution**: Check certificate paths in config, ensure cert/key are readable, verify cert is valid.

**Issue**: Database locked error  
**Solution**: Only one instance can access SQLite database. Stop other instances or use different database path.

**Issue**: Connection refused  
**Solution**: Check firewall rules, verify listen address in config, ensure port is available.

### Debug Logging

Enable verbose logging for troubleshooting:

```bash
RUST_LOG=debug ./target/release/slircd config.toml 2>&1 | tee debug.log
```

### Getting Help

- **Issues**: Open an issue on GitHub
- **Documentation**: See ARCHITECTURE.md for detailed information

## ⚠️ Known Limitations

### Alpha Release Caveats

1. **Limited Production Testing**: While feature-complete, this is an alpha release. Suitable for testing environments; monitor carefully in production.

2. **SQLite Backend**: Uses SQLite for persistence. Sufficient for small-to-medium networks (~5k users). PostgreSQL backend planned for 1.1.

3. **Single Maintainer**: The project is maintained by a single developer. Bus factor: 1.

### Resolved in v1.0.0-alpha.1

- ~~Missing dependencies~~ — slirc-proto and slirc-crdt now included in monorepo
- ~~Requires nightly Rust~~ — Edition 2024 stable since Rust 1.85
- ~~Default cloak secret~~ — Server now requires strong cloak secret on startup
- ~~Plaintext S2S~~ — TLS support for S2S links implemented
- ~~No S2S rate limiting~~ — Per-peer rate limiting implemented
- ~~DNSBL privacy leaks~~ — Privacy-preserving RBL service available

For architecture details, see [ARCHITECTURE.md](ARCHITECTURE.md).

## 📚 Documentation

- **[ARCHITECTURE.md](ARCHITECTURE.md)** - Complete architectural deep dive
- **[ALPHA_RELEASE_PLAN.md](ALPHA_RELEASE_PLAN.md)** - Current release status and roadmap
- **[DEPLOYMENT_CHECKLIST.md](DEPLOYMENT_CHECKLIST.md)** - Production deployment guide
- **[CHANGELOG.md](CHANGELOG.md)** - Version history

## 📜 License

This software is released into the public domain under the [Unlicense](LICENSE).

```
This is free and unencumbered software released into the public domain.
Anyone is free to copy, modify, publish, use, compile, sell, or
distribute this software for any purpose, commercial or non-commercial.
```

## 🙏 Acknowledgments

- **Rust Community**: For excellent async ecosystem
- **IRC Protocol**: RFC 1459, RFC 2812, IRCv3 working group
- **irctest**: Compliance testing suite
- **Dependencies**: See Cargo.toml for complete list

## 📞 Contact

**Author**: Sidney M Field III  
**Repository**: https://github.com/sid3xyz/slircd-ng

---

**Made with ❤️ and Rust** | **Not for production use** | **AI Research Experiment**
