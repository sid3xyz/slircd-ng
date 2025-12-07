# slircd-ng: The Research IRC Daemon

> **⚠️ AI RESEARCH EXPERIMENT: This software is a proof-of-concept developed using AI agents. It is NEVER production ready. Do not deploy, do not use for any real network.**

`slircd-ng` is a next-generation IRC server written in Rust. It serves as a testbed for radical architectural experiments in the IRC protocol, prioritizing correctness, type safety, and distributed consistency over backward compatibility or legacy support.

## AI-Driven Development

This project represents an **execution of a test using AI to develop software**. It demonstrates the capacity of AI agents to:
1.  Design and implement complex systems (Actors, CRDTs).
2.  Enforce strict type safety (Typestate Pattern).
3.  Maintain high test coverage and specification compliance.

## Development Philosophy: NO USERS, NO MERCY

- **Zero Users**: We have no users to support. Breaking changes are encouraged if they improve the architecture.
- **No Workarounds**: If a feature requires changing 50 files to be "correct", we change 50 files. We do not build compatibility shims.
- **Aggressive Refactoring**: Legacy code is deleted immediately. There are no `_old` or `_deprecated` modules.
- **Innovation First**: We build reference implementations of new ideas, not just another IRCd.

## The 4 Innovations

`slircd-ng` is built around four core architectural innovations:

### 1. Typestate Protocol (Innovation 1)

**Status:** ✅ Complete

We enforce the IRC protocol state machine at **compile time**.

- **TypedContext**: Handlers receive a `TypedContext<Registered>` that guarantees the connection is fully authenticated.
- **Zero Runtime Checks**: It is impossible to compile code that checks `if !registered` in a hot path.
- **Split Traits**: `PreRegHandler`, `PostRegHandler`, and `UniversalHandler` traits ensure commands are only dispatched in valid states.

### 2. CRDT Server Linking (Innovation 2)

**Status:** 🚧 In Progress (Primitives Complete)

We are replacing the traditional spanning-tree linking protocol with **Conflict-free Replicated Data Types (CRDTs)**.

- **Mathematically Proven Convergence**: Any two servers that receive the same updates will converge to the same state.
- **Primitives**: `LamportClock`, `GSet`, `LwwRegister`, and `ORSet` are implemented.
- **Goal**: True multi-master replication for channels and users.

### 3. Protocol-Aware Observability (Innovation 3)

**Status:** ✅ Complete

Observability is a first-class citizen, not an afterthought.

- **Structured Logging**: Every log line carries a `trace_id` and `span_id`.
- **Metrics**: Prometheus metrics for command latency, channel fanout, and error rates are built-in.
- **Command Timer**: RAII guards automatically record duration for every command dispatch.

### 4. Capability-Based Security (Innovation 4)

**Status:** ✅ Complete

We replaced `if is_oper()` checks with unforgeable **Capability Tokens**.

- **Token Auth**: A function can only perform a privileged action (like `KILL` or `SHUN`) if it possesses the corresponding `Cap<T>` token.
- **Unforgeable**: Tokens can only be minted by the `CapabilityAuthority`.
- **Granular**: Permissions are typed (e.g., `Cap<KillCap>`, `Cap<ShunCap>`), preventing privilege escalation.

## Architecture

- **Zero-Copy Parsing**: Built on `slirc-proto`, using `MessageRef<'a>` to parse commands without allocation.
- **Lock-Free State**: Uses `DashMap` for high-concurrency access to the global user/channel registry.
- **Actor Model**: Channels are implemented as actors (Tokio tasks) to serialize updates and prevent race conditions.
- **Async I/O**: Fully asynchronous networking stack based on `tokio`.

## Getting Started

### Prerequisites

- Rust (latest stable)
- Python 3 (for compliance testing)

### Building

```bash
cargo build -p slircd-ng
```

### Running

```bash
cargo run -p slircd-ng -- config.toml
```

### Testing

Run the unit test suite:

```bash
cargo test -p slircd-ng
```

Run the compliance suite (requires `irctest` setup):

```bash
./scripts/run_compliance.sh
```

## Directory Structure

- `src/handlers/`: Command handlers (organized by typestate phase).
- `src/state/`: Core state machines and CRDT definitions.
- `src/caps/`: Capability security system.
- `src/crdt/`: CRDT primitives.
- `docs/innovations/`: Detailed design documents for the 4 innovations.

## License

Unlicense.
