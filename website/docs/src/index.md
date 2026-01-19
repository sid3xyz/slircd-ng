# Introduction to SLIRCd

**Straylight IRC Daemon - Next Generation**

> **A high-performance, distributed IRC daemon** written in Rust with modern architecture: zero-copy parsing, actor-based channels, and CRDT state synchronization.

SLIRCd is a research project aiming to explore the limits of modern IRC architecture. It focuses on safety, performance, and distributed consistency.

## Key Features

- **Zero-Copy Message Parsing**: Direct buffer borrowing via `MessageRef<'a>` eliminates allocation overhead.
- **Actor Model Channels**: Each channel runs as its own Tokio task with bounded message queues—no global locks on hot path.
- **Typestate Protocol Enforcement**: Compile-time state machine via trait system prevents invalid state transitions.
- **CRDT-Based Sync**: Conflict-free replicated data types enable multi-server linking without coordination.
- **Bouncer Integration**: Native session management allows connection resumption and history replay.

## Status

SLIRCd is currently in **v1.0.0-rc.1**. It passes 92% of the standard irctest suite.
