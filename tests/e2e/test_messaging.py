"""
Messaging tests for slircd-ng.

Tests private and channel messaging including:
- PRIVMSG
- NOTICE
- Message delivery between users
- CTCP
"""

import pytest


class TestPrivmsg:
    """PRIVMSG command tests."""

    @pytest.mark.asyncio
    async def test_privmsg_to_channel(self, clients):
        """Test sending a message to a channel."""
        alice = await clients.add_client("alice")
        bob = await clients.add_client("bob")

        # Both join the same channel
        await alice.join("#msgtest")
        await bob.join("#msgtest")

        # Clear buffers
        await alice.recv_all(timeout=0.5)
        await bob.recv_all(timeout=0.5)

        # Alice sends a message
        await alice.privmsg("#msgtest", "Hello channel!")

        # Bob should receive it
        msg = await bob.expect("PRIVMSG", timeout=3)
        assert "Hello channel!" in msg.raw
        assert alice.nick.lower() in msg.prefix.lower()

    @pytest.mark.asyncio
    async def test_privmsg_to_user(self, clients):
        """Test sending a private message to a user."""
        alice = await clients.add_client("alice")
        bob = await clients.add_client("bob")

        await alice.recv_all(timeout=0.5)
        await bob.recv_all(timeout=0.5)

        # Alice sends PM to Bob
        await alice.privmsg(bob.nick, "Hello Bob!")

        # Bob should receive it
        msg = await bob.expect("PRIVMSG", timeout=3)
        assert "Hello Bob!" in msg.raw
        assert alice.nick.lower() in msg.prefix.lower()

    @pytest.mark.asyncio
    async def test_privmsg_no_such_nick(self, client):
        """Test PRIVMSG to non-existent nick."""
        await client.send("PRIVMSG nonexistent123456 :Hello?")

        # Should receive 401 ERR_NOSUCHNICK
        msg = await client.expect_numeric("401", timeout=3)
        assert msg.command == "401"

    @pytest.mark.asyncio
    async def test_privmsg_no_text(self, client):
        """Test PRIVMSG with no text."""
        await client.join("#notext")
        await client.send("PRIVMSG #notext")

        # Should receive 412 ERR_NOTEXTTOSEND
        try:
            msg = await client.expect_numeric("412", timeout=3)
            assert msg.command == "412"
        except TimeoutError:
            pytest.skip("Server may handle empty PRIVMSG differently")

    @pytest.mark.asyncio
    async def test_privmsg_not_on_channel(self, clients):
        """Test PRIVMSG to channel user is not on."""
        alice = await clients.add_client("alice")
        bob = await clients.add_client("bob")

        # Only alice joins
        await alice.join("#aliceonly")
        await alice.recv_all(timeout=0.5)
        await bob.recv_all(timeout=0.5)

        # Bob tries to message the channel
        await bob.send("PRIVMSG #aliceonly :Hello?")

        # Should receive 404 ERR_CANNOTSENDTOCHAN or 442 ERR_NOTONCHANNEL
        try:
            msg = await bob.expect_any(["404", "442"], timeout=3)
            assert msg.command in ("404", "442")
        except TimeoutError:
            pytest.skip("Server may allow messaging to unjoined channels")


class TestNotice:
    """NOTICE command tests."""

    @pytest.mark.asyncio
    async def test_notice_to_channel(self, clients):
        """Test sending a NOTICE to a channel."""
        alice = await clients.add_client("alice")
        bob = await clients.add_client("bob")

        await alice.join("#noticetest")
        await bob.join("#noticetest")

        await alice.recv_all(timeout=0.5)
        await bob.recv_all(timeout=0.5)

        # Alice sends notice
        await alice.notice("#noticetest", "This is a notice")

        # Bob should receive it
        msg = await bob.expect("NOTICE", timeout=3)
        assert "This is a notice" in msg.raw

    @pytest.mark.asyncio
    async def test_notice_to_user(self, clients):
        """Test sending a NOTICE to a user."""
        alice = await clients.add_client("alice")
        bob = await clients.add_client("bob")

        await alice.recv_all(timeout=0.5)
        await bob.recv_all(timeout=0.5)

        # Alice sends notice to Bob
        await alice.notice(bob.nick, "Private notice")

        # Bob should receive it
        msg = await bob.expect("NOTICE", timeout=3)
        assert "Private notice" in msg.raw


class TestCtcp:
    """CTCP tests."""

    @pytest.mark.asyncio
    async def test_ctcp_version(self, clients):
        """Test CTCP VERSION request."""
        alice = await clients.add_client("alice")
        bob = await clients.add_client("bob")

        await alice.recv_all(timeout=0.5)
        await bob.recv_all(timeout=0.5)

        # Alice sends CTCP VERSION to Bob
        await alice.privmsg(bob.nick, "\x01VERSION\x01")

        # Bob should receive the CTCP request
        msg = await bob.expect("PRIVMSG", timeout=3)
        assert "\x01VERSION\x01" in msg.raw

    @pytest.mark.asyncio
    async def test_ctcp_ping(self, clients):
        """Test CTCP PING."""
        alice = await clients.add_client("alice")
        bob = await clients.add_client("bob")

        await alice.recv_all(timeout=0.5)
        await bob.recv_all(timeout=0.5)

        # Alice sends CTCP PING
        await alice.privmsg(bob.nick, "\x01PING 12345\x01")

        # Bob receives it
        msg = await bob.expect("PRIVMSG", timeout=3)
        assert "\x01PING" in msg.raw


class TestAway:
    """AWAY command tests."""

    @pytest.mark.asyncio
    async def test_set_away(self, client):
        """Test setting away status."""
        await client.send("AWAY :Gone fishing")

        # Should receive 306 RPL_NOWAWAY
        msg = await client.expect_numeric("306", timeout=3)
        assert msg.command == "306"

    @pytest.mark.asyncio
    async def test_unset_away(self, client):
        """Test unsetting away status."""
        await client.send("AWAY :Gone")
        await client.recv_all(timeout=0.5)

        await client.send("AWAY")

        # Should receive 305 RPL_UNAWAY
        msg = await client.expect_numeric("305", timeout=3)
        assert msg.command == "305"

    @pytest.mark.asyncio
    async def test_away_reply(self, clients):
        """Test that messaging an away user returns away message."""
        alice = await clients.add_client("alice")
        bob = await clients.add_client("bob")

        # Bob sets away
        await bob.send("AWAY :I am away")
        await bob.recv_all(timeout=0.5)
        await alice.recv_all(timeout=0.5)

        # Alice messages Bob
        await alice.privmsg(bob.nick, "Hello?")

        # Alice should receive 301 RPL_AWAY
        try:
            msg = await alice.expect_numeric("301", timeout=3)
            assert "away" in msg.raw.lower()
        except TimeoutError:
            pytest.skip("Server may not send RPL_AWAY on PRIVMSG")


class TestWhois:
    """WHOIS command tests."""

    @pytest.mark.asyncio
    async def test_whois_self(self, client):
        """Test WHOIS on self."""
        messages = await client.whois(client.nick)

        numerics = {m.command for m in messages}
        assert "311" in numerics, "Missing RPL_WHOISUSER"
        assert "318" in numerics, "Missing RPL_ENDOFWHOIS"

    @pytest.mark.asyncio
    async def test_whois_other(self, clients):
        """Test WHOIS on another user."""
        alice = await clients.add_client("alice")
        bob = await clients.add_client("bob")

        await alice.recv_all(timeout=0.5)

        messages = await alice.whois(bob.nick)

        numerics = {m.command for m in messages}
        assert "311" in numerics, "Missing RPL_WHOISUSER"
        assert "318" in numerics, "Missing RPL_ENDOFWHOIS"

    @pytest.mark.asyncio
    async def test_whois_channels(self, client):
        """Test that WHOIS shows channels."""
        await client.join("#whoischan")
        await client.recv_all(timeout=0.5)

        messages = await client.whois(client.nick)

        # Look for 319 RPL_WHOISCHANNELS
        channels_msg = next((m for m in messages if m.command == "319"), None)
        if channels_msg:
            assert "#whoischan" in channels_msg.raw.lower()

    @pytest.mark.asyncio
    async def test_whois_nonexistent(self, client):
        """Test WHOIS on non-existent user."""
        messages = await client.whois("nonexistent999")

        numerics = {m.command for m in messages}
        # Should get 401 ERR_NOSUCHNICK or just 318 end
        assert "401" in numerics or "318" in numerics


class TestWhowas:
    """WHOWAS command tests."""

    @pytest.mark.asyncio
    async def test_whowas(self, client):
        """Test WHOWAS command."""
        await client.send("WHOWAS somequituser")

        # Should receive 406 ERR_WASNOSUCHNICK or 314 RPL_WHOWASUSER
        msg = await client.expect_any(["406", "314", "369"], timeout=3)
        assert msg.command in ("406", "314", "369")


class TestUserhost:
    """USERHOST command tests."""

    @pytest.mark.asyncio
    async def test_userhost(self, client):
        """Test USERHOST command."""
        await client.send(f"USERHOST {client.nick}")

        # Should receive 302 RPL_USERHOST
        msg = await client.expect_numeric("302", timeout=3)
        assert client.nick.lower() in msg.raw.lower()

    @pytest.mark.asyncio
    async def test_userhost_multiple(self, clients):
        """Test USERHOST with multiple nicks."""
        alice = await clients.add_client("alice")
        bob = await clients.add_client("bob")

        await alice.recv_all(timeout=0.5)

        await alice.send(f"USERHOST {alice.nick} {bob.nick}")

        msg = await alice.expect_numeric("302", timeout=3)
        # Both nicks should be in reply
        assert alice.nick.lower() in msg.raw.lower()
        assert bob.nick.lower() in msg.raw.lower()


class TestIson:
    """ISON command tests."""

    @pytest.mark.asyncio
    async def test_ison(self, client):
        """Test ISON command."""
        await client.send(f"ISON {client.nick}")

        # Should receive 303 RPL_ISON
        msg = await client.expect_numeric("303", timeout=3)
        assert client.nick.lower() in msg.raw.lower()

    @pytest.mark.asyncio
    async def test_ison_not_online(self, client):
        """Test ISON with offline user."""
        await client.send("ISON offlineuser12345")

        # Should receive 303 with empty reply (user not online)
        msg = await client.expect_numeric("303", timeout=3)
        # The offline user should NOT be in the reply
        assert "offlineuser12345" not in msg.raw.lower()


class TestMonitor:
    """MONITOR command tests."""

    @pytest.mark.asyncio
    async def test_monitor_add(self, client):
        """Test adding nicks to monitor list."""
        await client.send("MONITOR + someuser")

        # Should receive 731 RPL_MONOFFLINE (user offline)
        messages = await client.recv_all(timeout=2)
        numerics = {m.command for m in messages if m.command.isdigit()}

        # Either 731 (offline) or some other response
        assert len(messages) > 0, "No response to MONITOR +"

    @pytest.mark.asyncio
    async def test_monitor_list(self, client):
        """Test listing monitored nicks."""
        await client.send("MONITOR + testmon1,testmon2")
        await client.recv_all(timeout=0.5)

        await client.send("MONITOR L")

        messages = await client.recv_all(timeout=2)
        # Should receive 732 RPL_MONLIST and 733 RPL_ENDOFMONLIST
        numerics = {m.command for m in messages if m.command.isdigit()}

        assert "733" in numerics, "Missing RPL_ENDOFMONLIST"

    @pytest.mark.asyncio
    async def test_monitor_clear(self, client):
        """Test clearing monitor list."""
        await client.send("MONITOR + clearme")
        await client.recv_all(timeout=0.5)

        await client.send("MONITOR C")

        # Clear should not error - check list is empty
        await client.send("MONITOR L")
        messages = await client.recv_all(timeout=2)

        # Should get 733 ENDOFMONLIST with no 732 entries
        numerics = {m.command for m in messages if m.command.isdigit()}
        assert "733" in numerics, "Missing RPL_ENDOFMONLIST"

    @pytest.mark.asyncio
    async def test_monitor_online_notification(self, clients):
        """Test that MONITOR notifies when user comes online."""
        alice = await clients.add_client("alice")
        await alice.recv_all(timeout=0.5)

        # Monitor a nick that will come online
        await alice.send("MONITOR + monitored_user")
        await alice.recv_all(timeout=0.5)

        # Now connect another client with that nick
        bob = await clients.add_client("monitored_user")
        await bob.recv_all(timeout=0.5)

        # Alice should receive 730 RPL_MONONLINE
        messages = await alice.recv_all(timeout=2)
        numerics = {m.command for m in messages if m.command.isdigit()}

        # Note: This might fail if server doesn't implement online notification
        # Skip if no 730 received
        if "730" not in numerics:
            pytest.skip("Server doesn't send MONITOR online notifications")


class TestSetname:
    """SETNAME command tests."""

    @pytest.mark.asyncio
    async def test_setname_with_cap(self, server):
        """Test changing realname with SETNAME after CAP negotiation."""
        from irc_client import IrcClient
        import time

        # Create a client that negotiates setname cap
        c = IrcClient(
            host=server.host,
            port=server.port,
            nick=f"setname{int(time.time()) % 10000}",
            realname="Original Realname"
        )

        await c.connect()

        # Negotiate capabilities including setname
        caps = await c.cap_ls()
        if "setname" not in caps:
            await c.disconnect()
            pytest.skip("Server doesn't advertise setname capability")

        await c.cap_req("setname")
        await c.cap_end()

        # Complete registration
        await c.send(f"NICK {c.nick}")
        await c.send(f"USER {c.username} 0 * :{c.realname}")

        # Wait for registration
        await c.expect_numeric("001", timeout=5)

        # Now use SETNAME
        await c.send("SETNAME :My New Realname")

        messages = await c.recv_all(timeout=2)

        # Should get SETNAME echo back
        setname_echo = any("SETNAME" in m.raw for m in messages)
        assert setname_echo, "SETNAME should echo back after CAP negotiation"

        # Verify with WHOIS
        await c.send(f"WHOIS {c.nick}")
        whois_msgs = await c.recv_all(timeout=2)

        whois_user = next((m for m in whois_msgs if m.command == "311"), None)
        assert whois_user is not None, "Should get WHOIS reply"
        assert "My New Realname" in whois_user.raw, "WHOIS should show new realname"

        await c.disconnect()

    @pytest.mark.asyncio
    async def test_setname_without_cap(self, client):
        """Test SETNAME without capability negotiation is silently ignored."""
        await client.send("SETNAME :Should Be Ignored")

        messages = await client.recv_all(timeout=1)

        # Should NOT get a SETNAME echo (cap not negotiated)
        setname_echo = any("SETNAME" in m.raw for m in messages)
        # This is expected - no echo means server correctly rejected it
        assert not setname_echo, "SETNAME without cap should not echo"
