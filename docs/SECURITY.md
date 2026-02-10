# slircd-ng Security Architecture

> Generated from source code audit on 2026-02-10. Documents actual implemented security features.

## Overview

The security module (`src/security/`, 8 submodules) provides layered defense:

```
┌───────────────────────────────────────────────────────────────────────────────┐
│                           Security Module                                     │
├────────────┬──────────┬─────────────┬────────────────┬──────────┬─────────────┤
│ IpDenyList │ BanCache │  Cloaking   │ Rate Limiting  │ ExtBans  │  Spam/RBL   │
│ RoaringBmp │ DashMap  │ HMAC-SHA256 │   Governor     │ $a:/$r:  │ Heuristics  │
│ Z/D-lines  │  K/G     │ IP+Hostname │ Token Bucket   │ Chan +b  │ Entropy/URL │
└────────────┴──────────┴─────────────┴────────────────┴──────────┴─────────────┘
```

---

## Cloak Secret Enforcement

The server **refuses to start** with a weak `cloak_secret`. Validation checks:
- Minimum 16 characters
- At least 3 character classes (upper, lower, digit, special)
- At least 8 unique characters

Bypass: `SLIRCD_ALLOW_INSECURE_CLOAK=1` environment variable (testing only).

Generate a production secret: `openssl rand -hex 32`

---

## IP Cloaking (`cloaking.rs`)

- **Algorithm**: HMAC-SHA256 with configurable secret
- **Output format**: `<hash-segments>.<cloak_suffix>` (e.g., `drute.2mise.2e2wa.test`)
- IP addresses and hostnames are both cloaked
- Cloaked host is set during registration, before welcome burst
- Used in all user-visible contexts (WHO, WHOIS, message prefixes)

---

## IP Deny List (`ip_deny/`)

High-performance IP ban engine using **Roaring Bitmaps**:
- D-lines and Z-lines stored as bitmap ranges
- **Nanosecond rejection**: single bitmap lookup per connection
- Supports IPv4 and CIDR ranges
- Loaded from database at startup, updated via DLINE/ZLINE commands
- Z-lines are global (propagated via S2S), D-lines are local

---

## Ban Cache (`ban_cache.rs`)

In-memory `DashMap` cache for connection-time ban checks:
- K-lines: `user@host` pattern bans (local)
- G-lines: `user@host` pattern bans (global, propagated)
- Checked during registration before welcome burst
- Loaded from database at startup
- Updated via KLINE/GLINE operator commands

---

## Rate Limiting (`rate_limit.rs`)

Governor-based token bucket flood protection:

| Limiter | Default | Scope |
|---------|---------|-------|
| Message rate | 2/second | Per client |
| Connection burst | 3/10s | Per IP |
| Join burst | 5/10s | Per client |
| Max connections per IP | 10 | Per IP |

Configurable via `[security.rate_limits]`. IP exemptions via `exempt_ips` list.

Per-channel flood protection exists independently in the channel actor (separate from global rate limiting).

---

## Spam Detection (`spam.rs`, `heuristics.rs`)

Multi-layer content analysis engine:
- **Entropy analysis**: Detects random character spam
- **URL detection**: Identifies spam links
- **Repetition analysis**: Catches repeated messages
- **Configurable rules**: Runtime-adjustable via SPAMCONF operator command
- Enabled/disabled via `spam_detection_enabled` config

---

## Reputation System (`reputation.rs`)

User reputation scoring tracked per-user. Persisted to SQLite (`007_reputation.sql` migration).

---

## Real-time Blackhole Lists (`rbl.rs`)

Privacy-preserving RBL integration:
- HTTP/DNS-based lookups against configured RBL providers
- Uses `reqwest` with rustls-tls for API calls
- Uses `hickory-resolver` for DNS lookups
- Checked during connection registration

---

## Password Security (`password.rs`)

- **Algorithm**: Argon2id (via `argon2` crate)
- **Salt**: Random per-password (via `rand`)
- **Zeroize**: Password material zeroized after use (`zeroize` crate)
- SCRAM-SHA-256 verifiers for SASL (stored separately, `008_scram_verifiers.sql`)

---

## SASL Authentication

Three mechanisms:

| Mechanism | Module | Description |
|-----------|--------|-------------|
| PLAIN | `cap/sasl/plain.rs` | Password-based (base64 encoded) |
| EXTERNAL | `cap/sasl/external.rs` | TLS client certificate fingerprint |
| SCRAM-SHA-256 | `cap/sasl/scram.rs` | Challenge-response (no plaintext password) |

SASL is available pre-registration (via CAP) and supports `draft/account-registration` for creating accounts.

---

## Extended Bans (`xlines.rs`)

Pattern matching beyond traditional `nick!user@host`:

| Prefix | Match Target | Example |
|--------|-------------|---------|
| `$a:` | Account name | `$a:spammer` |
| `$r:` | Real name (GECOS) | `$r:*spam*` |
| `$j:` | Channel membership | `$j:#badchannel` |
| `$x:` | Full match (nick!user@host#realname) | `$x:*!*@*#*spam*` |
| `$z` | TLS users (no argument) | `$z` |

Used in channel +b (ban), +e (except), +I (invite exception), +q (quiet) lists.

---

## Ban Types

| Type | Command | Scope | Propagated | Matching |
|------|---------|-------|-----------|----------|
| K-line | KLINE/UNKLINE | Local server | No | user@host |
| D-line | DLINE/UNDLINE | Local server | No | IP/CIDR |
| G-line | GLINE/UNGLINE | Network-wide | Yes (S2S) | user@host |
| Z-line | ZLINE/UNZLINE | Network-wide | Yes (S2S) | IP/CIDR |
| R-line | RLINE/UNRLINE | Local server | No | Realname |
| Shun | SHUN/UNSHUN | Local server | No | user@host |

All bans support: optional expiry time, reason, set-by tracking. Stored in SQLite with automatic expiry cleanup.

Shuns are special: the user stays connected but all commands are silently ignored.

---

## Capability Token Authorization (`src/caps/`)

"Innovation 4" — replaces scattered `if is_oper()` checks:

- `Cap<T>` tokens are unforgeable (non-Clone, non-Copy, `pub(super)` constructor)
- `CapabilityAuthority` is the sole mint — evaluates permissions and issues tokens
- Handler functions require `Cap<T>` in their signature, making unauthorized calls a compile-time error
- All capability grants are logged for audit

Currently used for: Kick, Topic, Invite, Op, Kill, Wallops, Globops, Rehash, Die, Restart, all ban types, SA* commands.

---

## TLS

- **Library**: tokio-rustls with aws-lc-rs crypto provider
- **STARTTLS**: Supported pre-registration (RFC 7194)
- **Strict Transport Security (STS)**: IRCv3 STS capability, dynamically advertised based on TLS config
- **Client certificates**: Used for SASL EXTERNAL authentication
- **S2S TLS**: Separate TLS config for server links

---

## Crypto Dependencies

| Crate | Version | Usage |
|-------|---------|-------|
| `argon2` | 0.5 | Password hashing |
| `ring` | 0.17 | Cryptographic operations |
| `hmac` | 0.12 | IP cloaking |
| `sha2` | 0.10 | WebSocket handshake, SCRAM |
| `subtle` | 2.5 | Timing-safe comparisons |
| `base64` | 0.22 | SASL encoding |
| `scram` | 0.6 | SCRAM-SHA-256 |
| `tokio-rustls` | 0.26 | TLS |

---

## Security-Critical Code Paths

1. **Connection acceptance**: IP deny list → ban cache → rate limit → PROXY protocol → TLS/plaintext → registration
2. **Registration**: Cloak application → password check → SASL → ban check → welcome burst
3. **Message routing**: Shun check → rate limit → spam detection → channel mode enforcement → delivery
4. **Operator authentication**: Timing-safe password comparison → hostmask validation → capability token issuance
