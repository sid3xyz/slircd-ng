# S2S Implementation Status

> **Last verified:** 2025-12-24 (via grep/read_file code audit)
> **Verification method:** Direct code inspection, NOT documentation

This document tracks the **actual implementation state** of server-to-server (S2S) functionality.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│ SLIRC S2S Architecture (TS6 Wire Format + CRDT Semantics)           │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│   ┌──────────────┐      ◄═══════════▶     ┌──────────────┐         │
│   │   Node A     │   Bidirectional Link   │   Node B     │         │
│   │ (initiator)  │                        │ (responder)  │         │
│   └──────────────┘                        └──────────────┘         │
│         │                                        │                  │
│         ▼                                        ▼                  │
│   ┌──────────────┐                        ┌──────────────┐         │
│   │ SyncManager  │ ────── BURST/UID ───▶  │ SyncManager  │         │
│   │  (burst.rs)  │ ◄──── SJOIN/TMODE ──── │              │         │
│   └──────────────┘                        └──────────────┘         │
│         │                                        │                  │
│         ▼                                        ▼                  │
│   ┌──────────────┐                        ┌──────────────┐         │
│   │  Observer    │─── Split-Horizon ─────▶│  Observer    │         │
│   │ (observer.rs)│   (never echo back)    │              │         │
│   └──────────────┘                        └──────────────┘         │
│                                                                      │
│   Limitation: Node B cannot INITIATE to Node A                      │
│   (no inbound S2S listener - only outbound connections work)        │
└─────────────────────────────────────────────────────────────────────┘
```

## ✅ COMPLETE Components

### 1. Outbound Connection & Handshake
| Component | File | Function |
|-----------|------|----------|
| Connection initiation | `sync/mod.rs` | `connect_to_peer()` |
| Handshake state machine | `sync/handshake.rs` | `HandshakeMachine` |
| TLS upgrade | `sync/tls.rs` | `upgrade_to_tls()` |
| Stream abstraction | `sync/stream.rs` | `S2SStream` enum |

**Flow:** TCP connect → optional TLS → PASS/SERVER exchange → burst → message loop

### 2. Burst Generation & Sending
| Component | File | Function |
|-----------|------|----------|
| Burst generation | `sync/burst.rs` | `generate_burst()` |
| Inline burst send | `sync/mod.rs:423-441` | In handshake loop |

**Generates:**
- G-lines, Shuns, Z-lines (global bans)
- UID commands for all users
- SJOIN commands for all channels (modes, members, prefixes)

**Note:** Burst IS sent inline after handshake completes (not via stub method).

### 3. TS6 Command Handlers (Server-to-Server)
| Handler | File | Purpose | Status |
|---------|------|---------|--------|
| `UidHandler` | `handlers/server/uid.rs` | User introduction | ✅ With TS collision detection |
| `SJoinHandler` | `handlers/server/sjoin.rs` | Channel state sync | ✅ Routes to ChannelActor |
| `TModeHandler` | `handlers/server/tmode.rs` | Timestamped modes | ✅ LWW semantics |
| `SidHandler` | `handlers/server/sid.rs` | Server introduction | ✅ Topology update |
| `RoutedMessageHandler` | `handlers/server/routing.rs` | PRIVMSG/NOTICE routing | ✅ Local delivery + forwarding |
| `ServerPropagationHandler` | `handlers/server.rs` | SERVER command relay | ✅ Split-horizon |
| `BurstHandler` | `handlers/server.rs` | BURST marker | ✅ Registered |
| `DeltaHandler` | `handlers/server/delta.rs` | CRDT delta sync | ✅ Registered |
| `ServerBatchHandler` | `handlers/batch/server.rs` | BATCH for S2S | ✅ With broadcast |

**Registry Location:** `handlers/core/registry.rs:105-117`

### 4. State Observer Pattern (Real-time Propagation)
| Trait Method | Triggers On | Propagates |
|--------------|-------------|------------|
| `on_user_update` | NICK change, mode change, OPER | UID command |
| `on_user_quit` | User QUIT, KILL | QUIT command |
| `on_channel_update` | JOIN, PART, mode change | SJOIN command |
| `on_channel_destroy` | Empty channel cleanup | (no propagation needed) |
| `on_ban_add` | GLINE, ZLINE, RLINE, SHUN | Ban command |
| `on_ban_remove` | Un-GLINE, Un-SHUN, etc. | Unset command |
| `on_account_change` | LOGIN, LOGOUT | ACCOUNT command |

**Implementation:** `sync/observer.rs` (367 lines)
**Trait:** `state/observer.rs` (`StateObserver` trait)

**Wired up via:**
- `state/matrix.rs:171` → `user_manager.set_observer(sync_manager_arc.clone())`
- `state/matrix.rs:175` → `channel_manager.set_observer(sync_manager_arc.clone())`

**Callers:**
- `handlers/connection/nick.rs:229` → `notify_observer()` on NICK change
- `handlers/oper/auth.rs:245` → `notify_observer()` on OPER
- `handlers/user_status.rs:67,130` → `notify_observer()` on AWAY/mode
- `handlers/mode/user.rs:80` → `notify_observer()` on user mode change
- `state/managers/user.rs:221` → `notify_observer()` on user CRDT merge
- `state/managers/user.rs:237` → `on_user_quit()` on kill_user()
- `state/actor/crdt.rs:394` → `on_channel_update()` on channel state change
- `handlers/bans/shun.rs:58,135` → `on_ban_add/remove()`
- `handlers/bans/xlines/mod.rs:168,278` → `on_ban_add/remove()`

### 5. Netsplit Handling
| Component | File | Function |
|-----------|------|----------|
| Netsplit handler | `sync/split.rs` | `handle_netsplit()` |
| Downstream calculation | `sync/topology.rs` | `get_downstream_sids()` |
| User cleanup | `sync/split.rs` | `remove_user_from_channels()` |
| Local notification | `sync/split.rs` | `broadcast_to_local_users()` |

**On link drop:**
1. Calculate affected servers (downstream of dead link)
2. Identify users by UID prefix matching affected SIDs
3. Remove users from channels via `ChannelEvent::NetsplitQuit`
4. Broadcast QUIT to local users
5. Clean up topology graph
6. Remove dead link

### 6. Topology Graph
| Component | File | Purpose |
|-----------|------|---------|
| Server tracking | `sync/topology.rs` | `TopologyGraph` |
| Route calculation | `sync/topology.rs` | `get_route()` |
| Downstream query | `sync/topology.rs` | `get_downstream_sids()` |

### 7. Message Routing
| Component | File | Function |
|-----------|------|----------|
| Route to remote | `sync/mod.rs` | `route_to_remote_user()` |
| Get next hop | `sync/mod.rs` | `get_next_hop()` |
| Broadcast to peers | `sync/mod.rs` | `broadcast()` |

### 8. Heartbeat & Health
| Component | File | Timing |
|-----------|------|--------|
| PING/PONG | `sync/mod.rs:start_heartbeat()` | 30s PING, 90s timeout |
| Auto-reconnect | `sync/mod.rs` | 5s delay after disconnect |

### 9. Testing
| Test | File | Coverage |
|------|------|----------|
| Handshake flow | `sync/tests.rs` | Full handshake sequence |
| Peer registration | `sync/tests.rs` | Link management |
| Split-horizon | `sync/tests.rs` | Observer doesn't echo back |

## ❌ MISSING Components

### HIGH PRIORITY

#### 1. Inbound S2S Listener
**Problem:** No listener accepts incoming server connections.
- `Gateway` only handles client connections
- `HandshakeState::InboundReceived` exists but is never triggered
- Servers can only link if they initiate outbound

**Required:**
- Add `[s2s_listen]` config block
- Create dedicated S2S listener (port 6900 convention)
- Route to `HandshakeState::InboundReceived` path

#### 2. SQUIT Command
**Problem:** No SQUIT command handler exists.
- Operators cannot manually disconnect servers
- No S2S SQUIT propagation

**Required:**
- `handlers/operator/squit.rs` - Operator command
- `handlers/server/squit.rs` - S2S handler
- Trigger `handle_netsplit()` on receipt

### MEDIUM PRIORITY

#### 3. KILL S2S Handler
**Problem:** Local KILL sends message but no server handler receives it.
- `UidHandler` sends KILL on collision (lines 122-128, 134-140)
- No `KillHandler` registered for incoming KILL

**Required:**
- `handlers/server/kill.rs` - Process incoming KILL
- Remove user from state, broadcast QUIT locally

## Configuration

```toml
[[links]]
name = "peer.example.net"
hostname = "10.0.0.2"
port = 6670
password = "linkpassword"
sid = "00B"
tls = true
verify_cert = true
autoconnect = true  # If false, wait for inbound (BROKEN - no listener!)
```

## Verification Commands

```bash
# Check all S2S handlers registered
rg "server_handlers.insert" slircd-ng/src/handlers/core/registry.rs

# Check observer callers
rg "notify_observer|on_user_|on_channel_|on_ban_" slircd-ng/src/

# Check burst generation
rg "generate_burst" slircd-ng/src/

# Check netsplit handling
rg "handle_netsplit|NetsplitQuit" slircd-ng/src/

# Run S2S tests
cargo test -p slircd-ng sync::
```

## Manual Testing

```bash
# Terminal 1: Start node1 (initiator)
cargo run -p slircd-ng -- config.node1.toml

# Terminal 2: Start node2 (node1 connects to it)
cargo run -p slircd-ng -- config.node2.toml

# Verify in logs:
# - "TLS handshake completed for S2S link"
# - "Handshake complete (Outbound)"
# - "Broadcasting user update to peers"
# - "Sent to peer"
```

---

*This document is authoritative. Update after every S2S change.*
*Last audit: 2025-12-24 - Found observer pattern fully wired, propagation complete.*
