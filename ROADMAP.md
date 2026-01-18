# slircd-ng Release Roadmap

> Strategic direction and release timeline for slircd-ng development.

---

## Current Release: v1.0.0-alpha.1

**Status**: ✅ **RELEASED** (January 15, 2026)

### Release Metrics

| Metric | Value | Target | Status |
|--------|-------|--------|--------|
| Rust Tests | 664 passing | 600+ | ✅ |
| irctest Compliance | 376/387 (97.2%) | >90% | ✅ |
| Code Coverage | Unit + Integration | - | ✅ |
| Clippy Warnings | 0 | 0 | ✅ |
| Format Compliance | ✅ | ✅ | ✅ |
| TODO/FIXME Markers | 0 | 0 | ✅ |
| CI/CD Pipeline | GitHub Actions | Working | ✅ |
| Documentation | Complete | Current | ✅ |

### What's Included

#### Protocol Support (60+ handlers)
- ✅ **Core IRC**: NICK, USER, PASS, CAP, QUIT, PING, PONG
- ✅ **Messaging**: PRIVMSG, NOTICE, TAGMSG, ACCEPT
- ✅ **Channels**: JOIN, PART, NAMES, LIST, TOPIC, KICK, INVITE, CYCLE, KNOCK
- ✅ **Queries**: WHO, WHOIS, WHOWAS, ISON, USERHOST, USERIP, USERS
- ✅ **Modes**: MODE (user/channel), CHGIDENT, CHGHOST, VHOST, SETNAME
- ✅ **Moderation**: KLINE, DLINE, XLINE, SHUN, GLINE, KILL, SILENCE, MONITOR
- ✅ **Server**: ENCAP, SJOIN, TMODE, UID, SID, SVINFO, CAPAB
- ✅ **Chat History**: CHATHISTORY (all subcommands)
- ✅ **Account**: REGISTER (via SERVICE), authentication integration
- ✅ **NPC/Roleplay**: NPC command, MODE +E support
- ✅ **Extensions**: METADATA, RELAYMSG, BOUNCER (basic)

#### Infrastructure
- ✅ **Network**: Tokio async, TLS (rustls), WebSocket support
- ✅ **State Sync**: CRDT-based distributed state (now in slirc-proto)
- ✅ **Persistence**: SQLite with 7 migrations, Redb for history
- ✅ **Security**: SCRAM authentication, CertFP, SASL, moderation
- ✅ **Bouncer**: Session resumption architecture (core)

#### Quality
- ✅ **Zero-Copy Parsing**: MessageRef<'a> from slirc-proto
- ✅ **Typestate Handlers**: Compile-time protocol state enforcement
- ✅ **Actor Model**: Per-channel Tokio tasks with bounded mailboxes
- ✅ **Service Effects**: Pure functions for service logic
- ✅ **DashMap Discipline**: Proper lock ordering, no deadlocks

---

## alpha → beta (v1.0.0-beta.1)

**Target**: Q2 2026 | **Effort**: Medium | **Status**: Planning

### Focus Areas

#### 1. **Bouncer Completion** (High Priority)

**Current State**: ✅ **COMPLETE** (January 16, 2026)

| Feature | Status | Work Remaining |
|---------|--------|-----------------|
| Session Resumption | ✅ Complete | - |
| Client Tracking | ✅ Complete | - |
| State Sync | ✅ Complete | - |
| Authentication | ✅ Complete | - |
| Message Fan-out | ✅ Complete | - |
| Self-Echo | ✅ Complete | - |

**Deliverables**:
- [x] Complete client tracking in session manager
- [x] Account-based message fan-out to all sessions
- [x] State synchronization (JOIN/PART/NICK/MODE)
- [x] MONITOR integration for client state
- [x] Session resumption tests (irctest bouncer suite)
- [x] Documentation: Bouncer Architecture

**irctest Impact**: +7 tests (bouncer_resumption suite) ✅ All passing

---

#### 2. **irctest Compliance Push** (92.2% → 95%+)

**Known Gaps** (remaining 30 failing tests):

| Test Suite | Tests | Status | Notes |
|------------|-------|--------|-------|
| bouncer_resumption | 7 | ✅ | All tests passing |
| NPC/Roleplay | 1 | ✅ | MODE +E fully functional |
| READQ | 2 | ✅ | Parser enforces 16KB limit correctly |
| Confusables | 1 | ✅ | PRECIS casemapping handles Unicode |
| ZNC Playback | 1 | ❌ | Requires *playback service (ZNC-specific, out of scope) |
| RELAYMSG | 1 | ✅ | draft/relaymsg working correctly |

**Approach**:
- Focus on bouncer suite (7 tests) - unblocks with completion
- Polish NPC/READQ validators
- Edge case fixes for Unicode
- Defer ZNC (niche extension)

**Target**: 365+ tests passing (94%+)

---

#### 3. **Production Readiness** (Beta Focus)

**Deployability**:
- [x] Load test harness (benchmark suite)
- [ ] Memory profiling under sustained load
- [ ] High-latency network simulation
- [ ] DOS attack resilience verification

**Configuration**:
- [ ] Config file validation/schema
- [x] Graceful reload support (Partial: IP bans, Server Info via `REHASH`)
- [ ] Hot-swap ban lists (KLINE, DLINE)
- [ ] Operator command audit logging

**Observability**:
- [x] Prometheus metrics endpoints (Port 9090 default)
- [ ] Structured logging (JSON output)
- [ ] Runtime statistics (USER count, message rate, etc.)

---

#### 4. **Documentation Excellence**

**By Beta Release**:
- [ ] API Documentation (rustdoc for public modules)
- [ ] Operator's Manual (running slircd-ng)
- [ ] Configuration Guide (all settings explained)
- [ ] Module Reference (handler architecture)
- [ ] Contribution Guide (for external PRs)

---

## beta → stable (v1.0.0)

**Target**: Q3 2026 | **Effort**: High | **Status**: Planning

### Final Push Requirements

1. **irctest**: 360+ tests (93%+) - acceptable for stable
2. **Performance**: <100ms p99 latency at 1K users
3. **Uptime**: Stable for 7+ days under load
4. **Documentation**: Complete, tested examples
5. **Security Audit**: No critical issues

### Required Major Features

- [ ] Server-to-server linking (S2S) full mesh
- [ ] Advanced rate limiting (token bucket)
- [ ] Channel persistence (save/restore full state)
- [ ] IRCv3.3 full compliance

### Stability Focus

- Bug fixes only
- Performance optimization
- Documentation improvement
- Security patches for any issues found

---

## Post-1.0 Vision (v1.1+)

### High-Impact Features

1. **Distributed Resilience**
   - Multi-server redundancy
   - Automatic failover
   - State replication guarantees

2. **Advanced Moderation**
   - Spam detection (ML-assisted)
   - Account takeover prevention
   - Advanced ban expressions

3. **IRCv3 Complete**
   - ORAGONO-specific extensions
   - Multi-line messages
   - Labeled responses for all commands

4. **Performance**
   - Zero-copy everything
   - Custom memory allocator
   - Parallel query processing

---

## Development Methodology

### Release Cycle

```
alpha (92%)  →  beta (94%)  →  stable (93%+)
  ↓              ↓               ↓
 Tests         Perf           Ready
 Docs          Ops             Prod
 Quality       Load
```

### Per-Release Requirements

| Phase | Code | Tests | Docs | CI/CD | Status |
|-------|------|-------|------|-------|--------|
| **Alpha** | MVP | 664 | Core | ✅ | ✅ DONE |
| **Beta** | Complete | 750+ | Full | ✅ | Planned |
| **Stable** | Polish | 750+ | Ref | ✅ | Planned |

### Quality Gates

Every release requires:
1. ✅ `cargo build --release` succeeds
2. ✅ `cargo test` passes (all tests)
3. ✅ `cargo clippy -- -D warnings` passes
4. ✅ `cargo fmt -- --check` passes
5. ✅ irctest suite runs (no regressions)
6. ✅ Documentation updated
7. ✅ CHANGELOG entries added

---

## Key Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|-----------|
| irctest regressions | High | Continuous testing, pre-commit checks |
| Proto dependency changes | Medium | Feature flagging, version pinning |
| Database corruption | High | Backup/restore procedures, testing |
| Memory leaks | Medium | Regular profiling, address sanitizer |
| S2S synchronization | High | Comprehensive CRDT testing |

---

## Success Metrics

### By Beta
- ✅ irctest >94% (360+ tests)
- ✅ Sustained load test (1K users, 7 days)
- ✅ <100ms p99 latency
- ✅ Zero critical security issues
- ✅ Complete operator documentation

### By Stable
- ✅ irctest 93%+ (production acceptable)
- ✅ 30+ days continuous operation
- ✅ External user deployments
- ✅ Positive community feedback
- ✅ Referenced in industry standards

---

## References

- [ARCHITECTURE.md](ARCHITECTURE.md) - System design deep dive
- [README.md](README.md) - Quick start & overview
- [PROTO_REQUIREMENTS.md](PROTO_REQUIREMENTS.md) - Proto blocking issues
- [DEPLOYMENT_CHECKLIST.md](DEPLOYMENT_CHECKLIST.md) - Pre-deployment verification
- [CHANGELOG.md](CHANGELOG.md) - Release notes & history

