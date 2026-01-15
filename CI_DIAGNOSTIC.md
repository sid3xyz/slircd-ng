# GitHub Actions CI Diagnostic Guide

**Date**: 2026-01-15  
**Issue**: CI run reported failure

---

## ❓ What Failed?

To diagnose the issue, I need you to provide:

### 1. **Check GitHub Actions Status**
   - URL: https://github.com/sid3xyz/slircd-ng/actions
   - Find the most recent workflow run
   - Which job has the red ❌?
     - `test` (cargo test)
     - `fmt` (cargo fmt)
     - `clippy` (cargo clippy)

### 2. **Copy the Error Message**
   - Click on the failed job
   - Scroll to see the error output
   - Paste the error here

### 3. **Provide Context**
   - Which commit triggered the failure?
   - Was it one of the recent CI fix commits?
   - Or a new commit after that?

---

## 🔧 What We Know

**Local Verification** (just before pushing):
```bash
✅ cargo test --tests --verbose      # All tests passed
✅ cargo fmt -- --check              # All files formatted
✅ cargo clippy --bin slircd --release # No warnings
```

All 3 CI checks pass locally on the machine.

---

## 🎯 Most Likely Issues

### Issue 1: Rust Toolchain
**Symptom**: Build fails with "cannot find crate"
**Cause**: GitHub Actions using different Rust version
**Fix**: We used `dtolnay/rust-toolchain@stable` which auto-updates
**Verify**: What Rust version is GitHub Actions using?

### Issue 2: Dependencies
**Symptom**: "failed to fetch" or "couldn't resolve"
**Cause**: Network issue or crates.io down
**Fix**: Usually transient, retry the workflow
**Action**: Go to Actions → Failed run → Click "Re-run failed jobs"

### Issue 3: Cache Issues
**Symptom**: Build succeeds locally but fails in CI
**Cause**: Stale cache in GitHub Actions
**Fix**: Clear cache
**Action**: 
- Go to Actions → All workflows
- Click "Clear all caches" or specific workflow cache

### Issue 4: Test Server Port Conflict
**Symptom**: Tests timeout or hang
**Cause**: Test servers can't bind to ports in CI environment
**Fix**: May need to configure test isolation
**Current**: Tests use random temp directories, should be OK

### Issue 5: Missing Dependencies (Ubuntu)
**Symptom**: Linker errors, missing libraries
**Cause**: Some native dependencies needed for SQLx
**Fix**: May need to add build-essential or sqlite3 packages
**Action**: Check if SQLite development headers needed

---

## 💡 Next Steps

Please provide:
1. The specific GitHub Actions error message
2. Which job failed (test/fmt/clippy)
3. Whether it's a consistent failure or intermittent

Once you provide the error, I can:
- Identify the root cause
- Create a fix
- Test it locally
- Push the corrected workflow

---

## 🔄 Temporary Workaround

If you need to test changes while we debug:

1. **Test locally** (you can do this without GitHub):
   ```bash
   cargo test --tests --verbose
   cargo fmt -- --check
   cargo clippy --bin slircd --release -- -D warnings
   ```

2. **Only push when all 3 pass** locally

3. **If GitHub fails but local passes**: Usually transient (cache, toolchain)
   - Retry the workflow from GitHub Actions UI
   - Clear cache if persists

---

**Waiting for error details...**
