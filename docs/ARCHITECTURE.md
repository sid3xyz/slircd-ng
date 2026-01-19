# SLIRC-ng Architecture

## Overview
SLIRC-ng is a modern, high-performance IRCv3 daemon written in Rust. It emphasizes safety, concurrency, and protocol compliance (IRCv3.2).

## Core Components

### 1. State Management (`src/state`)
The state is managed using a "Matrix" hierarchy:
- **Matrix Global State**: Holds server-wide configuration and capabilities.
- **Managers**: Specialized components for different domains:
  - `UserManager`: Tracks connected users (local and remote).
  - `ChannelManager`: Manages channel state and membership.
  - `ServerManager`: Maintains the server topology graph.
  - `SecurityManager`: Handles bans (`ip_deny`), rate limiting (`token_bucket`), and cloaking.
- **Actor Model**: State is protected by `RwLock` and `Arc`, with actor-like event processing for channels (`src/state/actor`).

### 2. Handlers (`src/handlers`)
Handlers process incoming IRC commands. They are organized by function:
- **Core**: Registry, context, and traits (`src/handlers/core`).
- **Connection**: Lifecycle handlers (pre-registration) like `NICK`, `USER` (`src/handlers/connection`).
- **User**: User-focused commands (`src/handlers/user`) including `monitor`, `status`, and queries (`who`, `whois`).
- **Channel**: Channel operations (`src/handlers/channel`).
- **Server**: Server-to-server protocol handlers (`src/handlers/server`), including `SID`, `UID`, `SJOIN`.
- **Services**: Service integration (`src/handlers/services`) like `NICKSERV`, `CHANSERV`.
- **Messaging**: Message routing (`src/handlers/messaging`) like `PRIVMSG`, `NOTICE`.
- **Util**: Shared helpers (`src/handlers/util`).

### 3. Database & Persistence (`src/db`)
A hybrid approach is used:
- **SQLite (SQLx)**: Relational data (User accounts, channel limits).
- **Redb**: Embedded, high-performance storage for chat history (`src/history/redb.rs`) and transient state.

### 4. Security (`src/security`)
- **Argon2**: Default password hashing for users and operators (`src/security/password.rs`).
- **IP Deny**: Dynamic IP banning persisted via JSON/Redb.
- **Rate Limiting**: Flood protection using token buckets.

## Key Innovations
- **Zero-Copy Parsing**: Uses `slirc-proto` for efficient message handling.
- **Unified Event Loop**: A single event loop (`src/network/connection/event_loop.rs`) handles all user interactions post-registration.
- **Typestate Registry**: Commands are segregated by connection state (Pre-Reg, Post-Reg, Server), making invalid state access impossible.

## Directory Structure
```
src/
├── config/       # Configuration loading (TOML)
├── db/           # Database repositories
├── handlers/     # Command implementations
│   ├── channel/
│   ├── connection/
│   ├── core/
│   ├── messaging/
│   ├── server/
│   ├── services/
│   ├── user/
│   └── util/
├── history/      # Chat history (IRCv3 chathistory)
├── network/      # TCP/TLS transport layer
├── security/     # Crypto and access control
├── services/     # Internal services logic
└── state/        # In-memory state containers
```
