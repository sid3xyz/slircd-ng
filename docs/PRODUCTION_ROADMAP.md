# slircd-ng Production Roadmap

> **⚠️ PERMANENT NOTICE:** This software is NEVER production ready. It is a learning exercise and proof-of-concept only.

**Last Updated:** December 2025
**Target:** Feature parity with [SLIRCd](https://github.com/sid3xyz/slircd) v3.11.0

---

## Executive Summary

This document outlines the path from slircd-ng v0.1.0 to production-grade feature parity with SLIRCd. The work is organized into three phases over an estimated 7-10 weeks.

| Phase | Focus | Duration | Key Deliverables |
|-------|-------|----------|------------------|
| 1 | Core Feature Gaps | 3-4 weeks | WebSocket, Extended Modes, Hot Reload, irctest 80%+ |
| 2 | Operational Maturity | 2-3 weeks | Event Sourcing, Plugin System, State Replay |
| 3 | Validation & Deployment | 2-3 weeks | Load Testing, Security Audit, Beta Deployment |

---

## Current State

### slircd-ng v0.1.0 (12,658 lines)

**Completed Innovations:**
- ✅ Innovation 1: Typestate Protocol (compile-time registration enforcement)
- ✅ Innovation 2: CRDT Primitives (Phase 1 - LamportClock, GSet, LwwRegister, ORSet)
- ✅ Innovation 3: Protocol Observability (Prometheus metrics, structured tracing)
- ✅ Innovation 4: Capability-Based Security (unforgeable `Cap<T>` tokens)

**Architecture:**
- Actor model for channels (mpsc, no `RwLock` on hot path)
- DashMap for concurrent user/channel registries
- SQLite persistence for NickServ/ChanServ/bans
- AtomicCapabilities for IRCv3 negotiation

**Protocol Support:**
- IRC 2812 core commands
- Basic channel modes (+o, +v, +b, +i, +k, +l, +m, +n, +t, +s, +p)
- IRCv3.2: CAP, SASL PLAIN, message-tags, echo-message, server-time
- NickServ (REGISTER, IDENTIFY, DROP, INFO, SET)
- ChanServ (REGISTER, DROP, FLAGS, INFO)

### SLIRCd v3.11.0 (1.1M lines) - Production Reference

**Additional Features to Implement:**
- WebSocket transport (ws://, wss://)
- Extended channel modes (+f flood protection, +L link, +q quiet, +R registered-only, +M moderated-registered, +N no-nick-change, +c no-colors)
- Hot reload via SIGHUP
- Event sourcing with SQLite audit trail
- Plugin system (services, custom handlers)
- State replay for debugging
- 20+ operator types with granular permissions

---

## Phase Documents

Detailed implementation guides are organized by phase:

| Document | Description |
|----------|-------------|
| [PHASE_1_CORE_GAPS.md](./PHASE_1_CORE_GAPS.md) | WebSocket, extended modes, hot reload, irctest compliance |
| [PHASE_2_OPERATIONAL.md](./PHASE_2_OPERATIONAL.md) | Event sourcing, plugin system, state replay |
| [PHASE_3_VALIDATION.md](./PHASE_3_VALIDATION.md) | Load testing, security audit, deployment |

---

## AI Agent Orchestration

This project uses specialized AI agents for development. See:
- [agents/README.md](../../agents/README.md) - Agent team overview
- [agents/*.prompt.md](../../agents/) - Individual agent specifications

### Recommended Agent Assignments

| Phase | Primary Agent | Supporting Agents |
|-------|--------------|-------------------|
| 1.1 WebSocket | server-engineer | protocol-architect |
| 1.2 Extended Modes | server-engineer | qa-compliance-lead |
| 1.3 Hot Reload | server-engineer | security-ops |
| 1.4 irctest | qa-compliance-lead | protocol-architect |
| 2.1 Event Sourcing | server-engineer | observability-engineer |
| 2.2 Plugin System | server-engineer | security-ops |
| 3.1 Load Testing | qa-compliance-lead | observability-engineer |
| 3.2 Security Audit | security-ops | capability-security |

---

## Success Metrics

### Phase 1 Exit Criteria
- [ ] WebSocket transport operational (ws:// and wss://)
- [ ] All extended modes implemented and tested
- [ ] SIGHUP hot reload working without client disruption
- [ ] irctest compliance ≥ 80%

### Phase 2 Exit Criteria
- [ ] Event sourcing capturing all state changes
- [ ] Plugin system loading custom handlers
- [ ] State replay from any point in time
- [ ] Deployment documentation complete

### Phase 3 Exit Criteria
- [ ] 1000+ concurrent connection load test passing
- [ ] Security audit with no critical findings
- [ ] Beta deployment running parallel to SLIRCd
- [ ] Migration tools for existing networks

---

## Consolidated Reference

Previous planning documents have been superseded by this roadmap:

| Superseded Document | Status | Notes |
|---------------------|--------|-------|
| `IMPLEMENTATION_PLAN.md` | ✅ Complete | Channel actor migration finished |
| `REF_ARCH_PLAN.md` | ✅ Complete | Read/write operation refactor done |
| `REFACTORING_PLAN.md` | ✅ Complete | Priority 1-5 refactoring done |
| `TODO.md` | ✅ Complete | All items addressed |
| `ISSUES_PRIORITY.md` | ✅ Complete | 0 open issues |

Innovation status:
- `INNOVATION_1_TYPESTATE_PROTOCOL.md` - ✅ Complete
- `INNOVATION_2_CRDT_SERVER_LINKING.md` - Phase 1 ✅, Phase 2-3 deferred (S2S scope)
- `INNOVATION_3_PROTOCOL_OBSERVABILITY.md` - ✅ Complete
- `INNOVATION_4_CAPABILITY_SECURITY.md` - ✅ Complete
