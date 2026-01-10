# Master Context & Learnings (slircd-ng)

## Current Focus

Architectural enforcement to prevent async deadlocks/contention by ensuring **no DashMap shard-lock guard survives across any `.await`**.

## Truth Timeline (key commits)

- `6495664`: Initial sweep fixing a set of DashMap+await hazards (later discovered to be incomplete).
- `008f370`: Expanded codebase sweep:
  - Introduced `DashMapExt` helpers to make safe access easy/consistent.
  - Refactored handlers/services/managers to clone underlying values (`.value().clone()` / `get_cloned`) before awaiting.
- `aeb022d`: Corrected roadmap note to reflect the expanded sweep and avoid under-reporting scope.

## Learnings / Rules

- **DashMap guards are locks**: `DashMap::get()` / `iter()` return guard types that hold a shard lock.
- **Never await with a guard live**:
  - Avoid patterns like `let x = map.get(..); ... await ...`.
  - Avoid `Option<Ref>::cloned()` / `map(|r| r.clone())` on DashMap results.
- **Preferred patterns**:
  - Use `DashMapExt::get_cloned()` when available.
  - Otherwise: `map.get(key).map(|r| r.value().clone())`.
  - For fanout: collect cloned senders/Arcs into a `Vec<_>` first, then `await` sends/locks.

## Open Work (next reasonable steps)

- Pre-flight sanitation: identify and remove vestigial/legacy code paths where safe.
- Continue scanning for any subtle guard-lifetime leaks (e.g., guards stored in locals spanning control-flow that later awaits).
- Keep quality gates green: `cargo fmt -- --check`, `cargo clippy -- -D warnings`, `cargo test`.
