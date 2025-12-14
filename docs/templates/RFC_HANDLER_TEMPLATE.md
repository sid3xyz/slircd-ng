/// MODE command handler - User modes
///
/// # RFC 2812 Compliance
///
/// ## Section 3.1.5: User Mode Message
///
/// ```text
/// Command: MODE
/// Parameters: <nickname> *( ( "+" / "-" ) *( "i" / "w" / "o" / "O" / "r" ) )
/// ```
///
/// ### Correct Behavior (RFC 2812)
/// 1. **User mode changes are sent ONLY to the user** - NOT broadcast to channels
/// 2. User can only change their own modes (ERR_USERSDONTMATCH if target != self)
/// 3. Server-only modes (+o, +O, +r) cannot be set by users
/// 4. Removing server-only modes is silently ignored (not an error)
///
/// ### IRCv3 Extensions
/// - User modes do not interact with IRCv3 capabilities
/// - ACCOUNT message (separate command) notifies channel members of account status
///
/// ### Tests
/// - `irctest/server_tests/umodes/` - User mode tests
/// - `irctest/server_tests/connection_registration.py` - Initial mode setting
///
/// ### Common Mistakes
/// - ❌ Broadcasting MODE changes to channels (violation found 2025-12-13)
/// - ❌ Allowing users to set +o on themselves
/// - ❌ Treating MODE +r removal as error (should silently ignore)
///
/// ### Implementation Notes
/// - MODE +r is set by NickServ via ServiceEffect::AccountSet
/// - Mode changes echo back to user with their own prefix (not server prefix)
/// - Empty mode string in query returns RPL_UMODEIS with current modes
