# Copilot Instructions for slircd-ng

High-performance IRC daemon with zero-copy parsing. Public domain (Unlicense).

## Quick Reference

```bash
cargo build --release           # Build
cargo test                      # Tests (664+)
cargo clippy -- -D warnings     # Lint (must pass)
cargo fmt -- --check            # Format check
```

## Constraints

| Rule | Requirement |
|------|-------------|
| Edition | Rust 2024 (stable 1.85+) |
| Errors | Use `?` propagation, no `unwrap()` in handlers |
| Parsing | Zero-copy via `MessageRef<'a>` from slirc-proto |
| RFC | Strict RFC 1459, 2812, IRCv3 compliance |
| Proto-First | Fix slirc-proto first, never workaround proto bugs |

## Architecture (Brief)

- **Handlers**: Typestate pattern (PreRegHandler, PostRegHandler, UniversalHandler)
- **State**: `Arc<Matrix>` with 7 domain managers, DashMap for concurrent access
- **Channels**: Actor model with bounded mailboxes
- **Services**: Pure effect functions returning `ServiceEffect` vectors

## Critical Patterns

```rust
// Zero-copy: extract data before .await
async fn handle(&self, ctx: &mut Context<'_, S>, msg: &MessageRef<'_>) -> HandlerResult {
    let nick = msg.arg(0).map(|s| s.to_string()); // Extract first
    Ok(())
}

// IRC case: use proto utilities
use slirc_proto::{irc_to_lower, irc_eq};
let nick_lower = irc_to_lower(&nick); // NOT nick.to_lowercase()

// DashMap: short locks, clone before await
if let Some(user) = matrix.user_manager.get_user(&uid) {
    let nick = user.nick.clone();
} // Lock released before async work
```

## Anti-Patterns

- `Command::Raw` for known commands → Add variant to proto
- `.unwrap()` in handlers → Use `?`
- `std::to_lowercase()` on IRC strings → Use `irc_to_lower()`
- Holding DashMap locks across `.await` → Clone data first
- Empty `Ok(())` stubs → Use `todo!()` to panic if hit
- Traits with one impl → Delete the trait, use struct directly
- `Arc<RwLock<...>>` without need → Add when compiler demands
- New crates for std-solvable problems → Use std first

## Development Mode

This is active development. Prioritize:
- Working logic over abstraction
- Fast iteration over hardening
- Simplicity over enterprise patterns
- `todo!()` panics over silent failures

## Git Policy

- Use `origin` only (no upstream remote)
- Do not sync from upstream without explicit permission

## Docs Reference

- `ROADMAP.md`: Release timeline
