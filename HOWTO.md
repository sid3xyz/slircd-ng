# slirc-proto: The Definitive Guide

This guide provides a deep dive into using `slirc-proto`, a high-performance Rust library for parsing and serializing IRC protocol messages with full IRCv3 support.

## 1. Core Concepts

`slirc-proto` is designed around a few key types:

- **`Message`**: An owned, heap-allocated IRC message. Easy to use, safe to pass around.
- **`MessageRef<'a>`**: A zero-copy, borrowed view of an IRC message. Extremely fast, ties to the lifetime of the input string.
- **`Command`**: A strongly-typed enum representing every supported IRC command (e.g., `PRIVMSG`, `JOIN`, `KLINE`).
- **`Prefix`**: Represents the source of a message (e.g., `nick!user@host` or `servername`).
- **`Tag`**: IRCv3 message tags (key-value pairs).

## 2. Parsing Messages

### Owned Parsing (Easiest)
Use this when you need to keep the message around or pass it between threads.

```rust
use slirc_proto::{Message, Command};

let raw = "@time=12345 :nick!user@host PRIVMSG #channel :Hello world!";
let msg: Message = raw.parse().expect("Failed to parse");

if let Command::PRIVMSG(target, text) = msg.command {
    println!("{} says to {}: {}", msg.source_nickname().unwrap(), target, text);
}
```

### Zero-Copy Parsing (Fastest)
Use this in hot loops (like a server) where you process and discard messages immediately.

```rust
use slirc_proto::MessageRef;

let raw = "@time=12345 :nick!user@host PRIVMSG #channel :Hello world!";
// Returns a MessageRef<'a> borrowing from `raw`
let msg = MessageRef::parse(raw).expect("Failed to parse");

// Accessors are similar, but return &str
assert_eq!(msg.command_str(), "PRIVMSG");
```

## 3. Working with Commands

The `Command` enum is the heart of the library. It covers standard RFC 1459/2812 commands, IRCv3 extensions, and common server-side operations.

### Pattern Matching
```rust
match msg.command {
    Command::JOIN(channel, key) => {
        println!("Joining {}", channel);
    },
    Command::KLINE(duration, mask, reason) => {
        // Typed operator commands!
        println!("Banning {} for {:?}", mask, duration);
    },
    Command::Raw(cmd, args) => {
        // Fallback for unknown commands
        println!("Unknown command: {} {:?}", cmd, args);
    },
    _ => {}
}
```

### Operator Commands
We support typed variants for administrative actions to avoid `Command::Raw`:
- `KLINE`, `DLINE`, `UNKLINE`, `UNDLINE`
- `KNOCK`, `CHGHOST`, `SAJOIN`, `SAMODE`

## 4. Constructing & Serializing

You can build messages using helper methods or by constructing the struct directly.

### Using Builders (Recommended)
```rust
use slirc_proto::{Message, Prefix};

let msg = Message::privmsg("#rust", "Hello!")
    .with_prefix(Prefix::new_from_str("mybot!bot@example.com"))
    .with_tag("time", Some("2023-11-28T12:00:00Z"));

println!("{}", msg); // Serializes to wire format
```

### Manual Construction
```rust
use slirc_proto::{Message, Command};

let msg = Message {
    tags: None,
    prefix: None,
    command: Command::KLINE(Some("60".into()), "*@bad.host".into(), "Spam".into()),
};
```

## 5. Handling Modes

Modes are complex because they can be user modes (`+i`) or channel modes (`+o nick`). `slirc-proto` provides typed handling to avoid string parsing errors.

### The `Mode<T>` Type
```rust
use slirc_proto::{Mode, ChannelMode, UserMode};

// Constructing a channel mode change
let modes = vec![
    Mode::plus(ChannelMode::Op, Some("nick")),
    Mode::minus(ChannelMode::Secret, None),
    Mode::Plus(ChannelMode::Ban, Some("*!*@bad".into()))
];

let cmd = Command::ChannelMODE("#channel".into(), modes);
```

**Note**: Always use `Mode::plus`/`Mode::minus` or the enum variants. Avoid constructing raw mode strings manually if possible.

## 6. Transport & Async (Tokio)

If the `tokio` feature is enabled (default), you get `IrcCodec` for framing.

```rust
use futures::StreamExt;
use tokio::net::TcpStream;
use tokio_util::codec::Framed;
use slirc_proto::IrcCodec;

#[tokio::main]
async fn main() {
    let stream = TcpStream::connect("irc.example.com:6667").await.unwrap();
    let mut framed = Framed::new(stream, IrcCodec::new());

    // Send
    framed.send(Message::nick("mybot")).await.unwrap();
    framed.send(Message::user("mybot", "Bot")).await.unwrap();

    // Receive
    while let Some(Ok(msg)) = framed.next().await {
        println!("Received: {}", msg);
    }
}
```

## 7. IRCv3 Capabilities

The library has first-class support for `CAP` negotiation and subcommands.

```rust
use slirc_proto::{Command, CapSubCommand};

// Request capabilities
let cap_req = Command::CAP(
    None, 
    CapSubCommand::REQ, 
    None, 
    Some("server-time message-tags".into())
);
```

## 8. Best Practices

1.  **Prefer `MessageRef` for Servers**: If you are writing a server or a high-throughput bot, use `MessageRef` to avoid allocation overhead.
2.  **Use Typed Commands**: Don't rely on `Command::Raw` unless necessary. If a command is missing, open an issue or PR to add it to `Command` enum.
3.  **Sanitization**: The library handles basic sanitization (e.g., no newlines in commands), but always validate user input before constructing administrative commands.
4.  **ISUPPORT**: Use the `isupport` module to parse server features. This is critical for knowing which channel modes require arguments.

## 9. Feature Flags

- `tokio`: Enables `IrcCodec` and async utilities (Default: enabled).
- `proptest`: Enables property-based testing utilities.
- `encoding`: Enables `encoding_rs` support for non-UTF8 fallbacks.
