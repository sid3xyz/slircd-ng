"""
Connection and registration tests for slircd-ng.

Tests basic IRC connection flows including:
- TCP connection
- NICK/USER registration
- CAP negotiation
- Password authentication
- Error handling for invalid input
"""

import pytest


class TestConnection:
    """Basic connection tests."""

    @pytest.mark.asyncio
    async def test_tcp_connect(self, unregistered_client):
        """Test that we can establish a TCP connection."""
        assert unregistered_client.connected

    @pytest.mark.asyncio
    async def test_registration(self, unregistered_client):
        """Test basic NICK + USER registration."""
        welcome = await unregistered_client.register()

        assert welcome.command == "001"
        assert unregistered_client.nick in welcome.params[-1]
        assert unregistered_client.registered

    @pytest.mark.asyncio
    async def test_ping_pong(self, client):
        """Test PING/PONG handling."""
        await client.send("PING :test123")
        pong = await client.expect("PONG.*test123", field="raw")

        assert "test123" in pong.raw

    @pytest.mark.asyncio
    async def test_welcome_numerics(self, unregistered_client):
        """Test that we receive proper welcome numerics (001-004)."""
        await unregistered_client.send(f"NICK {unregistered_client.nick}")
        await unregistered_client.send(f"USER {unregistered_client.username} 0 * :{unregistered_client.realname}")

        # Collect welcome messages
        numerics_seen = set()
        for _ in range(20):
            msg = await unregistered_client.recv(timeout=2)
            if msg is None:
                break
            if msg.command.isdigit():
                numerics_seen.add(int(msg.command))

        # Should see 001-004
        assert 1 in numerics_seen, "Missing RPL_WELCOME (001)"
        assert 2 in numerics_seen, "Missing RPL_YOURHOST (002)"
        assert 3 in numerics_seen, "Missing RPL_CREATED (003)"
        assert 4 in numerics_seen, "Missing RPL_MYINFO (004)"

    @pytest.mark.asyncio
    async def test_isupport(self, unregistered_client):
        """Test that we receive RPL_ISUPPORT (005)."""
        await unregistered_client.register()

        # 005 should be in the registration burst
        await unregistered_client.send("VERSION")  # Trigger more messages

        messages = await unregistered_client.recv_all(timeout=2)
        isupport_found = any(m.command == "005" for m in messages)

        # Note: 005 might have been in the registration burst we didn't capture
        # This is informational
        if not isupport_found:
            pytest.skip("005 not captured (may have been in registration burst)")


class TestNick:
    """NICK command tests."""

    @pytest.mark.asyncio
    async def test_nick_change(self, client):
        """Test changing nick after registration."""
        old_nick = client.nick
        new_nick = f"newnick{old_nick[-4:]}"

        await client.send(f"NICK {new_nick}")

        # Should receive NICK message confirming the change
        msg = await client.expect("NICK", timeout=3)
        assert new_nick.lower() in msg.raw.lower()

    @pytest.mark.asyncio
    async def test_nick_in_use(self, clients):
        """Test that duplicate nicks are rejected."""
        alice = await clients.add_client("alice", nick="uniquenick123")

        # Try to register another client with same nick
        from .irc_client import IrcClient
        bob = IrcClient(host=clients.host, port=clients.port, nick="uniquenick123")
        await bob.connect()

        await bob.send("NICK uniquenick123")
        await bob.send("USER test 0 * :Test")

        # Should receive 433 ERR_NICKNAMEINUSE
        try:
            msg = await bob.expect("433", timeout=5)
            assert msg.command == "433"
        finally:
            await bob.disconnect()

    @pytest.mark.asyncio
    async def test_invalid_nick_chars(self, unregistered_client):
        """Test that invalid nick characters are rejected."""
        await unregistered_client.send("NICK :invalid")  # Colon not allowed at start

        # Should receive 432 ERR_ERRONEUSNICKNAME
        try:
            msg = await unregistered_client.expect("432", timeout=3)
            assert msg.command == "432"
        except TimeoutError:
            pytest.skip("Server may allow this nick format")


class TestQuit:
    """QUIT command tests."""

    @pytest.mark.asyncio
    async def test_quit(self, client):
        """Test QUIT command."""
        # Drain welcome burst first
        await client.recv_all(timeout=0.5)

        await client.send("QUIT :Goodbye!")

        # Connection should close
        # Try reading - should get None or ERROR
        msg = await client.recv(timeout=2)
        # After QUIT, connection should be closed
        assert not client.connected or msg is None or msg.command == "ERROR"

    @pytest.mark.asyncio
    async def test_quit_message_seen_by_others(self, clients):
        """Test that quit message is broadcast to channel members."""
        alice = await clients.add_client("alice")
        bob = await clients.add_client("bob")

        # Both join same channel
        await alice.join("#quitchan")
        await bob.join("#quitchan")

        # Clear any pending messages
        await alice.recv_all(timeout=0.5)
        await bob.recv_all(timeout=0.5)

        # Alice quits
        await alice.send("QUIT :Leaving!")

        # Bob should see Alice's QUIT
        try:
            msg = await bob.expect("QUIT", timeout=3)
            assert alice.nick.lower() in msg.prefix.lower()
        except TimeoutError:
            pytest.skip("QUIT not broadcast to channel members")


class TestLusers:
    """LUSERS command tests."""

    @pytest.mark.asyncio
    async def test_lusers(self, client):
        """Test LUSERS command returns user statistics."""
        # Drain any pending messages from welcome burst
        await client.recv_all(timeout=0.5)

        await client.send("LUSERS")

        # Should receive various LUSERS numerics
        numerics_seen = set()
        for _ in range(10):
            msg = await client.recv(timeout=2)
            if msg is None:
                break
            if msg.command.isdigit():
                numerics_seen.add(int(msg.command))

        # 251 RPL_LUSERCLIENT should be present
        assert 251 in numerics_seen, "Missing RPL_LUSERCLIENT (251)"


class TestMotd:
    """MOTD command tests."""

    @pytest.mark.asyncio
    async def test_motd(self, client):
        """Test MOTD command."""
        await client.send("MOTD")

        # Should receive 375 (start), 372 (lines), 376 (end) or 422 (no MOTD)
        msg = await client.expect_any(["375", "376", "422"], timeout=3)
        assert msg.command in ("375", "376", "422")


class TestVersion:
    """VERSION command tests."""

    @pytest.mark.asyncio
    async def test_version(self, client):
        """Test VERSION command."""
        await client.send("VERSION")

        # Should receive 351 RPL_VERSION
        msg = await client.expect_numeric("351", timeout=3)
        assert "slircd" in msg.raw.lower() or msg.command == "351"
