# SLIRC Migration TODO

## Phase 1: CHATHISTORY Migration ✅ COMPLETE (Audited 2025-12-02)

- [x] 1.1 Create db/history.rs - Port MessageEnvelope and storage logic
- [x] 1.2 Update Database Schema - Add message_history table (003_history.sql)
- [x] 1.3 Implement ChatHistoryHandler - LATEST, BEFORE, AFTER, BETWEEN, AROUND
- [x] 1.4 Wire into PrivmsgHandler - Store channel messages on send

## Phase 2: Security Hardening ✅ COMPLETE (Audited 2025-12-02)

- [x] 2.1 Add BanCache struct to security module
- [x] 2.2 Load bans on startup (K-lines, D-lines, G-lines, Z-lines)
- [x] 2.3 Add ban_cache field to Matrix
- [x] 2.4 Wire ban check into Gateway for TLS/plain/WebSocket connections
- [x] 2.5 Add ban check in handshake after USER (for user@host patterns)

## Phase 3: Cleanup ✅ COMPLETE (Audited 2025-12-02)

- [x] 3.1 Remove dead_code annotations for now-used methods (ban models, get_active_*)
- [x] 3.2 Wire remaining unused code or remove
- [x] 3.3 Final clippy/test verification

## Phase 3b: Admin Command Cache Sync ✅ COMPLETE

Wire admin ban commands to update BanCache (not just DB):

- [x] 3b.1 KLINE handler: Call `ban_cache.add_kline()` after DB insert
- [x] 3b.2 DLINE handler: Call `ban_cache.add_dline()` after DB insert
- [x] 3b.3 GLINE handler: Call `ban_cache.add_gline()` after DB insert
- [x] 3b.4 ZLINE handler: Call `ban_cache.add_zline()` after DB insert
- [x] 3b.5 UNKLINE handler: Call `ban_cache.remove_kline()` after DB delete
- [x] 3b.6 UNDLINE handler: Call `ban_cache.remove_dline()` after DB delete
- [x] 3b.7 UNGLINE handler: Call `ban_cache.remove_gline()` after DB delete
- [x] 3b.8 UNZLINE handler: Call `ban_cache.remove_zline()` after DB delete
- [x] 3b.9 Remove dead_code from cache add/remove methods
- [x] 3b.10 Final clippy/test verification

## Phase 3c: Stale Annotation Cleanup ✅ COMPLETE

Remove outdated Phase 3b dead_code annotations from now-used code:

- [x] 3c.1 Remove dead_code from gline.rs add_gline/remove_gline
- [x] 3c.2 Remove dead_code from zline.rs add_zline/remove_zline
- [x] 3c.3 Remove dead_code from queries/mod.rs wrapper methods
- [x] 3c.4 Final clippy/test verification

## Phase 4: Background Maintenance (Future)

- [ ] 4.1 Ban expiration pruning task
- [ ] 4.2 prune_expired() wiring
- [ ] 4.3 Startup task registration

## Phase 5: Server Linking S2S (Future)

- [ ] 5.1 S2S protocol design
- [ ] 5.2 Server state in Matrix
- [ ] 5.3 S2S message routing

---

## Summary

Phases 1-3c complete and audited. Phase 4 is background maintenance.


