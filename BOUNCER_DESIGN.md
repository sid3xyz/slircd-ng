# Bouncer Implementation Design: Better Than Ergo

**Created**: January 2026  
**Status**: Design Phase  
**Priority**: HIGH (Major differentiating feature)

---

## Executive Summary

This document outlines a bouncer implementation for slircd-ng that **exceeds Ergo's capabilities** by leveraging our unique architectural advantages:

| Feature | Ergo | slircd-ng (Planned) | Our Advantage |
|---------|------|---------------------|---------------|
| Multiclient | ✅ Same nick, multiple connections | ✅ Same | Parity |
| Always-On | ✅ Persist when disconnected | ✅ Same | Parity |
| History Playback | ✅ Per-device, MySQL | ✅ Per-device, **Redb (embedded)** | No MySQL required |
| Read Markers | 256 max, no sync | **CRDT-synced, unlimited** | Distributed sync |
| Federation | ❌ Single instance only | ✅ **Multi-server bouncer** | Killer feature |
| Encryption at Rest | ❌ | ✅ Optional AES-256-GCM | Security |
| Device Registration | Device ID only | **Registered devices with metadata** | Management |

---

## Architecture Decision: Session/Client Separation

### Current Architecture Problem

Currently, slircd-ng has a **1:1 relationship** between connections and users:

```
Connection → Session (RegisteredState) → User
     1              1                       1
```

For bouncer support, we need a **many-to-one** relationship:

```
Connection₁ ─┐                    
Connection₂ ─┼→ Client (Account) → Virtual Presence (User)
Connection₃ ─┘     1                      1
```

### Proposed Architecture

Based on Ergo's proven model, adapted for slircd-ng:

```rust
// NEW: Client represents an account's persistent state (can have 0+ sessions)
pub struct Client {
    /// Account name (primary identifier)
    pub account: String,
    /// Current nick (may == account in nick-equals-account mode)
    pub nick: String,
    /// Attached sessions (0 when always-on but disconnected)
    pub sessions: Vec<SessionId>,
    /// Always-on setting
    pub always_on: bool,
    /// User modes (persisted for always-on)
    pub modes: UserModes,
    /// Channels and per-channel modes
    pub channels: HashMap<String, ChannelMembership>,
    /// Per-device last-seen timestamps
    pub last_seen: HashMap<DeviceId, DateTime<Utc>>,
    /// CRDT-synced read markers (our advantage!)
    pub read_markers: ReadMarkersCrdt,
    /// Registered devices with metadata
    pub devices: HashMap<DeviceId, DeviceInfo>,
    /// Push notification subscriptions
    pub push_subscriptions: HashMap<String, PushSubscription>,
}

// EXISTING: Session represents a single TCP connection
pub struct Session {
    /// Connection identifier
    pub id: SessionId,
    /// Attached to which client (None during registration)
    pub client: Option<Arc<RwLock<Client>>>,
    /// Device identifier (from SASL or ident)
    pub device_id: Option<DeviceId>,
    /// Connection-specific capabilities
    pub capabilities: HashSet<String>,
    /// Connection-specific state (IP, hostname, TLS, etc.)
    pub connection: ConnectionState,
}

// MODIFIED: User becomes the "virtual presence" on the network
pub struct User {
    pub uid: String,
    /// Points to Client if this is a bouncer user
    pub client: Option<Arc<RwLock<Client>>>,
    // ... existing fields for nick, host, modes, etc.
}
```

---

## Phase 1: Multiclient Foundation (MVP)

### Goal
Allow multiple connections to share the same nickname when authenticated to the same account.

### Implementation Tasks

#### 1.1 Create Client Struct
**File**: `src/state/client.rs` (new)

```rust
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub type SessionId = u64;
pub type DeviceId = String;

#[derive(Debug)]
pub struct Client {
    pub account: String,
    pub nick: String,
    pub sessions: Vec<SessionId>,
    pub modes: UserModes,
    pub channels: HashMap<String, ChannelMembership>,
    pub always_on: bool,
    pub last_seen: HashMap<DeviceId, DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug)]
pub struct ChannelMembership {
    pub modes: String,  // e.g., "ov" for +o +v
    pub join_time: i64,
}
```

#### 1.2 Create ClientManager
**File**: `src/state/managers/client.rs` (new)

```rust
pub struct ClientManager {
    /// Clients by account name (casefolded)
    clients: DashMap<String, Arc<RwLock<Client>>>,
    /// Session ID to Client mapping
    session_to_client: DashMap<SessionId, Arc<RwLock<Client>>>,
}

impl ClientManager {
    /// Get or create a Client for an account
    pub fn get_or_create_client(&self, account: &str) -> Arc<RwLock<Client>>;
    
    /// Attach a session to a client
    pub fn attach_session(&self, client: &Arc<RwLock<Client>>, session_id: SessionId);
    
    /// Detach a session from a client
    pub fn detach_session(&self, session_id: SessionId) -> Option<Arc<RwLock<Client>>>;
    
    /// Check if client has any connected sessions
    pub fn is_connected(&self, account: &str) -> bool;
}
```

#### 1.3 Modify Session Attachment Flow

**When SASL succeeds (during registration)**:
1. Check if Client exists for this account
2. If exists and has connected sessions:
   - Check `multiclient.enabled` config
   - If allowed, attach new session to existing Client
   - If not allowed, return error
3. If exists but no sessions (always-on reattach):
   - Attach session, replay missed messages
4. If not exists:
   - Create new Client, attach session

**When connection disconnects**:
1. Detach session from Client
2. If Client has no more sessions:
   - If `always_on`, keep Client alive
   - Otherwise, destroy Client and User

#### 1.4 Configuration Options
**File**: `config.toml`

```toml
[accounts.multiclient]
# Enable multiple connections to same nickname
enabled = true

# Allow by default, or require opt-in via NickServ SET multiclient true
allowed-by-default = true

# Always-on setting: "disabled", "opt-in", "opt-out", "mandatory"
always-on = "opt-in"

# How long to keep always-on clients before expiring (0 = forever)
always-on-expiration = "30d"

# Auto-away: set away when all sessions disconnect
auto-away = "opt-out"
```

#### 1.5 NickServ SESSIONS Command
**File**: `src/services/nickserv/sessions.rs` (new)

```
/msg NickServ SESSIONS
Your sessions:
  1. Phone (192.168.1.10) - idle 5m
  2. Desktop (192.168.1.20) - idle 30s [current]
```

### Phase 1 Success Criteria
- [ ] Multiple clients can connect with same nick via SASL
- [ ] `SESSIONS` command shows all connected sessions
- [ ] Disconnecting one session doesn't affect others
- [ ] Config options control multiclient behavior

---

## Phase 2: Always-On Persistence

### Goal
Keep users present on the server even when all sessions disconnect.

### Implementation Tasks

#### 2.1 Redb Schema for Always-On State
**File**: `migrations/008_always_on.sql` → `src/db/always_on.rs`

```rust
// Redb table definitions
const ALWAYS_ON_CLIENTS: TableDefinition<&str, &[u8]> = TableDefinition::new("always_on_clients");
const DEVICE_STATE: TableDefinition<&str, &[u8]> = TableDefinition::new("device_state");
const READ_MARKERS: TableDefinition<&str, &[u8]> = TableDefinition::new("read_markers");

#[derive(Serialize, Deserialize)]
pub struct StoredClient {
    pub account: String,
    pub nick: String,
    pub modes: String,
    pub channels: Vec<StoredChannelMembership>,
    pub last_seen: HashMap<String, i64>,
    pub created_at: i64,
}
```

#### 2.2 Client State Writeback
**Strategy**: Dirty-bit writeback (like Ergo)

```rust
const (
    IncludeChannels = 1 << 0,
    IncludeUserModes = 1 << 1,
    IncludeRealname = 1 << 2,
    IncludeReadMarkers = 1 << 3,
)

impl Client {
    pub fn mark_dirty(&self, bits: u32) {
        self.dirty_bits.fetch_or(bits, Ordering::Relaxed);
        self.wake_writer();
    }
}
```

#### 2.3 Server Startup Restoration

```rust
async fn restore_always_on_clients(matrix: &Matrix, db: &Database) {
    for stored in db.list_always_on_clients().await? {
        let client = Client::from_stored(stored);
        
        // Create virtual User for this client
        let user = matrix.user_manager.create_virtual_user(&client);
        
        // Rejoin channels
        for (chname, membership) in &client.channels {
            matrix.channel_manager.join_virtual(&user, chname, membership);
        }
        
        // Set auto-away if enabled
        if client.auto_away {
            user.set_away("Auto-away: All sessions disconnected");
        }
    }
}
```

#### 2.4 Expiration Handling

```rust
async fn always_on_maintenance(matrix: &Matrix, config: &Config) {
    let expiration = config.accounts.multiclient.always_on_expiration;
    if expiration.is_zero() { return; }
    
    let cutoff = Utc::now() - expiration;
    
    for client in matrix.client_manager.always_on_clients() {
        let expired = client.last_seen.values().all(|ts| *ts < cutoff);
        if expired {
            matrix.destroy_client(&client, "Timed out due to inactivity").await;
        }
    }
}
```

### Phase 2 Success Criteria
- [ ] Disconnecting all sessions keeps user present
- [ ] Channels and modes persist across server restarts
- [ ] Auto-away sets when all sessions disconnect
- [ ] Expired always-on clients are cleaned up

---

## Phase 3: History Playback (Better Than Ergo)

### Goal
Provide intelligent history replay when sessions reconnect.

### Our Advantages Over Ergo
1. **Redb embedded** - No MySQL dependency
2. **CRDT read markers** - Sync across servers
3. **Per-device tracking** - Already in Phase 2

### Implementation Tasks

#### 3.1 Device ID Extraction
**From SASL username**: `alice@phone` → device_id = `phone`
**From ident**: `alice@phone` → device_id = `phone`
**From PASS**: `alice@phone:password` → device_id = `phone`

```rust
fn extract_device_id(sasl_username: &str) -> (String, Option<String>) {
    match sasl_username.split_once('@') {
        Some((account, device)) => (account.to_string(), Some(device.to_string())),
        None => (sasl_username.to_string(), None),
    }
}
```

#### 3.2 Per-Device Last-Seen Tracking

```rust
impl Client {
    pub fn update_last_seen(&mut self, device_id: &DeviceId) {
        self.last_seen.insert(device_id.clone(), Utc::now());
        self.mark_dirty(IncludeLastSeen);
    }
    
    pub fn get_missed_since(&self, device_id: &DeviceId) -> Option<DateTime<Utc>> {
        self.last_seen.get(device_id).copied()
    }
}
```

#### 3.3 Autoreplay on Reconnect

```rust
async fn perform_reattach(session: &Session, client: &Client) {
    // Play registration burst
    send_welcome(session).await;
    
    // Get missed-since time for this device
    let missed_since = client.get_missed_since(&session.device_id);
    
    for (chname, membership) in &client.channels {
        // Send JOIN for each channel
        send_join(session, chname).await;
        
        // Replay missed history
        if let Some(since) = missed_since {
            if !session.has_cap("chathistory") {
                replay_history(session, chname, since).await;
            }
        }
    }
    
    // Replay missed DMs
    if let Some(since) = missed_since {
        replay_dm_history(session, client, since).await;
    }
}
```

#### 3.4 ZNC Playback Compatibility

```rust
// Handle: PRIVMSG *playback :play * 1558374442
async fn handle_znc_playback(session: &Session, params: &[&str]) {
    let target = params.get(0).unwrap_or(&"*");
    let start = params.get(1).map(|s| parse_znc_time(s));
    let end = params.get(2).map(|s| parse_znc_time(s));
    
    if target == "*" {
        // Replay all channels and DMs
        for channel in session.client.channels.keys() {
            replay_history(session, channel, start, end).await;
        }
        replay_dm_history(session, start, end).await;
    } else {
        replay_history(session, target, start, end).await;
    }
}
```

#### 3.5 CRDT Read Markers (Unique Advantage!)

```rust
// slirc-crdt addition
pub struct ReadMarkersCrdt {
    /// Maps target (channel or nick) to read timestamp
    markers: LwwMap<String, DateTime<Utc>>,
}

// In Client
impl Client {
    pub fn set_read_marker(&mut self, target: &str, timestamp: DateTime<Utc>) {
        self.read_markers.set(target, timestamp, self.hlc.now());
        self.mark_dirty(IncludeReadMarkers);
    }
    
    pub fn get_read_marker(&self, target: &str) -> Option<DateTime<Utc>> {
        self.read_markers.get(target).copied()
    }
}
```

**MARKREAD command support (IRCv3)**:
```
MARKREAD #channel timestamp=2024-01-01T12:00:00.000Z
:server 802 nick #channel timestamp=2024-01-01T12:00:00.000Z
```

### Phase 3 Success Criteria
- [ ] Device ID extracted from SASL/ident
- [ ] Missed messages replayed on reconnect
- [ ] ZNC playback module works
- [ ] Read markers sync via CRDT

---

## Phase 4: Distributed Bouncer (Killer Feature)

### Goal
Enable bouncer functionality across federated servers - **impossible in Ergo**.

### Unique Capabilities

1. **Session Handoff**: Connect to server A, reconnect to server B, get same state
2. **Cross-Server History**: Query history from any server
3. **Federated Read Markers**: CRDT sync keeps markers consistent

### Implementation Tasks

#### 4.1 Client State as CRDT

```rust
// In slirc-crdt
pub struct ClientCrdt {
    pub account: String,
    pub nick: LwwRegister<String>,
    pub modes: UserModesCrdt,
    pub channels: LwwMap<String, ChannelMembershipCrdt>,
    pub read_markers: ReadMarkersCrdt,
    pub always_on: LwwRegister<bool>,
}
```

#### 4.2 Cross-Server Session Attachment

When a session authenticates to account "alice" on Server B:
1. Check if "alice" has a Client on Server B → attach
2. If not, query Server A (or federation) for Client state
3. Merge CRDT state, create local Client
4. Attach session to local Client

#### 4.3 BOUNCER Command (New IRC Command)

```
BOUNCER LISTDEVICES
:server 800 nick device_id ip last_seen
:server 801 nick :End of device list

BOUNCER MIGRATE <target-server>
:server ACK BOUNCER MIGRATE

BOUNCER DELDEVICE <device_id>
:server ACK BOUNCER DELDEVICE
```

### Phase 4 Success Criteria
- [ ] Client state propagates via CRDT
- [ ] Session can attach to account on different server
- [ ] BOUNCER command works for device management
- [ ] Read markers sync cross-server

---

## Phase 5: Enhanced Features

### 5.1 Push Notifications (webpush capability)

```rust
pub struct PushSubscription {
    pub endpoint: String,
    pub keys: WebPushKeys,
    pub last_refresh: DateTime<Utc>,
}

// When message received for always-on client with no sessions:
async fn maybe_send_push(client: &Client, message: &Message) {
    if client.sessions.is_empty() && !client.push_subscriptions.is_empty() {
        for sub in client.push_subscriptions.values() {
            send_webpush(sub, message).await;
        }
    }
}
```

### 5.2 Per-Device Certificate Auth

```rust
pub struct DeviceInfo {
    pub id: DeviceId,
    pub name: String,
    pub certfp: Option<String>,  // Bind TLS cert to this device
    pub created_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
}
```

### 5.3 Message Encryption at Rest

```rust
// Optional AES-256-GCM encryption for stored messages
pub struct EncryptedMessage {
    pub nonce: [u8; 12],
    pub ciphertext: Vec<u8>,
}

impl HistoryStore {
    pub fn store_message(&self, msg: &Message, key: Option<&EncryptionKey>) {
        let data = if let Some(key) = key {
            serialize(&encrypt(msg, key))
        } else {
            serialize(msg)
        };
        self.write(data);
    }
}
```

---

## Proto Requirements (slirc-proto Changes)

### New Commands Needed

```rust
// In slirc-proto/src/command.rs

/// BOUNCER command for device management
pub enum BouncerSubcommand {
    ListDevices,
    Device { id: String },
    DelDevice { id: String },
    Migrate { target: String },
}

/// MARKREAD command for read markers (IRCv3 draft)
pub struct MarkRead {
    pub target: String,
    pub timestamp: Option<String>,
}

/// WEBPUSH command for push notification registration
pub enum WebPushSubcommand {
    Register { endpoint: String, key: String, auth: String },
    Unregister { endpoint: String },
}
```

### New Numerics Needed

```rust
// In slirc-proto/src/numeric.rs
pub const RPL_BOUNCERDEVICE: u16 = 800;
pub const RPL_BOUNCERDEVICESEND: u16 = 801;
pub const RPL_MARKREAD: u16 = 802;
pub const RPL_MARKREADEND: u16 = 803;
pub const ERR_DEVICELIMIT: u16 = 900;
pub const ERR_UNKNOWNDEVICE: u16 = 901;
```

---

## Configuration Summary

```toml
[accounts.multiclient]
enabled = true
allowed-by-default = true
always-on = "opt-in"              # "disabled", "opt-in", "opt-out", "mandatory"
always-on-expiration = "30d"      # 0 = never expire
auto-away = "opt-out"             # Set away when all sessions disconnect
max-sessions-per-account = 10     # DoS protection

[history.playback]
autoreplay-on-join = 100          # Lines to replay on channel join
autoreplay-missed = true          # Replay missed messages on reconnect
znc-max = 10000                   # Max messages for ZNC playback

[push]
enabled = false
timeout = "30s"
delay = "5s"                      # Wait before pushing (check if read)
```

---

## Implementation Timeline

| Phase | Duration | Dependencies | Deliverables |
|-------|----------|--------------|--------------|
| **Phase 1** | 2 weeks | None | Multiclient MVP |
| **Phase 2** | 2 weeks | Phase 1 | Always-on persistence |
| **Phase 3** | 1 week | Phase 2 | History playback |
| **Phase 4** | 3 weeks | Phase 3, slirc-crdt | Distributed bouncer |
| **Phase 5** | 2 weeks | Phase 4 | Push, encryption, device mgmt |

**Total**: ~10 weeks for full implementation

---

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Breaking existing connections | Medium | High | Feature flag, gradual rollout |
| Performance with many sessions | Low | Medium | Benchmark, optimize hot paths |
| CRDT merge conflicts | Low | Medium | Extensive testing, fallback |
| Proto changes delayed | Medium | Medium | Define minimal proto needs early |

---

## Conclusion

This design leverages slircd-ng's unique advantages to create a bouncer implementation that is **objectively better** than Ergo's:

1. **No MySQL** - Embedded Redb makes deployment trivial
2. **CRDT sync** - Read markers and state sync across servers
3. **Federation** - Multi-server bouncer (impossible in Ergo)
4. **Encryption** - Optional at-rest encryption for privacy
5. **Device management** - Rich device registration and control

The phased approach allows delivering value quickly (Phase 1-2 in 4 weeks) while building toward the killer feature (federated bouncer in Phase 4).
