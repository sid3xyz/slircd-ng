# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2025-12-08

### Added

- **Typestate Protocol** (Innovation 1): Compile-time enforcement of registration state (`UnregisteredState` → `RegisteredState`).
- **CRDT Primitives** (Innovation 2): Initial implementation of `LamportClock`, `GSet`, `LwwRegister`, and `ORSet`.
- **Protocol Observability** (Innovation 3): Prometheus metrics and structured tracing for all protocol events.
- **Capability-Based Security** (Innovation 4): Unforgeable `Cap<T>` tokens for sensitive operations.
- **IRCv3 Support**:
  - `draft/chathistory`
  - `server-time`
  - `message-tags`
  - `batch`
  - `echo-message`
  - `labeled-response`
  - `sasl` (PLAIN)
- **Persistence**: SQLite backend for user accounts, channel registrations, and bans.
- **Architecture**: Actor model for channels, DashMap for concurrent registries.
