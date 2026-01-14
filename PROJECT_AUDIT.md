# Strict Execution Plan (Temporary Working Doc — remove after merge)

Policy: No PRs. No new branches until current work is finished, merged locally into main, and all other branches are reconciled.

## Branch State (now)
- Current: feat/bouncer-multiclient
- Main: main (remote: origin/main)
- Others: none present (local/remote list is minimal)

## Step 1 — Finalize Current Branch (no PRs)
- Goal: Finish and stabilize `feat/bouncer-multiclient`, then merge locally into `main` with fast-forward only.
- Actions:
  - Rebase (or confirm up-to-date) on `main`.
  - Run format, clippy (deny warnings), and full tests.
  - Merge locally with `--ff-only` and delete the feature branch locally.
- Acceptance:
  - `cargo fmt -- --check` passes (or repo formatted).
  - `cargo clippy -- -D warnings` passes with zero new exceptions.
  - `cargo test` passes across workspace.
  - `main` contains all bouncer/multiclient commits; feature branch deleted locally.
- Commands:
  ```bash
  git fetch origin --prune
  git checkout main && git pull --ff-only origin main
  git checkout feat/bouncer-multiclient && git rebase main
  cargo fmt -- --check || cargo fmt
  cargo clippy -- -D warnings
  cargo test --workspace
  git checkout main && git merge --ff-only feat/bouncer-multiclient
  git branch -d feat/bouncer-multiclient
  ```

## Step 2 — Reconcile Branches
- Goal: Converge to a single clean `main` reflecting current work; keep no stale branches.
- Actions:
  - Inventory local and remote branches.
  - Drop redundant/stale branches; no new branches created during reconciliation.
- Commands:
  ```bash
  git branch -a --no-color
  # delete local stale branches explicitly, e.g.:
  # git branch -D <stale-branch>
  ```

## Step 3 — Re-run Full Project Audit (on merged main)
- Goal: Verify correctness, completeness, and policy compliance after the merge.
- Checks:
  - Bouncer/Multiclient end-to-end: SASL attach → `set_reattach_info()` → registration copy → post-register autoreplay.
  - Autoreplay: JOIN echo + topic snapshot + CHATHISTORY replay with `server-time`/`msgid`; capability gating verified.
  - Read markers: in-memory manager hooked into autoreplay; persistence plan documented.
  - Services: `*playback` integration and config gating (e.g., `history.znc-maxmessages`).
  - Clippy/format/tests: all green with `-D warnings`.
  - Proto-first: no daemon workarounds; any proto gaps captured in PROTO_REQUIREMENTS.md.
  - Zero-cruft: no orphaned TODO/FIXME; no legacy docs left behind (remove this file after audit wrap-up).
- Docs to update:
  - PROTO_REQUIREMENTS.md (resolved vs. open proto items)
  - Master_Context_&_Learnings.md (Phase 3.x learnings)
  - This file: remove once audit tasks are tracked elsewhere (ROADMAP/CHANGELOG)

## Step 4 — New Branch For Audit Findings (after Steps 1–2 complete)
- Policy: Create one branch only after merge/reconciliation completes.
- Initial scope:
  - P0: Labeled-response echo reliability (ensure tags are echoed consistently).
  - P0: Replay capability gating hardening.
  - P1: Persist read markers (Redb) keyed by account/device/target with compaction.
  - P1: Optional NAMES bootstrap during autoreplay.
- Naming: `feat/audit-followups-YYYY-MM-DD`.

## Next Actions — Today
- Execute Step 1 to completion (rebase confirm → fmt/clippy/tests → fast-forward merge → delete branch), then proceed to Step 3 audit.
