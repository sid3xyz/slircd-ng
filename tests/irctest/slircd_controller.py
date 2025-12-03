"""
slircd-ng controller for irctest framework.

This module provides a controller that allows irctest to spawn and manage
slircd-ng instances for compliance testing.

Usage with irctest:
    cd ~/irctest
    pytest --controller path/to/slircd_controller.py -k 'not deprecated'

Or set IRCTEST_SERVER_HOSTNAME and IRCTEST_SERVER_PORT to test a running server:
    IRCTEST_SERVER_HOSTNAME=127.0.0.1 IRCTEST_SERVER_PORT=6667 \
    pytest --controller irctest.controllers.external_server -k 'not deprecated'
"""

import os
import shutil
import subprocess
import tempfile
from pathlib import Path
from typing import Optional, Type

# Import from irctest - these will be available when running under irctest
try:
    from irctest.basecontrollers import (
        BaseServerController,
        DirectoryBasedController,
    )
    from irctest.cases import BaseServerTestCase
    IRCTEST_AVAILABLE = True
except ImportError:
    # Stubs for when irctest is not installed
    IRCTEST_AVAILABLE = False

    class BaseServerController:
        pass

    class DirectoryBasedController:
        pass

    class BaseServerTestCase:
        pass


# Path to workspace root
WORKSPACE_ROOT = Path(__file__).parent.parent.parent.parent  # /home/straylight


def find_cargo_target() -> Optional[Path]:
    """Find cargo target directory."""
    target = WORKSPACE_ROOT / "target"
    if target.exists():
        return target
    return None


class SlircdController(BaseServerController, DirectoryBasedController):
    """Controller for slircd-ng IRC server."""

    software_name = "slircd-ng"

    # Capabilities we support
    supported_sasl_mechanisms = {"PLAIN"}

    # Feature flags
    supports_sts = False  # STS not implemented yet

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self._config_path: Optional[Path] = None

    def create_config(self) -> None:
        """Create configuration files."""
        super().create_config()
        # Create empty config file to be filled in run()
        if hasattr(self, 'directory') and self.directory:
            (self.directory / "config.toml").touch()

    def _generate_config(
        self,
        hostname: str,
        port: int,
        password: Optional[str] = None,
        ssl: bool = False,
    ) -> str:
        """Generate slircd-ng config.toml content."""
        config = f'''
[server]
name = "{hostname}"
network = "IRCTest"
sid = "001"
description = "slircd-ng IRCTest Server"
metrics_port = 0

[listen]
address = "127.0.0.1:{port}"

[database]
path = "{self.directory}/test.db"

[limits]
rate = 100.0
burst = 50.0

[security]
cloak_secret = "irctest-secret-key"
cloak_suffix = "test"
spam_detection_enabled = false

[security.rate_limits]
message_rate_per_second = 100
connection_burst_per_ip = 100
join_burst_per_client = 100
'''

        if password:
            config += f'\n[auth]\nserver_password = "{password}"\n'

        if ssl:
            # Generate self-signed certs
            cert_path = self.directory / "server.crt"
            key_path = self.directory / "server.key"

            # Generate certs using openssl if available
            if shutil.which("openssl"):
                subprocess.run([
                    "openssl", "req", "-x509", "-newkey", "rsa:2048",
                    "-keyout", str(key_path),
                    "-out", str(cert_path),
                    "-days", "1",
                    "-nodes",
                    "-subj", "/CN=localhost"
                ], check=True, capture_output=True)

                config += f'''
[tls]
address = "127.0.0.1:{port}"
cert_path = "{cert_path}"
key_path = "{key_path}"
'''

        return config

    def run(
        self,
        hostname: str,
        port: int,
        *,
        password: Optional[str],
        ssl: bool,
        run_services: bool,
        faketime: Optional[str],
    ) -> None:
        """Start the slircd-ng server."""
        self.create_config()

        assert self.directory is not None

        # Generate config
        config_content = self._generate_config(hostname, port, password, ssl)
        self._config_path = self.directory / "config.toml"
        self._config_path.write_text(config_content)

        # Build the server if needed
        build_result = subprocess.run(
            ["cargo", "build", "-p", "slircd-ng", "--release"],
            cwd=str(WORKSPACE_ROOT),
            capture_output=True,
            text=True,
        )

        if build_result.returncode != 0:
            raise RuntimeError(f"Failed to build slircd-ng: {build_result.stderr}")

        # Find the binary
        binary = WORKSPACE_ROOT / "target" / "release" / "slircd-ng"
        if not binary.exists():
            binary = WORKSPACE_ROOT / "target" / "debug" / "slircd-ng"

        if not binary.exists():
            raise RuntimeError("slircd-ng binary not found after build")

        # Handle faketime
        command = []
        if faketime and shutil.which("faketime"):
            command.extend(["faketime", "-f", faketime])
            self.faketime_enabled = True
        else:
            self.faketime_enabled = False

        command.extend([str(binary), str(self._config_path)])

        # Start the server
        self.proc = self.execute(command)
        self.port = port

    def wait_for_services(self) -> None:
        """Wait for services to be ready."""
        # slircd-ng has built-in services that start with the server
        pass

    def registerUser(
        self,
        case: BaseServerTestCase,
        username: str,
        password: Optional[str] = None,
    ) -> None:
        """Register a user with NickServ."""
        if not case.run_services:
            raise ValueError(
                "Attempted to register a nick, but run_services is not True."
            )

        # Connect a client
        client = case.addClient(show_io=False)
        case.sendLine(client, "CAP LS 302")
        case.sendLine(client, f"NICK {username}")
        case.sendLine(client, f"USER {username} 0 * :Test User")
        case.sendLine(client, "CAP END")

        # Wait for registration
        while True:
            msg = case.getRegistrationMessage(client)
            if msg.command == "001":
                break

        case.getMessages(client)

        # Register with NickServ
        if password:
            case.sendLine(client, f"PRIVMSG NickServ :REGISTER {password}")
            # Wait for confirmation
            for _ in range(10):
                msgs = case.getMessages(client)
                for msg in msgs:
                    if "registered" in msg.params[-1].lower():
                        break

        case.sendLine(client, "QUIT")
        case.assertDisconnected(client)


def get_irctest_controller_class() -> Type[SlircdController]:
    """Entry point for irctest to find our controller."""
    return SlircdController


# For testing this module directly
if __name__ == "__main__":
    print(f"IRCTEST_AVAILABLE: {IRCTEST_AVAILABLE}")
    print(f"WORKSPACE_ROOT: {WORKSPACE_ROOT}")

    if IRCTEST_AVAILABLE:
        print("Controller class ready for irctest")
    else:
        print("irctest not installed - install with: pip install irctest")
