# SLIRC-ng Roadmap

> **Focus**: Production Readiness, IRCv3 Compliance, and Persistence.
> All completed items have been archived. This document tracks **remaining work**.

## Phase 1: Production Visibility (Monitor & Measure)
*Goal: Provide operators with deep visibility into server state.*

- [x] **Runtime Statistics (#1 Priority)**
    - [x] `StatsManager`: Atomic counters for accurate real-time metrics.
    - [x] `LUSERS`: RFC-compliant global user/channel counts.
    - [x] `STATS`: Configurable output (uptime, message rates, connection counts).
- [x] **Operator Audit**
    - [x] Log privileged commands (`OPER`, `KILL`, `KLINE`) to secure audit log.

## Phase 2: IRCv3.3 Compliance (Standardize)
*Goal: 100% compliance with modern client expectations.*

- [x] **Standard Replies (FAIL/WARN/NOTE)**
    - [x] Advertise `standard-replies` capability.
    - [x] Implement `WARN` for deprecated commands.
    - [x] Implement `NOTE` for connection info/MOTD.
- [x] **Setname**
    - [x] Audit `SETNAME` length checks and error responses.
- [x] **Bot Mode (+B)**
    - [x] Ensure `+B` mode is correctly propagated and visible in `WHOIS`.

## Phase 3: Data Safety (Persistence)
*Goal: Zero data loss across restarts.*

- [x] **Channel Persistence (Must Complete)**
    - [x] Wire `src/state/persistence.rs` into `ChannelManager`.
    - [x] Load channels/modes/topics at startup.
    - [x] Save state on mode changes/shutdown.
- [ ] **History Completeness**
    - [ ] Verify `chathistory` v2 compliance.

## Phase 4: Configuration Mastery (Exceeds)
*Goal: Best-in-class configuration experience.*

- [x] **Modular Config**
    - [x] Support `include "conf.d/*.toml"` directive.
- [ ] **Advanced Hot-Reload**
    - [ ] Atomic swap of global config (Limits, Security) via `REHASH`.
    - [ ] Transactional reload (validate *before* apply).

## Phase 5: Ecosystem (Scaling)
*Goal: Network federation and external auth.*

- [ ] **External Authentication**
    - [ ] SASL PLAIN/EXTERNAL via external auth provider script.
- [ ] **Server Links (TS6)**
    - [ ] Validate handshake against UnrealIRCd/InspIRCd.
    - [ ] Stress test S2S routing.

## Phase 6: Advanced Protection
*Goal: Robust defense against flooding and abuse.*

- [x] **Flood Protection (+f)**
    - [x] Implement channel mode `+f <lines>:<seconds>`.
    - [ ] Verify message rate enforcement (Integration Test needed).
- [x] **Spam Defense**
    - [x] Integrate molecular entropy scanner (completed in `spam.rs`).
    - [x] Tune thresholds for false positives.
