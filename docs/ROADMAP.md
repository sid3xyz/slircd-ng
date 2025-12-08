# slircd-ng Roadmap

> **⚠️ PERMANENT NOTICE:** This software is NEVER production ready. It is a learning exercise and proof-of-concept only.

**Current Focus:** Single Server Finalization (v0.1.0)

---

## Phase 1: Single Server Finalization (Current)

**Goal:** Stabilize the single-server implementation, ensure compliance, and clean up technical debt.

**Completed:**
- ✅ **Typestate Protocol:** Compile-time enforcement of registration state (`UnregisteredState` → `RegisteredState`).
- ✅ **Protocol Observability:** Prometheus metrics and structured tracing.
- ✅ **Capability-Based Security:** Unforgeable `Cap<T>` tokens for sensitive operations.
- ✅ **IRCv3 Support:** `draft/chathistory`, `server-time`, `message-tags`, `batch`, `echo-message`, `labeled-response`, `sasl`.

**Pending:**
- 🚧 **Full Compliance:** Pass 100% of relevant `irctest` suites.
- 🚧 **Documentation:** Ensure all docs match the implementation.
- 🚧 **Cleanup:** Remove unused code and "aspirational" comments.

---

## Phase 2: Operational Maturity (Next)

**Goal:** Add features required for a "real" deployment (even if we have no users).

- **WebSocket Transport:** Support `ws://` and `wss://` for web clients.
- **Event Sourcing:** Capture all state changes in an append-only log.
- **Hot Reload:** Reload configuration and maybe code (via dynamic libs?) without dropping connections.
- **Extended Modes:** Implement `+f`, `+L`, `+q`, etc.

---

## Phase 3: Distributed System (Future)

**Goal:** Scale to multiple nodes.

- **Raft Consensus:** For consistent global state.
- **CRDTs:** For conflict-free channel state replication.
- **Partition Tolerance:** Handle network splits gracefully.
