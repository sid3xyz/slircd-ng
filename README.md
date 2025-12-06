# slircd-ng

> **⚠️ PERMANENT NOTICE: This software is NEVER production ready. It is a learning exercise and proof-of-concept only. Do not deploy, do not use for any real network, do not trust any claims of security or stability. All documentation and code are for developer reference and experimentation only.**

`slircd-ng` (Straylight IRC Daemon - Next Generation) is a modern, high-performance IRC server written in Rust. It leverages the `slirc-proto` library for zero-copy protocol handling and `tokio` for asynchronous I/O, designed to be robust, scalable, and compliant with modern IRCv3 standards.

## Features

- **High Performance**: Built on a zero-copy parsing architecture using `slirc-proto`.
- **IRCv3 Compliance**: Native support for:
  - Capability Negotiation (`CAP`)
  - SASL Authentication (`PLAIN`, `SCRAM-SHA-256`)
  - Message Tags (`@time`, `@msgid`, `@account`)
  - Batch (`BATCH`)
  - Chat History (`CHATHISTORY`)
  - Monitor (`MONITOR`)
  - Account Tracking (`account-tag`, `account-notify`)
- **Persistence**: SQLite backend for storing:
  - Registered channels and topics
  - Operator bans (G-lines, K-lines, Z-lines, D-lines)
  - Shuns
- **Security**:
  - IP Cloaking
  - Granular ban system (Global, Local, IP, Duration-based)
  - Rate limiting
- **Observability**: Integrated `tracing` for structured logging and metrics.

## Architecture

`slircd-ng` uses a hybrid architecture:

- **Gateway**: Handles incoming TCP/TLS connections and framing.
- **Matrix**: The central shared state container, managing the global view of users and channels.
- **Actors**: Channels are implemented as actors to serialize state updates and prevent race conditions.
- **Handlers**: Command handlers operate on `MessageRef` types, minimizing memory allocations during command processing.

## Getting Started

### Prerequisites

- Rust 1.70+
- SQLite

### Configuration

Copy the example configuration:

```bash
cp config.toml.example config.toml
```

Edit `config.toml` to set your server name, network info, and listen ports.

### Running

Run the server:

```bash
cargo run --release
```

The server will create and initialize `slircd.db` automatically if configured.

### Database

`slircd-ng` uses `sqlx` with SQLite. Migrations are embedded and run on startup.

## Development

- **Build**: `cargo build`
- **Test**: `cargo test`
- **Lint**: `cargo clippy`

## License

Unlicense.
