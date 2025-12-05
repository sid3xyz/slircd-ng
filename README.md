# slircd-ng

Straylight IRC Daemon - Next Generation. A high-performance, multi-threaded IRC server built on zero-copy parsing.

## Features

- **High Performance**: Built on `tokio` for async I/O and `slirc-proto` for efficient message handling.
- **Zero-Copy Parsing**: Minimizes memory allocations during message processing.
- **Database Backed**: Uses SQLite (`sqlx`) for persistent storage of channels, bans, and shuns.
- **Security**:
  - **Cloaking**: IP cloaking support.
  - **Bans/Shuns**: K-lines (bans) and Shuns (silencing) support.
  - **TLS**: Secure connections via `tokio-rustls`.
- **Observability**: Structured logging with `tracing`.
- **Configuration**: TOML-based configuration.

## Getting Started

### Prerequisites

- Rust (latest stable)
- SQLite

### Running the Server

1.  **Configuration**: Ensure `config.toml` is present. You can copy `config.test.toml` as a starting point.
2.  **Run**:
    ```bash
    cargo run -p slircd-ng
    ```

### Development

- **Build**: `cargo build -p slircd-ng`
- **Test**: `cargo test -p slircd-ng`

## Architecture

- **Gateway**: Handles incoming connections and protocol decoding.
- **Matrix**: Manages global server state (users, channels) using lock-free `DashMap`s.
- **Handlers**: Process specific IRC commands.
- **Services**: Background tasks for enforcement and maintenance.

## License

Unlicense
