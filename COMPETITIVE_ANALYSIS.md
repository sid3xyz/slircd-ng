# slircd-ng Competitive Analysis

**Created**: January 2026  
**Purpose**: Feature comparison against major IRC servers to identify gaps and opportunities

---

## Executive Summary

This document compares slircd-ng against the major IRC server implementations:

| Server | Language | License | Architecture | Key Differentiator |
|--------|----------|---------|--------------|-------------------|
| **slircd-ng** | Rust | Unlicense | Monolithic + Actors | Zero-copy parsing, integrated services |
| **Ergo** | Go | MIT | Monolithic | Integrated bouncer, PRECIS, full IRCv3 |
| **InspIRCd** | C++ | GPLv2 | Modular | 150+ modules, extreme customization |
| **UnrealIRCd** | C | GPLv2 | Modular | 38% market share, JSON-RPC API |
| **Solanum** | C | ISC | Federated | Powers Libera Chat, traditional S2S |
| **ngIRCd** | C | GPLv2+ | Lightweight | Simple config, portable |

---

## Feature Comparison Matrix

### ✅ = Full Support | ⚠️ = Partial | ❌ = Missing | N/A = Not Applicable

---

## 1. IRCv3 Capabilities

| Capability | slircd-ng | Ergo | InspIRCd | UnrealIRCd | Solanum |
|------------|-----------|------|----------|------------|---------|
| **account-notify** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **account-tag** | ✅ | ✅ | ✅ | ✅ | ⚠️ |
| **away-notify** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **batch** | ✅ | ✅ | ✅ | ✅ | ⚠️ |
| **cap-notify** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **chghost** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **echo-message** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **extended-join** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **invite-notify** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **labeled-response** | ✅ | ✅ | ✅ | ✅ | ⚠️ |
| **message-tags** | ✅ | ✅ | ✅ | ✅ | ⚠️ |
| **monitor** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **extended-monitor** | ✅ | ✅ | ❌ | ❌ | ❌ |
| **multi-prefix** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **SASL** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **server-time** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **setname** | ✅ | ✅ | ✅ | ✅ | ⚠️ |
| **userhost-in-names** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **STS** | ✅ | ✅ | ✅ | ✅ | ❌ |
| **STARTTLS** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **draft/chathistory** | ✅ | ✅ | ⚠️ | ✅ | ❌ |
| **draft/event-playback** | ✅ | ✅ | ❌ | ⚠️ | ❌ |
| **draft/multiline** | ✅ | ✅ | ❌ | ❌ | ❌ |
| **draft/account-registration** | ✅ | ✅ | ❌ | ❌ | ❌ |
| **draft/relaymsg** | ⚠️ | ✅ | ❌ | ❌ | ❌ |
| **draft/metadata** | ✅ | ❌ | ❌ | ❌ | ❌ |
| **draft/webpush** | ❌ | ✅ | ❌ | ❌ | ❌ |

### IRCv3 Gap Analysis (slircd-ng)

| Missing Feature | Priority | Effort | Impact |
|-----------------|----------|--------|--------|
| **webpush** | LOW | HIGH | Mobile notifications |
| **bouncer resumption** | MEDIUM | VERY HIGH | 7 irctest failures |
| **ZNC playback** | LOW | MEDIUM | ZNC compatibility |

---

## 2. Integrated Services

| Feature | slircd-ng | Ergo | InspIRCd | UnrealIRCd | Solanum |
|---------|-----------|------|----------|------------|---------|
| **NickServ** | ✅ | ✅ | ❌ (Anope/Atheme) | ❌ (Anope/Atheme) | ❌ (Atheme) |
| **ChanServ** | ✅ | ✅ | ❌ (Anope/Atheme) | ❌ (Anope/Atheme) | ❌ (Atheme) |
| **HostServ** | ⚠️ VHOST | ✅ | ❌ (Anope/Atheme) | ❌ (Anope/Atheme) | ❌ (Atheme) |
| **MemoServ** | ❌ | ❌ | ❌ (Anope/Atheme) | ❌ (Anope/Atheme) | ❌ (Atheme) |
| **BotServ** | ❌ | ❌ | ❌ (Anope/Atheme) | ❌ (Anope/Atheme) | ❌ (Atheme) |
| **SASL PLAIN** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **SASL EXTERNAL** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **SASL SCRAM-SHA-256** | ✅ | ❌ | ❌ | ❌ | ❌ |
| **Account registration (in-band)** | ✅ | ✅ | ❌ | ❌ | ❌ |
| **Nick enforcement** | ✅ | ✅ | ❌ (external) | ❌ (external) | ❌ (external) |
| **Channel registration** | ✅ | ✅ | ❌ (external) | ❌ (external) | ❌ (external) |
| **Access lists (AMODE)** | ✅ | ✅ | ❌ (external) | ❌ (external) | ❌ (external) |
| **Email verification** | ⚠️ | ✅ | ❌ | ❌ | ❌ |

### Services Advantages (slircd-ng)
- ✅ **SASL SCRAM-SHA-256**: Unique among IRC servers - secure password auth
- ✅ **Zero external dependencies**: No Anope/Atheme needed
- ✅ **Integrated account registration**: IRCv3 draft/account-registration

### Services Gaps
| Missing Feature | Priority | Effort |
|-----------------|----------|--------|
| MemoServ | LOW | MEDIUM |
| BotServ | LOW | MEDIUM |
| SASL OAUTHBEARER | LOW | MEDIUM |
| LDAP auth backend | MEDIUM | HIGH |
| Full HostServ | LOW | LOW |

---

## 3. Security Features

| Feature | slircd-ng | Ergo | InspIRCd | UnrealIRCd | Solanum |
|---------|-----------|------|----------|------------|---------|
| **TLS/SSL** | ✅ rustls | ✅ Go crypto | ✅ OpenSSL/GnuTLS | ✅ OpenSSL | ✅ OpenSSL |
| **Strict Transport Security** | ✅ | ✅ | ✅ | ✅ | ❌ |
| **DNSBL** | ✅ | ✅ (script) | ✅ | ✅ | ⚠️ |
| **RBL (Realtime Blocklist)** | ✅ | ✅ | ✅ | ✅ | ❌ |
| **K-line** | ✅ | ✅ UBAN | ✅ | ✅ | ✅ |
| **D-line** | ✅ | ✅ UBAN | ✅ | ✅ | ✅ |
| **G-line (network-wide)** | ✅ | ✅ UBAN | ✅ | ✅ | ✅ |
| **X-line (realname)** | ✅ | ❌ | ✅ | ✅ | ⚠️ |
| **SHUN** | ✅ | ❌ | ✅ | ✅ | ❌ |
| **Spamfilter (regex)** | ⚠️ basic | ❌ | ✅ | ✅✅ | ❌ |
| **IP cloaking** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **Rate limiting** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **Connection throttling** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **WEBIRC/CGI:IRC** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **Reputation system** | ✅ | ❌ | ❌ | ✅ | ❌ |
| **GeoIP** | ❌ | ❌ | ✅ | ✅ | ❌ |
| **ASN lookup** | ❌ | ❌ | ❌ | ✅ | ❌ |
| **DEFCON levels** | ❌ | ✅ | ❌ | ❌ | ❌ |
| **Argon2 passwords** | ❌ | ❌ | ✅ | ✅ | ❌ |
| **bcrypt passwords** | ✅ | ✅ | ✅ | ✅ | ❌ |

### Security Advantages (slircd-ng)
- ✅ **Memory safety**: Rust eliminates entire classes of security bugs
- ✅ **Zero-copy parsing**: Reduced attack surface from buffer handling
- ✅ **SCRAM-SHA-256**: Secure SASL without plaintext passwords
- ✅ **Reputation system**: Adaptive trust scoring

### Security Gaps
| Missing Feature | Priority | Effort |
|-----------------|----------|--------|
| GeoIP | MEDIUM | MEDIUM |
| ASN lookup | LOW | MEDIUM |
| Advanced spamfilter | MEDIUM | HIGH |
| Argon2 passwords | LOW | LOW |
| DEFCON levels | LOW | MEDIUM |

---

## 4. Bouncer/Multi-Client Features

| Feature | slircd-ng | Ergo | InspIRCd | UnrealIRCd | Solanum |
|---------|-----------|------|----------|------------|---------|
| **Multiple devices, same nick** | ❌ | ✅ | ❌ | ❌ | ❌ |
| **Always-on clients** | ❌ | ✅ | ❌ | ❌ | ❌ |
| **Automatic history replay** | ⚠️ | ✅ | ❌ | ⚠️ | ❌ |
| **Device-specific history** | ❌ | ✅ | ❌ | ❌ | ❌ |
| **CHATHISTORY** | ✅ | ✅ | ⚠️ | ✅ | ❌ |
| **ZNC playback emulation** | ❌ | ✅ | ❌ | ❌ | ❌ |
| **Bouncer resumption** | ❌ | ❌ | ❌ | ❌ | ❌ |
| **Push notifications** | ❌ | ✅ | ❌ | ❌ | ❌ |

### Bouncer Gap Analysis (slircd-ng)
This is Ergo's major differentiator. To match:

| Feature | Priority | Effort | Complexity |
|---------|----------|--------|------------|
| Multi-device same nick | HIGH | VERY HIGH | Major architecture change |
| Always-on clients | HIGH | VERY HIGH | Requires session persistence |
| Device-specific history | MEDIUM | HIGH | Database schema changes |
| ZNC playback | LOW | MEDIUM | Protocol emulation |
| Push notifications | LOW | HIGH | External service integration |

**Recommendation**: Defer bouncer features to 2.0. They require fundamental architecture changes.

---

## 5. Federation/Linking

| Feature | slircd-ng | Ergo | InspIRCd | UnrealIRCd | Solanum |
|---------|-----------|------|----------|------------|---------|
| **Server-to-server linking** | ✅ | ❌ (planned) | ✅ | ✅ | ✅ |
| **TS6 protocol** | ❌ | ❌ | ❌ | ❌ | ✅ |
| **Custom S2S protocol** | ✅ | N/A | ✅ (spanning tree) | ✅ | ❌ |
| **TLS for links** | ✅ | N/A | ✅ | ✅ | ✅ |
| **CRDT state sync** | ✅ | ❌ | ❌ | ❌ | ❌ |
| **Certificate fingerprint auth** | ✅ | N/A | ✅ | ✅ | ✅ |
| **Remote includes** | ❌ | ❌ | ❌ | ✅ | ❌ |

### Federation Advantage (slircd-ng)
- ✅ **CRDT state sync**: Unique - automatic conflict resolution
- ✅ **S2S with TLS**: Modern, secure linking

**Note**: Ergo is single-server only (no federation). For networks requiring multiple servers, this is a major limitation.

---

## 6. Channel Features

| Feature | slircd-ng | Ergo | InspIRCd | UnrealIRCd | Solanum |
|---------|-----------|------|----------|------------|---------|
| **Standard modes (+i,+m,+n,+t,+k,+l)** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **Ban (+b)** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **Ban exception (+e)** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **Invite exception (+I)** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **Quiet/mute (+q)** | ⚠️ | ✅ (~q) | ✅ | ✅ | ✅ |
| **Channel forwarding (+f)** | ⚠️ | ✅ | ✅ | ✅ | ✅ |
| **Registered only (+R)** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **Moderated registered (+M)** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **SSL only (+z)** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **Permanent channel (+P)** | ✅ | ✅ | ✅ | ✅ | ❌ |
| **Auditorium (+u)** | ✅ | ✅ | ✅ | ❌ | ❌ |
| **Op-moderated (+U)** | ✅ | ✅ | ✅ | ❌ | ❌ |
| **No color (+c)** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **Strip color (+S)** | ✅ | ❌ | ✅ | ✅ | ❌ |
| **No CTCP (+C)** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **No notice (+T)** | ✅ | ❌ | ✅ | ✅ | ❌ |
| **Delayed join (+D)** | ✅ | ❌ | ✅ | ✅ | ❌ |
| **Join flood protection (+j)** | ✅ | ❌ | ✅ | ✅ | ❌ |
| **Message flood protection (+f)** | ⚠️ | ❌ | ✅ | ✅ | ❌ |
| **Nick flood protection (+F)** | ❌ | ❌ | ✅ | ❌ | ❌ |
| **Kick no rejoin (+J)** | ✅ | ❌ | ✅ | ✅ | ❌ |
| **No nick change (+N)** | ✅ | ❌ | ✅ | ✅ | ❌ |
| **No knock (+K)** | ✅ | ❌ | ✅ | ✅ | ❌ |
| **Channel history (+H)** | ⚠️ auto | ❌ | ✅ | ✅ | ❌ |
| **Extended bans (~a, ~c, ~r)** | ⚠️ | ✅ | ✅ | ✅✅ | ⚠️ |
| **Timed bans (~t)** | ❌ | ❌ | ✅ | ✅ | ❌ |
| **ROLEPLAY (+E, NPC)** | ⚠️ | ✅ | ❌ | ❌ | ❌ |

### Channel Feature Gaps
| Missing Feature | Priority | Effort |
|-----------------|----------|--------|
| Extended bans (full) | MEDIUM | MEDIUM |
| Timed bans | LOW | LOW |
| Nick flood protection | LOW | LOW |
| Channel +f full implementation | LOW | MEDIUM |

---

## 7. User Features

| Feature | slircd-ng | Ergo | InspIRCd | UnrealIRCd | Solanum |
|---------|-----------|------|----------|------------|---------|
| **VHOST** | ✅ | ✅ | ✅ | ✅ | ⚠️ |
| **SETNAME** | ✅ | ✅ | ✅ | ✅ | ❌ |
| **CHGHOST (oper)** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **CHGIDENT (oper)** | ✅ | ✅ | ✅ | ✅ | ❌ |
| **Bot mode (+B)** | ✅ | ✅ | ✅ | ✅ | ❌ |
| **Deaf mode (+d)** | ✅ | ❌ | ✅ | ✅ | ❌ |
| **SILENCE** | ⚠️ | ✅ | ✅ | ❌ | ❌ |
| **ACCEPT** | ✅ | ✅ | ✅ | ❌ | ✅ |
| **CALLERID (+g)** | ✅ | ❌ | ✅ | ❌ | ✅ |
| **Hide channels (+I)** | ✅ | ✅ | ✅ | ✅ | ❌ |
| **Hide idle (+I)** | ❌ | ❌ | ❌ | ✅ | ❌ |
| **No CTCP (+T)** | ✅ | ✅ | ✅ | ✅ | ❌ |
| **Private deaf (+D)** | ✅ | ❌ | ✅ | ✅ | ❌ |

---

## 8. Operator Features

| Feature | slircd-ng | Ergo | InspIRCd | UnrealIRCd | Solanum |
|---------|-----------|------|----------|------------|---------|
| **KILL** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **KLINE/DLINE/GLINE** | ✅ | ✅ UBAN | ✅ | ✅ | ✅ |
| **SAJOIN/SAPART** | ✅ | ✅ | ✅ | ✅ | ❌ |
| **SAMODE** | ✅ | ✅ | ✅ | ✅ | ❌ |
| **SANICK** | ✅ | ❌ | ✅ | ✅ | ❌ |
| **WALLOPS** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **GLOBOPS** | ✅ | ❌ | ✅ | ✅ | ✅ |
| **Snomasks** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **Oper override** | ⚠️ | ✅ | ✅ | ✅ | ⚠️ |
| **REHASH** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **DIE/RESTART** | ⚠️ | ✅ | ✅ | ✅ | ✅ |
| **Oper classes** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **Certificate auth** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **Multiple oper types** | ✅ | ✅ | ✅ | ✅ | ✅ |

---

## 9. Administration & Management

| Feature | slircd-ng | Ergo | InspIRCd | UnrealIRCd | Solanum |
|---------|-----------|------|----------|------------|---------|
| **TOML config** | ✅ | ✅ YAML | ❌ custom | ❌ custom | ❌ custom |
| **Hot reload (REHASH)** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **MOTD** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **RULES** | ✅ | ❌ | ❌ | ✅ | ❌ |
| **HELPOP** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **STATS** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **Prometheus metrics** | ✅ | ❌ | ❌ | ❌ | ❌ |
| **JSON-RPC API** | ❌ | ✅ | ❌ | ✅ | ❌ |
| **Admin webpanel** | ❌ | ❌ | ❌ | ✅ | ❌ |
| **SQLite backend** | ✅ | ✅ | ✅ | ❌ | ❌ |
| **MySQL backend** | ❌ | ✅ | ✅ | ❌ | ❌ |
| **PostgreSQL backend** | ❌ | ❌ | ✅ | ❌ | ❌ |
| **Log to channel** | ⚠️ | ❌ | ✅ | ✅ | ❌ |
| **JSON logging** | ⚠️ | ❌ | ✅ | ✅ | ❌ |

### Admin Advantages (slircd-ng)
- ✅ **Prometheus metrics**: Only IRCd with native metrics endpoint
- ✅ **TOML config**: Modern, readable configuration

### Admin Gaps
| Missing Feature | Priority | Effort |
|-----------------|----------|--------|
| JSON-RPC API | MEDIUM | HIGH |
| MySQL backend | LOW | MEDIUM |
| Admin webpanel | LOW | VERY HIGH |

---

## 10. Protocol & Connectivity

| Feature | slircd-ng | Ergo | InspIRCd | UnrealIRCd | Solanum |
|---------|-----------|------|----------|------------|---------|
| **RFC 1459** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **RFC 2812** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **WebSocket** | ✅ | ✅ | ✅ | ✅ | ❌ |
| **PROXY protocol v1** | ✅ | ✅ | ✅ | ✅ | ⚠️ |
| **PROXY protocol v2** | ✅ | ✅ | ❌ | ❌ | ❌ |
| **Unix socket** | ❌ | ✅ | ❌ | ❌ | ❌ |
| **IPv6** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **Tor onion service** | ⚠️ | ✅ | ❌ | ❌ | ❌ |
| **I2P** | ❌ | ✅ | ❌ | ❌ | ❌ |
| **PRECIS casemapping** | ✅ | ✅ | ❌ | ❌ | ❌ |
| **UTF-8 nicknames** | ⚠️ | ✅ | ❌ | ✅ | ❌ |
| **Custom nick charsets** | ❌ | ❌ | ✅ | ✅✅ | ❌ |

### Protocol Advantages (slircd-ng)
- ✅ **PRECIS casemapping**: Modern Unicode handling
- ✅ **PROXY v1+v2**: Full proxy protocol support
- ✅ **Zero-copy parsing**: Unique performance architecture

---

## 11. Internationalization

| Feature | slircd-ng | Ergo | InspIRCd | UnrealIRCd | Solanum |
|---------|-----------|------|----------|------------|---------|
| **Localized server messages** | ❌ | ✅ (12+ languages) | ❌ | ⚠️ (docs only) | ❌ |
| **LANGUAGE command** | ❌ | ✅ | ❌ | ❌ | ❌ |
| **UTF-8 support** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **PRECIS normalization** | ✅ | ✅ | ❌ | ❌ | ❌ |
| **Confusables detection** | ❌ | ✅ | ❌ | ❌ | ❌ |

### I18n Gaps
| Missing Feature | Priority | Effort |
|-----------------|----------|--------|
| Localized messages | LOW | MEDIUM |
| LANGUAGE command | LOW | LOW |
| Confusables detection | MEDIUM | MEDIUM |

---

## 12. Performance & Scalability

| Aspect | slircd-ng | Ergo | InspIRCd | UnrealIRCd | Solanum |
|--------|-----------|------|----------|------------|---------|
| **Target scale** | 10k+ | 10k | 100k+ | 100k+ | 100k+ |
| **Zero-copy parsing** | ✅ | ❌ | ❌ | ❌ | ❌ |
| **Async I/O** | ✅ Tokio | ✅ goroutines | ✅ epoll | ✅ epoll | ✅ epoll |
| **Memory efficiency** | HIGH | MEDIUM | HIGH | HIGH | HIGH |
| **Multi-threading** | ✅ | ✅ | ❌ | ❌ | ❌ |
| **Federation** | ✅ | ❌ | ✅ | ✅ | ✅ |
| **Channel actors** | ✅ | ❌ | ❌ | ❌ | ❌ |
| **Benchmarks published** | ❌ | ❌ | ❌ | ❌ | ❌ |

### Performance Advantages (slircd-ng)
- ✅ **Zero-copy parsing**: Unique - no string allocations on parse
- ✅ **Channel actors**: Per-channel Tokio tasks with bounded mailboxes
- ✅ **Rust memory safety**: No GC pauses, predictable latency

---

## Priority Action Items for 1.0

### Critical (Must Have)
All already implemented ✅

### High Priority Gaps
| Gap | Competitor | Impact | Effort | 1.0? |
|-----|------------|--------|--------|------|
| GeoIP/ASN | UnrealIRCd | Operator tooling | MEDIUM | ⚠️ Maybe |
| Advanced spamfilter | UnrealIRCd | Abuse prevention | HIGH | ❌ Post-1.0 |
| Extended bans (full) | All | Channel moderation | MEDIUM | ⚠️ Maybe |
| JSON-RPC API | Ergo/UnrealIRCd | Integration | HIGH | ❌ Post-1.0 |

### Medium Priority Gaps (1.1+)
| Gap | Competitor | Impact |
|-----|------------|--------|
| Localized messages | Ergo | i18n |
| Confusables detection | Ergo | Security |
| Channel +f full impl | All | Moderation |
| Timed bans | InspIRCd/UnrealIRCd | Convenience |
| MySQL backend | Ergo/InspIRCd | Large networks |

### Low Priority / 2.0 (Major Architecture)
| Gap | Competitor | Notes |
|-----|------------|-------|
| Multi-device bouncer | Ergo | Major rearchitecture |
| Always-on clients | Ergo | Session persistence |
| Push notifications | Ergo | External dependency |
| Admin webpanel | UnrealIRCd | Large project |

---

## Unique slircd-ng Advantages

### Features NO Other IRCd Has

1. **SASL SCRAM-SHA-256**: Secure password authentication without plaintext
2. **Zero-copy message parsing**: Performance architecture unique in IRC space
3. **CRDT-based federation**: Automatic conflict resolution for distributed state
4. **Native Prometheus metrics**: Built-in observability
5. **Rust memory safety**: Entire class of security bugs eliminated
6. **Channel actor model**: Scalable per-channel task isolation
7. **Typestate handler system**: Compile-time protocol state enforcement

### Competitive Positioning

| Against | slircd-ng Advantage | slircd-ng Disadvantage |
|---------|---------------------|------------------------|
| **Ergo** | Federation, SCRAM-SHA-256, zero-copy | No bouncer features |
| **InspIRCd** | Memory safety, integrated services, modern config | Fewer modules |
| **UnrealIRCd** | Memory safety, CRDT sync, Prometheus | No GeoIP, no webpanel |
| **Solanum** | Modern IRCv3, integrated services, TLS | Established ecosystem |

---

## Conclusion

**slircd-ng is already competitive** with major IRC servers for core functionality. 

**Strengths**: IRCv3 compliance, integrated services, federation, security, performance architecture.

**Areas for improvement**: Bouncer features (long-term), GeoIP (medium-term), advanced spam filtering (medium-term).

**Recommendation**: Release 1.0 focused on current strengths. Bouncer features require major rearchitecture and should be 2.0 goals.

---

*Last updated: January 2026*
