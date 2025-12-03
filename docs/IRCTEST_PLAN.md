# Plan: irctest CI Integration for slircd-ng

## Goal
Integrate `irctest` (the standard IRC compliance test suite) into the `slircd-ng` CI pipeline to ensure RFC compliance and prevent regressions.

## Steps

### 1. Environment Setup
- [x] Clone `irctest` repository to `/home/straylight/irctest`.
- [ ] Create a Python virtual environment and install dependencies (`requirements.txt`).

### 2. Test Runner Script (`slircd-ng/scripts/run_irctest.sh`)
Create a shell script to automate the testing process locally and in CI.
- **Build**: Compile `slircd-ng` in release mode.
- **Configure**: Generate or use a specific `config.irctest.toml` (based on `config.test.toml`) with:
    - Random/Fixed port to avoid conflicts.
    - In-memory database (if supported) or temp file DB.
    - Disabled rate limits (crucial for test suites).
    - Plaintext listener (no TLS for basic tests).
- **Execution**:
    - Start `slircd-ng` in the background.
    - Wait for port availability.
    - Run `pytest` from `irctest` using the `external_server` controller.
    - Capture logs and exit codes.
- **Cleanup**: Ensure `slircd-ng` process is killed after tests.

### 3. CI Workflow (`slircd-ng/.github/workflows/compliance.yml`)
Create a GitHub Actions workflow.
- **Triggers**: Push to `main`, Pull Requests.
- **Job**: `compliance`
    - Checkout `slircd-ng`.
    - Checkout `irctest` (using `actions/checkout` or git clone).
    - Install Rust (stable).
    - Install Python 3.x.
    - Run `slircd-ng/scripts/run_irctest.sh`.
    - Upload test results/logs as artifacts.

### 4. Verification
- Run the script locally to ensure `slircd-ng` passes (or at least runs) the test suite.
- Identify immediate failures/blockers (e.g., missing capabilities).

## Dependencies
- `python3`, `pip`
- `cargo`
- `irctest` (cloned)
