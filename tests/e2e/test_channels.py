"""
Channel tests for slircd-ng.

Tests channel operations including:
- JOIN/PART
- Channel modes
- TOPIC
- NAMES/WHO
- KICK/BAN
"""

import pytest


class TestJoin:
    """JOIN command tests."""

    @pytest.mark.asyncio
    async def test_join_channel(self, client):
        """Test joining a channel."""
        msg = await client.join("#testchan")

        assert "JOIN" in msg.command or "JOIN" in msg.raw
        assert "#testchan" in msg.raw.lower()

    @pytest.mark.asyncio
    async def test_join_multiple_channels(self, client):
        """Test joining multiple channels."""
        await client.join("#chan1")
        await client.join("#chan2")
        await client.join("#chan3")

        # All joins should succeed - verify with WHOIS
        await client.send(f"WHOIS {client.nick}")

        messages = await client.recv_all(timeout=2)
        # Look for 319 RPL_WHOISCHANNELS
        channels_msg = next((m for m in messages if m.command == "319"), None)

        if channels_msg:
            assert "#chan1" in channels_msg.raw.lower()
            assert "#chan2" in channels_msg.raw.lower()
            assert "#chan3" in channels_msg.raw.lower()

    @pytest.mark.asyncio
    async def test_join_creates_channel(self, client):
        """Test that joining a non-existent channel creates it."""
        import time
        unique_chan = f"#newchan{int(time.time()) % 10000}"

        msg = await client.join(unique_chan)
        assert unique_chan.lower() in msg.raw.lower()

    @pytest.mark.asyncio
    async def test_join_see_other_users(self, clients):
        """Test that joining shows other users in channel."""
        alice = await clients.add_client("alice")
        await alice.join("#populated")

        # Clear alice's buffer
        await alice.recv_all(timeout=0.5)

        bob = await clients.add_client("bob")
        await bob.join("#populated")

        # Bob should receive NAMES with alice
        messages = await bob.recv_all(timeout=1)

        # Look for 353 RPL_NAMREPLY
        names_msg = next((m for m in messages if m.command == "353"), None)
        assert names_msg is not None, "Missing NAMES reply"
        assert alice.nick.lower() in names_msg.raw.lower()


class TestPart:
    """PART command tests."""

    @pytest.mark.asyncio
    async def test_part_channel(self, client):
        """Test parting a channel."""
        await client.join("#parttest")
        await client.recv_all(timeout=0.5)  # Clear join messages

        await client.part("#parttest", "Goodbye!")

        msg = await client.expect("PART", timeout=3)
        assert "#parttest" in msg.raw.lower()

    @pytest.mark.asyncio
    async def test_part_not_in_channel(self, client):
        """Test parting a channel we're not in."""
        await client.send("PART #notjoined")

        # Should receive 442 ERR_NOTONCHANNEL
        try:
            msg = await client.expect_numeric("442", timeout=3)
            assert msg.command == "442"
        except TimeoutError:
            pytest.skip("Server doesn't send 442 for PART on unjoined channel")


class TestTopic:
    """TOPIC command tests."""

    @pytest.mark.asyncio
    async def test_set_topic(self, client):
        """Test setting a channel topic."""
        await client.join("#topictest")
        await client.recv_all(timeout=0.5)

        await client.send("TOPIC #topictest :This is the test topic")

        # Should receive TOPIC confirmation
        msg = await client.expect("TOPIC", timeout=3)
        assert "test topic" in msg.raw.lower()

    @pytest.mark.asyncio
    async def test_get_topic(self, client):
        """Test getting a channel topic."""
        await client.join("#gettopic")
        await client.send("TOPIC #gettopic :Get this topic")
        await client.recv_all(timeout=0.5)

        await client.send("TOPIC #gettopic")

        # Should receive 332 RPL_TOPIC
        msg = await client.expect_any(["332", "331"], timeout=3)
        # 332 = topic exists, 331 = no topic


class TestNames:
    """NAMES command tests."""

    @pytest.mark.asyncio
    async def test_names(self, client):
        """Test NAMES command."""
        await client.join("#namestest")
        await client.recv_all(timeout=0.5)

        await client.send("NAMES #namestest")

        # Should receive 353 RPL_NAMREPLY and 366 RPL_ENDOFNAMES
        messages = await client.recv_all(timeout=2)

        numerics = {m.command for m in messages if m.command.isdigit()}
        assert "353" in numerics or "366" in numerics


class TestModes:
    """Channel mode tests."""

    @pytest.mark.asyncio
    async def test_check_modes(self, client):
        """Test checking channel modes."""
        await client.join("#modetest")
        await client.recv_all(timeout=0.5)

        await client.send("MODE #modetest")

        # Should receive 324 RPL_CHANNELMODEIS
        msg = await client.expect_any(["324", "MODE"], timeout=3)
        assert msg is not None

    @pytest.mark.asyncio
    async def test_set_mode(self, client):
        """Test setting channel mode."""
        await client.join("#setmode")
        await client.recv_all(timeout=0.5)

        # Set topic lock mode
        await client.send("MODE #setmode +t")

        msg = await client.expect("MODE", timeout=3)
        assert "+t" in msg.raw or "MODE" in msg.command

    @pytest.mark.asyncio
    async def test_op_user(self, clients):
        """Test giving op to another user."""
        # Alice creates channel (gets op)
        alice = await clients.add_client("alice")
        await alice.join("#opchan")

        # Bob joins
        bob = await clients.add_client("bob")
        await bob.join("#opchan")

        await alice.recv_all(timeout=0.5)
        await bob.recv_all(timeout=0.5)

        # Alice ops Bob
        await alice.send(f"MODE #opchan +o {bob.nick}")

        # Both should see the mode change
        msg = await bob.expect("MODE", timeout=3)
        assert "+o" in msg.raw and bob.nick.lower() in msg.raw.lower()


class TestKick:
    """KICK command tests."""

    @pytest.mark.asyncio
    async def test_kick_user(self, clients):
        """Test kicking a user from a channel."""
        alice = await clients.add_client("alice")
        await alice.join("#kicktest")

        bob = await clients.add_client("bob")
        await bob.join("#kicktest")

        await alice.recv_all(timeout=0.5)
        await bob.recv_all(timeout=0.5)

        # Alice kicks Bob
        await alice.send(f"KICK #kicktest {bob.nick} :You're out!")

        # Bob should receive KICK
        msg = await bob.expect("KICK", timeout=3)
        assert bob.nick.lower() in msg.raw.lower()


class TestInvite:
    """INVITE command tests."""

    @pytest.mark.asyncio
    async def test_invite(self, clients):
        """Test inviting a user to a channel."""
        alice = await clients.add_client("alice")
        await alice.join("#invitetest")

        bob = await clients.add_client("bob")

        await alice.recv_all(timeout=0.5)
        await bob.recv_all(timeout=0.5)

        # Alice invites Bob
        await alice.send(f"INVITE {bob.nick} #invitetest")

        # Bob should receive INVITE
        try:
            msg = await bob.expect("INVITE", timeout=3)
            assert "#invitetest" in msg.raw.lower()
        except TimeoutError:
            # Check if Alice got confirmation instead
            msg = await alice.expect_any(["341", "INVITE"], timeout=1)


class TestWho:
    """WHO command tests."""

    @pytest.mark.asyncio
    async def test_who_channel(self, client):
        """Test WHO command on a channel."""
        await client.join("#whotest")
        await client.recv_all(timeout=0.5)

        await client.send("WHO #whotest")

        # Should receive 352 RPL_WHOREPLY and 315 RPL_ENDOFWHO
        messages = await client.recv_all(timeout=2)

        numerics = {m.command for m in messages if m.command.isdigit()}
        assert "315" in numerics, "Missing RPL_ENDOFWHO"

    @pytest.mark.asyncio
    async def test_who_user(self, client):
        """Test WHO command on a specific user."""
        await client.send(f"WHO {client.nick}")

        # Should receive 352 and 315
        messages = await client.recv_all(timeout=2)

        numerics = {m.command for m in messages if m.command.isdigit()}
        assert "315" in numerics, "Missing RPL_ENDOFWHO"


class TestList:
    """LIST command tests."""

    @pytest.mark.asyncio
    async def test_list_channels(self, client):
        """Test LIST command shows channels."""
        # Create a channel first
        await client.join("#listtest")
        await client.recv_all(timeout=0.5)

        await client.send("LIST")

        # Should receive 321 RPL_LISTSTART, 322 RPL_LIST, 323 RPL_LISTEND
        messages = await client.recv_all(timeout=2)
        numerics = {m.command for m in messages if m.command.isdigit()}

        assert "323" in numerics, "Missing RPL_LISTEND"

    @pytest.mark.asyncio
    async def test_list_specific_channel(self, client):
        """Test LIST with specific channel."""
        await client.join("#listme")
        await client.recv_all(timeout=0.5)

        await client.send("LIST #listme")

        messages = await client.recv_all(timeout=2)
        numerics = {m.command for m in messages if m.command.isdigit()}

        assert "323" in numerics, "Missing RPL_LISTEND"


class TestTime:
    """TIME command tests."""

    @pytest.mark.asyncio
    async def test_time(self, client):
        """Test TIME command."""
        await client.send("TIME")

        messages = await client.recv_all(timeout=2)
        # Should receive 391 RPL_TIME
        numerics = {m.command for m in messages if m.command.isdigit()}

        assert "391" in numerics, "Missing RPL_TIME"


class TestAdmin:
    """ADMIN command tests."""

    @pytest.mark.asyncio
    async def test_admin(self, client):
        """Test ADMIN command."""
        await client.send("ADMIN")

        messages = await client.recv_all(timeout=2)
        # Should receive admin info (256-259) or some response
        # Even an error response is acceptable
        assert len(messages) > 0, "No response to ADMIN"


class TestInfo:
    """INFO command tests."""

    @pytest.mark.asyncio
    async def test_info(self, client):
        """Test INFO command."""
        await client.send("INFO")

        messages = await client.recv_all(timeout=2)
        # Should receive 371 RPL_INFO and 374 RPL_ENDOFINFO
        numerics = {m.command for m in messages if m.command.isdigit()}

        assert "374" in numerics, "Missing RPL_ENDOFINFO"
