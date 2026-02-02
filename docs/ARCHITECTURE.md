# slircd-ng Architecture

## Overview

Modern IRC daemon in Rust 2024. Actor-based channels, typestate handlers, zero-copy parsing.

## Core Components

### Matrix (`src/state/matrix.rs`)
Central state container. Everything flows through `Arc<Matrix>`:
- `user_manager` - Connected users, nicks, WHOWAS
- `channel_manager` - Channel actors
- `client_manager` - Bouncer sessions
- `security_manager` - Bans, rate limits
- `service_manager` - NickServ, ChanServ
- `sync_manager` - S2S federation
- `stats_manager` - LUSERS, uptime

### Handlers (`src/handlers/`)
141 files, 25 directories. Typestate traits:
- `PreRegHandler` - Before registration
- `PostRegHandler` - After registration  
- `ServerHandler` - S2S commands

Directories:
```
handlers/
├── bans/           # KLINE, GLINE, etc.
├── cap/            # CAP, SASL
├── channel/        # JOIN, PART, MODE
├── chathistory/    # CHATHISTORY
├── connection/     # NICK, USER, QUIT
├── messaging/      # PRIVMSG, NOTICE
├── mode/           # User/channel modes
├── oper/           # Operator commands
├── s2s/            # Server-to-server
├── server/         # SERVER, BURST
├── server_query/   # ADMIN, INFO, LUSERS
├── services/       # NickServ, ChanServ routing
└── user/           # WHO, WHOIS, AWAY
```

### Channel Actors (`src/state/actor/`)
Each channel is a Tokio task with bounded mailbox:
- Members, modes, topic, bans owned by actor
- Events via `mpsc::Sender<ChannelEvent>`
- No lock contention on message routing

### Services (`src/services/`)
Pure functions returning effects:
```rust
pub enum ServiceEffect {
    Reply { target_uid, msg },
    AccountIdentify { target_uid, account },
    Kill { target_uid, killer, reason },
}
```

### Database (`src/db/`)
- SQLite (SQLx) - Accounts, bans, channel registration
- Redb - Message history

## Directory Structure

```
src/
├── main.rs           # Entry point
├── config/           # TOML config loading
├── db/               # Database repositories
├── handlers/         # 141 command handlers
├── history/          # CHATHISTORY storage
├── network/          # TCP/TLS gateway
├── security/         # Auth, rate limiting
├── services/         # NickServ, ChanServ
├── state/            # Matrix, managers, actors
└── sync/             # S2S federation
```

## Key Patterns

### Zero-Copy Parsing
`MessageRef<'a>` borrows from buffer. Clone before `.await`:
```rust
let nick = msg.arg(0).map(|s| s.to_string());
async_op().await;  // Safe
```

### Lock Ordering
DashMap → Channel RwLock → User RwLock

### IRC Case Rules
Use `slirc_proto::irc_to_lower()`, not `to_lowercase()`.
