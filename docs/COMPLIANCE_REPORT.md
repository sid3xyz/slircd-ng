# slircd-ng RFC Compliance Report

**Date:** 2025-12-03 17:09:37
**Total Tests:** 55
**Passed:** 16
**Failed:** 39

| Test Module | Status | Duration | Details |
|-------------|--------|----------|---------|
| `__init__.py` | ❌ | 1.79s | See logs for details |
| `account_registration.py` | ❌ | 1.88s | irctest/server_tests/account_registration.py::RegisterTestCase::testRegisterDefaultName FAILED [ ... |
| `account_tag.py` | ✅ | 1.79s |  |
| `away.py` | ✅ | 2.35s |  |
| `away_notify.py` | ❌ | 2.30s | irctest/server_tests/away_notify.py::AwayNotifyTestCase::testAwayNotifyOnJoin FAILED [100%]<br>FA... |
| `bot_mode.py` | ❌ | 31.55s | See logs for details |
| `bouncer.py` | ✅ | 1.95s |  |
| `buffering.py` | ✅ | 2.36s |  |
| `cap.py` | ❌ | 2.09s | irctest/server_tests/cap.py::CapTestCase::testInvalidCapSubcommand FAILED [  7%]<br>irctest/serve... |
| `channel.py` | ✅ | 2.31s |  |
| `channel_forward.py` | ❌ | 1.95s | irctest/server_tests/channel_forward.py::ChannelForwardingTestCase::testChannelForwarding FAILED ... |
| `channel_rename.py` | ✅ | 1.79s |  |
| `chathistory.py` | ❌ | 1.99s | irctest/server_tests/chathistory.py::ChathistoryTestCase::testChathistory[LATEST] FAILED [ 13%]<b... |
| `confusables.py` | ✅ | 1.96s |  |
| `connection_registration.py` | ❌ | 6.08s | irctest/server_tests/connection_registration.py::PasswordedConnectionRegistrationTestCase::testNo... |
| `echo_message.py` | ❌ | 2.59s | irctest/server_tests/echo_message.py::EchoMessageTestCase::testEchoMessage[PRIVMSG-False-False] F... |
| `extended_join.py` | ❌ | 1.80s | irctest/server_tests/extended_join.py::MetadataTestCase::testNotLoggedIn FAILED [ 50%]<br>FAILED ... |
| `help.py` | ✅ | 1.93s |  |
| `info.py` | ❌ | 2.14s | irctest/server_tests/info.py::InfoTestCase::testInfo FAILED              [ 16%]<br>irctest/server... |
| `invite.py` | ❌ | 3.22s | irctest/server_tests/invite.py::InviteTestCase::testInvites FAILED       [  7%]<br>irctest/server... |
| `isupport.py` | ✅ | 1.84s |  |
| `join.py` | ❌ | 2.88s | irctest/server_tests/join.py::JoinTestCase::testJoinNamreply FAILED      [ 25%]<br>irctest/server... |
| `kick.py` | ❌ | 3.44s | irctest/server_tests/kick.py::KickTestCase::testKickPrivileges FAILED    [ 57%]<br>irctest/server... |
| `kill.py` | ❌ | 2.05s | irctest/server_tests/kill.py::KillTestCase::testKill FAILED              [ 50%]<br>irctest/server... |
| `labeled_responses.py` | ❌ | 3.26s | irctest/server_tests/labeled_responses.py::LabeledResponsesTestCase::testLabeledPrivmsgResponsesT... |
| `links.py` | ❌ | 2.04s | irctest/server_tests/links.py::LinksTestCase::testLinksSingleServer FAILED [ 50%]<br>irctest/serv... |
| `list.py` | ✅ | 2.31s |  |
| `lusers.py` | ❌ | 2.79s | irctest/server_tests/lusers.py::BasicLusersTestCase::testLusersFull FAILED [ 22%]<br>irctest/serv... |
| `message_tags.py` | ❌ | 2.35s | irctest/server_tests/message_tags.py::MessageTagsTestCase::testBasic FAILED [ 50%]<br>irctest/ser... |
| `messages.py` | ❌ | 2.81s | irctest/server_tests/messages.py::PrivmsgTestCase::testEmptyPrivmsg FAILED [ 45%]<br>irctest/serv... |
| `metadata.py` | ❌ | 2.20s | irctest/server_tests/metadata.py::MetadataDeprecatedTestCase::testInIsupport FAILED [ 11%]<br>irc... |
| `metadata_2.py` | ✅ | 1.82s |  |
| `monitor.py` | ✅ | 2.20s |  |
| `multi_prefix.py` | ❌ | 2.18s | irctest/server_tests/multi_prefix.py::MultiPrefixTestCase::testMultiPrefix FAILED [ 50%]<br>ircte... |
| `multiline.py` | ❌ | 2.99s | irctest/server_tests/multiline.py::MultilineTestCase::testBasic FAILED   [ 16%]<br>irctest/server... |
| `names.py` | ❌ | 3.03s | irctest/server_tests/names.py::NamesTestCase::testNames1459 FAILED       [  8%]<br>irctest/server... |
| `oper.py` | ❌ | 2.03s | irctest/server_tests/oper.py::OperTestCase::testOperSuccess FAILED       [ 25%]<br>FAILED irctest... |
| `part.py` | ✅ | 4.40s |  |
| `pingpong.py` | ❌ | 2.02s | irctest/server_tests/pingpong.py::PingPongTestCase::testPing FAILED      [ 33%]<br>irctest/server... |
| `quit.py` | ❌ | 3.07s | irctest/server_tests/quit.py::ChannelQuitTestCase::testQuit FAILED       [100%]<br>1764799675.315... |
| `readq.py` | ✅ | 1.94s |  |
| `regressions.py` | ❌ | 31.53s | irctest/server_tests/regressions.py::RegressionsTestCase::testCaseChanges FAILED [ 22%]<br>irctes... |
| `relaymsg.py` | ❌ | 1.95s | irctest/server_tests/relaymsg.py::RelaymsgTestCase::testRelaymsg FAILED  [100%]<br>FAILED irctest... |
| `roleplay.py` | ❌ | 2.07s | irctest/server_tests/roleplay.py::RoleplayTestCase::testRoleplay FAILED  [100%]<br>FAILED irctest... |
| `sasl.py` | ✅ | 1.96s |  |
| `setname.py` | ❌ | 2.37s | irctest/server_tests/setname.py::SetnameMessageTestCase::testSetnameChannel FAILED [100%]<br>FAIL... |
| `statusmsg.py` | ❌ | 2.10s | irctest/server_tests/statusmsg.py::StatusmsgTestCase::testInIsupport FAILED [ 33%]<br>FAILED irct... |
| `time.py` | ❌ | 1.96s | irctest/server_tests/time.py::TimeTestCase::testTime FAILED              [100%]<br>FAILED irctest... |
| `topic.py` | ❌ | 2.82s | irctest/server_tests/topic.py::TopicTestCase::testTopicMode FAILED       [ 50%]<br>irctest/server... |
| `utf8.py` | ❌ | 31.53s | irctest/server_tests/utf8.py::Utf8TestCase::testNonUtf8Filtering FAILED  [ 16%] |
| `wallops.py` | ❌ | 2.04s | irctest/server_tests/wallops.py::WallopsTestCase::testWallops FAILED     [ 50%]<br>FAILED irctest... |
| `who.py` | ❌ | 6.26s | irctest/server_tests/who.py::WhoTestCase::testWhoStar FAILED             [  2%]<br>irctest/server... |
| `whois.py` | ❌ | 2.64s | irctest/server_tests/whois.py::WhoisTestCase::testWhoisUser[no-target] FAILED [  9%]<br>irctest/s... |
| `whowas.py` | ❌ | 8.57s | irctest/server_tests/whowas.py::WhowasTestCase::testWhowasNumerics FAILED [  7%]<br>irctest/serve... |
| `znc_playback.py` | ✅ | 1.95s |  |
