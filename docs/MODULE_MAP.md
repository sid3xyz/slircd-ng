# slircd-ng Module Map

> Generated from source code audit on 2026-02-10. Complete map of every source module.

## Top-Level Modules (`src/`)

| File | Lines | Purpose |
|------|-------|---------|
| `main.rs` | 402 | Entry point, startup sequence, background task spawning |
| `error.rs` | — | Error types |
| `http.rs` | — | Prometheus metrics HTTP server (axum) |
| `metrics.rs` | — | Prometheus counter/gauge definitions |
| `telemetry.rs` | — | Tracing/logging setup |

---

## `src/config/` — Configuration

| File | Purpose |
|------|---------|
| `mod.rs` | Re-exports all config types |
| `types.rs` | `Config`, `ServerConfig`, `DatabaseConfig`, `MotdConfig`, `AccountRegistrationConfig`, `LogFormat`, `IdleTimeoutsConfig`, `Casemapping` |
| `listen.rs` | `ListenConfig`, `TlsConfig`, `WebSocketConfig`, `S2STlsConfig`, `StsConfig`, `ClientAuth` |
| `security.rs` | `SecurityConfig`, `RateLimitConfig`, `HeuristicsConfig`, `RblConfig` |
| `history.rs` | `HistoryConfig` |
| `limits.rs` | `LimitsConfig` (WHO/LIST/NAMES output caps) |
| `oper.rs` | `OperBlock`, `WebircBlock` |
| `links.rs` | `LinkBlock` (S2S peering) |
| `multiclient.rs` | `MulticlientConfig`, `AlwaysOnPolicy` |
| `validation.rs` | `validate()` — config validation rules |

---

## `src/state/` — Server State

| File | Purpose |
|------|---------|
| `mod.rs` | Re-exports all state types |
| `matrix.rs` | `Matrix`, `MatrixConfig`, `HotConfig`, `ServerInfo`, `MatrixParams` — central state container |
| `user.rs` | `User`, `UserModes`, `UserParams`, `WhowasEntry` — user data model |
| `channel.rs` | `Topic`, `MemberModes`, `ListEntry` — channel data model |
| `client.rs` | `SessionId`, `ChannelMembership` — bouncer/multiclient types |
| `session.rs` | `SessionState`, `UnregisteredState`, `RegisteredState`, `ServerState`, `SaslAccess`, `BatchRouting`, `ReattachInfo`, `InitiatorData` — typestate protocol types |
| `uid.rs` | `Uid` (type alias), `UidGenerator` — TS6 UID generation |
| `observer.rs` | State change observer trait for S2S |
| `persistence.rs` | Channel persistence logic |
| `dashmap_ext.rs` | DashMap extension traits |

### `src/state/managers/`

| File | Purpose |
|------|---------|
| `mod.rs` | Re-exports all managers |
| `user.rs` | `UserManager` — users, nicks, WHOWAS, UID gen, session senders |
| `channel.rs` | `ChannelManager` — channel actors, registered channels |
| `client.rs` | `ClientManager` — bouncer state per account, always-on |
| `security.rs` | `SecurityManager` — rate limiting, spam, bans, IP deny |
| `service.rs` | `ServiceManager` — NickServ, ChanServ, Playback, history |
| `monitor.rs` | `MonitorManager` — IRCv3 MONITOR state |
| `lifecycle.rs` | `LifecycleManager` — shutdown, background tasks |
| `stats.rs` | `StatsManager` — atomic runtime counters |
| `read_marker.rs` | `ReadMarkerManager` — IRCv3 read-marker |

### `src/state/actor/`

| File | Purpose |
|------|---------|
| (mod.rs) | Channel actor event loop |
| (types.rs) | `ChannelEvent` (23 variants), `ChannelActorState` |

---

## `src/handlers/` — Command Handlers (141 files)

### `handlers/core/` — Infrastructure

| File | Purpose |
|------|---------|
| `mod.rs` | Re-exports |
| `traits.rs` | `PreRegHandler`, `PostRegHandler`, `UniversalHandler<S>`, `ServerHandler`, `DynUniversalHandler` |
| `registry.rs` | `Registry` — command dispatch table |
| `context.rs` | `Context`, `HandlerError`, `HandlerResult` |
| `middleware.rs` | `ResponseMiddleware` — message routing/capturing with BATCH/label support |

### `handlers/connection/` — Registration & Keepalive

| File | Commands | Trait |
|------|----------|-------|
| `nick.rs` | NICK | UniversalHandler |
| `user.rs` | USER | PreRegHandler |
| `pass.rs` | PASS | PreRegHandler |
| `ping.rs` | PING, PONG | UniversalHandler |
| `quit.rs` | QUIT | UniversalHandler |
| `starttls.rs` | STARTTLS | PreRegHandler |
| `webirc.rs` | WEBIRC | PreRegHandler |
| `welcome_burst.rs` | — | Welcome 001-005 + MOTD |

### `handlers/cap/` — IRCv3 Capabilities

| File | Commands | Trait |
|------|----------|-------|
| `mod.rs` | CAP | UniversalHandler |
| `subcommands.rs` | CAP LS/LIST/REQ/END | — |
| `helpers.rs` | — | Cap list building |
| `types.rs` | — | SaslState, supported caps list |
| `sasl/mod.rs` | AUTHENTICATE | UniversalHandler |
| `sasl/plain.rs` | SASL PLAIN | — |
| `sasl/scram.rs` | SASL SCRAM-SHA-256 | — |
| `sasl/external.rs` | SASL EXTERNAL | — |
| `sasl/common.rs` | — | Shared SASL helpers |

### `handlers/channel/` — Channel Operations

| File | Commands | Trait |
|------|----------|-------|
| `join/mod.rs` | JOIN | PostRegHandler |
| `join/creation.rs` | — | Channel creation |
| `join/enforcement.rs` | — | +k/+i/+l/+b checks |
| `join/responses.rs` | — | JOIN reply builder |
| `part.rs` | PART | PostRegHandler |
| `topic.rs` | TOPIC | PostRegHandler |
| `kick.rs` | KICK | PostRegHandler |
| `invite.rs` | INVITE | PostRegHandler |
| `knock.rs` | KNOCK | PostRegHandler |
| `cycle.rs` | CYCLE | PostRegHandler |
| `list.rs` | LIST | PostRegHandler |
| `names.rs` | NAMES | PostRegHandler |
| `ops.rs` | — | force_join/force_part |
| `common.rs` | — | Parsing utilities |

### `handlers/messaging/` — Message Routing

| File | Commands | Trait |
|------|----------|-------|
| `privmsg.rs` | PRIVMSG | PostRegHandler |
| `notice.rs` | NOTICE | PostRegHandler |
| `tagmsg.rs` | TAGMSG | PostRegHandler |
| `accept.rs` | ACCEPT | PostRegHandler |
| `relaymsg.rs` | RELAYMSG | PostRegHandler |
| `metadata.rs` | METADATA | PostRegHandler |
| `routing.rs` | — | Core message routing |
| `delivery.rs` | — | Cap-filtered delivery |
| `validation.rs` | — | Shun/spam validation |
| `multiclient.rs` | — | Bouncer echo |
| `errors.rs` | — | Error constants |
| `types.rs` | — | Type definitions |

### `handlers/mode/` — Mode Handling

| File | Commands |
|------|----------|
| `mod.rs` | MODE (dispatch) |
| `user.rs` | User mode changes |
| `channel/mod.rs` | Channel mode changes |
| `channel/lists.rs` | Ban/except/invex/quiet list queries |
| `channel/mlock.rs` | MLOCK enforcement |
| `common.rs` | Mode parsing utilities |

### `handlers/oper/` — Operator Commands

| File | Commands |
|------|----------|
| `auth.rs` | OPER |
| `kill.rs` | KILL |
| `wallops.rs` | WALLOPS |
| `globops.rs` | GLOBOPS |
| `lifecycle.rs` | DIE, REHASH, RESTART |
| `chghost.rs` | CHGHOST |
| `chgident.rs` | CHGIDENT |
| `vhost.rs` | VHOST |
| `trace.rs` | TRACE |
| `spamconf.rs` | SPAMCONF |
| `clearchan.rs` | CLEARCHAN |
| `connect.rs` | CONNECT |
| `squit.rs` | SQUIT |

### `handlers/bans/` — Ban Management

| File | Commands |
|------|----------|
| `shun.rs` | SHUN, UNSHUN |
| `xlines/mod.rs` | KLINE, UNKLINE, DLINE, UNDLINE, GLINE, UNGLINE, ZLINE, UNZLINE, RLINE, UNRLINE |
| `common.rs` | Shared ban utilities |

### `handlers/chathistory/` — Message History

| File | Commands |
|------|----------|
| `mod.rs` | CHATHISTORY |
| `batch.rs` | History batch sending |
| `helpers.rs` | Constants, timestamp parsing |
| `queries.rs` | History query execution |
| `slicing.rs` | Time/msgid slicing |

### `handlers/server/` — S2S Protocol

| File | Commands |
|------|----------|
| `base.rs` | SERVER (handshake + propagation) |
| `capab.rs` | CAPAB |
| `svinfo.rs` | SVINFO |
| `sid.rs` | SID |
| `uid.rs` | UID |
| `sjoin.rs` | SJOIN |
| `tmode.rs` | TMODE |
| `topic.rs` | TOPIC (server) |
| `tb.rs` | TB (Topic Burst) |
| `kick.rs` | KICK (server) |
| `kill.rs` | KILL (server) |
| `encap.rs` | ENCAP |
| `routing.rs` | PRIVMSG/NOTICE (server) |
| `source.rs` | Source SID extraction |

### `handlers/server_query/` — Server Information

| File | Commands |
|------|----------|
| `admin.rs` | ADMIN |
| `version.rs` | VERSION |
| `time.rs` | TIME |
| `info.rs` | INFO |
| `lusers.rs` | LUSERS |
| `stats.rs` | STATS |
| `motd.rs` | MOTD |
| `rules.rs` | RULES |
| `help.rs` | HELP |
| `userip.rs` | USERIP |
| `service.rs` | SERVICE, SERVLIST, SQUERY |
| `disabled.rs` | SUMMON, USERS |

### `handlers/admin/` — Admin Commands

| File | Commands |
|------|----------|
| `admin.rs` | SAJOIN, SAPART, SANICK, SAMODE |

### `handlers/batch/` — Batch Processing

| File | Commands |
|------|----------|
| `mod.rs` | BATCH (client) |
| `server.rs` | BATCH (server) |
| `processing.rs` | Batch accumulation |
| `validation.rs` | Batch validation |
| `types.rs` | BatchState types |

### `handlers/s2s/` — S2S Client Commands

| File | Commands |
|------|----------|
| `connect.rs` | CONNECT |
| `links.rs` | LINKS |
| `map.rs` | MAP |
| `squit.rs` | SQUIT |
| `kline.rs` | KLN, UNKLN (server) |

### `handlers/services/` — Service Shortcuts

| File | Commands |
|------|----------|
| `account.rs` | REGISTER |
| `aliases.rs` | NS, NICKSERV, CS, CHANSERV |

### `handlers/user/` — User Commands

| File | Commands |
|------|----------|
| `monitor.rs` | MONITOR |
| `status.rs` | AWAY, SETNAME, SILENCE |
| `query/who/mod.rs` | WHO |
| `query/who/legacy.rs` | RFC 2812 WHO |
| `query/who/v3.rs` | WHOX |
| `query/who/search.rs` | Search/filtering |
| `query/who/common.rs` | WHOX field parsing |
| `query/whois/whois_cmd.rs` | WHOIS |
| `query/whois/whowas.rs` | WHOWAS |
| `query/whois/ison.rs` | ISON |
| `query/whois/userhost.rs` | USERHOST |

### `handlers/util/` — Helpers

| File | Purpose |
|------|---------|
| `helpers.rs` | user_prefix, server_notice, labeled_ack, matches_hostmask, etc. |
| `helpers/fanout.rs` | Multi-session message fanout |

---

## `src/network/` — TCP/TLS/WebSocket

| File | Purpose |
|------|---------|
| `mod.rs` | Re-exports |
| `gateway.rs` | TCP accept loop, TLS, WebSocket listeners |
| `connection/` | Per-connection Tokio task, handshake, event loop |
| `proxy_protocol.rs` | HAProxy PROXY protocol support |

---

## `src/security/` — Security

| File | Purpose |
|------|---------|
| `mod.rs` | Re-exports, `matches_ban_or_except()` |
| `cloaking.rs` | HMAC-SHA256 IP/hostname cloaking |
| `rate_limit.rs` | Governor token bucket flood protection |
| `ban_cache.rs` | In-memory K/G-line cache |
| `ip_deny/` | Roaring Bitmap IP deny (D/Z-lines) |
| `spam.rs` | Content analysis engine |
| `heuristics.rs` | Configurable spam detection rules |
| `reputation.rs` | User reputation scoring |
| `rbl.rs` | Real-time Blackhole List lookups |
| `password.rs` | Argon2 password hashing |
| `xlines.rs` | Extended bans ($a:/$r:/$j:/$x:/$z) |

---

## `src/services/` — IRC Services

| File | Purpose |
|------|---------|
| `mod.rs` | `route_service_message()` — dispatch to NickServ/ChanServ |
| `base.rs` | `ServiceBase` trait — common service helpers |
| `traits.rs` | `Service` trait definition |
| `effect.rs` | `ServiceEffect` enum, `apply_effect()`/`apply_effects()` |
| `enforce.rs` | Nick enforcement logic |
| `playback.rs` | ZNC-compatible playback service |
| `nickserv/` | NickServ implementation (REGISTER, IDENTIFY, DROP, GROUP, UNGROUP, GHOST, INFO, SET, CERT, SESSIONS) |
| `chanserv/` | ChanServ implementation (REGISTER, ACCESS, INFO, SET, DROP, OP, DEOP, VOICE, DEVOICE, AKICK, CLEAR) |

---

## `src/sync/` — Server Linking

| File | Purpose |
|------|---------|
| `mod.rs` | Re-exports `SyncManager` |
| `manager.rs` | `SyncManager` — peer management, topology, routing |
| `handshake.rs` | TS6 handshake state machine |
| `burst.rs` | State burst generation (bans → users → channels → topics → topology) |
| `link.rs` | Per-peer connection state |
| `network.rs` | Spanning tree topology |
| `split.rs` | Netsplit detection, mass-quit |
| `observer.rs` | CRDT state change propagation |
| `stream.rs` | S2S stream I/O |
| `tls.rs` | S2S TLS configuration |
| `topology.rs` | Topology data structures |
| `tests.rs` | Sync module tests |

---

## `src/history/` — Message History

| File | Purpose |
|------|---------|
| `mod.rs` | `HistoryProvider` trait, `HistoryQuery`, `HistoryError` |
| `types.rs` | `StoredMessage`, `MessageEnvelope`, `HistoryItem` |
| `redb.rs` | Redb-backed persistent history |
| `noop.rs` | NoOp provider (discards everything) |

---

## `src/caps/` — Capability Tokens

| File | Purpose |
|------|---------|
| `mod.rs` | Module docs, re-exports |
| `authority.rs` | `CapabilityAuthority` — token mint |
| `tokens.rs` | `Cap<T>`, `Capability` trait, all capability types |
| `irc.rs` | IRCv3 capability constants (SUPPORTED_CAPS) |

---

## `src/db/` — Database

| File | Purpose |
|------|---------|
| `mod.rs` | `Database`, connection pool, migration runner |
| `accounts.rs` | `AccountRepository` — NickServ accounts, nicknames |
| `bans/` | `BanRepository` — K/D/G/Z-lines, Shuns |
| `channels/` | `ChannelRepository` — registered channels, access lists, AKICK |
| `always_on.rs` | `AlwaysOnStore` — Redb bouncer persistence |

---

## `crates/slirc-proto/src/` — Protocol Library

| File/Module | Purpose |
|-------------|---------|
| `lib.rs` | Crate root, re-exports |
| `message.rs` | `Message` (owned), serialization |
| `message_ref.rs` | `MessageRef` (zero-copy borrowed) |
| `command.rs` | `Command` enum (~100 variants) |
| `command_ref.rs` | `CommandRef` (zero-copy) |
| `parser.rs` | nom-based zero-copy parser |
| `prefix.rs` | `Prefix` (Nickname/Server) |
| `tags.rs` | IRCv3 message tags |
| `numeric.rs` | IRC numeric reply codes |
| `casemap.rs` | `irc_to_lower()`, `irc_eq()` |
| `channel.rs` | `IsChannel` trait |
| `ctcp.rs` | CTCP parsing |
| `hostmask.rs` | `matches_hostmask()` |
| `sasl/` | SASL PLAIN, EXTERNAL, SCRAM-SHA-256 |
| `codec/` | Tokio codec, transport types |
| `mode.rs` | Mode parsing types |
| `sync/` | CRDT: clock, crdt trait, lww, awset, channel_crdt, user_crdt |
| `websocket.rs` | WebSocket handshake validation |
| `batch.rs` | Batch reference ID types |

---

## `tests/` — Integration Tests

| File | Tests | Focus |
|------|-------|-------|
| `channel_flow.rs` | 1 | Basic channel join/part/message flow |
| `channel_ops.rs` | 5 | Channel operator actions (op/deop/kick) |
| `channel_queries.rs` | 5 | WHO, NAMES, LIST queries |
| `chathistory.rs` | 1 | CHATHISTORY command with Redb |
| `chrono_check.rs` | 1 | Time handling correctness |
| `compliance.rs` | 3 | RFC 2812 compliance |
| `connection_lifecycle.rs` | 4 | Connect/disconnect/registration |
| `distributed_channel_sync.rs` | 5 | CRDT channel synchronization |
| `integration_bouncer.rs` | 3 | Multiclient/bouncer functionality |
| `integration_partition.rs` | 4 | Netsplit detection and recovery |
| `integration_rehash.rs` | 3 | Hot configuration reload |
| `ircv3_features.rs` | 3 | IRCv3 capability negotiation |
| `ircv3_gaps.rs` | 5 | IRCv3 edge cases |
| `operator_commands.rs` | 4 | OPER, KILL, WALLOPS |
| `operator_moderation.rs` | 10 | Ban commands, moderation tools |
| `s2s_two_server.rs` | 4 | Two-server linking and burst |
| `sasl_buffer_overflow.rs` | 1 | SASL buffer overflow protection |
| `sasl_external.rs` | 2 | SASL EXTERNAL (TLS cert) |
| `security_channel_freeze.rs` | 2 | Channel freeze attack mitigation |
| `security_channel_key.rs` | 1 | Channel key enforcement |
| `security_flood_dos.rs` | 4 | Flood/DoS protection |
| `security_slow_handshake.rs` | 1 | Slow handshake timeout |
| `server_queries.rs` | 8 | LUSERS, STATS, VERSION, etc. |
| `services_chanserv.rs` | 1 | ChanServ register/access |
| `stress_sasl.rs` | 2 | SASL under concurrent load |
| `unified_read_state.rs` | 1 | Read marker functionality |
| `user_commands.rs` | 8 | NICK, AWAY, WHOIS, MODE, etc. |
| `common/` | — | Shared test utilities (TestServer) |

---

## Database Migrations (`migrations/`)

| File | Tables Created |
|------|---------------|
| `001_init.sql` | accounts, nicknames, klines, dlines, channels, channel_access, channel_akick |
| `002_shuns.sql` | shuns |
| `003_xlines.sql` | glines, zlines |
| `004_history.sql` | (history schema) |
| `005_certfp.sql` | (certificate fingerprints) |
| `006_channel_topics.sql` | (channel topic persistence) |
| `007_reputation.sql` | (reputation scores) |
| `008_scram_verifiers.sql` | (SCRAM-SHA-256 verifiers) |
| `009_channels.sql` | (channel schema extensions) |
| `010_metadata.sql` | (user/channel metadata) |
