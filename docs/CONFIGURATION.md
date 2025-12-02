# slircd-ng Configuration Guide

This document describes all configuration options for slircd-ng.

## Configuration File

The server reads configuration from a TOML file, typically `config.toml`. Pass the path as the first command-line argument:

```bash
./slircd config.toml
```

## Sections

### [server]

Core server identity and settings.

```toml
[server]
name = "irc.example.net"        # Server hostname (shown to clients)
network = "ExampleNet"          # Network name (shown in MOTD, etc.)
sid = "001"                     # TS6 server ID (3 characters)
description = "My IRC Server"   # Server description
metrics_port = 9090             # Prometheus metrics HTTP port
```

| Option         | Type    | Required | Default | Description                          |
| -------------- | ------- | -------- | ------- | ------------------------------------ |
| `name`         | string  | yes      | -       | Server hostname shown to clients     |
| `network`      | string  | yes      | -       | Network name                         |
| `sid`          | string  | yes      | -       | TS6 server ID (3 alphanumeric chars) |
| `description`  | string  | no       | -       | Server description                   |
| `metrics_port` | integer | no       | 9090    | Prometheus HTTP port                 |

### [listen]

Plaintext TCP listener.

```toml
[listen]
address = "0.0.0.0:6667"
```

| Option    | Type   | Required | Default | Description                            |
| --------- | ------ | -------- | ------- | -------------------------------------- |
| `address` | string | yes      | -       | IP:port to bind (e.g., `0.0.0.0:6667`) |

### [tls]

Optional TLS/SSL listener.

```toml
[tls]
address = "0.0.0.0:6697"
cert_path = "/path/to/server.crt"
key_path = "/path/to/server.key"
```

| Option      | Type   | Required | Default | Description                   |
| ----------- | ------ | -------- | ------- | ----------------------------- |
| `address`   | string | yes      | -       | TLS listener address          |
| `cert_path` | string | yes      | -       | Path to PEM certificate chain |
| `key_path`  | string | yes      | -       | Path to PKCS8 private key     |

### [websocket]

Optional WebSocket listener for web clients.

```toml
[websocket]
address = "0.0.0.0:8080"
allow_origins = ["https://example.com"]
```

| Option          | Type   | Required | Default | Description                |
| --------------- | ------ | -------- | ------- | -------------------------- |
| `address`       | string | yes      | -       | WebSocket listener address |
| `allow_origins` | array  | no       | []      | CORS allowed origins       |

### [database]

SQLite database for persistent storage (services, bans, history).

```toml
[database]
path = "slircd.db"
```

| Option | Type   | Required | Default     | Description                  |
| ------ | ------ | -------- | ----------- | ---------------------------- |
| `path` | string | no       | `slircd.db` | Path to SQLite database file |

### [security]

Security and anti-abuse settings.

```toml
[security]
cloak_secret = "your-secret-key-here"
cloak_suffix = "ip"
spam_detection_enabled = true
```

| Option                   | Type    | Required | Default | Description                                      |
| ------------------------ | ------- | -------- | ------- | ------------------------------------------------ |
| `cloak_secret`           | string  | yes      | -       | HMAC secret for host cloaking (**CHANGE THIS!**) |
| `cloak_suffix`           | string  | no       | `ip`    | Suffix for cloaked addresses                     |
| `spam_detection_enabled` | boolean | no       | true    | Enable spam detection                            |

**WARNING**: The default `cloak_secret` provides NO privacy protection. Always set a unique, random secret in production.

### [security.rate_limits]

Per-client rate limiting for flood protection.

```toml
[security.rate_limits]
message_rate_per_second = 2
connection_burst_per_ip = 3
join_burst_per_client = 5
```

| Option                    | Type    | Required | Default | Description                      |
| ------------------------- | ------- | -------- | ------- | -------------------------------- |
| `message_rate_per_second` | integer | no       | 2       | Max messages/second per client   |
| `connection_burst_per_ip` | integer | no       | 3       | Max connections/10s per IP       |
| `join_burst_per_client`   | integer | no       | 5       | Max channel joins/10s per client |

### [limits]

Legacy rate limiting (deprecated, use `[security.rate_limits]`).

```toml
[limits]
rate = 2.5
burst = 5.0
```

### [[oper]]

IRC operator blocks. Repeat this section for each operator.

```toml
[[oper]]
name = "admin"
password_hash = "$argon2id$v=19$m=19456,t=2,p=1$..."
host = "*@trusted.host"
```

| Option          | Type   | Required | Description                          |
| --------------- | ------ | -------- | ------------------------------------ |
| `name`          | string | yes      | Operator username (for OPER command) |
| `password_hash` | string | yes      | Argon2id password hash               |
| `host`          | string | yes      | Required user@host mask              |

#### Generating Password Hashes

Use the `argon2` crate or a compatible tool:

```bash
# Using Python
python3 -c "from argon2 import PasswordHasher; print(PasswordHasher().hash('your_password'))"
```

### [[webirc]]

WEBIRC gateway blocks for trusted proxies.

```toml
[[webirc]]
password = "gateway-password"
hosts = ["192.168.1.0/24", "10.0.0.1"]
```

| Option     | Type   | Required | Description                        |
| ---------- | ------ | -------- | ---------------------------------- |
| `password` | string | yes      | Password sent by gateway           |
| `hosts`    | array  | yes      | Allowed gateway IP addresses/CIDRs |

## Complete Example

```toml
# slircd-ng Configuration

[server]
name = "irc.example.net"
network = "ExampleNet"
sid = "001"
description = "Example IRC Server"
metrics_port = 9090

[listen]
address = "0.0.0.0:6667"

[tls]
address = "0.0.0.0:6697"
cert_path = "/etc/ssl/certs/irc.pem"
key_path = "/etc/ssl/private/irc.key"

[websocket]
address = "0.0.0.0:8080"
allow_origins = ["https://webchat.example.net"]

[database]
path = "/var/lib/slircd/slircd.db"

[security]
cloak_secret = "change-this-to-a-random-64-char-string"
cloak_suffix = "users"
spam_detection_enabled = true

[security.rate_limits]
message_rate_per_second = 2
connection_burst_per_ip = 5
join_burst_per_client = 10

[[oper]]
name = "admin"
password_hash = "$argon2id$v=19$m=19456,t=2,p=1$YWJjZGVm$..."
host = "*@*.admin.example.net"

[[oper]]
name = "oper"
password_hash = "$argon2id$v=19$m=19456,t=2,p=1$Z2hpamts$..."
host = "*@*"

[[webirc]]
password = "webchat-secret"
hosts = ["192.168.1.100"]
```

## Environment Variables

| Variable   | Description                                              |
| ---------- | -------------------------------------------------------- |
| `RUST_LOG` | Logging level: `error`, `warn`, `info`, `debug`, `trace` |

Example:
```bash
RUST_LOG=debug ./slircd config.toml
```
