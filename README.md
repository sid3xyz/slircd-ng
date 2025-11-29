# slircd-ng

> **Straylight IRC Daemon — Next Generation**
> A high-performance, thread-safe, zero-copy IRC server written in Rust.

![Status](https://img.shields.io/badge/status-production_ready-green)
![License](https://img.shields.io/badge/license-Unlicense-blue)
![Rust](https://img.shields.io/badge/rust-1.70%2B-orange)

`slircd-ng` is a modern IRC daemon built on the [Tokio](https://tokio.rs) asynchronous runtime. It prioritizes performance and architectural purity, utilizing the `slirc-proto` library for zero-allocation message parsing in the hot path.

## 🚀 Key Features

### Core & Performance
* **Zero-Copy Parsing:** Utilizes `MessageRef<'a>` to borrow directly from the transport buffer, minimizing heap allocations.
* **Lock-Free State:** Uses `DashMap` for high-concurrency user and channel management.
* **RFC Compliance:** Full support for RFC 1459 and RFC 2812 protocols.

### Modern IRCv3 Support
* **Capabilities:** `account-notify`, `away-notify`, `extended-join`, `multi-prefix`, `server-time`.
* **Tags:** Full support for IRCv3 message tags and `TAGMSG`.
* **Authentication:** SASL PLAIN and EXTERNAL mechanisms active.

### Integrated Services
Built-in service bots with SQLite persistence:
* **NickServ:** Account registration, grouping, enforcement, and certfp.
* **ChanServ:** Channel registration, access lists (flags system), and auto-kicks (AKICK).
* **Architecture:** Services operate as pure functions returning `ServiceEffect` vectors, ensuring state isolation.

### Security
* **Native TLS:** Integrated `rustls` support for secure connections (port 6697).
* **Moderation:** Persistent K-Lines (User bans) and D-Lines (IP bans).
* **Strict Mode:** Enforcement timers for unregistered nicknames.

---

## 🛠️ Installation & Build

**Prerequisites:**
* Rust 1.70 or higher
* OpenSSL (optional, only if not using pure Rust TLS ring)

```bash
# Clone the repository
git clone https://github.com/sid3xyz/slircd-ng.git
cd slircd-ng

# Build in release mode
cargo build --release

# Run the server
./target/release/slircd config.toml
```

---

## ⚙️ Configuration

Create a `config.toml` file in the root directory:

```toml
[server]
name = "irc.straylight.net"
network = "Straylight"
sid = "001"                 # TS6 Server ID (Unique 3-char string)
description = "slircd-ng Production Server"

[listen]
address = "0.0.0.0:6667"    # Plaintext port

[tls]
address = "0.0.0.0:6697"    # Secure port
cert_path = "certs/fullchain.pem"
key_path = "certs/privkey.pem"

[database]
path = "slircd.db"          # SQLite storage path

[[oper]]
name = "admin"
password = "password"       # Plaintext for now
```

### Generating Self-Signed Certs (Testing)

```bash
openssl req -x509 -newkey rsa:4096 -nodes -keyout key.pem -out cert.pem -days 365 -subj "/CN=localhost"
mkdir certs && mv *.pem certs/
```

---

## 📖 Operator Commands

| Command | Syntax | Description |
|---------|--------|-------------|
| **OPER** | `OPER <name> <pass>` | Authenticate as an IRC operator |
| **KLINE** | `KLINE user@host :reason` | Ban a user mask permanently |
| **DLINE** | `DLINE ip_addr :reason` | Ban an IP address permanently |
| **KILL** | `KILL <nick> :reason` | Force disconnect a user |
| **REHASH**| `REHASH` | Reload configuration (Stub) |
| **SAJOIN**| `SAJOIN <nick> <chan>` | Force a user into a channel |
| **SAMODE**| `SAMODE <target> <modes>` | Force mode changes without ops |

---

## 🧩 Architecture

`slircd-ng` enforces a strict separation of concerns:

1.  **Gateway:** Manages TCP/TLS listeners and spawns connection tasks.
2.  **Connection:** Handles the `tokio::select!` hot loop, reading `MessageRef` from the transport.
3.  **Handlers:** Process commands using a read-only `Context`.
4.  **Services:** NickServ/ChanServ are implemented as pure logic returning `Vec<ServiceEffect>`.
5.  **Matrix:** The shared state container, modified only by the Router applying effects.

---

## 📄 License

This project is released into the public domain under [The Unlicense](https://unlicense.org).
