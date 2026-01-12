# slircd-ng 1.0-alpha Release Plan

**Created**: January 12, 2026  
**Target Release**: v1.0.0-alpha.1  
**Status**: IN PROGRESS

---

## Current State Assessment

| Metric | Value | Status |
|--------|-------|--------|
| **Rust Tests** | 664 passing | ✅ |
| **irctest Compliance** | 92.2% (357/387) | ✅ |
| **TODO/FIXME Markers** | 0 | ✅ |
| **Clippy Warnings** | 0 (with `-D warnings`) | ✅ |
| **Format Check** | Passes | ✅ |
| **CI Pipeline** | Missing | ❌ |
| **Crates Published** | No (monorepo) | ⚠️ |

### What's Already Done (from original ROADMAP_TO_1.0.md)

| Item | Status |
|------|--------|
| Tier 0.1: Core Dependencies | ✅ Resolved (monorepo) |
| Tier 0.2: Rust Edition 2024 | ✅ Resolved (stable 1.85+) |
| Tier 0.3: Reproducible Builds | ✅ Resolved (Cargo.lock) |
| Tier 0.4: Cloak Secret | ✅ Resolved (entropy check) |
| Tier 1.1: Error Handling | ✅ Mostly complete |
| Tier 1.3.1.1: S2S TLS | ✅ Resolved |
| Tier 1.3.1.2: S2S Rate Limiting | ✅ Resolved |
| Tier 1.3.1.3: DNSBL Privacy | ✅ Resolved (RBL service) |
| Security Self-Audit | ✅ Complete |

---

## Alpha Release Definition

An **alpha release** is:
- Feature-complete for core IRC protocol
- Suitable for testing and development use
- NOT recommended for production without supervision
- API may change before 1.0 stable

### Alpha Release Criteria

| Criterion | Required | Status |
|-----------|----------|--------|
| Compiles on stable Rust | Yes | ✅ |
| All unit tests pass | Yes | ✅ |
| irctest >90% | Yes | ✅ 92.2% |
| No panic in handlers | Yes | ✅ |
| CI pipeline exists | Yes | ❌ |
| Basic docs exist | Yes | ⚠️ |
| Tagged release | Yes | ❌ |
| CHANGELOG updated | Yes | ❌ |

---

## Alpha Release Tasks

### Phase 1: CI/CD Pipeline (4 hours)

Create GitHub Actions workflows for:

1. **Build & Test** (`.github/workflows/ci.yml`)
   - Build on Linux/macOS/Windows
   - Run `cargo test`
   - Run `cargo clippy -- -D warnings`
   - Run `cargo fmt -- --check`

2. **Release Automation** (`.github/workflows/release.yml`)
   - Trigger on version tags (`v*`)
   - Build release binaries
   - Create GitHub release with artifacts

### Phase 2: Documentation Cleanup (2 hours)

1. Update README.md with:
   - Clear installation instructions
   - Quick start guide
   - Configuration overview
   - Link to ARCHITECTURE.md

2. Archive/delete obsolete docs:
   - ROADMAP_TO_1.0.md (replaced by this plan)
   - Any superseded documents

3. Update CHANGELOG.md with all changes since last entry

### Phase 3: Version Bump & Tag (1 hour)

1. Update `Cargo.toml` version to `1.0.0-alpha.1`
2. Update crate versions if needed
3. Create annotated tag: `git tag -a v1.0.0-alpha.1`
4. Push tag to trigger release workflow

---

## Post-Alpha Roadmap (1.0-beta)

After alpha release, focus on:

### Beta Blockers (estimate: 40 hours total)

1. **Production Testing** (ongoing)
   - Deploy to staging environment
   - Monitor for 2-4 weeks
   - Fix any discovered issues

2. **Load Testing** (8 hours)
   - Create basic load test harness
   - Document capacity limits
   - Identify bottlenecks

3. **Remaining irctest Fixes** (16 hours, optional)
   - METADATA handler improvements
   - UTF-8 FAIL responses
   - Channel forwarding (+f)

4. **Documentation** (16 hours)
   - Administrator guide
   - Operator manual
   - Deployment examples

### 1.0 Stable Requirements

- 4+ weeks production testing without critical bugs
- Third-party security audit (can be post-1.0)
- Community feedback incorporated
- Performance benchmarks documented

---

## irctest Gaps (30 remaining failures)

Current: **92.2% passing (357/387)**. Target: **95%+**

### Already Fixed in Proto
| Test | Status | Notes |
|------|--------|-------|
| CHATHISTORY TARGETS | ✅ | Fixed timestamp format |
| InvalidUtf8 | ✅ | Preserves command name |
| METADATA | ✅ 9/9 | Binary data supported |
| ROLEPLAY (NPC) | ✅ | Handler implemented |
| RELAYMSG param order | ✅ | Proto fixed |

### Remaining Gaps by Category

| Category | Failures | Effort | Priority |
|----------|----------|--------|----------|
| Bouncer resumption | 7 | VERY HIGH | ❌ 2.0 |
| READQ (16KB limit) | 2 | LOW | ⚠️ Maybe 1.0 |
| Channel +f forwarding | 1 | MEDIUM | ⚠️ Maybe 1.0 |
| Unicode confusables | 1 | MEDIUM | ❌ Post-1.0 |
| ZNC playback | 1 | MEDIUM | ❌ Post-1.0 |
| SAREGISTER | 1 | LOW | ⚠️ Maybe 1.0 |
| RELAYMSG label echo | 1 | LOW | ✅ Fix now |
| Misc protocol edge cases | ~16 | VARIES | Review |

### Quick Wins (Fix for Alpha)

1. **RELAYMSG label echo** - Framework-level labeled-response fix
2. **READQ disconnect** - Return 417 + disconnect instead of continue
3. **SAREGISTER** - NickServ command variant

---

## Not Required for Alpha

These items are explicitly **deferred** to post-alpha:

| Item | Reason | Target |
|------|--------|--------|
| PostgreSQL backend | Nice-to-have, SQLite works | 1.1+ |
| Redis caching | Optimization, not required | 1.1+ |
| Bouncer features | **Major feature, see BOUNCER_DESIGN.md** | 1.2+ |
| ZNC integration | Part of bouncer feature set | 1.2+ |
| Third-party audit | Expensive, do after stable | 1.0+ |
| Crates.io publish | Monorepo works fine | 1.0+ |

---

## Bouncer Feature Roadmap (1.2+)

A comprehensive bouncer design has been created to **exceed Ergo's capabilities**.

See: **[BOUNCER_DESIGN.md](BOUNCER_DESIGN.md)**

### Key Advantages Over Ergo

| Feature | Ergo | slircd-ng (Planned) |
|---------|------|---------------------|
| History Storage | MySQL required | **Redb embedded** |
| Read Markers | 256 max, no sync | **CRDT-synced, unlimited** |
| Federation | ❌ Single instance | ✅ **Multi-server bouncer** |
| Encryption | ❌ | ✅ **AES-256-GCM at rest** |

### Implementation Phases

| Phase | Feature | Duration |
|-------|---------|----------|
| 1 | Multiclient (multiple sessions/nick) | 2 weeks |
| 2 | Always-On persistence | 2 weeks |
| 3 | History playback (better than Ergo) | 1 week |
| 4 | **Distributed bouncer (unique!)** | 3 weeks |
| 5 | Push notifications, encryption | 2 weeks |

**Total**: ~10 weeks post-1.0

---

## Execution Checklist

### Pre-Flight ✅

- [x] Git clean (no uncommitted changes)
- [x] All tests pass
- [x] Clippy passes
- [x] Format passes
- [x] Branch created (`release/1.0-alpha-prep`)

### Phase 1: CI/CD

- [ ] Create `.github/workflows/ci.yml`
- [ ] Create `.github/workflows/release.yml`
- [ ] Verify workflows pass
- [ ] Commit: `ci: add GitHub Actions workflows`

### Phase 2: Documentation

- [ ] Update README.md
- [ ] Delete ROADMAP_TO_1.0.md (replaced)
- [ ] Update CHANGELOG.md
- [ ] Commit: `docs: prepare for alpha release`

### Phase 3: Release

- [ ] Update Cargo.toml versions
- [ ] Merge to main
- [ ] Create tag `v1.0.0-alpha.1`
- [ ] Push tag
- [ ] Verify release created

---

## Success Metrics

| Metric | Target |
|--------|--------|
| CI builds pass | 100% |
| Release binaries available | Linux x86_64, macOS, Windows |
| GitHub release created | Yes |
| Download count (week 1) | >10 |
| Critical bugs reported | 0 |

---

## Timeline

| Phase | Effort | Date |
|-------|--------|------|
| Phase 1: CI/CD | 4 hours | Today |
| Phase 2: Docs | 2 hours | Today |
| Phase 3: Release | 1 hour | Today |
| **Total** | **7 hours** | **January 12, 2026** |

---

## Competitive Position Summary

See [COMPETITIVE_ANALYSIS.md](COMPETITIVE_ANALYSIS.md) for detailed comparison.

### slircd-ng Unique Advantages (No Other IRCd Has)

1. **SASL SCRAM-SHA-256** - Secure password authentication
2. **Zero-copy message parsing** - Unique performance architecture  
3. **CRDT-based federation** - Automatic conflict resolution
4. **Native Prometheus metrics** - Built-in observability
5. **Rust memory safety** - Entire class of security bugs eliminated
6. **Channel actor model** - Per-channel task isolation
7. **Typestate handlers** - Compile-time protocol state enforcement

### Competitive Gaps for 1.0

| Feature | Competitor | Priority | Decision |
|---------|------------|----------|----------|
| GeoIP/ASN lookup | UnrealIRCd | MEDIUM | ⏳ Maybe 1.0 |
| Extended bans (full) | All | MEDIUM | ⏳ Maybe 1.0 |
| Confusables detection | Ergo | MEDIUM | ❌ Post-1.0 |
| JSON-RPC API | Ergo/UnrealIRCd | MEDIUM | ❌ Post-1.0 |
| Advanced spamfilter | UnrealIRCd | MEDIUM | ❌ Post-1.0 |

### Major Features Deferred to 2.0

| Feature | Competitor | Reason |
|---------|------------|--------|
| Multi-device bouncer | Ergo | Requires major rearchitecture |
| Always-on clients | Ergo | Session persistence rewrite |
| Push notifications | Ergo | External service dependency |
| Admin webpanel | UnrealIRCd | Large standalone project |

### Conclusion

**slircd-ng is already competitive** with major IRC servers. Release 1.0 focused on current strengths: IRCv3 compliance, integrated services, federation, security, and unique performance architecture.

---

*This plan replaces ROADMAP_TO_1.0.md which estimated 2400-3200 hours. The project is much further along than that document suggested.*
