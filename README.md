# slircd-ng

**Straylight IRC Daemon — Next Generation**

A high-performance, multi-threaded IRC server built in Rust with zero-copy parsing, per-channel actor isolation, CRDT-based distributed state, and native bouncer support.

## Features

- **Zero-copy parsing** — Protocol library (`slirc-proto`) parses messages without allocation using borrowed slices and stack-allocated argument arrays
- **Per-channel actor isolation** — Each channel runs as an independent Tokio task, eliminating lock contention on the message hot path
- **CRDT-based S2S** — Server linking uses Last-Writer-Wins Registers and Add-Wins Sets for conflict-free distributed state
- **Native bouncer** — Multi-session per account, always-on persistence, per-session capability tracking, message echo across sessions
- **27 IRCv3 capabilities** — SASL (PLAIN/EXTERNAL/SCRAM-SHA-256), CHATHISTORY, MONITOR, multiline, read-marker, account-registration, and more
- **Layered security** — Roaring Bitmap IP deny (nanosecond rejection), Governor rate limiting, HMAC-SHA256 cloaking, Argon2 passwords, spam detection, RBL integration
- **Capability token authorization** — Unforgeable non-Clone/non-Copy tokens replace `if is_oper()` checks with compile-time enforcement
- **Services** — Built-in NickServ (11 commands), ChanServ (12 commands), ZNC-compatible Playback
- **Persistent storage** — SQLite for accounts/bans/channels, Redb for message history and always-on state

## Quick Start

```bash
# Build
cargo build --release

# Configure
cp config.toml my-config.toml
# Edit my-config.toml — at minimum, set a strong cloak_secret:
#   openssl rand -hex 32

# Run
./target/release/slircd my-config.toml
# or
./target/release/slircd -c my-config.toml
```

Default listen: `0.0.0.0:6667` (plaintext).

## Configuration

TOML-based with `include` directive support for modular configs. Key sections:

| Section | Purpose |
|---------|---------|
| `[server]` | Server identity (name, network, sid), metrics port, idle timeouts |
| `[listen]` | Plaintext TCP address |
| `[tls]` | TLS listener (cert/key) |
| `[websocket]` | WebSocket listener |
| `[database]` | SQLite path |
| `[security]` | Cloak secret, spam detection, rate limits, exempt IPs |
| `[multiclient]` | Bouncer settings (always-on, max sessions, auto-away) |
| `[history]` | CHATHISTORY backend (redb/none) |
| `[account_registration]` | SASL/REGISTER settings |
| `[[oper]]` | Operator blocks |
| `[[link]]` | S2S peering |

See `config.toml` for a full commented example.

## Building

Requires Rust edition 2024 (nightly or stable with MSRV ≥ 1.85).

```bash
cargo build --release          # Optimized binary
cargo test                     # Run all 91 tests
cargo clippy --all-targets     # Lint
cargo doc --no-deps            # Generate documentation
```

## Architecture

```
main.rs → Config → Database → Matrix → Gateway → Connection Event Loop
                                │
                    ┌───────────┼───────────────┐
                    │           │               │
              ┌─────▼────┐ ┌───▼─────┐ ┌──────▼──────┐
              │ Managers │ │Registry │ │  Channel    │
              │ User     │ │Handler  │ │  Actors     │
              │ Channel  │ │Dispatch │ │  (isolated  │
              │ Security │ │         │ │   tasks)    │
              │ Service  │ │ PreReg  │ └─────────────┘
              │ Sync     │ │ PostReg │
              │ Client   │ │ Server  │
              │ etc.     │ │ Univers │
              └──────────┘ └─────────┘
```

The **Matrix** is the central state container (`Arc<Matrix>`), holding all managers as public fields. Handlers receive `Arc<Matrix>` via `Context` and route through capability-token authorization.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for detailed architecture documentation.

## Protocol Library (`crates/slirc-proto/`)

Standalone IRC protocol crate (v1.3.0) providing:
- Zero-copy message parsing (`MessageRef<'_>`, `CommandRef<'_>`)
- ~100 command variants (owned `Command` + borrowed `CommandRef`)
- RFC 2812 case folding (`irc_to_lower`)
- IRCv3 tag parsing
- CRDT types for distributed state (HybridTimestamp, LWWRegister, AWSet, ChannelCrdt, UserCrdt)
- SASL mechanism support (PLAIN, EXTERNAL, SCRAM-SHA-256)
- Tokio codec for async I/O

## IRC Commands (~95+)

Full RFC 2812 + IRCv3 + operator extensions. Handler directories:

| Area | Commands |
|------|----------|
| Connection | NICK, USER, PASS, PING, PONG, QUIT, CAP, AUTHENTICATE, STARTTLS, WEBIRC |
| Channel | JOIN, PART, TOPIC, KICK, INVITE, KNOCK, CYCLE, LIST, NAMES, MODE |
| Messaging | PRIVMSG, NOTICE, TAGMSG, ACCEPT, RELAYMSG, METADATA, BATCH |
| User Query | WHO, WHOIS, WHOWAS, ISON, USERHOST, MONITOR, AWAY, SETNAME, SILENCE |
| Server Query | LUSERS, STATS, VERSION, TIME, ADMIN, INFO, MOTD, RULES, HELP, USERIP |
| History | CHATHISTORY (LATEST, BEFORE, AFTER, BETWEEN, AROUND, TARGETS) |
| Services | REGISTER, NS/NICKSERV, CS/CHANSERV |
| Operator | OPER, KILL, WALLOPS, GLOBOPS, DIE, REHASH, RESTART, CHGHOST, CHGIDENT, VHOST, TRACE, SPAMCONF, CLEARCHAN |
| Bans | KLINE, DLINE, GLINE, ZLINE, RLINE, SHUN + UN- variants |
| Admin | SAJOIN, SAPART, SANICK, SAMODE |
| S2S | SERVER, SID, UID, SJOIN, TMODE, TB, ENCAP, CONNECT, SQUIT, LINKS, MAP |

## Security

- **Cloak enforcement**: Server refuses to start with weak `cloak_secret`
- **IP deny list**: Roaring Bitmap engine for nanosecond IP rejection (D/Z-lines)
- **Rate limiting**: Governor token bucket (per-client message, per-IP connection, per-client join)
- **Spam detection**: Multi-layer heuristics (entropy, URL, repetition)
- **Extended bans**: `$a:` (account), `$r:` (realname), `$j:` (channel), `$x:` (full), `$z` (TLS)
- **RBL integration**: Real-time Blackhole List lookups
- **Password hashing**: Argon2id with per-password random salt
- **TLS**: STARTTLS + Strict Transport Security (STS)

See [docs/SECURITY.md](docs/SECURITY.md) for details.

## Server Linking

TS6-like protocol with CRDT extensions:
- Handshake: PASS → CAPAB → SERVER → SVINFO
- State burst: bans → users → channels → topics → topology
- Netsplit handling: topology-based scope, mass-quit, cleanup
- Heartbeat: 30s PING, 90s timeout

See [docs/S2S_PROTOCOL.md](docs/S2S_PROTOCOL.md) for protocol details.

## Documentation

| Document | Description |
|----------|-------------|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Detailed architecture and module documentation |
| [docs/SECURITY.md](docs/SECURITY.md) | Security features and implementation |
| [docs/S2S_PROTOCOL.md](docs/S2S_PROTOCOL.md) | Server-to-server protocol specification |
| [docs/MODULE_MAP.md](docs/MODULE_MAP.md) | Complete source file map |
| [STATUS.md](STATUS.md) | Build status, test results, feature matrix |
| [CHANGELOG.md](CHANGELOG.md) | Version history |

Generate API documentation: `cargo doc --no-deps --document-private-items`

## Testing

```bash
# All tests
cargo test

# Specific test file
cargo test --test channel_ops

# With output
cargo test -- --nocapture
```

26 test files, 91+ tests covering: channels, messaging, services, bouncer, S2S, security, SASL, compliance.

## License

[Unlicense](LICENSE)
