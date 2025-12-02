# SLIRC Migration TODO

## Phase 1: CHATHISTORY Migration
- [ ] 1.1 Create db/history.rs - Port MessageEnvelope and storage logic
- [ ] 1.2 Update Database Schema - Add message_history table
- [ ] 1.3 Implement ChatHistoryHandler - LATEST, BEFORE, AFTER, BETWEEN
- [ ] 1.4 Wire into PrivmsgHandler - Store channel messages

## Phase 2: Security Hardening
- [ ] 2.1 Cache bans in Matrix - Add klines/glines/zlines DashMaps
- [ ] 2.2 Implement check_bans helper - Create security/bans.rs
- [ ] 2.3 Wire into handshake - Check bans before registration

## Phase 3: Cleanup
- [ ] 3.1 Remove dead_code annotations
- [ ] 3.2 Final verification

