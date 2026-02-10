# slircd-ng Ground Truth Audit

> Full codebase audit performed 2026-02-10 from source code, not existing documentation.

## Methodology

This audit was performed by:
1. Reading every `mod.rs`, `Cargo.toml`, and key source files
2. Mapping all 286 source files and 108 protocol library files
3. Running `cargo build --release`, `cargo clippy --all-targets`, `cargo test` (all files)
4. Running `cargo doc --no-deps --document-private-items`
5. Auditing all 17 handler directories and ~141 handler files
6. Auditing all 10 state managers, the channel actor system, and the sync module
7. Auditing the protocol library (Command enum, parser, CRDT layer)
8. Auditing all services (NickServ, ChanServ, Playback)
9. Reading all 10 database migration files
10. Reading both config files (production and test)

## Build Results

```
cargo build --release: SUCCESS (12 warnings)
cargo test (all 26 files): 91 tests, 0 failures
cargo clippy --all-targets: 81 warnings, 2 errors (test file only)
cargo doc: SUCCESS (25 warnings)
```

## Quantitative Summary

| Metric | Count |
|--------|-------|
| Rust source files (src/) | 286 |
| Lines of code (src/) | 62,814 |
| Protocol library files | 108 |
| Protocol library lines | 26,373 |
| Integration test files | 31 |
| Test lines | 6,683 |
| Total Rust lines | ~95,870 |
| Handler directories | 17 |
| Handler files | ~141 |
| IRC commands handled | ~95+ |
| IRCv3 capabilities advertised | 27 |
| Database migrations | 10 |
| State managers | 10 |
| Channel event variants | 23 |
| Command enum variants | ~100 |
| SASL mechanisms | 3 (PLAIN, EXTERNAL, SCRAM-SHA-256) |
| Service commands (NickServ) | 11 |
| Service commands (ChanServ) | 12 |
| Ban types | 6 (K/D/G/Z/R-line, Shun) |
| Extended ban types | 5 ($a:/$r:/$j:/$x:/$z) |
| Direct Cargo dependencies | ~40 |

## Architecture Verification

### Confirmed Architecture Patterns

1. **Central Matrix** — `Arc<Matrix>` with 10 public manager fields, passed everywhere
2. **Per-channel actors** — Each channel is a Tokio task with `mpsc::Receiver<ChannelEvent>`
3. **Typestate handlers** — `PreRegHandler`/`PostRegHandler`/`UniversalHandler<S>`/`ServerHandler`
4. **Service effects** — Services return `Vec<ServiceEffect>`, never mutate state
5. **Capability tokens** — `Cap<T>` non-Clone/non-Copy, `CapabilityAuthority` is sole mint
6. **CRDT sync** — LWWRegister + AWSet for distributed state, TS6-like S2S protocol
7. **Dual persistence** — SQLite for structured data, Redb for history + always-on
8. **Zero-copy parsing** — `MessageRef<'_>` with `SmallVec<[&str; 15]>` arguments
9. **Lock ordering** — DashMap → Channel RwLock → User RwLock (documented in matrix.rs)
10. **Observer pattern** — UserManager/ChannelManager notify SyncManager of changes

### Confirmed Feature Set

- Full RFC 2812 IRC implementation
- 27 IRCv3 capabilities (including drafts: multiline, chathistory, read-marker, account-registration, relaymsg)
- Server linking (TS6-like with CRDT extensions, tested with 2-server topology)
- Built-in services (NickServ with 11 commands, ChanServ with 12 commands, Playback)
- Native bouncer (multi-session, always-on, auto-away, per-session caps)
- Layered security (IP deny bitmap, rate limiting, spam detection, RBL, cloaking, extended bans)
- Message history (Redb backend, CHATHISTORY with all 6 subcommands)
- Prometheus metrics (optional HTTP endpoint)
- WebSocket support (optional)
- TLS + STARTTLS + STS
- HAProxy PROXY protocol
- Hot config reload (REHASH command)
- Config include directives with glob patterns

### Dead Code / Unused Items

12 build warnings for unused items:
- `hash_password`, `verify_password`, `PasswordHash` imports (password.rs)
- `MAX_HISTORY_LIMIT` import (chathistory)
- `channel_has_mode` function and import
- `join_message_args` function
- `inc_channel_messages_dropped` function (metrics)
- `whowas_maxgroups`, `whowas_groupsize`, `whowas_maxkeep_days` fields
- `configure_whowas` method
- `get_metadata` method
- `GetModes` variant of `ChannelEvent`

### Potential Issues

1. **MutexGuard across await** — 1 clippy warning (could deadlock under contention)
2. **Expressions always false** — 9 clippy warnings (dead branches or logic issues)
3. **Read amount not handled** — 2 clippy errors in `security_channel_freeze` test
4. **Collapsible if statements** — 27 instances (style, not bugs)

## Documentation Generated

| Document | Path | Content |
|----------|------|---------|
| Architecture | `docs/ARCHITECTURE.md` | Full architecture with module descriptions |
| Security | `docs/SECURITY.md` | All security features documented from code |
| S2S Protocol | `docs/S2S_PROTOCOL.md` | Server linking protocol specification |
| Module Map | `docs/MODULE_MAP.md` | Every source file mapped |
| Status | `STATUS.md` | Build/test results, feature matrix |
| README | `README.md` | Project overview and quick start |
| Copilot Instructions | `.github/copilot-instructions.md` | AI coding assistant context |
| This Audit | `AUDIT.md` | Ground truth snapshot |
| Cargo Doc | `target/doc/` | Generated API documentation |
