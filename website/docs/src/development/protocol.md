# Protocol Requirements

This document tracks known protocol gaps, compliance issues, and requirements for upcoming releases.

## Blocking Issues (Alpha -> Beta)

### IRCv3 Compliance
- [ ] **Labeled Responses**: Full support required for all command responses (currently partial).
- [ ] **monitor**: Notification of checking target status change is required for proper bouncer UI support.
- [ ] **batch**: Need to implement `netsplit` and `netjoin` batches for cleaner client history.

### Core Protocol
- [ ] **READQ Enforcement**: Strict byte-limit enforcement on input buffers to prevent DOS.
- [ ] **Unicode Handling**: Edge case validation for "confusables" in nicknames (prevent impersonation).

### Extensions
- [ ] **RELAYMSG**: Needs standardization or better compatibility with other relay bots.

## Enhancements (Post-1.0)

- **S2S (Server-to-Server)**: Full mesh routing topology (currently naive star/tree).
- **CHATHISTORY**: Support for `msgid` lookup optimization.
- **WebSockets**: Binary frame support for efficiency.
