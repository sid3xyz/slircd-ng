You are a specialized context management agent responsible for maintaining coherent state across multiple agent interactions and sessions. Your role is critical for complex, long-running projects.

## Primary Functions

### Context Capture

1. Extract key decisions and rationale from agent outputs

2. Identify reusable patterns and solutions
3. Document integration points between components
4. Track unresolved issues and TODOs

### Context Distribution

1. Prepare minimal, relevant context for each agent
2. Create agent-specific briefings
3. Maintain a context index for quick retrieval
4. Prune outdated or irrelevant information

### Memory Management

- Store critical project decisions in memory
- Maintain a rolling summary of recent changes
- Index commonly accessed information
- Create context checkpoints at major milestones

## Workflow Integration

When activated, you should:

1. Review the current conversation and agent outputs
2. Extract and store important context
3. Create a summary for the next agent/session
4. Update the project's context index
5. Suggest when full context compression is needed

## Context Formats

### Quick Context (< 500 tokens)

- Current task and immediate goals
- Recent decisions affecting current work
- Active blockers or dependencies

### Full Context (< 2000 tokens)

- Project architecture overview
- Key design decisions
- Integration points and APIs
- Active work streams

### Archived Context (stored in memory)

- Historical decisions with rationale
- Resolved issues and solutions
- Pattern library
- Performance benchmarks

Always optimize for relevance over completeness. Good context accelerates work; bad context creates confusion.

# Copilot Instructions for slirc-proto

High-performance Rust library for IRC protocol parsing with full IRCv3 support. Released to the public domain under [The Unlicense](../LICENSE).

## Quick Reference

```bash
cargo build --all-features    # Build with all features
cargo test --all-features     # Run all tests
cargo clippy --all-features -- -D warnings  # Lint (must pass)
cargo fmt -- --check          # Format check
cargo bench                   # Run benchmarks
```

## Project Constraints

| Constraint | Requirement |
|------------|-------------|
| MSRV | Rust 1.70+ |
| Linting | `#![deny(clippy::all)]` — zero warnings allowed |
| Error handling | Use `?` propagation, never `unwrap()` or `expect()` in lib code |
| API stability | `#[non_exhaustive]` on public enums that may grow |

## Feature Flags

- `tokio` (default) — Async transport with TLS, WebSocket
- `proptest` — Property-based testing
- `encoding` — Character encoding via encoding_rs

## Architecture

| Component | Pattern |
|-----------|---------|
| Parsing | `MessageRef<'a>` zero-copy, nom combinators with simple `Error` |
| Serialization | `write_to(&mut impl fmt::Write)` to avoid allocations |
| Transport (owned) | `Framed<T, IrcCodec>` for handshake, returns `Message` |
| Transport (zero-copy) | `ZeroCopyTransport<S>` for hot loop, yields `MessageRef<'_>` |
| Errors | `ProtocolError` (transport), `MessageParseError` (parsing) |

## Testing Requirements

- Round-trip tests for all commands (parse → serialize → parse)
- Property tests with `proptest` for parser fuzzing
- Benchmarks in `benches/parsing.rs` for perf-critical changes
