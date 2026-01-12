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

## Not Required for Alpha

These items are explicitly **deferred** to post-alpha:

| Item | Reason | Target |
|------|--------|--------|
| PostgreSQL backend | Nice-to-have, SQLite works | 1.1+ |
| Redis caching | Optimization, not required | 1.1+ |
| Bouncer resumption | Advanced feature | 1.2+ |
| ZNC integration | Niche feature | 1.2+ |
| Third-party audit | Expensive, do after stable | 1.0+ |
| Crates.io publish | Monorepo works fine | 1.0+ |

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

*This plan replaces ROADMAP_TO_1.0.md which estimated 2400-3200 hours. The project is much further along than that document suggested.*
