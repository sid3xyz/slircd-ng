# slircd-ng S2S Protocol

> Generated from source code audit on 2026-02-10. Documents actual S2S implementation in `src/sync/` and `src/handlers/server/`.

## Overview

slircd-ng uses a **TS6-like server-to-server protocol** with CRDT extensions for distributed state synchronization. The implementation is in `src/sync/` (manager, handshake, burst, link, topology, netsplit) and `src/handlers/server/` (message handlers).

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        SyncManager                               │
│                                                                   │
│  ┌──────────┐  ┌───────────────┐  ┌────────────┐  ┌──────────┐ │
│  │  Peers   │  │   Topology    │  │  Heartbeat │  │ Observer │ │
│  │ DashMap  │  │ Spanning Tree │  │  PING/PONG │  │  CRDT    │ │
│  │ Per-Link │  │ SID → hops   │  │  30s / 90s │  │ Propagate│ │
│  └──────────┘  └───────────────┘  └────────────┘  └──────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

---

## Configuration

```toml
# S2S link block
[[link]]
name = "hub.example.net"
address = "192.168.1.1:6668"
send_password = "linkpass123"
receive_password = "linkpass123"
autoconnect = true
tls = false

# S2S TLS listener (optional)
[s2s_tls]
address = "0.0.0.0:6698"
cert_path = "certs/server.crt"
key_path = "certs/server.key"

# Plaintext S2S listener (optional, not recommended for production)
[s2s]
address = "0.0.0.0:6668"
```

---

## Handshake Protocol (`src/sync/handshake.rs`)

### State Machine

```
WaitingForPass → WaitingForCapab → WaitingForServer → WaitingForSvinfo → Complete
```

### Sequence (both sides simultaneously)

```
Server A                          Server B
────────                          ────────
PASS <password> TS 6 <SID>  →
                             ←    PASS <password> TS 6 <SID>
CAPAB :<capabilities>       →
                             ←    CAPAB :<capabilities>
SERVER <name> <hop> :<desc>  →
                             ←    SERVER <name> <hop> :<desc>
SVINFO 6 6 0 :<timestamp>   →
                             ←    SVINFO 6 6 0 :<timestamp>
<burst>                      →
                             ←    <burst>
```

### PASS Format

```
PASS <password> TS 6 <SID>
```
- `TS 6` — TS6 protocol version
- `<SID>` — 3-character Server ID

### CAPAB

Capabilities exchanged during handshake:

| Token | Description |
|-------|-------------|
| `QS` | QuitStorm (efficient netsplit handling) |
| `ENCAP` | Encapsulated commands for extensions |
| `EX` | Ban exceptions (+e mode) |
| `IE` | Invite exceptions (+I mode) |
| `KLN` | K-line propagation |
| `UNKLN` | K-line removal propagation |
| `GLN` | G-line propagation |
| `HOPS` | Hop count in topology |
| `CHW` | Channel half-ops/owner support |
| `KNOCK` | Channel knock support |
| `SERVICES` | Services integration |

### Verification

- Remote server name must match a configured `[[link]]` block
- Password is validated against `receive_password`
- SID must be unique on the network

---

## State Burst (`src/sync/burst.rs`)

After handshake completes, each server sends its full state to the peer. Burst order is critical for consistency:

### 1. Global Bans (first)
```
:<SID> ENCAP * GLINE <mask> <duration> :<reason>
:<SID> ENCAP * SHUN <mask> <duration> :<reason>
:<SID> ENCAP * ZLINE <mask> <duration> :<reason>
```
Sent first to prevent race conditions where a banned user could join channels during burst.

### 2. Users (UID)
```
:<SID> UID <nick> <hopcount> <timestamp> <modes> <username> <hostname> <IP> <UID> :<realname>
```
- Only local users are sent (split-horizon: skip users from target server's SID)
- CRDT merge on receiving end handles nick collisions

### 3. Channels (SJOIN)
```
:<SID> SJOIN <timestamp> <channel> <modes> [<modeargs>] :<members>
```
- Members prefixed with status chars: `@` (op), `+` (voice), etc.
- Channel modes and arguments included

### 4. Topics (TB)
```
:<SID> TB <channel> <timestamp> <setter> :<topic>
```

### 5. Topology (SID)
```
:<SID> SID <name> <hopcount+1> <SID> :<description>
```
Known servers propagated with incremented hop count.

---

## Operational Messages

### User Introduction
```
:<SID> UID <nick> <hop> <ts> <modes> <user> <host> <ip> <uid> :<realname>
```
Received via `ServerHandler` in `src/handlers/server/uid.rs`. CRDT merge with nick collision resolution: older timestamp wins, ties kill both users.

### Channel Mode Change
```
:<SID> TMODE <timestamp> <channel> <modes> [<args>...]
```
Timestamped mode changes for conflict resolution.

### Message Routing
```
:<prefix> PRIVMSG <target> :<text>
:<prefix> NOTICE <target> :<text>
```
Routed via `src/handlers/server/routing.rs`. Target can be a channel (broadcast locally) or a UID (forward to correct server via SID prefix routing).

### ENCAP (Encapsulated Commands)
```
:<SID> ENCAP <target> <command> [<args>...]
```
Used for: CHGHOST, REALHOST, CERTFP, METADATA, and ban propagation.

### Kick/Kill Propagation
```
:<prefix> KICK <channel> <target> :<reason>
:<prefix> KILL <target> :<reason>
```

---

## Message Routing

### UID-Based Routing
1. Extract target UID from message
2. First 3 characters of UID = target SID
3. Look up next-hop peer for target SID in topology
4. Forward message to that peer's send channel

### Router Task (in `main.rs`)
A dedicated Tokio task processes `router_tx` messages:
- Checks `x-target-uid` tag for explicit routing
- Falls back to command target parsing
- Looks up SID prefix → peer mapping
- Forwards via peer's `tx` channel

---

## Netsplit Handling (`src/sync/split.rs`)

When a link drops:

1. **Detect**: Connection error or heartbeat timeout (90s)
2. **Compute scope**: `topology.downstream_sids(lost_sid)` → all affected SIDs
3. **Mass QUIT**: Collect all users whose UID prefix matches affected SIDs
4. **Build QUIT messages**: Reason format: `<local_server> <remote_server>`
5. **Remove users**: Via `user_manager.remove_user()` (handles stats, metrics)
6. **Cleanup topology**: Remove affected SID entries

---

## Heartbeat

- **PING interval**: Every 30 seconds
- **Timeout**: 90 seconds without PONG
- Runs as background task via `sync_manager.start_heartbeat()`
- Uses shutdown signal for graceful termination

---

## Topology (`src/sync/network.rs`)

Spanning tree representation:
- Maps `ServerId` → `ServerEntry` (name, description, hop count, upstream SID)
- Supports: `add_server()`, `remove_server()`, `downstream_sids()`, `route_to()`
- Used for message routing and netsplit scope calculation

---

## Observer Pattern (`src/sync/observer.rs`)

`UserManager` and `ChannelManager` notify the `SyncManager` of state changes:
- User registration/nick change → broadcast UID/NICK to peers
- Channel create/mode change → broadcast SJOIN/TMODE to peers
- Enables CRDT propagation without tight coupling

---

## S2S Flood Protection

Rate limiting applied to S2S connections using the same Governor-based system as client connections, with separate configuration.
