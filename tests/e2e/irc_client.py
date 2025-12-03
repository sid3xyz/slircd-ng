"""
Async IRC client for end-to-end testing of slircd-ng.

This module provides a lightweight async IRC client designed for automated
testing. It supports:
- Connection with timeout
- Sending raw IRC commands
- Waiting for specific responses with pattern matching
- CAP negotiation helpers
- SASL authentication helpers
"""

import asyncio
import re
import time
from dataclasses import dataclass, field
from typing import Optional, List, Pattern, Union, Callable, Any


@dataclass
class IrcMessage:
    """Parsed IRC message."""
    raw: str
    tags: dict = field(default_factory=dict)
    prefix: Optional[str] = None
    command: str = ""
    params: List[str] = field(default_factory=list)

    @classmethod
    def parse(cls, line: str) -> "IrcMessage":
        """Parse an IRC protocol line into an IrcMessage."""
        raw = line
        tags = {}
        prefix = None

        # Strip trailing \r\n
        line = line.rstrip("\r\n")

        # Parse tags (@key=value;key2=value2)
        if line.startswith("@"):
            tag_part, line = line[1:].split(" ", 1)
            for tag in tag_part.split(";"):
                if "=" in tag:
                    k, v = tag.split("=", 1)
                    # Unescape tag values
                    v = v.replace("\\:", ";").replace("\\s", " ").replace("\\\\", "\\")
                    tags[k] = v
                else:
                    tags[tag] = True

        # Parse prefix (:nick!user@host)
        if line.startswith(":"):
            prefix, line = line[1:].split(" ", 1)

        # Parse command and params
        if " :" in line:
            front, trailing = line.split(" :", 1)
            parts = front.split()
            command = parts[0] if parts else ""
            params = parts[1:] + [trailing]
        else:
            parts = line.split()
            command = parts[0] if parts else ""
            params = parts[1:]

        return cls(raw=raw, tags=tags, prefix=prefix, command=command.upper(), params=params)

    @property
    def nick(self) -> Optional[str]:
        """Extract nick from prefix."""
        if self.prefix and "!" in self.prefix:
            return self.prefix.split("!")[0]
        return self.prefix

    def __str__(self) -> str:
        return self.raw.rstrip()


class IrcClient:
    """Async IRC client for testing."""

    def __init__(
        self,
        host: str = "127.0.0.1",
        port: int = 6667,
        timeout: float = 10.0,
        nick: Optional[str] = None,
        username: str = "test",
        realname: str = "Test User",
    ):
        self.host = host
        self.port = port
        self.timeout = timeout
        self.nick = nick or f"test{int(time.time()) % 10000}"
        self.username = username
        self.realname = realname

        self._reader: Optional[asyncio.StreamReader] = None
        self._writer: Optional[asyncio.StreamWriter] = None
        self._buffer: List[IrcMessage] = []
        self._connected = False
        self._registered = False
        self._caps_available: set = set()
        self._caps_enabled: set = set()

    async def connect(self) -> None:
        """Establish TCP connection to the IRC server."""
        try:
            self._reader, self._writer = await asyncio.wait_for(
                asyncio.open_connection(self.host, self.port),
                timeout=self.timeout
            )
            self._connected = True
        except asyncio.TimeoutError:
            raise ConnectionError(f"Timeout connecting to {self.host}:{self.port}")
        except OSError as e:
            raise ConnectionError(f"Failed to connect to {self.host}:{self.port}: {e}")

    async def disconnect(self) -> None:
        """Close the connection."""
        if self._writer:
            try:
                self._writer.close()
                await self._writer.wait_closed()
            except Exception:
                pass
        self._connected = False
        self._registered = False

    async def send(self, command: str) -> None:
        """Send a raw IRC command."""
        if not self._writer:
            raise ConnectionError("Not connected")

        if not command.endswith("\r\n"):
            command += "\r\n"

        self._writer.write(command.encode("utf-8"))
        await self._writer.drain()

    async def send_many(self, *commands: str) -> None:
        """Send multiple commands."""
        for cmd in commands:
            await self.send(cmd)

    async def recv(self, timeout: Optional[float] = None) -> Optional[IrcMessage]:
        """Receive and parse the next IRC message."""
        if not self._reader:
            raise ConnectionError("Not connected")

        timeout = timeout or self.timeout

        try:
            line = await asyncio.wait_for(
                self._reader.readline(),
                timeout=timeout
            )
        except asyncio.TimeoutError:
            return None

        if not line:
            self._connected = False
            return None

        msg = IrcMessage.parse(line.decode("utf-8", errors="replace"))

        # Auto-respond to PING
        if msg.command == "PING":
            server = msg.params[0] if msg.params else ""
            await self.send(f"PONG :{server}")

        return msg

    async def recv_all(self, timeout: float = 0.5) -> List[IrcMessage]:
        """Receive all pending messages within timeout."""
        messages = []
        deadline = time.time() + timeout

        while time.time() < deadline:
            remaining = deadline - time.time()
            if remaining <= 0:
                break
            msg = await self.recv(timeout=remaining)
            if msg:
                messages.append(msg)
            else:
                break

        return messages

    async def expect(
        self,
        pattern: Union[str, Pattern],
        timeout: Optional[float] = None,
        field: str = "command"
    ) -> IrcMessage:
        """
        Wait for a message matching the pattern.

        Args:
            pattern: Regex pattern or exact string to match
            timeout: Override default timeout
            field: Which field to match against ('command', 'raw', 'prefix')

        Returns:
            The matching IrcMessage

        Raises:
            TimeoutError: If no matching message received within timeout
        """
        timeout = timeout or self.timeout
        deadline = time.time() + timeout

        if isinstance(pattern, str):
            pattern = re.compile(pattern, re.IGNORECASE)

        while time.time() < deadline:
            remaining = deadline - time.time()
            if remaining <= 0:
                break

            msg = await self.recv(timeout=remaining)
            if msg is None:
                continue

            value = getattr(msg, field, msg.raw)
            if pattern.search(str(value)):
                return msg

        raise TimeoutError(f"Did not receive message matching {pattern.pattern}")

    async def expect_numeric(
        self,
        numeric: Union[int, str],
        timeout: Optional[float] = None
    ) -> IrcMessage:
        """Wait for a specific numeric response."""
        pattern = f"^{numeric}$"
        return await self.expect(pattern, timeout=timeout, field="command")

    async def expect_any(
        self,
        patterns: List[Union[str, Pattern]],
        timeout: Optional[float] = None
    ) -> IrcMessage:
        """Wait for any of the given patterns."""
        timeout = timeout or self.timeout
        deadline = time.time() + timeout

        compiled = []
        for p in patterns:
            if isinstance(p, str):
                compiled.append(re.compile(p, re.IGNORECASE))
            else:
                compiled.append(p)

        while time.time() < deadline:
            remaining = deadline - time.time()
            if remaining <= 0:
                break

            msg = await self.recv(timeout=remaining)
            if msg is None:
                continue

            for pattern in compiled:
                if pattern.search(msg.command) or pattern.search(msg.raw):
                    return msg

        raise TimeoutError(f"Did not receive message matching any of {patterns}")

    async def register(self, password: Optional[str] = None) -> IrcMessage:
        """
        Perform basic IRC registration (NICK + USER).

        Returns:
            The RPL_WELCOME (001) message
        """
        if password:
            await self.send(f"PASS {password}")

        await self.send(f"NICK {self.nick}")
        await self.send(f"USER {self.username} 0 * :{self.realname}")

        # Wait for 001 RPL_WELCOME
        welcome = await self.expect_numeric("001")
        self._registered = True
        return welcome

    async def cap_ls(self, version: int = 302) -> set:
        """Request capability list."""
        await self.send(f"CAP LS {version}")

        caps = set()
        while True:
            msg = await self.expect("CAP", timeout=5)
            if len(msg.params) >= 3 and msg.params[1] == "LS":
                # May be multi-line (* indicates more coming)
                cap_str = msg.params[-1]
                for cap in cap_str.split():
                    if "=" in cap:
                        name, _ = cap.split("=", 1)
                        caps.add(name)
                    else:
                        caps.add(cap)

                if msg.params[1] != "*":  # Not a continuation
                    break

        self._caps_available = caps
        return caps

    async def cap_req(self, *capabilities: str) -> bool:
        """Request capabilities."""
        caps = " ".join(capabilities)
        await self.send(f"CAP REQ :{caps}")

        msg = await self.expect("CAP", timeout=5)
        if len(msg.params) >= 2 and msg.params[1] == "ACK":
            for cap in capabilities:
                self._caps_enabled.add(cap)
            return True
        return False

    async def cap_end(self) -> None:
        """End capability negotiation."""
        await self.send("CAP END")

    async def join(self, channel: str, key: Optional[str] = None) -> IrcMessage:
        """Join a channel and wait for confirmation."""
        if key:
            await self.send(f"JOIN {channel} {key}")
        else:
            await self.send(f"JOIN {channel}")

        return await self.expect(f"JOIN.*{re.escape(channel)}", field="raw")

    async def part(self, channel: str, reason: Optional[str] = None) -> None:
        """Part a channel."""
        if reason:
            await self.send(f"PART {channel} :{reason}")
        else:
            await self.send(f"PART {channel}")

    async def privmsg(self, target: str, message: str) -> None:
        """Send a PRIVMSG."""
        await self.send(f"PRIVMSG {target} :{message}")

    async def notice(self, target: str, message: str) -> None:
        """Send a NOTICE."""
        await self.send(f"NOTICE {target} :{message}")

    async def quit(self, reason: str = "Goodbye") -> None:
        """Send QUIT and disconnect."""
        try:
            await self.send(f"QUIT :{reason}")
        except Exception:
            pass
        await self.disconnect()

    async def whois(self, nick: str) -> List[IrcMessage]:
        """Send WHOIS and collect response."""
        await self.send(f"WHOIS {nick}")

        messages = []
        while True:
            msg = await self.recv(timeout=5)
            if msg is None:
                break
            messages.append(msg)
            if msg.command in ("318", "401"):  # End of WHOIS or no such nick
                break

        return messages

    @property
    def connected(self) -> bool:
        return self._connected

    @property
    def registered(self) -> bool:
        return self._registered


class MultiClientManager:
    """Manage multiple IRC clients for multi-user tests."""

    def __init__(self, host: str = "127.0.0.1", port: int = 6667):
        self.host = host
        self.port = port
        self.clients: dict[str, IrcClient] = {}

    async def add_client(
        self,
        name: str,
        nick: Optional[str] = None,
        register: bool = True
    ) -> IrcClient:
        """Add and connect a new client."""
        nick = nick or f"{name}{int(time.time()) % 1000}"
        client = IrcClient(host=self.host, port=self.port, nick=nick)
        await client.connect()

        if register:
            await client.register()

        self.clients[name] = client
        return client

    def get(self, name: str) -> IrcClient:
        """Get a client by name."""
        return self.clients[name]

    async def cleanup(self) -> None:
        """Disconnect all clients."""
        for client in self.clients.values():
            try:
                await client.quit()
            except Exception:
                pass
        self.clients.clear()

    async def __aenter__(self) -> "MultiClientManager":
        return self

    async def __aexit__(self, *args) -> None:
        await self.cleanup()


# Convenience function for quick tests
async def quick_test(
    host: str = "127.0.0.1",
    port: int = 6667,
    commands: List[str] = None
) -> List[IrcMessage]:
    """
    Quick connection test - connect, register, run commands, disconnect.

    Example:
        messages = await quick_test(commands=["LUSERS", "MOTD"])
    """
    client = IrcClient(host=host, port=port)
    messages = []

    try:
        await client.connect()
        await client.register()

        for cmd in (commands or []):
            await client.send(cmd)
            msgs = await client.recv_all(timeout=1)
            messages.extend(msgs)

    finally:
        await client.quit()

    return messages


if __name__ == "__main__":
    # Quick manual test
    async def main():
        client = IrcClient()
        try:
            print(f"Connecting to {client.host}:{client.port}...")
            await client.connect()
            print("Connected!")

            print("Registering...")
            welcome = await client.register()
            print(f"Registered: {welcome}")

            print("Getting LUSERS...")
            await client.send("LUSERS")
            for _ in range(10):
                msg = await client.recv(timeout=1)
                if msg:
                    print(f"  {msg}")

            print("Joining #test...")
            join_msg = await client.join("#test")
            print(f"Joined: {join_msg}")

        except Exception as e:
            print(f"Error: {e}")
        finally:
            await client.quit()
            print("Disconnected")

    asyncio.run(main())
