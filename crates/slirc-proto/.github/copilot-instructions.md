# Copilot Instructions for slirc-proto

High-performance Rust library for IRC protocol parsing with full IRCv3 support. Public domain (Unlicense).

## Quick Reference

```bash
cargo build --all-features    # Build with all features
cargo test --all-features     # Run all tests
cargo clippy --all-features -- -D warnings  # Lint (must pass)
cargo fmt -- --check          # Format check
```

## Constraints

| Rule | Requirement |
|------|-------------|
| MSRV | Rust 1.70+ |
| Linting | `#![deny(clippy::all)]` — zero warnings |
| Errors | Use `?` propagation, never `unwrap()` in lib code |
| Enums | `#[non_exhaustive]` on public enums that may grow |

## Architecture

| Component | Pattern |
|-----------|---------|
| Parsing | `MessageRef<'a>` zero-copy, nom combinators |
| Serialization | `write_to(&mut impl fmt::Write)` to avoid allocations |
| Transport | `ZeroCopyTransport<S>` for hot loop, yields `MessageRef<'_>` |
| Errors | `ProtocolError` (transport), `MessageParseError` (parsing) |

## Testing

- Round-trip tests for all commands (parse → serialize → parse)
- Property tests with `proptest` for parser fuzzing
