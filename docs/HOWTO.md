# slircd-ng Development Guide

This guide covers common development tasks for the slircd-ng IRC daemon.

## Quick Start

### Building

```bash
cargo build --release
```

### Running

```bash
./target/release/slircd config.toml
```

See `config.toml` in the repository root for configuration examples.

## Development Workflow

### Testing

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run with logging
RUST_LOG=debug cargo test
```

### Database Migrations

Migrations are in `migrations/` directory and run automatically on startup.

To create a new migration:

1. Create `migrations/XXX_description.sql`
2. Migrations run in alphanumeric order
3. Test with a fresh database

### Adding New Commands

1. Add command variant to `slirc-proto::Command` (if not already present)
2. Create handler in `src/handlers/`
3. Register handler in `src/handlers/mod.rs::HandlerRegistry::new()`
4. Add tests

### Service Development (NickServ/ChanServ)

Services use the `ServiceEffect` pattern:

```rust
pub fn handle_nickserv_register(
    ctx: &Context,
    nick: &str,
    password: &str,
) -> Vec<ServiceEffect> {
    vec![
        ServiceEffect::SetRegistered(nick.to_string()),
        ServiceEffect::SendNotice(nick.to_string(), "Account registered.".to_string()),
    ]
}
```

Effects are applied by the caller, keeping services as pure functions.

## Architecture

See `docs/ARCHITECTURE.md` for detailed architecture documentation.

### Key Components

- **Gateway**: TCP/TLS listeners
- **Connection**: Per-client tokio task with MessageRef hot loop
- **Handlers**: Command processing with read-only Context
- **Matrix**: Shared state (DashMap for lock-free access)
- **Services**: NickServ/ChanServ as pure functions returning effects

## Troubleshooting

### Database Errors

Delete `slircd.db` to reset (development only):

```bash
rm slircd.db
cargo run
```

### TLS Certificate Issues

Generate self-signed cert for testing:

```bash
openssl req -x509 -newkey rsa:4096 -nodes \
  -keyout certs/key.pem -out certs/cert.pem \
  -days 365 -subj "/CN=localhost"
```

### Performance Profiling

```bash
# Build with debug symbols
cargo build --release

# Run with flamegraph
cargo flamegraph -- config.toml
```

## References

- RFC 1459: Original IRC specification
- RFC 2812: Updated IRC protocol
- IRCv3 specifications: https://ircv3.net/
- `slirc-proto` documentation: https://docs.rs/slirc-proto/
