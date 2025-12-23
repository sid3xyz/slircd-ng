# S2S Implementation Status

> **Last verified:** 2025-01-XX (via code audit, not documentation)
> **Verification method:** `grep`, `read_file`, `git log`

This document tracks the **actual implementation state** of server-to-server (S2S) functionality.
It is updated by auditing code, not by wishful thinking.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│ SLIRC S2S Architecture                                               │
├─────────────────────────────────────────────────────────────────────┤
│                                                                       │
│   ┌──────────────┐     OUTBOUND ONLY      ┌──────────────┐          │
│   │   Node A     │ ────────────────────▶  │   Node B     │          │
│   │ (initiator)  │                         │ (acceptor)   │          │
│   └──────────────┘                         └──────────────┘          │
│                                                                       │
│   Current limitation: Node B cannot initiate to Node A              │
│   (no inbound listener exists)                                       │
└─────────────────────────────────────────────────────────────────────┘
```

## ✅ COMPLETE Components

### Outbound Connection Initiation
- **File:** `src/sync/mod.rs`
- **Entry:** `connect_to_peer()` spawns task for each `[[links]]` with `autoconnect = true`
- **Flow:** TCP connect → optional TLS upgrade → handshake → burst → message loop

### Handshake Protocol (Outbound)
- **File:** `src/sync/handshake.rs`
- **States:** `Unconnected → OutboundInitiated → Bursting → Synced`
- **Wire format:** TS6-like (`PASS <pw> TS=6 <sid>`, `SERVER <name> <hop> <sid> <desc>`)

### Burst Generation
- **File:** `src/sync/burst.rs`
- **Function:** `generate_burst(state, local_sid)`
- **Generates:**
  - Global bans (GLINE, SHUN, ZLINE)
  - UID commands for all users
  - SJOIN commands for all channels (with modes, members)
- **Note:** Burst is generated but `send_burst()` is a stub!

### TS6 Command Handlers
| Handler | File                       | Status                              |
| ------- | -------------------------- | ----------------------------------- |
| UID     | `handlers/server/uid.rs`   | ✅ With TS-based collision detection |
| SJOIN   | `handlers/server/sjoin.rs` | ✅ Forwards to ChannelActor          |
| TMODE   | `handlers/server/tmode.rs` | ✅ Timestamped mode changes          |
| SID     | `handlers/server/sid.rs`   | ✅ Server introduction               |
| BURST   | `handlers/server/mod.rs`   | ✅ Registered in registry            |
| DELTA   | `handlers/server/delta.rs` | ✅ CRDT delta sync                   |
| BATCH   | `handlers/server/mod.rs`   | ✅ Registered                        |

### Netsplit Handling
- **File:** `src/sync/split.rs`
- **Function:** `handle_netsplit(matrix, remote_sid, local_name, remote_name)`
- **Behavior:**
  1. Identifies all users from disconnected server (UID prefix match)
  2. Removes users from channels via `ChannelEvent::NetsplitQuit`
  3. Broadcasts QUIT to local users
  4. Removes users from `user_manager`
  5. Cleans up topology graph

### Topology Graph
- **File:** `src/sync/topology.rs`
- **Struct:** `TopologyGraph` with `servers: DashMap<ServerId, ServerInfo>`
- **Used for:** Routing decisions (next-hop calculation)

### Message Routing
- **File:** `src/sync/mod.rs`
- **Function:** `route_to_remote_user(target_uid, msg)`
- **Logic:** Extract SID from UID prefix → find route → send via link

### Heartbeat
- **File:** `src/sync/mod.rs`
- **Function:** `start_heartbeat()`
- **Timing:** PING every 30s, timeout after 90s

### TLS Support
- **Files:** `src/sync/tls.rs`, `src/sync/stream.rs`
- **Modes:** Plain TCP, TLS client (outbound), TLS server (inbound - unused)

## ❌ MISSING Components

### HIGH PRIORITY

#### 1. Inbound S2S Listener
**Problem:** No listener accepts incoming server connections.
- `Gateway` only handles client connections
- `HandshakeState::InboundReceived` exists but is never triggered
- `S2SStream::TlsServer` variant exists but marked `#[allow(dead_code)]`

**Required:**
- Add `[s2s_listen]` config block (address, port)
- Create `S2SListener` in `network/` or `sync/`
- Route incoming connections through handshake with `InboundReceived` state
- Distinguish server PASS/SERVER from client NICK/USER

#### 2. SQUIT Command Handler
**Problem:** Operators cannot disconnect servers.
- No grep results for "SQUIT" or "squit" in codebase
- Need both operator command and S2S propagation

**Required:**
- `handlers/operator/squit.rs` - Operator command
- `handlers/server/squit.rs` - S2S handler (receive from peers)
- Trigger netsplit cleanup on receipt

### MEDIUM PRIORITY

#### 3. KILL Propagation
**Problem:** No KILL forwarding between servers.
- No `KillHandler` for S2S
- `handlers/operator/kill.rs` exists for operator command (verify)

**Required:**
- Forward KILL to peers
- Handle incoming KILL from peers

#### 4. Nick Change Propagation
**Problem:** Local NICK changes not sent to peers.
- No evidence of nick change → peer notification

**Required:**
- After successful local NICK change, send `NICK <new> <ts>` to peers
- Handle incoming NICK from peers (update user_manager)

#### 5. QUIT Propagation
**Problem:** Local user QUITs not sent to peers.
- Netsplit sends QUITs locally but not to other servers

**Required:**
- On local user QUIT, send to all peers
- Remove user state network-wide

### LOW PRIORITY

#### 6. Topic Propagation
**Problem:** TOPIC changes not forwarded.

## ⚠️ STUBS (Code exists but doesn't work)

### `send_burst()` - Logs only
```rust
// src/sync/mod.rs:634
pub async fn send_burst(&self, sid: &ServerId, ...) {
    info!("Sending burst to {}", sid.as_str());
}
```
**Impact:** This stub is unused. Actual burst sending happens inline in the outbound
handshake flow (sync/mod.rs lines 423-441). Not a real problem.

## ✅ FIXED Components

### `broadcast()` - Now Functional (2025-12-24)
Previously an empty stub. Now implements split-horizon broadcast to all connected peers.
Used by: `handlers/server.rs` (SERVER propagation), `handlers/batch/server.rs` (BATCH relay).

## Configuration

Current `[[links]]` config structure:
```toml
[[links]]
name = "peer.example.net"
hostname = "10.0.0.2"
port = 6670
password = "linkpassword"
sid = "00B"
autoconnect = true  # false = wait for inbound (BROKEN - no inbound listener!)
```

## Testing

### Manual linking test
```bash
# Start node1
cargo run -p slircd-ng -- config.node1.toml

# Start node2 (node1 autoconnects to node2)
cargo run -p slircd-ng -- config.node2.toml

# Observe link establishment in logs
```

### Verify burst
1. Connect user to node1
2. Check node2 for UID
3. Join channel on node1
4. Check node2 for SJOIN

## Next Steps

1. **Implement inbound S2S listener** (required for bidirectional links)
2. **Wire up send_burst()** to actually send generated burst
3. **Implement broadcast()** for event propagation
4. **Add SQUIT handler**
5. **Add event propagation** (NICK, QUIT, KILL)

---

*This document is authoritative. If it conflicts with other docs, this wins.*
*Update this document after every S2S change.*
