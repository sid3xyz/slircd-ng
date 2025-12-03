# Documentation Consolidation & Improvement Plan

## Executive Summary

This document outlines a plan to elevate the `slirc` ecosystem documentation to industry standards, comparable to established IRC daemons like **InspIRCd**, **Ergo**, and **Solanum**.

Current documentation is functional but fragmented. While `slirc-proto` has excellent developer documentation, `slircd-ng` lacks end-user references (Commands, Modes), and `slirc-client` is effectively undocumented.

## Industry Standard Benchmarks

| Feature          | InspIRCd         | Ergo            | Solanum          | slircd-ng (Current) |
| ---------------- | ---------------- | --------------- | ---------------- | ------------------- |
| **Config Guide** | ✅ Extensive      | ✅ Manual        | ✅ Reference.conf | ✅ Complete          |
| **Command Ref**  | ✅ Wiki/Docs      | ✅ Manual        | ❌ (Man pages)    | ✅ COMMANDS.md       |
| **Mode Ref**     | ✅ Wiki/Docs      | ✅ Manual        | ❌ (Man pages)    | ✅ MODES.md          |
| **Deployment**   | ✅ Docker/Systemd | ✅ Manual        | ✅ Install.txt    | ❌ Quickstart only   |
| **Architecture** | ✅ Module API     | ✅ Developers.md | ⚠️ Technical docs | ✅ ARCHITECTURE.md   |

## Gap Analysis

### 1. slircd-ng (Server)

- **Missing Command Reference**: Users and operators need a definitive list of supported commands (e.g., `JOIN`, `PART`, `WHOIS`, `MODE`).
- **Missing Mode Reference**: No documentation on supported User Modes (e.g., `+i`, `+o`) or Channel Modes (e.g., `+n`, `+t`, `+m`).
- **Incomplete Config Guide**: `CONFIGURATION.md` misses `[database]`, `[limits]`, and `[security]` sections found in `config.toml`.
- **Missing Deployment Guide**: No instructions for running as a system service (systemd) or behind a reverse proxy.

### 2. slirc-client (Client)

- **Zero Documentation**: `slirc-client/docs/` is empty.
- **Needs**: User manual, keyboard shortcuts, configuration file location/format.

### 3. slirc-proto (Library)

- **Strong**: `HOWTO.md` is comprehensive.
- **Action**: Maintain current quality.

## Action Plan

### Phase 1: Server Reference (High Priority)

1. ✅ **Create `slircd-ng/docs/COMMANDS.md`** — DONE
   - 70+ commands documented with syntax and examples
   - User commands, oper commands, ban commands, STATS letters

2. ✅ **Create `slircd-ng/docs/MODES.md`** — DONE
   - All user modes (7), channel modes (25+), prefixes (5)
   - Extended bans, ISUPPORT tokens, common mode combinations

3. ✅ **Update `slircd-ng/docs/CONFIGURATION.md`** — Already complete
   - All sections documented: server, listen, tls, websocket, database, security, oper, webirc

### Phase 2: Operations & Deployment (Medium Priority)

1. **Create `slircd-ng/docs/DEPLOYMENT.md`**
   - **Systemd Unit**: Provide a standard `.service` file.
   - **Docker**: Explain how to build/run a container.
   - **Reverse Proxy**: Nginx/Caddy config for WebSocket/TLS termination.

2. **Create `slircd-ng/docs/FAQ.md`**
   - Common errors ("Connection refused", "Bad password").
   - Cloaking setup.

### Phase 3: Client Documentation (Low Priority)

1. **Create `slirc-client/docs/MANUAL.md`**
   - First-run setup.
   - Connecting to servers.
   - Managing certificates.
   - Keyboard shortcuts.

## Status

- **Phase 1**: ✅ Complete (COMMANDS.md, MODES.md, CONFIGURATION.md)
- **Phase 2**: ⬜ Pending
- **Phase 3**: ⬜ Pending
