# End-to-End Tests for slircd-ng

Automated IRC protocol tests for the slircd-ng server.

## Quick Start

### Manual Mode (test against already-running server)

```bash
# Start server in one terminal
cargo run -p slircd-ng

# Run tests in another terminal
cd tests/e2e
source .venv/bin/activate
E2E_PORT=6667 E2E_MANUAL=1 pytest -v
```

### Automatic Mode (tests start their own server)

```bash
# Build server first
cargo build -p slircd-ng

# Run tests (will start server automatically)
cd tests/e2e
source .venv/bin/activate
pytest -v
```

## Test Structure

```
tests/e2e/
├── .venv/              # Python virtual environment
├── irc_client.py       # Async IRC client library
├── conftest.py         # pytest fixtures (server, client, etc.)
├── test_connection.py  # Connection/registration tests
├── test_channels.py    # Channel operation tests
├── test_messaging.py   # PRIVMSG/NOTICE/CTCP tests
├── requirements.txt    # Python dependencies
└── pytest.ini          # pytest configuration
```

## Environment Variables

| Variable     | Default     | Description                         |
| ------------ | ----------- | ----------------------------------- |
| `E2E_HOST`   | `127.0.0.1` | Server host                         |
| `E2E_PORT`   | `16667`     | Server port (auto), `6667` (manual) |
| `E2E_MANUAL` | `0`         | Set to `1` to skip auto-start       |

## Running Specific Tests

```bash
# Run only connection tests
pytest test_connection.py -v

# Run only channel tests
pytest test_channels.py -v

# Run tests matching a pattern
pytest -k "join" -v
pytest -k "privmsg" -v

# Run with short traceback
pytest --tb=short -v
```

## Fixtures Available

- `server` - Session-scoped server process
- `client` - Registered IRC client
- `unregistered_client` - Connected but not registered client
- `clients` - Multi-client manager for testing interactions

## Writing New Tests

```python
import pytest

class TestNewFeature:
    @pytest.mark.asyncio
    async def test_my_feature(self, client):
        """Test description."""
        await client.send("MYCOMMAND arg1 arg2")
        response = await client.expect("MYREPLY", field="command")
        assert response.params[0] == "expected"
```

## Known Issues

The E2E tests can expose real server bugs. Current known issues:
- Rate limiting is aggressive (some tests hit connection rate limits)
- QUIT handling has edge cases
- Some numerics may be missing from responses

These are server bugs, not test bugs - fix the server to make tests pass!
