# Protocol Requirements

This document tracks known protocol gaps, compliance issues, and requirements for upcoming releases.

## Blocking Issues (Beta -> Stable)

### IRCv3 Compliance
- [x] **Labeled Responses**: Working in RELAYMSG tests.
- [ ] **monitor**: Basic support exists; extended-monitor needs verification (Priority: Low).
- [x] **batch**: CHATHISTORY batches working.
- [x] **RELAYMSG**: draft/relaymsg fully functional.

### Core Protocol
- [x] **READQ Enforcement**: 16KB parser limit enforced.
- [x] **Unicode Handling**: PRECIS casemapping handles Cyrillic.
- [ ] **CHATHISTORY**: Partial support (some queries work), edge cases remain.


## Enhancements (Post-1.0)

- **S2S (Server-to-Server)**: Full mesh routing topology (currently naive star/tree).
- **CHATHISTORY**: Support for `msgid` lookup optimization.
- **WebSockets**: Binary frame support for efficiency.
