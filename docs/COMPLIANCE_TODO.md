# Compliance Fix Todo List

## Status Summary

| Test File                  | Status  | Notes                                                |
| -------------------------- | ------- | ---------------------------------------------------- |
| account_registration.py    | ✅ 8/8   | Complete                                             |
| connection_registration.py | ✅ 11/12 | testNonutf8Realname needs proto layer UTF-8 handling |

## Tasks

- [x] Create Compliance Expert Agent
- [x] Fix Core Registration Failures
  - Target: `account_registration.py`, `connection_registration.py`
  - Result: 19/20 passing
- [ ] Fix Messaging & Echo Failures
  - Target: `echo_message.py`, `messages.py`
- [ ] Fix Operator Command Failures
  - Target: `oper.py`, `kill.py`, `wallops.py`, `kick.py`
- [ ] Fix Information Command Failures
  - Target: `who.py`, `whois.py`, `whowas.py`, `lusers.py`, `info.py`, `links.py`
- [ ] Fix Channel Feature Failures
  - Target: `invite.py`, `join.py`, `topic.py`, `extended_join.py`
- [ ] Fix Capabilities & Metadata Failures
  - Target: `cap.py`, `multi_prefix.py`, `message_tags.py`, `metadata.py`
- [ ] Investigate Test Timeouts
  - Target: `bot_mode.py`, `utf8.py`, `regressions.py`

## Completed Work

### Core Registration (account_registration.py, connection_registration.py)

**account_registration.py (8/8 passing)**:
- Implemented `REGISTER` command handler
- Added `AccountRegistrationConfig` with `before_connect`, `email_required`, `custom_account_name` flags
- Added `account_exists()` to NickServ
- Proper error responses (`FAIL REGISTER ...`)

**connection_registration.py (11/12 passing)**:
- Added password validation in `send_welcome_burst`
- Enforced PASS before NICK/USER per RFC 2812
- Added `AccessDenied` error variant for clean disconnect
- **Known issue**: `testNonutf8Realname` fails because proto layer rejects non-UTF8
