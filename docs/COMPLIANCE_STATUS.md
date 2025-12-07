# Compliance Status

This document tracks the compliance status of `slircd-ng` against IRCv3 specifications and other standards.

## Summary

| Feature                 | Status        | Notes                                 |
| :---------------------- | :------------ | :------------------------------------ |
| **Core Protocol**       |               |                                       |
| Connection Registration | ✅ Passing     | Verified with `irctest`               |
| Message Parsing         | ✅ Passing     |                                       |
| Channel Operations      | 🚧 In Progress |                                       |
| **IRCv3 Extensions**    |               |                                       |
| `draft/chathistory`     | ✅ Passing     | Fixed `LATEST` semantics (2025-05-21) |
| `server-time`           | ✅ Passing     |                                       |
| `message-tags`          | ✅ Passing     |                                       |
| `batch`                 | ✅ Passing     |                                       |
| `echo-message`          | ✅ Passing     |                                       |
| `labeled-response`      | ✅ Passing     |                                       |
| `sasl`                  | ✅ Passing     | PLAIN mechanism supported             |

## Detailed Test Results

### `draft/chathistory`

**Status:** ✅ Passing
**Last Verified:** 2025-05-21
**Tests Run:** `irctest/server_tests/chathistory.py`

**Notes:**

- `LATEST` command was previously failing due to incorrect semantics (returning messages *before* reference instead of *after*).
- Fixed by implementing `query_latest_after` in `HistoryRepository` and updating `ChatHistoryHandler`.
- All 22 tests in `chathistory.py` passed (some skipped due to optional features like event playback).

### Connection Registration

**Status:** ✅ Passing
**Last Verified:** 2025-05-21
**Tests Run:** `irctest/server_tests/connection_registration.py`

## Known Issues

- None currently tracked for implemented features.

## Next Steps

- Run full `irctest` suite to identify other gaps.
- Implement `draft/event-playback` for `chathistory`.
