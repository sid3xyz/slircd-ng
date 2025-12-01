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
* **Lock-Free State:** Uses `DashMap` for high-concurrency user and channel management (The "Matrix").
* **RFC Compliance:** Full support for RFC 1459 and RFC 2812 protocols.
* **Observability:** Built-in Prometheus metrics exporter on port 9090.

### Connectivity

* **TCP & TLS:** Native support for standard and secure (SSL/TLS) connections.
* **WebSocket:** Integrated WebSocket gateway for web-based clients (e.g., `slirc-web`).

### Integrated Services

Built-in service bots with SQLite persistence:

* **NickServ:** Account registration, grouping, and enforcement.
* **ChanServ:** Channel registration, access lists (AKICK/ACCESS), and topic control.
* **Service Effects:** Services operate as pure functions returning `ServiceEffect` vectors, ensuring state isolation and testability.

### Security

* **Host Cloaking:** HMAC-based IP cloaking to protect user privacy.
* **Rate Limiting:** Token bucket algorithms for flood protection.
* **Spam Detection:** Heuristic analysis to detect and block spam bots.
* **X-Lines:** Persistent server-side bans (K-Line, G-Line, Z-Line) stored in SQLite.

## 🏗️ Architecture

### The Matrix

The core of `slircd-ng` is the **Matrix** (`src/state/matrix.rs`), a shared state container holding all users, channels, and server configurations. It uses `DashMap` to allow lock-free concurrent access from multiple async tasks.

```rust
pub struct Matrix {
    pub users: DashMap<Uid, Arc<RwLock<User>>>,
    pub channels: DashMap<String, Arc<RwLock<Channel>>>,
    pub nicks: DashMap<String, Uid>,
    // ...
}
```

### Service Effects Pattern

Services like NickServ and ChanServ do not mutate server state directly. Instead, they return a vector of `ServiceEffect` enums. This decouples business logic from state management.

```rust
// Example: Services return effects, they don't change state directly
pub enum ServiceEffect {
    Reply { msg: Message },
    AccountIdentify { target_uid: String, account: String },
    Kill { target_uid: String, reason: String },
    // ...
}
```

## ⚙️ Configuration

Configuration is handled via `config.toml`. Key sections include:

```toml
[server]
name = "irc.straylight.net"
sid = "001"
metrics_port = 9090

[listen]
address = "0.0.0.0:6667"

[database]
path = "slircd.db"

[security]
cloak_secret = "CHANGE_THIS_IN_PRODUCTION"
spam_detection_enabled = true

[limits]
rate = 2.5  # Messages per second
burst = 5.0
```

## 🛠️ Build & Run

**Prerequisites:**

* Rust 1.70+
* SQLite (bundled or system)

```bash
# Build the server
cargo build -p slircd-ng --release

# Run with configuration
cargo run -p slircd-ng -- config.toml
```

## 📂 Project Structure

* `src/state/`: The Matrix and shared state definitions.
* `src/handlers/`: IRC command handlers (JOIN, PRIVMSG, etc.).
* `src/services/`: NickServ, ChanServ, and the Service Effects engine.
* `src/network/`: TCP/TLS/WebSocket transport layers.
* `src/db/`: SQLite persistence layer.
