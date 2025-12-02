# SLIRC Migration TODO

## Phase 1: CHATHISTORY Migration ✅ COMPLETE

- [x] 1.1 Create db/history.rs - Port MessageEnvelope and storage logic
- [x] 1.2 Update Database Schema - Add message_history table (003_history.sql)
- [x] 1.3 Implement ChatHistoryHandler - LATEST, BEFORE, AFTER, BETWEEN, AROUND
- [x] 1.4 Wire into PrivmsgHandler - Store channel messages on send

## Phase 2: Security Hardening ✅ COMPLETE

- [x] 2.1 Add BanCache struct to security module
- [x] 2.2 Load bans on startup (K-lines, D-lines, G-lines, Z-lines)
- [x] 2.3 Add ban_cache field to Matrix
- [x] 2.4 Wire ban check into Gateway for TLS/plain/WebSocket connections
- [x] 2.5 Add ban check in handshake after USER (for user@host patterns)

## Phase 3: Cleanup ✅ COMPLETE

- [x] 3.1 Remove dead_code annotations for now-used methods (ban models, get_active_*)
- [x] 3.2 Wire remaining unused code or remove
- [x] 3.3 Final clippy/test verification

---

## Summary

All three phases of the security hardening migration are complete:

1. **CHATHISTORY**: IRCv3 message history with database storage
2. **BanCache**: Fast in-memory ban checks at connection time (IP) and registration (user@host)
3. **Cleanup**: Removed outdated dead_code annotations, added proper documentation

### Future Work (Phase 3b+)

Remaining dead_code annotations are intentional for:

- Admin commands (KLINE, DLINE, GLINE, ZLINE, etc.) - Phase 3b
- Server linking (S2S) - Phase 4+
- Background maintenance tasks (ban expiration pruning)


