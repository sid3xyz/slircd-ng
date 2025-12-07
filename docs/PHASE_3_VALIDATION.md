# Phase 3: Validation & Deployment

> **Target Duration:** 2-3 weeks
> **Primary Agent:** qa-compliance-lead
> **Exit Criteria:** Load test passing, security audit complete, beta deployment running

---

## Overview

Phase 3 validates that slircd-ng is ready for production deployment alongside or replacing SLIRCd. This phase focuses on testing, security, and operational readiness.

---

## 3.1 Load Testing

**Priority:** Critical
**Agent:** qa-compliance-lead (supported by: observability-engineer)
**Estimated Effort:** 1 week

### Objective

Verify slircd-ng handles production-scale load: 1000+ concurrent connections with realistic message patterns.

### Test Scenarios

| Scenario | Connections | Channels | Messages/sec | Duration |
| -------- | ----------- | -------- | ------------ | -------- |
| Baseline | 100         | 10       | 100          | 5 min    |
| Medium   | 500         | 50       | 500          | 10 min   |
| High     | 1000        | 100      | 1000         | 30 min   |
| Spike    | 1000 → 2000 | 100      | 2000         | 5 min    |
| Soak     | 500         | 50       | 200          | 4 hours  |

### Performance Targets

| Metric                   | Target | Acceptable |
| ------------------------ | ------ | ---------- |
| Connection latency (p99) | < 50ms | < 100ms    |
| Message latency (p99)    | < 10ms | < 50ms     |
| Memory per connection    | < 10KB | < 50KB     |
| CPU usage (1000 conn)    | < 25%  | < 50%      |
| No failed connections    | 100%   | > 99.9%    |

### Implementation Steps

```
STEP 1: Set up load testing infrastructure
  FILE: tests/load/Cargo.toml (new)
  - Create separate crate for load tests
  - Dependencies: tokio, slirc-proto, clap, metrics

STEP 2: Implement IRC load generator
  FILE: tests/load/src/generator.rs (new)
  - Configurable connection count
  - Realistic message patterns (PRIVMSG, JOIN, PART)
  - Connection churn (disconnect/reconnect)
  - Report latency percentiles

STEP 3: Create test scenarios
  FILE: tests/load/scenarios/ (new directory)
  - baseline.toml
  - high_load.toml
  - spike.toml
  - soak.toml

STEP 4: Run baseline tests
  COMMAND:
    cargo run -p slircd-load-test -- --scenario baseline \
      --server localhost:6667 --report report.json

STEP 5: Collect metrics during load test
  - Prometheus scrape during test
  - Record memory/CPU via /proc
  - Record network I/O

STEP 6: Analyze results
  FILE: tests/load/src/analyze.rs
  - Parse report.json
  - Generate markdown summary
  - Flag any metrics exceeding thresholds

STEP 7: Optimize bottlenecks
  - Profile with flamegraph
  - Focus on hot paths: message broadcast, nick lookup
  - Iterate until targets met
```

### Load Generator Architecture

```rust
struct LoadConfig {
    server: String,
    connections: usize,
    channels_per_user: usize,
    messages_per_second: f64,
    duration_seconds: u64,
    ramp_up_seconds: u64,
}

struct LoadStats {
    connections_established: AtomicU64,
    connections_failed: AtomicU64,
    messages_sent: AtomicU64,
    messages_received: AtomicU64,
    latencies: Histogram,
}
```

### Verification

```bash
# Run baseline test
cargo run -p slircd-load-test -- --scenario baseline

# Expected output:
# Connections: 100/100 (100%)
# Messages sent: 30000
# Messages received: 30000
# Latency p50: 2.1ms, p99: 12.3ms
# Duration: 5m 0.2s
```

### Files to Create

| File                        | Action | Lines (est.) |
| --------------------------- | ------ | ------------ |
| tests/load/Cargo.toml       | Create | 20           |
| tests/load/src/main.rs      | Create | 100          |
| tests/load/src/generator.rs | Create | 300          |
| tests/load/src/client.rs    | Create | 200          |
| tests/load/src/analyze.rs   | Create | 150          |
| tests/load/scenarios/*.toml | Create | 100          |

---

## 3.2 Security Audit

**Priority:** Critical
**Agent:** security-ops (supported by: capability-security)
**Estimated Effort:** 1 week

### Objective

Identify and remediate security vulnerabilities before production deployment.

### Audit Scope

| Area             | Focus                                  |
| ---------------- | -------------------------------------- |
| Authentication   | SASL implementation, password handling |
| Authorization    | Capability system, oper permissions    |
| Input Validation | Message parsing, command parameters    |
| DoS Resistance   | Rate limiting, resource limits         |
| TLS              | Certificate handling, cipher suites    |
| Data Protection  | Password storage, PII handling         |
| Dependencies     | Known CVEs in dependencies             |

### Implementation Steps

```
STEP 1: Dependency audit
  COMMAND: cargo audit
  COMMAND: cargo deny check
  ACTION: Update or replace any vulnerable dependencies

STEP 2: Static analysis
  COMMAND: cargo clippy --workspace -- -D warnings -D clippy::pedantic
  COMMAND: cargo +nightly udeps
  ACTION: Address all warnings

STEP 3: Fuzz testing
  DIRECTORY: slircd-ng/fuzz/
  TARGETS:
    - Message parser (already in slirc-proto)
    - Command handler dispatch
    - Mode string parser
  COMMAND: cargo +nightly fuzz run parse_command -- -max_total_time=3600

STEP 4: Authentication review
  FILES: src/handlers/auth/*.rs, src/services/nickserv/*.rs
  CHECKLIST:
    - [ ] Passwords never logged
    - [ ] Argon2 for password hashing
    - [ ] Timing-safe comparison
    - [ ] Rate limiting on IDENTIFY
    - [ ] Account lockout after N failures

STEP 5: Authorization review
  FILES: src/caps/*.rs
  CHECKLIST:
    - [ ] Cap tokens non-forgeable
    - [ ] All privileged operations require caps
    - [ ] No privilege escalation paths
    - [ ] Audit logging for cap grants

STEP 6: DoS resistance review
  FILES: src/connection/*.rs, src/handlers/*.rs
  CHECKLIST:
    - [ ] Connection rate limiting
    - [ ] Message rate limiting
    - [ ] Maximum message size enforced
    - [ ] Maximum channels per user
    - [ ] Maximum line length
    - [ ] Timeout on idle connections

STEP 7: TLS review
  FILES: src/server.rs, config.toml
  CHECKLIST:
    - [ ] TLS 1.2+ required
    - [ ] Strong cipher suites only
    - [ ] Certificate validation
    - [ ] OCSP stapling (optional)

STEP 8: Document findings
  FILE: docs/SECURITY_AUDIT.md (new)
  - List all findings with severity
  - Remediation status
  - Recommendations
```

### Security Checklist

```
[ ] No secrets in code or logs
[ ] All user input validated
[ ] SQL injection prevented (parameterized queries)
[ ] Path traversal prevented (if applicable)
[ ] No unsafe Rust blocks (or justified and reviewed)
[ ] Dependencies up to date
[ ] Fuzz testing passed (4+ hours)
[ ] Rate limiting effective
[ ] Authentication secure
[ ] Authorization complete
```

### Files to Create/Modify

| File                   | Action | Lines (est.) |
| ---------------------- | ------ | ------------ |
| docs/SECURITY_AUDIT.md | Create | 200          |
| fuzz/Cargo.toml        | Create | 15           |
| fuzz/fuzz_targets/*.rs | Create | 100          |
| Various fixes          | Modify | Variable     |

---

## 3.3 Beta Deployment

**Priority:** High
**Agent:** release-manager (supported by: security-ops)
**Estimated Effort:** 3 days

### Objective

Deploy slircd-ng alongside existing SLIRCd for real-world validation.

### Deployment Architecture

```
                    ┌─────────────────┐
                    │   Load Balancer │
                    │   (HAProxy)     │
                    └────────┬────────┘
                             │
            ┌────────────────┼────────────────┐
            │                │                │
            ▼                ▼                ▼
     ┌──────────┐     ┌──────────┐     ┌──────────┐
     │ SLIRCd   │     │ SLIRCd   │     │ slircd-ng│
     │ Primary  │     │ Secondary│     │ Beta     │
     │ :6667    │     │ :6667    │     │ :6668    │
     └──────────┘     └──────────┘     └──────────┘
           │                │                │
           └────────────────┼────────────────┘
                            │
                     ┌──────▼──────┐
                     │   Shared    │
                     │   Database  │
                     └─────────────┘
```

### Implementation Steps

```
STEP 1: Prepare beta environment
  - Provision server (same specs as production)
  - Install slircd-ng binary
  - Configure with production-like settings
  - Set up monitoring (Prometheus + Grafana)

STEP 2: Database migration
  FILE: scripts/migrate_from_slircd.sh (new)
  - Export NickServ/ChanServ data from SLIRCd
  - Transform to slircd-ng schema
  - Import to slircd-ng database

STEP 3: Configure load balancer
  - Add slircd-ng as backend
  - Route 5% of new connections to beta
  - Gradual increase: 5% → 10% → 25% → 50%

STEP 4: Monitor beta
  CHECKLIST:
    - [ ] Connection success rate
    - [ ] Error rate in logs
    - [ ] Latency compared to SLIRCd
    - [ ] Memory usage stability
    - [ ] User complaints (support tickets)

STEP 5: Rollback plan
  FILE: docs/ROLLBACK.md (new)
  - HAProxy config to remove slircd-ng
  - Expected time to rollback: < 1 minute
  - Data sync considerations

STEP 6: Gradual promotion
  - If metrics acceptable after 1 week at 5%: increase to 10%
  - Continue until 50% or issues found
  - Document all observations
```

### Files to Create

| File                           | Action | Lines (est.) |
| ------------------------------ | ------ | ------------ |
| scripts/migrate_from_slircd.sh | Create | 100          |
| deploy/haproxy.cfg             | Create | 50           |
| docs/ROLLBACK.md               | Create | 50           |
| docs/BETA_OBSERVATIONS.md      | Create | ongoing      |

---

## 3.4 Migration Tools

**Priority:** Medium
**Agent:** server-engineer
**Estimated Effort:** 3 days

### Objective

Provide tools for networks to migrate from SLIRCd (or other IRCds) to slircd-ng.

### Migration Path

1. **Export** - Extract data from existing IRCd
2. **Transform** - Convert to slircd-ng format
3. **Import** - Load into slircd-ng database
4. **Validate** - Verify data integrity
5. **Switch** - Update DNS/firewall to point to new server

### Implementation Steps

```
STEP 1: SLIRCd data exporter
  FILE: tools/slircd-export/main.rs (new)
  - Read SLIRCd SQLite database
  - Export: nicks, channels, bans, operators
  - Output: JSON format

STEP 2: Data transformer
  FILE: tools/slircd-migrate/transform.rs (new)
  - Read export JSON
  - Transform to slircd-ng schema
  - Handle differences (e.g., permission mappings)

STEP 3: slircd-ng importer
  FILE: tools/slircd-migrate/import.rs (new)
  - Read transformed JSON
  - Insert into slircd-ng SQLite
  - Report: imported counts, skipped records

STEP 4: Validation tool
  FILE: tools/slircd-migrate/validate.rs (new)
  - Compare source and destination databases
  - Report discrepancies
  - Check referential integrity

STEP 5: Generic IRCd support
  FILE: tools/ircd-migrate/
  - Anope/Atheme export support
  - UnrealIRCd database support
  - InspIRCd database support
```

### Files to Create

| File                                  | Action | Lines (est.) |
| ------------------------------------- | ------ | ------------ |
| tools/slircd-export/Cargo.toml        | Create | 15           |
| tools/slircd-export/src/main.rs       | Create | 200          |
| tools/slircd-migrate/Cargo.toml       | Create | 15           |
| tools/slircd-migrate/src/main.rs      | Create | 100          |
| tools/slircd-migrate/src/transform.rs | Create | 150          |
| tools/slircd-migrate/src/import.rs    | Create | 150          |
| tools/slircd-migrate/src/validate.rs  | Create | 100          |

---

## Phase 3 Completion Checklist

- [ ] 3.1 Load Testing
  - [ ] Load test framework created
  - [ ] Baseline test passing (100 conn)
  - [ ] Medium load passing (500 conn)
  - [ ] High load passing (1000 conn)
  - [ ] Spike test passing (1000→2000)
  - [ ] Soak test passing (4 hours)
  - [ ] All performance targets met
  - [ ] Bottlenecks identified and optimized

- [ ] 3.2 Security Audit
  - [ ] Dependency audit clean
  - [ ] Static analysis passing
  - [ ] Fuzz testing complete (4+ hours)
  - [ ] Authentication review complete
  - [ ] Authorization review complete
  - [ ] DoS resistance verified
  - [ ] TLS configuration hardened
  - [ ] No critical findings open

- [ ] 3.3 Beta Deployment
  - [ ] Beta environment provisioned
  - [ ] Database migration tested
  - [ ] Load balancer configured
  - [ ] Monitoring active
  - [ ] Rollback plan documented
  - [ ] 5% traffic running successfully
  - [ ] 1 week observation complete

- [ ] 3.4 Migration Tools
  - [ ] SLIRCd exporter complete
  - [ ] Data transformer complete
  - [ ] slircd-ng importer complete
  - [ ] Validation tool complete
  - [ ] End-to-end migration tested

---

## Agent Handoff Notes

When assigning this phase to AI agents:

1. **Load testing is critical** - Do not skip any scenario
2. **Security audit requires thoroughness** - Check every code path
3. **Beta deployment is gradual** - Never rush traffic increases
4. **Document everything** - Observations become institutional knowledge
5. **Rollback must work** - Test rollback before increasing traffic

### Recommended Prompts for GPT-5.1-codex-max

```
TASK: Create load testing framework for slircd-ng
CONTEXT: Read PHASE_3_VALIDATION.md section 3.1
FILES TO READ FIRST:
  - slirc-proto/src/lib.rs (protocol library)
  - tests/ (existing test patterns)
CONSTRAINTS:
  - Use tokio for async connections
  - Use slirc-proto for IRC parsing
  - Output metrics compatible with Prometheus
OUTPUT: Separate crate in tests/load/ with scenarios
```

```
TASK: Perform security audit of slircd-ng
CONTEXT: Read PHASE_3_VALIDATION.md section 3.2
FILES TO READ FIRST:
  - src/handlers/auth/
  - src/services/nickserv/
  - src/caps/
CONSTRAINTS:
  - Run cargo audit and cargo deny
  - Fuzz test for minimum 4 hours
  - Document all findings with severity
OUTPUT: docs/SECURITY_AUDIT.md with findings and remediations
```

---

## Production Readiness Certification

Before considering slircd-ng production-ready:

| Requirement                 | Status | Evidence             |
| --------------------------- | ------ | -------------------- |
| Load test 1000+ connections | ⬜      | Load test report     |
| Security audit no critical  | ⬜      | SECURITY_AUDIT.md    |
| Beta deployment 1 week      | ⬜      | BETA_OBSERVATIONS.md |
| Migration tools tested      | ⬜      | Migration test log   |
| Documentation complete      | ⬜      | docs/ directory      |
| irctest ≥ 80%               | ⬜      | irctest report       |
| All Phase 1-2 complete      | ⬜      | Roadmap checkboxes   |

**Certification Date:** ___________
**Certified By:** ___________
