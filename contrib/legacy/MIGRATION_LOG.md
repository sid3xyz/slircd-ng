# Legacy Code Migration Log

> **⚠️ This directory is READ-ONLY reference material.**
> Do not modify files here. Use this log to track what has been adapted.

This file tracks the granular migration status of features from the legacy slircd
codebase into slircd-ng. The legacy code uses direct state mutation; slircd-ng
uses the Matrix/Effects architecture.

## Status Legend

| Status | Meaning |
|--------|---------|
| ⏳ | Not started |
| 🚧 | In progress |
| ✅ | Adapted to slircd-ng |
| ❌ | Will not adapt (obsolete/replaced) |

---

## Security (`security/`)

### IP Cloaking (`cloaking/mod.rs`)

| Component | Status | slircd-ng Location | Notes |
|-----------|--------|-------------------|-------|
| HMAC-SHA256 hashing | ⏳ | — | Core algorithm is portable |
| Hierarchical segment cloaking | ⏳ | — | `host.example.com` → `abc123.example.com` |
| Base32 output encoding | ⏳ | — | Need to add `base32` crate |
| IPv4/IPv6 detection | ⏳ | — | Different prefix handling |

**Dependencies needed:** `hmac`, `sha2`, `base32`

### Anti-Abuse (`anti_abuse/`)

| Component | Status | slircd-ng Location | Notes |
|-----------|--------|-------------------|-------|
| `ExtendedBan` enum ($a:, $r:, etc.) | ⏳ | — | Types are directly usable |
| `XLine` enum (K/G/Z/R/S-lines) | 🚧 | `src/handlers/bans.rs` | Partially implemented |
| Connection tracking | ⏳ | — | Needs Matrix adaptation |
| Rate limiting (governor) | ✅ | `src/security/rate_limit.rs` | Already implemented |
| CTCP flood detection | ⏳ | — | Pattern detection logic |
| Repeat message detection | ⏳ | — | Needs ringbuffer per-user |

### Spam Detection (`anti_abuse/spam_detection.rs`)

| Component | Status | slircd-ng Location | Notes |
|-----------|--------|-------------------|-------|
| CTCP version flood | ⏳ | — | Track per-source |
| Repeat text detection | ⏳ | — | Hash-based dedup |
| Channel spam scoring | ⏳ | — | Multi-factor scoring |

---

## Services (`services/`)

> **Architecture Note:** Legacy services mutate state directly.
> slircd-ng uses `ServiceEffect` return values. Adapt logic, not structure.

### NickServ (`nickserv.rs`)

| Command | Status | slircd-ng Location | Notes |
|---------|--------|-------------------|-------|
| REGISTER | ⏳ | `src/services/nickserv.rs` | DB schema ready |
| IDENTIFY | ⏳ | `src/services/nickserv.rs` | SASL preferred |
| GHOST | ⏳ | — | Kill other session |
| DROP | ⏳ | — | Delete registration |
| SET PASSWORD | ⏳ | — | bcrypt hashing |
| INFO | ⏳ | — | Account metadata |

**Dependencies needed:** `bcrypt`

### ChanServ (`chanserv.rs`)

| Command | Status | slircd-ng Location | Notes |
|---------|--------|-------------------|-------|
| REGISTER | ⏳ | `src/services/chanserv.rs` | DB schema ready |
| OP/DEOP | ⏳ | — | Mode change effect |
| KICK | ⏳ | — | Kick effect |
| ACCESS LIST | ⏳ | — | DB-backed ACL |
| SET FOUNDER | ⏳ | — | `set_founder` query ready |
| AKICK | ⏳ | — | Auto-kick on join |

---

## Observability (`prometheus/`)

| Component | Status | slircd-ng Location | Notes |
|-----------|--------|-------------------|-------|
| Metrics registry | ⏳ | — | Use `vise` or `metrics` |
| HTTP `/metrics` endpoint | ⏳ | — | axum server |
| Connection counters | ⏳ | — | Gauge metrics |
| Command latency histograms | ⏳ | — | Per-command timing |

**Dependencies needed:** `vise` or `metrics`, `axum` (or reuse existing HTTP)

---

## Infrastructure (`infrastructure/`)

| Component | Status | slircd-ng Location | Notes |
|-----------|--------|-------------------|-------|
| Database layer | ✅ | `src/db/` | Same SQLx stack |
| Chat history | ⏳ | — | Schema needed |
| TOML config | ✅ | `src/config.rs` | Already implemented |

---

## Commands (`commands/`)

> These are mostly reference for edge cases. Core commands are reimplemented.

| Component | Status | Notes |
|-----------|--------|-------|
| Mode edge cases | ⏳ | Reference for +e/+I handling |
| Nick collision | ⏳ | Reference for TS rules |
| PRIVMSG routing | ✅ | Reimplemented |

---

## Migration Checklist

When adapting a component:

1. [ ] Read the legacy implementation
2. [ ] Identify Matrix access patterns
3. [ ] Design the `ServiceEffect` (if service) or handler response
4. [ ] Write the slircd-ng implementation
5. [ ] Write tests (unit + integration)
6. [ ] Update this log with ✅ and location

---

*Last updated: 2024-11-30*
