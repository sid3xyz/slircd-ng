# Channel Operations Baseline (2025-05-21)

**Tests Run:** `irctest/server_tests/{join,part,channel,topic,kick,invite,names,list,who,chmodes/}`
**Total:** 134 tests
**Passed:** 84
**Failed:** 12
**Skipped:** 38

## Failure Analysis

### 1. `NAMES` Reply Format
*   **Tests:** `NamesTestCase::testNames1459`, `testNamesNoArgumentPublic1459`, etc.
*   **Issue:** Server includes the channel symbol (`=`, `*`, `@`) in `RPL_NAMREPLY` (353) even when `irctest` expects RFC1459 format (no symbol).
*   **Error:** `expected params to match ['nick1', '#chan', ...], got ['nick1', '=', '#chan', ...]`
*   **Impact:** Compliance with strict RFC1459 clients. Modern clients usually expect the symbol.

### 2. Ban List (`MODE +b`)
*   **Tests:** `BanModeTestCase::testBanList`
*   **Issue:** The "setter" field in `RPL_BANLIST` (367) contains an internal ID (e.g., `001AAAAAB`) instead of the setter's Nickmask (e.g., `chanop!user@host`).
*   **Error:** `got ['chanop', '#chan', 'bar!*@*', '001AAAAAB', '1765122172']`

### 3. Ban Case Insensitivity
*   **Tests:** `BanModeTestCase::testCaseInsensitive`
*   **Issue:** Unbanning `bar!*@*` does not remove `BAR!*@*` (or vice versa).
*   **Error:** `NoMessageException` (Mode change not echoed).

### 4. Invite vs Ban
*   **Tests:** `InviteTestCase::testInviteExemptsFromBan`
*   **Issue:** `INVITE` does not allow a banned user to join.
*   **Error:** `AssertionError`

### 5. Mode `+o` Target Validation
*   **Tests:** `ChannelOperatorModeTestCase::testChannelOperatorModeTargetNotInChannel`
*   **Issue:** Setting `+o` on a user not in the channel seems to be silently ignored or fails to send `ERR_USERNOTINCHANNEL` (441).
*   **Error:** `AssertionError: assert 0 == 1` (Expected 1 message, got 0).

### 6. Missing Capabilities / Modes
*   **Tests:** `AuditoriumTestCase`, `OpModeratedTestCase`, `RegisteredOnlySpeakModeTestCase`
*   **Issue:**
    *   Missing `account-tag` capability (causes CAP NAK).
    *   Missing `+M` (Registered Only) mode support.
    *   Missing `EXTBAN` support (ISUPPORT check fails).

## Action Plan

1.  **Fix Ban Setter:** Ensure `RPL_BANLIST` resolves the setter's ID to a proper string (or stores the string).
2.  **Fix `ERR_USERNOTINCHANNEL`:** Ensure `MODE +o` checks membership and returns 441.
3.  **Implement `account-tag`:** Add to `CAP` list if supported, or investigate why it's requested.
4.  **Review `NAMES`:** Decide if we want strict RFC1459 compliance or if `irctest` needs adjustment/configuration for modern server behavior.
