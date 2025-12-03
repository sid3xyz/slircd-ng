"""
Pytest fixtures for slircd-ng end-to-end tests.

This module provides fixtures for:
- Starting/stopping the slircd-ng server
- Creating connected IRC clients
- Multi-client scenarios
"""

import asyncio
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import AsyncGenerator, Generator, Optional

import pytest
import pytest_asyncio

from .irc_client import IrcClient, MultiClientManager


# Test configuration
SERVER_HOST = os.environ.get("E2E_HOST", "127.0.0.1")
SERVER_PORT = int(os.environ.get("E2E_PORT", "16667"))  # Use non-standard port for tests
# If E2E_MANUAL=1, assume server is already running and don't start one
MANUAL_MODE = os.environ.get("E2E_MANUAL", "0") == "1"
STARTUP_TIMEOUT = 10.0
CLIENT_TIMEOUT = 5.0

# Paths
WORKSPACE_ROOT = Path(__file__).parent.parent.parent.parent  # /home/straylight
SLIRCD_ROOT = WORKSPACE_ROOT / "slircd-ng"


def find_server_binary() -> Optional[Path]:
    """Find the slircd binary."""
    # Try release first, then debug
    for profile in ["release", "debug"]:
        binary = WORKSPACE_ROOT / "target" / profile / "slircd"
        if binary.exists():
            return binary
    return None


def generate_test_config(tmpdir: Path, port: int) -> Path:
    """Generate a minimal test configuration."""
    config_content = f'''
[server]
name = "test.irc.local"
network = "TestNet"
sid = "001"
description = "slircd-ng Test Server"
metrics_port = 0

[listen]
address = "127.0.0.1:{port}"

[database]
path = "{tmpdir}/test.db"

[limits]
rate = 100.0
burst = 50.0

[security]
cloak_secret = "test-secret-for-e2e-testing"
cloak_suffix = "test"
spam_detection_enabled = false

[security.rate_limits]
message_rate_per_second = 100
connection_burst_per_ip = 100
join_burst_per_client = 100
'''
    config_path = tmpdir / "config.toml"
    config_path.write_text(config_content)
    return config_path


async def wait_for_port(host: str, port: int, timeout: float = 10.0) -> bool:
    """Wait for a port to become available."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            reader, writer = await asyncio.wait_for(
                asyncio.open_connection(host, port),
                timeout=1.0
            )
            writer.close()
            await writer.wait_closed()
            return True
        except (OSError, asyncio.TimeoutError):
            await asyncio.sleep(0.1)
    return False


class ManualServerProcess:
    """A fake server process for when the server is already running externally."""

    def __init__(self, host: str, port: int):
        self.host = host
        self.port = port

    @property
    def running(self) -> bool:
        return True


def kill_stale_servers(port: int) -> None:
    """Kill any stale slircd processes that might be running."""
    # First, check if anything is listening on our port
    try:
        result = subprocess.run(
            ["lsof", "-ti", f":{port}"],
            capture_output=True,
            text=True,
            timeout=5
        )
        if result.stdout.strip():
            pids = result.stdout.strip().split('\n')
            for pid in pids:
                try:
                    os.kill(int(pid), signal.SIGTERM)
                    time.sleep(0.1)
                except (ValueError, OSError):
                    pass
    except (subprocess.TimeoutExpired, FileNotFoundError):
        pass

    # Also kill any slircd processes in target directory
    try:
        result = subprocess.run(
            ["pgrep", "-f", "target/.*slircd"],
            capture_output=True,
            text=True,
            timeout=5
        )
        if result.stdout.strip():
            pids = result.stdout.strip().split('\n')
            for pid in pids:
                try:
                    os.kill(int(pid), signal.SIGTERM)
                except (ValueError, OSError):
                    pass
            time.sleep(0.2)  # Give processes time to exit
    except (subprocess.TimeoutExpired, FileNotFoundError):
        pass


class ServerProcess:
    """Manages a slircd-ng server process for testing."""

    def __init__(self, host: str, port: int, tmpdir: Path):
        self.host = host
        self.port = port
        self.tmpdir = tmpdir
        self.proc: Optional[subprocess.Popen] = None
        self.config_path: Optional[Path] = None

    async def start(self) -> None:
        """Start the server."""
        # Kill any stale server processes first
        kill_stale_servers(self.port)

        self.config_path = generate_test_config(self.tmpdir, self.port)

        # Always rebuild to ensure we test current code
        print(f"Building slircd-ng...", file=sys.stderr)
        build_result = subprocess.run(
            ["cargo", "build", "-p", "slircd-ng"],
            cwd=str(WORKSPACE_ROOT),
            capture_output=True,
            text=True
        )
        if build_result.returncode != 0:
            raise RuntimeError(f"Failed to build server: {build_result.stderr}")

        # Start server
        self.proc = subprocess.Popen(
            ["cargo", "run", "-p", "slircd-ng", "--", str(self.config_path)],
            cwd=str(WORKSPACE_ROOT),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

        # Wait for port to be available
        if not await wait_for_port(self.host, self.port):
            self.stop()
            raise RuntimeError(f"Server did not start on {self.host}:{self.port}")

    def stop(self) -> None:
        """Stop the server."""
        if self.proc:
            self.proc.send_signal(signal.SIGTERM)
            try:
                self.proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()
            self.proc = None

    @property
    def running(self) -> bool:
        return self.proc is not None and self.proc.poll() is None


# Global server instance for session scope
_server_instance: Optional[ServerProcess] = None
_server_tmpdir: Optional[Path] = None


@pytest.fixture(scope="session")
def event_loop():
    """Create event loop for the session."""
    loop = asyncio.new_event_loop()
    yield loop
    loop.close()


@pytest.fixture(scope="session")
def server_tmpdir() -> Generator[Path, None, None]:
    """Create a temporary directory for the server."""
    global _server_tmpdir
    if MANUAL_MODE:
        # In manual mode, we don't need a temp dir but fixtures might reference it
        _server_tmpdir = Path(tempfile.mkdtemp(prefix="slircd-e2e-"))
    else:
        _server_tmpdir = Path(tempfile.mkdtemp(prefix="slircd-e2e-"))
    yield _server_tmpdir
    shutil.rmtree(_server_tmpdir, ignore_errors=True)


@pytest_asyncio.fixture(scope="session")
async def server(server_tmpdir: Path):
    """
    Start slircd-ng server for the test session.

    The server runs for the entire test session to avoid startup overhead.
    Set E2E_MANUAL=1 to use an externally running server instead.
    """
    global _server_instance

    if MANUAL_MODE:
        # Use already running server
        fake_server = ManualServerProcess(SERVER_HOST, SERVER_PORT)
        # Verify server is actually running
        if not await wait_for_port(SERVER_HOST, SERVER_PORT, timeout=2.0):
            pytest.skip(f"Manual mode enabled but server not running on {SERVER_HOST}:{SERVER_PORT}")
        yield fake_server
        return

    _server_instance = ServerProcess(SERVER_HOST, SERVER_PORT, server_tmpdir)
    await _server_instance.start()

    yield _server_instance

    _server_instance.stop()
    _server_instance = None


@pytest_asyncio.fixture
async def client(server: ServerProcess) -> AsyncGenerator[IrcClient, None]:
    """
    Create a connected and registered IRC client.

    This fixture provides a fresh client for each test.
    """
    c = IrcClient(
        host=server.host,
        port=server.port,
        timeout=CLIENT_TIMEOUT,
    )

    await c.connect()
    await c.register()

    yield c

    try:
        await c.quit()
    except Exception:
        pass


@pytest_asyncio.fixture
async def unregistered_client(server: ServerProcess) -> AsyncGenerator[IrcClient, None]:
    """
    Create a connected but NOT registered IRC client.

    Useful for testing registration flows.
    """
    c = IrcClient(
        host=server.host,
        port=server.port,
        timeout=CLIENT_TIMEOUT,
    )

    await c.connect()

    yield c

    try:
        await c.disconnect()
    except Exception:
        pass


@pytest_asyncio.fixture
async def clients(server: ServerProcess) -> AsyncGenerator[MultiClientManager, None]:
    """
    Create a multi-client manager for tests needing multiple connections.

    Usage:
        async def test_messaging(clients):
            alice = await clients.add_client("alice")
            bob = await clients.add_client("bob")
            await alice.privmsg(bob.nick, "Hello!")
    """
    async with MultiClientManager(host=server.host, port=server.port) as manager:
        yield manager


@pytest.fixture
def server_host(server: ServerProcess) -> str:
    """Get the server host."""
    return server.host


@pytest.fixture
def server_port(server: ServerProcess) -> int:
    """Get the server port."""
    return server.port


# Markers for test categorization
def pytest_configure(config):
    """Register custom markers."""
    config.addinivalue_line("markers", "slow: marks tests as slow")
    config.addinivalue_line("markers", "services: tests requiring NickServ/ChanServ")
    config.addinivalue_line("markers", "ircv3: IRCv3 capability tests")
    config.addinivalue_line("markers", "rfc1459: RFC 1459 compliance tests")
    config.addinivalue_line("markers", "rfc2812: RFC 2812 compliance tests")


# Skip if server not available
def pytest_collection_modifyitems(config, items):
    """Skip tests if server binary not found (unless in manual mode)."""
    # In manual mode, we use an already-running server, so binary check is irrelevant
    if os.environ.get("E2E_MANUAL", "0") == "1":
        return

    binary = find_server_binary()
    if binary is None:
        skip_no_server = pytest.mark.skip(reason="slircd-ng binary not found - run cargo build first")
        for item in items:
            if "e2e" in str(item.fspath):
                item.add_marker(skip_no_server)
