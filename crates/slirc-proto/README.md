# slirc-proto

IRC protocol library for parsing and encoding IRC messages with IRCv3 support.

**Status**: Research prototype. Not production ready.

## Metrics

| Metric        | Value  |
| ------------- | ------ |
| Source files  | 82     |
| Lines of Rust | 17,210 |
| Version       | 1.3.0  |

## Features

Verified from `src/` and `Cargo.toml`:

- **Zero-copy parsing**: `MessageRef<'a>` in `src/message/borrowed.rs`
- **Owned messages**: `Message` in `src/message/mod.rs`
- **RFC compliance**: RFC 1459, RFC 2812, IRCv3 extensions
- **Response builders**: 37 constructor functions in `src/response/`
- **IRCv3 support**: Message tags, capabilities, SASL, batch in `src/ircv3/`
- **Mode parsing**: Channel and user modes in `src/mode/`
- **ISUPPORT**: Server parameter parsing in `src/isupport/`
- **Capabilities**: `src/caps/`
- **SASL**: PLAIN, EXTERNAL, SCRAM-SHA-256 (optional) in `src/sasl/`

### Optional Features (Cargo.toml)

- **tokio** (default): Async codec in `src/irc.rs`, transport in `src/transport/`
- **scram**: SCRAM-SHA-256 authentication
- **serde**: Serialization support
- **proptest**: Property-based testing

## Module Structure

```
src/
├── message/      # MessageRef<'a> (borrowed) and Message (owned)
├── command/      # Command parsing and construction
├── response/     # Numeric responses and constructors
├── mode/         # Channel and user mode parsing
├── prefix/       # Message prefix (nick!user@host)
├── ircv3/        # IRCv3 message tags
├── caps/         # Capability negotiation
├── isupport/     # ISUPPORT token parsing
├── sasl/         # SASL authentication
├── transport/    # Async networking (tokio feature)
├── websocket.rs  # IRC-over-WebSocket
├── casemap.rs    # RFC 1459 case mapping
├── chan.rs       # Channel name utilities
├── nick.rs       # Nickname utilities
├── ctcp.rs       # CTCP message handling
└── crdt/         # CRDT primitives for distributed state
```

## Usage

```rust
use slirc_proto::{Message, MessageRef};

// Zero-copy parsing
let raw = ":nick!user@host PRIVMSG #channel :Hello!";
let msg = MessageRef::parse(raw).unwrap();
assert_eq!(msg.command_name(), "PRIVMSG");

// Owned message construction
let privmsg = Message::privmsg("#rust", "Hello, world!");
println!("{}", privmsg); // Serializes to IRC protocol
```

## Build

```bash
cargo build --all-features
cargo test --all-features
cargo clippy -- -D warnings
```

## License

Unlicense
