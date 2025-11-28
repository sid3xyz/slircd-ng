# slircd-ng Feature Parity TODO

> Master checklist for achieving feature completeness with slircd
> Generated: November 28, 2025

## Executive Summary

This document tracks all features present in `slircd` that need to be implemented in `slircd-ng` to achieve feature parity. The slircd reference implementation has **54 commands**, comprehensive services, IRCv3.2 support, TLS/WebSocket transports, and database persistence.

---

## 1. Commands Implementation Status

### 1.1 Connection/Registration Commands

| Command | slircd | slircd-ng | Notes |
|---------|--------|-----------|-------|
| NICK | ✅ | ✅ | Implemented |
| USER | ✅ | ✅ | Implemented |
| PASS | ✅ | ✅ | Implemented |
| PING | ✅ | ✅ | Implemented |
| PONG | ✅ | ✅ | Implemented |
| QUIT | ✅ | ✅ | Implemented |
| CAP | ✅ | ❌ | **Missing: IRCv3 capability negotiation** |
| AUTHENTICATE | ✅ | ❌ | **Missing: SASL authentication** |

### 1.2 Channel Commands

| Command | slircd | slircd-ng | Notes |
|---------|--------|-----------|-------|
| JOIN | ✅ | ✅ | Implemented |
| PART | ✅ | ✅ | Implemented |
| TOPIC | ✅ | ✅ | Implemented |
| NAMES | ✅ | ✅ | Implemented |
| LIST | ✅ | ✅ | Implemented |
| KICK | ✅ | ✅ | Implemented |
| MODE | ✅ | ✅ | Implemented (Type A lists, ABCD modes) |
| INVITE | ✅ | ✅ | Implemented |
| KNOCK | ✅ | ✅ | Implemented |

### 1.3 Messaging Commands

| Command | slircd | slircd-ng | Notes |
|---------|--------|-----------|-------|
| PRIVMSG | ✅ | ✅ | Implemented |
| NOTICE | ✅ | ✅ | Implemented |
| TAGMSG | ✅ | ❌ | **Missing: IRCv3 tags-only message** |

### 1.4 User Query Commands

| Command | slircd | slircd-ng | Notes |
|---------|--------|-----------|-------|
| WHO | ✅ | ✅ | Implemented |
| WHOIS | ✅ | ✅ | Implemented |
| WHOWAS | ✅ | ✅ | Implemented |
| USERHOST | ✅ | ✅ | Implemented |
| ISON | ✅ | ✅ | Implemented |
| USERIP | ✅ | ❌ | **Missing: Returns user's IP (oper only)** |
| MONITOR | ✅ | ❌ | **Missing: IRCv3 presence monitoring** |

### 1.5 Server Query Commands

| Command | slircd | slircd-ng | Notes |
|---------|--------|-----------|-------|
| VERSION | ✅ | ✅ | Implemented |
| TIME | ✅ | ✅ | Implemented |
| ADMIN | ✅ | ✅ | Implemented |
| INFO | ✅ | ✅ | Implemented |
| LUSERS | ✅ | ✅ | Implemented |
| MOTD | ✅ | ✅ | Implemented |
| STATS | ✅ | ✅ | Implemented |
| LINKS | ✅ | ❌ | **Missing: Server links info** |
| MAP | ✅ | ❌ | **Missing: Network map** |
| TRACE | ✅ | ❌ | **Missing: Route to server/user** |
| HELP / HELPOP | ✅ | ❌ | **Missing: Help system** |
| RULES | ✅ | ❌ | **Missing: Server rules display** |
| SUMMON | ✅ | ❌ | **Missing: Summon user (stub OK)** |
| USERS | ✅ | ❌ | **Missing: Users on host (stub OK)** |
| SERVLIST | ✅ | ❌ | **Missing: Services list (stub OK)** |

### 1.6 Operator Commands

| Command | slircd | slircd-ng | Notes |
|---------|--------|-----------|-------|
| OPER | ✅ | ✅ | Implemented |
| KILL | ✅ | ✅ | Implemented |
| REHASH | ✅ | ✅ | Implemented |
| DIE | ✅ | ✅ | Implemented |
| WALLOPS | ✅ | ✅ | Implemented |
| KLINE | ✅ | ✅ | Implemented |
| UNKLINE | ✅ | ✅ | Implemented |
| DLINE | ✅ | ✅ | Implemented |
| UNDLINE | ✅ | ✅ | Implemented |
| SHUN | ✅ | ❌ | **Missing: Shun (quiet ban)** |
| UNSHUN | ✅ | ❌ | **Missing: Remove shun** |
| RESTART | ✅ | ❌ | **Missing: Server restart** |
| CHGHOST | ✅ | ❌ | **Missing: Change user's host (oper)** |

### 1.7 Admin SA* Commands

| Command | slircd | slircd-ng | Notes |
|---------|--------|-----------|-------|
| SAJOIN | ✅ | ✅ | Implemented |
| SAPART | ✅ | ✅ | Implemented |
| SANICK | ✅ | ✅ | Implemented |
| SAMODE | ✅ | ✅ | Implemented |

### 1.8 Miscellaneous Commands

| Command | slircd | slircd-ng | Notes |
|---------|--------|-----------|-------|
| AWAY | ✅ | ✅ | Implemented |
| SETNAME | ✅ | ❌ | **Missing: Change realname (IRCv3)** |
| WEBIRC | ✅ | ❌ | **Missing: WebIRC gateway support** |

---

## 2. IRCv3 Capabilities

### 2.1 Required Capabilities

| Capability | slircd | slircd-ng | Priority | Notes |
|------------|--------|-----------|----------|-------|
| multi-prefix | ✅ | ❌ | P1 | Show all user prefixes in NAMES |
| userhost-in-names | ✅ | ❌ | P1 | Include nick!user@host in NAMES |
| echo-message | ✅ | ❌ | P1 | Echo PRIVMSG/NOTICE to sender |
| server-time | ✅ | ❌ | P1 | ISO 8601 time tag on messages |
| message-tags | ✅ | ❌ | P1 | Parse/forward client tags |
| labeled-response | ✅ | ❌ | P1 | Label tag for request correlation |
| batch | ✅ | ❌ | P2 | Multi-line response batching |
| setname | ✅ | ❌ | P2 | SETNAME command support |
| away-notify | ✅ | ❌ | P2 | Broadcast AWAY status to channels |
| account-notify | ✅ | ❌ | P2 | Account changes broadcast |
| extended-join | ✅ | ❌ | P2 | JOIN with account + realname |
| cap-notify | ✅ | ❌ | P2 | CAP NEW/DEL notifications |
| sasl | ✅ | ❌ | P1 | SASL authentication |
| account-tag | ✅ | ❌ | P2 | Account tag on messages |

### 2.2 CAP Handler Implementation

- [ ] CAP LS [302] - List capabilities with version negotiation
- [ ] CAP REQ - Request capabilities
- [ ] CAP ACK - Acknowledge requested capabilities
- [ ] CAP END - End capability negotiation
- [ ] CAP NEW / CAP DEL - Dynamic capability changes
- [ ] Multi-line CAP LS for many capabilities

---

## 3. Services (NickServ/ChanServ)

### 3.1 NickServ Commands

| Command | slircd | slircd-ng | Notes |
|---------|--------|-----------|-------|
| REGISTER | ✅ | ❌ | Register nickname with password/email |
| IDENTIFY | ✅ | ❌ | Authenticate to account |
| GHOST | ✅ | ❌ | Kill session using your nick |
| GROUP | ✅ | ❌ | Group nick to account |
| UNGROUP | ✅ | ❌ | Remove nick from account |
| INFO | ✅ | ❌ | Account information |
| SET | ✅ | ❌ | Account settings (EMAIL, ENFORCE, etc.) |
| VERIFY | ✅ | ❌ | Email verification |
| DROP | ✅ | ❌ | Drop nickname registration |
| RECOVER | ✅ | ❌ | Recover registered nick |

### 3.2 ChanServ Commands

| Command | slircd | slircd-ng | Notes |
|---------|--------|-----------|-------|
| REGISTER | ✅ | ❌ | Register channel |
| DROP | ✅ | ❌ | Drop channel registration |
| ACCESS ADD | ✅ | ❌ | Add user to access list |
| ACCESS DEL | ✅ | ❌ | Remove from access list |
| ACCESS LIST | ✅ | ❌ | List access entries |
| OP | ✅ | ❌ | Grant op status |
| DEOP | ✅ | ❌ | Remove op status |
| VOICE | ✅ | ❌ | Grant voice |
| DEVOICE | ✅ | ❌ | Remove voice |
| INFO | ✅ | ❌ | Channel information |
| SET | ✅ | ❌ | Channel settings (MLOCK, TOPICLOCK, etc.) |
| AKICK | ✅ | ❌ | Auto-kick list management |
| CLEAR | ✅ | ❌ | Clear modes/bans/ops |

### 3.3 Services Infrastructure

- [ ] Service message routing (PRIVMSG NickServ)
- [ ] Service aliases (NS, CS shortcuts)
- [ ] Account state in Matrix (identified users)
- [ ] Auto-op/voice on join for identified users
- [ ] Nick enforcement (timer + Guest rename)
- [ ] +r (registered) user mode integration

---

## 4. Database/Persistence

### 4.1 SQLite Integration

| Feature | slircd | slircd-ng | Notes |
|---------|--------|-----------|-------|
| SQLx async database | ✅ | ❌ | Async SQLite with sqlx |
| Accounts table | ✅ | ❌ | NickServ accounts |
| Nicknames table | ✅ | ❌ | Nick → account mapping |
| Channels table | ✅ | ❌ | ChanServ registrations |
| Access table | ✅ | ❌ | Channel access lists |
| KLines table | ✅ | ❌ | Persistent K-lines |
| DLines table | ✅ | ❌ | Persistent D-lines |
| Shuns table | ✅ | ❌ | Persistent shuns |
| Event store | ✅ | ❌ | Event sourcing (optional) |
| Snapshots | ✅ | ❌ | State snapshots for recovery |

### 4.2 Database Schema (Required)

```sql
-- Accounts (NickServ)
CREATE TABLE accounts (
    id INTEGER PRIMARY KEY,
    name TEXT UNIQUE NOT NULL COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    email TEXT,
    registered_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    enforce BOOLEAN DEFAULT FALSE,
    hide_email BOOLEAN DEFAULT TRUE
);

-- Nicknames
CREATE TABLE nicknames (
    name TEXT PRIMARY KEY COLLATE NOCASE,
    account_id INTEGER REFERENCES accounts(id) ON DELETE CASCADE
);

-- Channels (ChanServ)
CREATE TABLE channels (
    id INTEGER PRIMARY KEY,
    name TEXT UNIQUE NOT NULL COLLATE NOCASE,
    founder_account INTEGER REFERENCES accounts(id),
    registered_at INTEGER NOT NULL,
    last_used_at INTEGER NOT NULL,
    mlock TEXT,
    keeptopic BOOLEAN DEFAULT TRUE
);

-- Channel Access
CREATE TABLE channel_access (
    channel_id INTEGER REFERENCES channels(id) ON DELETE CASCADE,
    account_id INTEGER REFERENCES accounts(id) ON DELETE CASCADE,
    flags TEXT NOT NULL,
    PRIMARY KEY (channel_id, account_id)
);

-- K-Lines
CREATE TABLE klines (
    mask TEXT PRIMARY KEY,
    reason TEXT,
    set_by TEXT,
    set_at INTEGER,
    expires_at INTEGER
);
```

---

## 5. Transport/Network

### 5.1 TLS Support

| Feature | slircd | slircd-ng | Notes |
|---------|--------|-----------|-------|
| Implicit TLS (6697) | ✅ | ❌ | TLS from connection start |
| STARTTLS upgrade | ✅ | ❌ | Upgrade plaintext to TLS |
| Client cert auth | ✅ | ❌ | TLS fingerprint for SASL EXTERNAL |
| rustls integration | ✅ | ❌ | TLS without OpenSSL |

### 5.2 WebSocket Support

| Feature | slircd | slircd-ng | Notes |
|---------|--------|-----------|-------|
| IRC-over-WebSocket | ✅ | ❌ | ws:// protocol |
| WebSocket+TLS | ✅ | ❌ | wss:// protocol |
| WebIRC gateway | ✅ | ❌ | Pass real client IP |

### 5.3 Connection Handling

| Feature | slircd | slircd-ng | Notes |
|---------|--------|-----------|-------|
| DNS reverse lookup | ✅ | ❌ | Resolve hostnames |
| IP cloaking | ✅ | ❌ | HMAC-based host cloaking |
| Flood protection | ✅ | ❌ | Rate limiting per user |
| Per-command rate limits | ✅ | ❌ | WHO, LIST, etc. limits |
| Max connections per IP | ✅ | ❌ | Anti-abuse limit |
| Registration timeout | ✅ | ❌ | Kick unregistered clients |
| Ping timeout | ✅ | ❌ | Disconnect idle clients |

---

## 6. Configuration

### 6.1 Configuration Options

| Feature | slircd | slircd-ng | Notes |
|---------|--------|-----------|-------|
| Admin info block | ✅ | ❌ | ADMIN reply data |
| TLS cert/key paths | ✅ | ❌ | TLS configuration |
| WebSocket listeners | ✅ | ❌ | WS/WSS bind addresses |
| Oper hostmask check | ✅ | Partial | Has field, not enforced |
| Per-command limits | ✅ | ❌ | Rate limit config |
| Anti-spam config | ✅ | ❌ | Burst/sustained rates |
| WebIRC blocks | ✅ | ❌ | Gateway config |
| NickServ config | ✅ | ❌ | Service settings |
| ChanServ config | ✅ | ❌ | Service settings |
| MOTD file path | ✅ | ❌ | External MOTD file |
| Cloak secret | ✅ | ❌ | Host cloaking key |

---

## 7. Infrastructure/Quality

### 7.1 Monitoring

| Feature | slircd | slircd-ng | Notes |
|---------|--------|-----------|-------|
| Prometheus metrics | ✅ | ❌ | /metrics endpoint |
| Connection count | ✅ | ❌ | Gauge metric |
| Message throughput | ✅ | ❌ | Counter metric |
| Command latency | ✅ | ❌ | Histogram metric |

### 7.2 Logging

| Feature | slircd | slircd-ng | Notes |
|---------|--------|-----------|-------|
| Structured logging | ✅ | ✅ | tracing crate |
| Log levels | ✅ | ✅ | RUST_LOG env |
| Span context | ✅ | Partial | Per-connection tracing |

### 7.3 Error Handling

| Feature | slircd | slircd-ng | Notes |
|---------|--------|-----------|-------|
| ERR_UNKNOWNCOMMAND | ✅ | ❌ | Reply for unknown cmds |
| Graceful shutdown | ✅ | ❌ | Signal handling |
| Connection cleanup | ✅ | Partial | QUIT handling |

---

## 8. Implementation Priority

### Phase 1: Core Protocol Completeness (P0)
1. [ ] CAP handler (IRCv3 negotiation)
2. [ ] AUTHENTICATE (SASL PLAIN)
3. [ ] server-time capability
4. [ ] multi-prefix capability
5. [ ] userhost-in-names capability
6. [ ] echo-message capability
7. [ ] TAGMSG command
8. [ ] ERR_UNKNOWNCOMMAND for unknown commands

### Phase 2: Services Foundation (P1)
1. [ ] Database schema and SQLx integration
2. [ ] NickServ REGISTER/IDENTIFY
3. [ ] Account state in User struct
4. [ ] +r mode for identified users
5. [ ] Service message routing (PRIVMSG NickServ)
6. [ ] NS/CS command aliases

### Phase 3: Services Complete (P1)
1. [ ] NickServ: GROUP, UNGROUP, INFO, SET, DROP, GHOST
2. [ ] ChanServ: REGISTER, DROP
3. [ ] ChanServ: ACCESS ADD/DEL/LIST
4. [ ] ChanServ: OP/DEOP/VOICE/DEVOICE
5. [ ] Auto-op/voice on join
6. [ ] Nick enforcement

### Phase 4: Security Features (P2)
1. [ ] TLS support (rustls)
2. [ ] IP cloaking
3. [ ] Flood protection (burst/sustained)
4. [ ] Per-command rate limits
5. [ ] Max connections per IP
6. [ ] SHUN/UNSHUN commands
7. [ ] Oper hostmask enforcement

### Phase 5: Extended Commands (P2)
1. [ ] MONITOR command
2. [ ] HELP/HELPOP
3. [ ] LINKS, MAP
4. [ ] TRACE
5. [ ] RESTART
6. [ ] CHGHOST
7. [ ] SETNAME
8. [ ] WEBIRC
9. [ ] USERIP

### Phase 6: Advanced IRCv3 (P3)
1. [ ] labeled-response
2. [ ] batch
3. [ ] away-notify
4. [ ] account-notify
5. [ ] extended-join
6. [ ] cap-notify
7. [ ] account-tag
8. [ ] message-tags forwarding

### Phase 7: Transport Expansion (P3)
1. [ ] WebSocket support
2. [ ] WebSocket+TLS
3. [ ] STARTTLS upgrade

### Phase 8: Operations (P3)
1. [ ] Prometheus metrics
2. [ ] Graceful shutdown
3. [ ] Config hot reload
4. [ ] Database persistence for K/D-lines
5. [ ] Event sourcing (optional)

---

## 9. Missing Commands Quick Reference

Commands in slircd but NOT in slircd-ng:

```
CAP, AUTHENTICATE, TAGMSG, USERIP, MONITOR, LINKS, MAP, TRACE,
HELP, HELPOP, RULES, SUMMON, USERS, SERVLIST, SHUN, UNSHUN,
RESTART, CHGHOST, SETNAME, WEBIRC
```

**Total: 20 commands missing**

---

## 10. Dependency on slirc-proto

Before implementing certain features, verify `slirc-proto` has:

| Feature | Status | Notes |
|---------|--------|-------|
| Capability enum | ✅ | Full IRCv3.2 caps |
| SASL support | ✅ | PLAIN mechanism |
| Message tags | ✅ | IRCv3 tags parsing |
| TAGMSG command | Verify | May need Command variant |
| MONITOR command | Verify | May need Command variant |
| SETNAME command | Verify | May need Command variant |
| CHGHOST command | Verify | May need Command variant |

**🛑 Protocol-First Rule:** If any command/capability is missing from `slirc-proto`, that is a blocking dependency. Do not implement with raw strings.

---

## Appendix A: slircd Command List (54 total)

```
ADMIN, AUTHENTICATE, AWAY, CAP, CHGHOST, DIE, DLINE, HELP, HELPOP,
INFO, INVITE, ISON, JOIN, KICK, KILL, KLINE, KNOCK, LINKS, LIST,
LUSERS, MAP, MODE, MONITOR, MOTD, NAMES, NICK, NOTICE, OPER, PART,
PASS, PING, PONG, PRIVMSG, QUIT, REHASH, RESTART, RULES, SAJOIN,
SAMODE, SANICK, SAPART, SERVLIST, SETNAME, SHUN, STATS, SUMMON,
TAGMSG, TIME, TRACE, UNDLINE, UNKLINE, UNSHUN, USER, USERHOST,
USERIP, USERS, VERSION, WALLOPS, WEBIRC, WHO, WHOIS, WHOWAS
```

## Appendix B: slircd-ng Command List (34 total)

```
ADMIN, AWAY, DIE, DLINE, INFO, INVITE, ISON, JOIN, KICK, KILL,
KLINE, KNOCK, LIST, LUSERS, MODE, MOTD, NAMES, NICK, NOTICE, OPER,
PART, PASS, PING, PONG, PRIVMSG, QUIT, REHASH, SAJOIN, SAMODE,
SANICK, SAPART, STATS, TIME, UNDLINE, UNKLINE, USER, USERHOST,
VERSION, WALLOPS, WHO, WHOIS, WHOWAS
```

---

*Last updated: November 28, 2025*
