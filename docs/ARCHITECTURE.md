# slircd-ng Architecture

> Generated from source code audit on 2026-02-10. This document reflects the actual codebase, not aspirational design.

## Overview

slircd-ng is a high-performance IRC daemon written in Rust (edition 2024). It uses Tokio for async I/O, zero-copy message parsing, per-channel actor isolation, and a CRDT-based distributed state layer for server linking.

**Version**: 1.0.0-rc.1  
**Binary name**: `slircd`  
**Codebase**: ~63K lines (286 files) + ~26K lines protocol library (108 files) + ~6.7K lines tests (31 files)

---

## Startup Sequence (`src/main.rs`, 402 lines)

1. Resolve config path from CLI args (`-c`/`--config` or bare path, default `config.toml`)
2. Load and validate TOML configuration (supports `include` directives with glob patterns)
3. Initialize TLS provider (aws-lc-rs via rustls)
4. Set up tracing (JSON or pretty format, controlled by `log_format` config)
5. **Security gate**: refuse to start with weak `cloak_secret` (unless `SLIRCD_ALLOW_INSECURE_CLOAK` env var)
6. Initialize SQLite database, run migrations
7. Load persistent state: registered channels, shuns, K/G/D/Z-lines
8. Initialize history provider (Redb or NoOp) and AlwaysOn store (shares Redb database)
9. Construct `Matrix` (central state container) with all managers
10. Spawn background tasks (signal handler, persistence, enforcement, cleanup)
11. Spawn router task for S2S message routing (UID prefix → SID → peer lookup)
12. Spawn disconnect worker (bounded mpsc channel, 1024 slots)
13. Optionally start Prometheus metrics HTTP server (port 0 disables)
14. Restore always-on bouncer clients from persistent storage
15. Create handler `Registry` (command dispatch table)
16. Bind `Gateway` (TCP listeners: plaintext + optional TLS + optional WebSocket)
17. Start outgoing S2S connections (autoconnect link blocks)
18. Start inbound S2S listener (TLS and/or plaintext)
19. Start S2S heartbeat (PING every 30s, timeout at 90s)
20. Run gateway accept loop until shutdown

---

## Dependency Graph

```
┌─────────────────────────────────────────────────────────────────────────┐
│                            main.rs                                       │
│  Config ──→ Database ──→ Matrix ──→ Gateway ──→ Connection Event Loop    │
└──────┬──────────────────────┬──────────────┬───────────────────┬────────┘
       │                      │              │                   │
   ┌───▼───────┐        ┌────▼──────┐  ┌────▼────┐        ┌────▼────────┐
   │  Config   │        │  Matrix   │  │ Gateway │        │  Registry   │
   │   TOML    │        │  (State)  │  │  (Net)  │        │ (Handlers)  │
   │ + include │        │ Arc<Self> │  │ TCP+TLS │        │ PreReg/Post │
   └───────────┘        └────┬──────┘  └────┬────┘        └────┬────────┘
                              │              │                   │
       ┌──────────────────────┼──────────────┼───────────────────┘
       │                      │              │
       ▼                      ▼              ▼
  ┌─────────┐  ┌───────────────────────┐  ┌──────────────┐
  │Database │  │      Managers         │  │  Connection  │
  │ SQLite  │  │ User · Channel · Sec  │  │  Per-conn    │
  │ + Redb  │  │ Client · Service ·Mon │  │  Tokio task  │
  └─────────┘  │ Lifecycle · Stats     │  │  Event loop  │
               │ Sync · ReadMarker     │  └──────────────┘
               └───────────────────────┘
```

---

## Central State: The Matrix (`src/state/matrix.rs`, 605 lines)

The `Matrix` struct is the dependency injection container. Passed as `Arc<Matrix>` to all handlers and background tasks.

### Fields

| Field | Type | Purpose |
|-------|------|---------|
| `user_manager` | `UserManager` | Users, nicks, WHOWAS, UID generation, session senders |
| `channel_manager` | `ChannelManager` | Channel actors (mpsc senders), registered channel set |
| `client_manager` | `ClientManager` | Bouncer/multiclient state per account |
| `security_manager` | `SecurityManager` | Rate limiting, spam, ban cache, IP deny list |
| `service_manager` | `ServiceManager` | NickServ, ChanServ, Playback, history provider |
| `monitor_manager` | `MonitorManager` | IRCv3 MONITOR presence tracking |
| `lifecycle_manager` | `LifecycleManager` | Shutdown signals, background task spawning |
| `sync_manager` | `SyncManager` | S2S linking, topology, CRDT propagation |
| `stats_manager` | `Arc<StatsManager>` | Atomic runtime counters |
| `read_marker_manager` | `ReadMarkerManager` | IRCv3 read-marker state |
| `server_info` | `ServerInfo` | Name, network, SID, MOTD, idle timeouts |
| `server_id` | `ServerId` | 3-char TS6 server ID |
| `config` | `MatrixConfig` | Frozen config (server, oper, security, limits, etc.) |
| `hot_config` | `RwLock<HotConfig>` | REHASH-safe config (description, MOTD, oper blocks, admin) |
| `router_tx` | `mpsc::Sender<Arc<Message>>` | S2S message routing |
| `db` | `Database` | SQLite connection pool |

### Key Methods

- `disconnect_user(uid, reason)` — Canonical disconnect: WHOWAS → monitors → QUIT broadcast → channel leave → cleanup
- `disconnect_user_session(uid, reason, session_id)` — Session-aware: handles bouncer detach vs full disconnect
- `register_session_sender(uid, session_id, sender, caps)` — Register message routing for a connection
- `clock()` — Generate `HybridTimestamp` for CRDT operations
- `request_disconnect(uid, reason)` — Non-blocking via `try_send` (safe from channel actors)

### Lock Ordering (Deadlock Prevention)

```
DashMap shard lock → Channel RwLock → User RwLock
```
**Never reverse. Never hold across `.await`.**

Safe patterns:
- Read-only iteration: iterate DashMap, acquire read locks inside
- Collect-then-mutate: collect UIDs to Vec, release DashMap, then mutate
- Lock-copy-release: acquire lock, copy data, release before next operation

---

## State Managers (`src/state/managers/`)

### UserManager (`user.rs`)
- **Data**: `DashMap<Uid, Arc<RwLock<User>>>` (users), `DashMap<String, Vec<Uid>>` (nicks→UIDs), `DashMap<Uid, Vec<SessionSender>>` (senders), `DashMap<SessionId, HashSet<String>>` (session caps)
- **UID Generation**: TS6 format — 3-char SID + 6-char base36 (`UidGenerator`)
- **CRDT Merge**: Remote user introduction with nick collision resolution (older wins, tie kills both)
- **Session-Aware Delivery**: `send_to_user_sessions()` fans out to all sessions with capability filtering
- **Observer Pattern**: Notifies `SyncManager` of user state changes for S2S propagation

### ChannelManager (`channel.rs`)
- **Data**: `DashMap<String, mpsc::Sender<ChannelEvent>>` (actors), `DashSet<String>` (registered)
- **Lazy Creation**: `get_or_create()` spawns actor with default +nt modes
- **Persistence**: `persist_channel_from_db()` loads from SQLite, `trigger_persistence_all()` dirty-bit writeback
- **Observer Pattern**: Notifies `SyncManager` of channel changes

### ClientManager (`client.rs`)
- **Data**: `DashMap<String, Arc<RwLock<Client>>>` (by account), `DashMap<SessionId, String>` (session→account)
- **Attach/Detach**: `attach_session()` → `AttachResult` (Attached/Reattached/MulticlientNotAllowed/TooManySessions)
- **Always-On**: Persistent clients survive all sessions disconnecting, restored on restart
- **Cleanup**: `cleanup_stale_clients()` removes expired always-on clients

### SecurityManager (`security.rs`)
- `rate_limiter: RateLimitManager` — Governor-based token bucket (message, connection, join rates)
- `heuristics: HeuristicsEngine` — Multi-layer spam detection
- `shuns: DashMap<String, Shun>` — Active shuns by mask
- `ban_cache: BanCache` — In-memory K/G-line cache for connection-time checks
- `ip_deny: IpDenyList` — Roaring Bitmap engine for D/Z-line nanosecond IP rejection

### ServiceManager (`service.rs`)
- Holds `NickServ`, `ChanServ` singletons and history provider
- Extra services: `Playback` (ZNC-compatible replay)
- Creates pseudoclient `User` structs (mode +S, deterministic UIDs from SID)

### MonitorManager (`monitor.rs`)
- Bidirectional: UID→monitored nicks, nick→monitoring UIDs
- Used for MONITOR +/- and online/offline notifications

### StatsManager (`stats.rs`)
- All-atomic counters (Relaxed ordering)
- Tracks: local/remote users, invisible, opers, channels, servers, max users
- Used by LUSERS, STATS, Prometheus metrics

### ReadMarkerManager (`read_marker.rs`)
- `DashMap<(account, target), timestamp>` — max-forward semantics
- Used by IRCv3 `read-marker` capability

### LifecycleManager (`lifecycle.rs`)
- Shutdown broadcast channel (`tokio::sync::broadcast`)
- Disconnect request channel (bounded mpsc, 1024 slots)
- Spawns 7+ background tasks (see Startup Sequence)

---

## Channel Actor System (`src/state/actor/`)

Each channel runs as an **isolated Tokio task** processing `ChannelEvent` messages sequentially through `mpsc::Receiver`. This eliminates lock contention on the message routing hot path.

### Actor State
- `members: im::HashMap<Uid, MemberModes>` — Persistent immutable map (snapshot safe)
- `nick_cache: HashMap<Uid, String>` — Fast nick lookups
- `sender_cache: HashMap<Uid, mpsc::Sender<Arc<Message>>>` — Direct message routing
- `user_caps: HashMap<Uid, HashSet<String>>` — Per-member IRCv3 capabilities
- Channel modes, mode timestamps, topic, topic timestamp
- Ban/except/invex/quiet lists (all `Vec<(String, String, i64)>`)
- `delayed_joins: HashSet<Uid>` — +D mode tracking
- `invites: VecDeque<InviteEntry>` — Capped at 100, 1hr TTL
- Per-channel flood protection (message and join limiters)
- Persistence dirty bit

### ChannelEvent (23 variants)
Join, Part, Quit, SessionQuit, Message, Broadcast, BroadcastWithCaps, CapSync, GetInfo, CrdtMerge, GetBanList, GetMembers, GetMemberModes, GetModes, ModeChange, Kick, TopicChange, Invite, Knock, NickChange, ClearChannel, ServerOp, NetsplitRemove, MetadataOp, MultiSessionAttach, PersistState

### Broadcasting
Iterates `sender_cache`, sends `Arc<Message>` to each member's channel. Capability-filtered broadcasts check `user_caps` to choose primary message, fallback, or skip. Sender excluded via UID match.

### Lifecycle
Self-destruct when last member leaves (unless +P permanent). On destruction: metrics decremented, persistent state cleaned, entry removed from `ChannelManager.channels`.

---

## Handler System (`src/handlers/`, 141 files, 17 directories)

### Typestate Dispatch (compile-time protocol state enforcement)

| Trait | Phase | Stored In |
|-------|-------|-----------|
| `PreRegHandler` | Before NICK+USER complete | `Registry.pre_reg_handlers` |
| `PostRegHandler` | After registration | `Registry.post_reg_handlers` |
| `UniversalHandler<S>` | Any state | `Registry.universal_handlers` (via DynUniversalHandler) |
| `ServerHandler` | S2S protocol | `Registry.server_handlers` |

### Handler Directories

| Directory | Files | Commands Handled |
|-----------|-------|-----------------|
| `admin/` | 1 | SAJOIN, SAPART, SANICK, SAMODE |
| `bans/` | 5+ | KLINE, UNKLINE, DLINE, UNDLINE, GLINE, UNGLINE, ZLINE, UNZLINE, RLINE, UNRLINE, SHUN, UNSHUN |
| `batch/` | 5 | BATCH (client + server) |
| `cap/` | 9 | CAP (LS/LIST/REQ/END), AUTHENTICATE (PLAIN/EXTERNAL/SCRAM-SHA-256) |
| `channel/` | 13 | JOIN, PART, TOPIC, KICK, INVITE, KNOCK, CYCLE, LIST, NAMES |
| `chathistory/` | 5 | CHATHISTORY (LATEST/BEFORE/AFTER/BETWEEN/AROUND/TARGETS) |
| `connection/` | 9 | NICK, USER, PASS, PING, PONG, QUIT, STARTTLS, WEBIRC |
| `core/` | 5 | (infrastructure: traits, context, registry, middleware) |
| `messaging/` | 13 | PRIVMSG, NOTICE, TAGMSG, ACCEPT, RELAYMSG, METADATA |
| `mode/` | 6 | MODE (user + channel, includes MLOCK enforcement) |
| `oper/` | 15 | OPER, KILL, WALLOPS, GLOBOPS, DIE, REHASH, RESTART, CHGHOST, CHGIDENT, VHOST, TRACE, SPAMCONF, CLEARCHAN, CONNECT, SQUIT |
| `s2s/` | 5 | CONNECT, LINKS, MAP, SQUIT, KLN/UNKLN (server) |
| `server/` | 14 | SERVER, SID, UID, SJOIN, TMODE, TB, ENCAP, KICK, KILL, PRIVMSG/NOTICE routing, TOPIC |
| `server_query/` | 13 | ADMIN, VERSION, TIME, INFO, LUSERS, STATS, MOTD, RULES, HELP, USERIP, SERVICE, SERVLIST, SQUERY, SUMMON, USERS |
| `services/` | 3 | REGISTER, NS/NICKSERV, CS/CHANSERV |
| `user/` | 13 | MONITOR, AWAY, SETNAME, SILENCE, WHO (with WHOX), WHOIS, WHOWAS, ISON, USERHOST |
| `util/` | 3 | (helpers: prefixes, labeled responses, fanout) |

---

## Protocol Library (`crates/slirc-proto/`, v1.3.0)

### Zero-Copy Parsing
- `MessageRef<'_>` borrows from transport buffer
- Arguments via `SmallVec<[&str; 15]>` (stack-allocated for ≤15 params)
- `CommandRef<'_>` — zero-copy command variant
- Parser handles IRCv3 tags, prefix, command, parameters per RFC 2812

### Command Enum (~100 variants)
Organized: Connection, Channel, Messaging, Queries, S2S, Operator, Bans, IRCv3, Services, Standard Replies, Numeric fallback, Raw fallback.

### Case Folding
`irc_to_lower()` — RFC 2812 compliant (`[]{}|~` mapped). **Never use `.to_lowercase()`.**

### CRDT Layer (`sync/`)
- `ServerId` — 3-char SID
- `HybridTimestamp` — Total ordering across cluster
- `LWWRegister<T>` — Last-Writer-Wins register
- `AWSet<T>` — Add-Wins Set (tombstone-free)
- `ChannelCrdt` — Composite: LWW for scalar fields, AWSet for collections
- `UserCrdt` — Composite: LWW for scalar fields, AWSet for collections
- Traits: `Crdt` (merge), `DeltaCrdt` (incremental), `ConflictResolver` (timestamp)

### Feature Flags
| Feature | Purpose |
|---------|---------|
| `tokio` (default) | Async networking, TLS, WebSocket codec |
| `sync` | CRDT types, distributed state |
| `scram` | SCRAM-SHA-256 SASL |
| `serde` | Serialization |
| `proptest` | Property-based testing |

---

## Network Layer (`src/network/`)

- **Gateway** (`gateway.rs`): TCP accept loop, binds plaintext + optional TLS + optional WebSocket listeners. Supports HAProxy PROXY protocol.
- **Connection** (`connection/`): Per-connection Tokio task. Handshake → welcome burst → event loop. Idle timeout with PING/PONG keepalive.

---

## Services (`src/services/`)

### Effect Pattern
Services return `Vec<ServiceEffect>` — never mutate state directly:

| Effect | Purpose |
|--------|---------|
| `Reply` | Send NOTICE to user |
| `AccountIdentify` | Set account + mode +r |
| `AccountLogout` | Clear account + mode -r |
| `CancelEnforcement` | Stop nick enforcement |
| `Kill` | Disconnect user (GHOST, AKICK) |
| `Kick` | Kick from channel |
| `ChannelMode` | Set channel modes |
| `EnforceNick` | Start nick enforcement timer |
| `Wallops` | Oper broadcast |

### NickServ Commands
REGISTER, IDENTIFY, DROP, GROUP, UNGROUP, GHOST, INFO, SET, CERT, SESSIONS, HELP

### ChanServ Commands
REGISTER, ACCESS (LIST/ADD/DEL), INFO, SET, DROP, OP, DEOP, VOICE, DEVOICE, AKICK, CLEAR, HELP

### Playback Service
ZNC-compatible: `*playback PLAY`, `LIST`, `CLEAR`

---

## Security (`src/security/`)

| Module | Tech | Purpose |
|--------|------|---------|
| `ip_deny/` | Roaring Bitmap | Nanosecond D/Z-line IP rejection |
| `ban_cache.rs` | DashMap | In-memory K/G-line connection-time checks |
| `cloaking.rs` | HMAC-SHA256 | IP/hostname privacy (configurable suffix) |
| `rate_limit.rs` | Governor | Token bucket flood protection (msg/conn/join) |
| `spam.rs` | Heuristics | Content analysis engine |
| `heuristics.rs` | Pattern engine | Configurable spam rules |
| `reputation.rs` | Scoring | User reputation tracking |
| `rbl.rs` | HTTP/DNS | Real-time Blackhole List lookups |
| `password.rs` | Argon2 | Password hashing and verification |
| `xlines.rs` | Patterns | Extended bans ($a:account, $r:realname, etc.) |

---

## S2S Server Linking (`src/sync/`)

### Protocol
TS6-like with CRDT extensions.

### Handshake
PASS → CAPAB → SERVER → SVINFO

CAPAB tokens: QS, ENCAP, EX, IE, UNKLN, KLN, GLN, HOPS, CHW, KNOCK, SERVICES, etc.

### State Burst Order
1. Global bans (G-lines, Shuns, Z-lines)
2. Users (UID) — local only, split-horizon
3. Channels (SJOIN) — members with status prefixes, modes
4. Topics (TB)
5. Topology (SID) — known servers with incremented hopcount

### Netsplit Handling
Link drop → compute affected SIDs via topology → mass-QUIT affected users → cleanup maps/channels

### Key Files
| File | Purpose |
|------|---------|
| `manager.rs` | `SyncManager` — peer connections, topology, heartbeat |
| `handshake.rs` | TS6 handshake state machine |
| `burst.rs` | State burst generation |
| `link.rs` | Per-peer connection state |
| `network.rs` | Spanning tree topology |
| `split.rs` | Netsplit detection and mass-quit |
| `observer.rs` | CRDT propagation (observes user/channel changes) |
| `tls.rs` | S2S TLS configuration |

---

## Capability Token System (`src/caps/`)

Unforgeable capability tokens ("Innovation 4"):

- `Cap<T>` — Non-Clone, non-Copy. Only `CapabilityAuthority` can mint via `pub(super)`.
- **Channel caps**: KickCap, TopicCap, InviteCap, OpCap, VoiceCap, BanCap, ChannelModeCap
- **Oper caps**: KillCap, WallopsCap, GlobopsCap, RehashCap, DieCap, RestartCap, KlineCap, DlineCap, GlineCap, ZlineCap, RlineCap, ShunCap, SajoinCap, SapartCap, SanickCap, SamodeCap
- **Special**: BypassFloodCap, BypassModeCap, GlobalNoticeCap

---

## Database (`src/db/`)

### SQLite (sqlx, async)
10 migrations: accounts, nicknames, K/D/G/Z-lines, shuns, channels, access lists, AKICK, cert fingerprints, topics, reputation, SCRAM verifiers, metadata.

Connection pool with: 5s acquire timeout, 60s idle timeout, WAL mode for concurrency.

### Redb
Message history storage + always-on client persistence. Shared database instance between providers.

---

## Configuration (`src/config/`)

TOML with `include` directive (glob patterns). Hot-reloadable fields (via REHASH): description, MOTD, oper blocks, admin info.

| Section | Purpose |
|---------|---------|
| `[server]` | Identity (name, network, sid), metrics, idle timeouts |
| `[listen]` | Plaintext TCP bind address |
| `[tls]` | TLS listener (cert/key paths) |
| `[websocket]` | WebSocket listener |
| `[database]` | SQLite path (`:memory:` for testing) |
| `[security]` | Cloak secret/suffix, spam toggle |
| `[security.rate_limits]` | Flood protection thresholds, exempt IPs |
| `[multiclient]` | Bouncer config (enabled, always-on, max sessions) |
| `[motd]` | Message of the Day (inline or file) |
| `[history]` | Message history (backend, path, retention) |
| `[account_registration]` | SASL/REGISTER settings |
| `[[oper]]` | Operator blocks (name, password, hostmask) |
| `[[link]]` | S2S peering (name, address, password, autoconnect) |
| `[s2s_tls]` / `[s2s]` | S2S listener config |

---

## IRCv3 Capabilities Advertised (27)

| Capability | Status |
|-----------|--------|
| multi-prefix | ✅ |
| userhost-in-names | ✅ |
| extended-join | ✅ |
| account-notify | ✅ |
| sasl (PLAIN,EXTERNAL,SCRAM-SHA-256) | ✅ |
| batch | ✅ |
| labeled-response | ✅ |
| echo-message | ✅ |
| setname | ✅ |
| server-time | ✅ |
| message-tags | ✅ |
| cap-notify | ✅ |
| invite-notify | ✅ |
| chghost | ✅ |
| extended-monitor (MONITOR) | ✅ |
| away-notify | ✅ |
| account-tag | ✅ |
| msgid | ✅ |
| draft/multiline | ✅ (40KB, 100 lines) |
| draft/chathistory | ✅ |
| draft/event-playback | ✅ |
| draft/read-marker | ✅ |
| draft/relaymsg | ✅ |
| draft/account-registration | ✅ |
| tls (STARTTLS) | ✅ (plaintext only) |
| sts (Strict Transport Security) | ✅ (dynamic) |
| standard-replies | ✅ |

---

## Metrics

Prometheus-compatible via `metrics` + `metrics-exporter-prometheus`. HTTP endpoint on configurable port. Counters for users, channels, messages, bytes, connections.

---

## Testing

26 integration test files, 91+ tests, all passing as of 2026-02-10.

| Test File | Tests | Focus |
|-----------|-------|-------|
| channel_flow | 1 | Basic channel join/part/message |
| channel_ops | 5 | Channel operator actions |
| channel_queries | 5 | WHO, NAMES, LIST |
| chathistory | 1 | CHATHISTORY command |
| chrono_check | 1 | Time handling |
| compliance | 3 | RFC compliance |
| connection_lifecycle | 4 | Connect/disconnect/registration |
| distributed_channel_sync | 5 | CRDT channel sync |
| integration_bouncer | 3 | Multiclient/bouncer |
| integration_partition | 4 | Netsplit handling |
| integration_rehash | 3 | Hot config reload |
| ircv3_features | 3 | IRCv3 cap negotiation |
| ircv3_gaps | 5 | IRCv3 edge cases |
| operator_commands | 4 | OPER, KILL, WALLOPS |
| operator_moderation | 10 | Ban commands, moderation |
| s2s_two_server | 4 | Two-server linking |
| sasl_buffer_overflow | 1 | SASL security |
| sasl_external | 2 | SASL EXTERNAL (TLS cert) |
| security_channel_freeze | 2 | Channel freeze attack |
| security_channel_key | 1 | Channel key enforcement |
| security_flood_dos | 4 | Flood protection |
| security_slow_handshake | 1 | Slow handshake timeout |
| server_queries | 8 | LUSERS, STATS, VERSION, etc. |
| services_chanserv | 1 | ChanServ operations |
| stress_sasl | 2 | SASL under load |
| unified_read_state | 1 | Read markers |
| user_commands | 8 | NICK, AWAY, WHOIS, etc. |
