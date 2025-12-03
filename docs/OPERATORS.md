# slircd-ng Operator Guide

This document covers IRC operator commands and server administration.

## Becoming an Operator

To become an operator, you need:
1. An `[[oper]]` block in `config.toml`
2. A matching user@host mask
3. The correct password

```
/oper admin mypassword
```

On success, you receive user mode `+o` and a server notice.

## Operator Commands

### Server Control

| Command   | Usage                | Description               |
| --------- | -------------------- | ------------------------- |
| `DIE`     | `/die`               | Shutdown the server       |
| `REHASH`  | `/rehash`            | Reload configuration      |
| `WALLOPS` | `/wallops <message>` | Send to all users with +w |

### User Management

| Command   | Usage                       | Description                    |
| --------- | --------------------------- | ------------------------------ |
| `KILL`    | `/kill <nick> <reason>`     | Disconnect a user              |
| `CHGHOST` | `/chghost <nick> <newhost>` | Change user's visible hostname |

### Server-Wide Actions

| Command  | Usage                        | Description               |
| -------- | ---------------------------- | ------------------------- |
| `SAJOIN` | `/sajoin <nick> <#channel>`  | Force user into channel   |
| `SAPART` | `/sapart <nick> <#channel>`  | Force user out of channel |
| `SAMODE` | `/samode <#channel> <modes>` | Set modes without op      |
| `SANICK` | `/sanick <nick> <newnick>`   | Force nick change         |

### Server Information

| Command | Usage             | Description                 |
| ------- | ----------------- | --------------------------- |
| `TRACE` | `/trace [target]` | Show connection information |
| `STATS` | `/stats <letter>` | Server statistics           |
| `MAP`   | `/map`            | Show server map             |
| `LINKS` | `/links`          | Show linked servers         |

## Server Bans (X-Lines)

### Ban Types

| Type   | Command | Target              | Duration            |
| ------ | ------- | ------------------- | ------------------- |
| K-Line | `KLINE` | user@host (local)   | Temporary/permanent |
| G-Line | `GLINE` | user@host (network) | Temporary/permanent |
| D-Line | `DLINE` | IP address          | Temporary/permanent |
| Z-Line | `ZLINE` | IP (no DNS lookup)  | Temporary/permanent |
| R-Line | `RLINE` | Realname pattern    | Temporary/permanent |
| SHUN   | `SHUN`  | user@host (silent)  | Temporary/permanent |

### Adding Bans

Permanent ban:
```
/kline *@badhost.example.com :Reason for ban
/dline 192.168.1.100 :Reason for ban
/gline *@*.badnetwork.com :Network-wide ban
/zline 10.0.0.0/8 :Block entire range
/rline *bot* :Block matching realnames
```

Temporary ban (with duration):
```
/kline 1h *@tempban.host :Banned for 1 hour
/dline 30m 192.168.1.100 :Banned for 30 minutes
/gline 7d *@*.spam.net :Banned for 7 days
```

Duration formats:
- `30s` - 30 seconds
- `5m` - 5 minutes
- `2h` - 2 hours
- `7d` - 7 days

### Removing Bans

```
/unkline *@badhost.example.com
/undline 192.168.1.100
/ungline *@*.badnetwork.com
/unzline 10.0.0.0/8
/unrline *bot*
/unshun *@shunned.host
```

### SHUN

SHUN is a "silent ban" - the user stays connected but their messages are dropped:

```
/shun *@spammer.host :Spamming channels
```

Shunned users:
- Cannot send PRIVMSG/NOTICE
- Cannot join channels
- Cannot change nicks
- See no indication they're shunned

Shuns are stored in memory only (not persisted to database).

## STATS Command

| Letter | Information            |
| ------ | ---------------------- |
| `u`    | Server uptime          |
| `o`    | Configured operators   |
| `k`    | K-Lines                |
| `g`    | G-Lines                |
| `d`    | D-Lines                |
| `z`    | Z-Lines                |
| `c`    | Connection statistics  |
| `m`    | Command usage counts   |
| `?`    | Help (list all stats)  |

Example:
```
/stats o
/stats k
```

## Rate Limiting

The server applies rate limits automatically:

- **Message rate**: 2 messages/second per client
- **Connection rate**: 3 connections/10 seconds per IP
- **Join rate**: 5 joins/10 seconds per client

Users exceeding limits are temporarily throttled. Severe abuse may result in automatic disconnection.

Operators can monitor rate limit events in the Prometheus metrics:
```
irc_rate_limited_total
```

## Prometheus Metrics

The server exposes metrics on the configured port (default: 9090):

```
curl http://localhost:9090/metrics
```

Key metrics:

| Metric                      | Type    | Description               |
| --------------------------- | ------- | ------------------------- |
| `irc_connected_users`       | gauge   | Currently connected users |
| `irc_active_channels`       | gauge   | Active channels           |
| `irc_messages_sent_total`   | counter | Total messages sent       |
| `irc_spam_blocked_total`    | counter | Messages blocked as spam  |
| `irc_bans_triggered_total`  | counter | Ban enforcement events    |
| `irc_xlines_enforced_total` | counter | X-line enforcement events |
| `irc_rate_limited_total`    | counter | Rate limit hits           |

## Background Tasks

The server runs automatic maintenance:

| Task                 | Interval  | Action                 |
| -------------------- | --------- | ---------------------- |
| Nick enforcement     | 100ms     | Force nick changes     |
| WHOWAS cleanup       | 1 hour    | Prune old entries      |
| Shun expiry          | 1 minute  | Remove expired shuns   |
| Ban cache prune      | 5 minutes | Remove expired X-lines |
| Rate limiter cleanup | 5 minutes | Clean old buckets      |
| History prune        | 24 hours  | Remove old messages    |

## Logging

Control logging verbosity with `RUST_LOG`:

```bash
# Default (info)
./slircd config.toml

# Debug logging
RUST_LOG=debug ./slircd config.toml

# Trace logging (very verbose)
RUST_LOG=trace ./slircd config.toml

# Module-specific logging
RUST_LOG=slircd=debug,sqlx=warn ./slircd config.toml
```

Log levels: `error`, `warn`, `info`, `debug`, `trace`

## Troubleshooting

### Users Can't Connect

1. Check the listener is bound: `netstat -tlnp | grep 6667`
2. Check firewall rules
3. Check for D-line or Z-line blocking the IP
4. Check rate limits (too many connections)

### Users Can't Join Channels

1. Check channel modes (+i, +l, +k, +r)
2. Check ban list (+b)
3. Check if channel is +O (oper-only)
4. Check AKICK list

### Services Not Responding

1. Verify database path in config
2. Check database file permissions
3. Check logs for SQLite errors

### High Memory Usage

The server stores:
- User/channel state in memory (expected)
- Ban cache (pruned every 5 minutes)
- Rate limiter buckets (pruned every 5 minutes)
- Message history (pruned every 24 hours, stored in SQLite)

If memory grows unexpectedly, check:
- Number of connected users
- Number of channels
- CHATHISTORY volume
