# System Architecture

## Overview

slircd-ng is a high-performance, distributed IRC daemon written in Rust. It leverages modern asynchronous patterns and a distributed state model to provide a robust and scalable messaging platform.

## Core Components

### 1. Zero-Copy Protocol Parsing (`slirc-proto`)
- Located in `crates/slirc-proto`.
- Uses `MessageRef<'a>` to parse IRC commands without allocating strings for parameters.
- Provides strict validation of IRC-style inputs (tags, prefixes, commands).

### 2. Async Runtime
- Built on **Tokio** for asynchronous I/O.
- Uses `MainLoop` pattern to handle incoming connections and signals.
- **Client Tasks**: Each client connection runs in its own lightweight Tokio task.

### 3. Actor Model for Channels
- Channels are implemented as actors (`Channel` struct).
- Clients communicate with channels via `mpsc` channels (bounded mailboxes).
- Ensures thread safety and prevents lock contention on hot paths (message broadcasting).

### 4. Modular Handler System
- **Typestate Pattern**: Handlers enforce state transitions (e.g., `Unregistered` -> `Registered`).
- **Traits**:
  - `PostRegHandler`: For standard commands (PRIVMSG, JOIN).
  - `PreRegHandler`: For registration handshake (NICK, USER, CAP).
  - `UniversalHandler<S>`: For universal commands (PING, QUIT).

### 5. Distributed State (CRDT)
- **Conflict-Free Replicated Data Types** manage state across linked servers.
- **LWW-Element-Set** (Last-Write-Wins) used for channel membership and topic synchronization.
- Allows split-brain resolution without data loss.

### 6. Bouncer / Multiclient
- **Session Management**: Tracks multiple connections (sessions) per user account.
- **Fan-out**: Incoming messages to an account are distributed to all connected sessions.
- **Self-Echo**: Messages sent by one session are echoed to all other sessions of the same account, ensuring consistency across devices.
- **Persistence**: "Always-on" clients remain in channels even when all physical connections disconnect.

## Data Storage

- **SQLite (sqlx)**:
  - User accounts (credentials, preferences).
  - Operator permissions (`oper` blocks).
  - Persistent bans (X-lines).
- **Redb (Embedded Key-Value)**:
  - Message history (`CHATHISTORY` command).
  - Efficient range queries for message retrieval.

## Security Architecture

- **TLS**: `rustls` based encryption.
- **SASL**: Pluggable authentication (PLAIN, SCRAM).
- **Cloaking**: HMAC-based IP address obfuscation for privacy.
- **Rate Limiting**: Token bucket algorithms per IP and per account.
