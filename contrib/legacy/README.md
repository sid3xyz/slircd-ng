# Legacy slircd Code Reference

> **⚠️ READ-ONLY REFERENCE MATERIAL**
>
> This directory contains archived code from the original slircd implementation.
> **Do not modify these files.** They exist solely as reference for adapting
> features into slircd-ng's Matrix/Effects architecture.
>
> See [`MIGRATION_LOG.md`](../../../../docs/archive/slircd-ng/MIGRATION_LOG.md) for granular tracking of what
> has been adapted and what remains.

This folder contains code extracted from the original slircd implementation
(located at `/home/case/_SLIRC_DEVELOPMENT_/ircd/`) for reference and potential
adaptation into slircd-ng.

**Total: ~9,445 lines of Rust code**

## Contents

### 🛡️ Security (`security/`)

| Module                         | Lines | Description                                           | Reusability                |
| ------------------------------ | ----- | ----------------------------------------------------- | -------------------------- |
| `cloaking/mod.rs`              | 264   | HMAC-SHA256 IP cloaking with hierarchical segments    | ⭐⭐⭐ Ready to adapt         |
| `anti_abuse/primitives.rs`     | 604   | ExtendedBan types, X-lines (K/G/Z/R/S), rate limiting | ⭐⭐⭐ Ready to adapt         |
| `anti_abuse/service.rs`        | 634   | Full anti-abuse service with connection tracking      | ⭐⭐ Needs Matrix adaptation |
| `anti_abuse/spam_detection.rs` | 395   | CTCP flood, repeat message detection                  | ⭐⭐ Needs Matrix adaptation |

### 🔧 Services (`services/`)

| Module             | Lines | Description                          | Reusability               |
| ------------------ | ----- | ------------------------------------ | ------------------------- |
| `nickserv.rs`      | 699   | REGISTER, IDENTIFY, GHOST, DROP      | ⭐⭐ Different architecture |
| `chanserv.rs`      | 906   | REGISTER, OP/DEOP, KICK, ACCESS LIST | ⭐⭐ Different architecture |
| `routing.rs`       | 142   | Service message routing              | ⭐ Reference only          |
| `pseudo_client.rs` | 151   | Pseudo-client for service bots       | ⭐ Reference only          |

### 📊 Observability (`prometheus/`)

| Module      | Lines | Description                      | Reusability         |
| ----------- | ----- | -------------------------------- | ------------------- |
| `mod.rs`    | 164   | Plugin interface, config parsing | ⭐⭐ Needs adaptation |
| `server.rs` | 552   | HTTP /metrics endpoint with axum | ⭐⭐⭐ Ready to adapt  |

### 🗄️ Infrastructure (`infrastructure/`)

| Module                    | Lines | Description                      | Reusability                 |
| ------------------------- | ----- | -------------------------------- | --------------------------- |
| `persistence/database.rs` | ~1000 | SQLx + SQLite, account storage   | ⭐⭐ Same SQLx stack          |
| `persistence/history.rs`  | ~500  | Chat history with message search | ⭐⭐ Needs schema adaptation  |
| `config/`                 | ~400  | TOML config parsing              | ⭐ Already have in slircd-ng |

### 📝 Commands (`commands/`)

| Module            | Lines | Description                  | Reusability                |
| ----------------- | ----- | ---------------------------- | -------------------------- |
| `core/mode.rs`    | 192   | User/channel mode handling   | ⭐ Reference for edge cases |
| `core/nick.rs`    | 137   | Nick collision handling      | ⭐ Reference only           |
| `core/privmsg.rs` | 141   | Message routing logic        | ⭐ Reference only           |
| `registry.rs`     | 92    | Command registration pattern | ⭐ Reference only           |

## Adaptation Priority

### High Priority (Path 2 - Security)
1. **`cloaking/mod.rs`** - Can be adapted with minimal changes
   - Change `ServerState` → `Matrix`
   - Keep HMAC-SHA256 + base32 algorithm
   - Integrate with connection handler

2. **`anti_abuse/primitives.rs`** - Types are directly usable
   - `ExtendedBan` enum for $a:, $r:, etc.
   - `XLine` enum for K/G/Z/R/S-lines
   - Add to `slircd-ng/src/state/mod.rs`

### Medium Priority (Path 3 - Observability)
3. **`prometheus/server.rs`** - HTTP server for /metrics
   - Uses axum (we may want to reuse WebSocket http crate)
   - Metrics collection pattern is reusable

### Lower Priority
4. **Services** - Different architecture (direct state mutation vs. effects)
   - Reference for command parsing and help text
   - slircd-ng uses `ServiceEffect` pattern instead

## Dependencies from Old slircd

These crates were used in the original and are compatible:

```toml
# Already in slircd-ng
dashmap = "6"
tokio = "1"
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite"] }

# May need to add
governor = "0.6"        # Rate limiting
hmac = "0.12"           # For cloaking
sha2 = "0.10"           # For cloaking
base32 = "0.5"          # For cloaking output
bcrypt = "0.15"         # For NickServ passwords
vise = "0.2"            # Prometheus metrics
axum = "0.7"            # For metrics HTTP (or reuse existing http)
```

## Integration Notes

### Cloaking Integration
```rust
// In connection.rs, after TLS handshake:
let cloaked_host = cloak_ip(&peer_addr.ip(), &config.cloak_key);
user.cloaked_host = Some(cloaked_host);
```

### ExtendedBan Integration
```rust
// In state/mod.rs, add to ban matching:
pub fn matches_extended_ban(user: &User, ban: &ExtendedBan) -> bool {
    match ban {
        ExtendedBan::Account(pattern) => user.account.as_ref()
            .map(|a| wildcard_match(pattern, a)).unwrap_or(false),
        ExtendedBan::Realname(pattern) => wildcard_match(pattern, &user.realname),
        // ... etc
    }
}
```

### X-Line Integration
```rust
// Add DashMap to Matrix:
pub struct Matrix {
    // ... existing fields
    pub klines: DashMap<String, XLine>,  // host pattern → XLine
    pub glines: DashMap<String, XLine>,  // host pattern → XLine (global)
    pub zlines: DashMap<IpAddr, XLine>,  // IP → ZLine
}
```

---

**Note**: This code is for reference. Direct copy-paste may not work due to
architectural differences. Use as a guide for implementing equivalent
functionality in the slircd-ng effect-based architecture.
