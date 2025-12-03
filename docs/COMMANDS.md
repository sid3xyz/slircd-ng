# slircd-ng Command Reference

This document lists all IRC commands supported by slircd-ng.

## User Commands

### Connection & Registration

| Command | Syntax                            | Description                                   |
| ------- | --------------------------------- | --------------------------------------------- |
| `NICK`  | `NICK <nickname>`                 | Set or change your nickname                   |
| `USER`  | `USER <username> 0 * :<realname>` | Set username and realname during registration |
| `PASS`  | `PASS <password>`                 | Set connection password (before NICK/USER)    |
| `QUIT`  | `QUIT [:<reason>]`                | Disconnect from the server                    |
| `PING`  | `PING <token>`                    | Send ping to server (keep-alive)              |
| `PONG`  | `PONG <token>`                    | Respond to server PING                        |
| `CAP`   | `CAP <subcommand> [args]`         | IRCv3 capability negotiation                  |

### Channels

| Command  | Syntax                               | Description                              |
| -------- | ------------------------------------ | ---------------------------------------- |
| `JOIN`   | `JOIN <#channel> [key]`              | Join a channel                           |
| `PART`   | `PART <#channel> [:<reason>]`        | Leave a channel                          |
| `CYCLE`  | `CYCLE <#channel> [:<reason>]`       | Leave and rejoin a channel               |
| `TOPIC`  | `TOPIC <#channel> [:<new topic>]`    | View or set channel topic                |
| `NAMES`  | `NAMES <#channel>`                   | List users in a channel                  |
| `LIST`   | `LIST [pattern]`                     | List channels on the server              |
| `KICK`   | `KICK <#channel> <nick> [:<reason>]` | Kick a user from a channel (requires +o) |
| `INVITE` | `INVITE <nick> <#channel>`           | Invite a user to a channel               |
| `KNOCK`  | `KNOCK <#channel> [:<message>]`      | Request invite to a +i channel           |
| `MODE`   | `MODE <target> [modes] [params]`     | View or set channel/user modes           |

### Messaging

| Command   | Syntax                        | Description                            |
| --------- | ----------------------------- | -------------------------------------- |
| `PRIVMSG` | `PRIVMSG <target> :<message>` | Send a message to user or channel      |
| `NOTICE`  | `NOTICE <target> :<message>`  | Send a notice (no auto-reply expected) |
| `TAGMSG`  | `TAGMSG <target>`             | Send message tags only (IRCv3)         |

### User Queries

| Command    | Syntax                       | Description                         |
| ---------- | ---------------------------- | ----------------------------------- |
| `WHOIS`    | `WHOIS <nick>`               | Get information about a user        |
| `WHOWAS`   | `WHOWAS <nick> [count]`      | Get info about a disconnected user  |
| `WHO`      | `WHO <mask> [flags]`         | Search for users matching a pattern |
| `USERHOST` | `USERHOST <nick> [nick2...]` | Get user@host for nick(s)           |
| `ISON`     | `ISON <nick> [nick2...]`     | Check if nick(s) are online         |

### User Status

| Command   | Syntax                        | Description                          |
| --------- | ----------------------------- | ------------------------------------ |
| `AWAY`    | `AWAY [:<message>]`           | Set or clear away status             |
| `SETNAME` | `SETNAME :<new realname>`     | Change your realname (IRCv3)         |
| `SILENCE` | `SILENCE [+/-mask]`           | Manage server-side ignore list       |
| `MONITOR` | `MONITOR <+/-/C/L/S> [nicks]` | Track online status of nicks (IRCv3) |

### Server Information

| Command   | Syntax           | Description                    |
| --------- | ---------------- | ------------------------------ |
| `MOTD`    | `MOTD`           | Display message of the day     |
| `LUSERS`  | `LUSERS`         | Show user/channel statistics   |
| `VERSION` | `VERSION`        | Show server version            |
| `TIME`    | `TIME`           | Show server time               |
| `INFO`    | `INFO`           | Show server information        |
| `ADMIN`   | `ADMIN`          | Show administrative info       |
| `HELP`    | `HELP [command]` | Get help on commands           |
| `RULES`   | `RULES`          | Display server rules           |
| `MAP`     | `MAP`            | Show network map               |
| `LINKS`   | `LINKS`          | Show linked servers            |
| `STATS`   | `STATS <letter>` | Server statistics (see below)  |
| `USERIP`  | `USERIP <nick>`  | Get IP address for nick (oper) |

### Services

| Command        | Syntax                          | Description                  |
| -------------- | ------------------------------- | ---------------------------- |
| `NS`           | `NS <command> [args]`           | Shortcut for `/msg NickServ` |
| `CS`           | `CS <command> [args]`           | Shortcut for `/msg ChanServ` |
| `AUTHENTICATE` | `AUTHENTICATE <mechanism/data>` | SASL authentication          |

### Chat History (IRCv3)

| Command       | Syntax                                       | Description              |
| ------------- | -------------------------------------------- | ------------------------ |
| `CHATHISTORY` | `CHATHISTORY <subcommand> <target> <params>` | Retrieve message history |

---

## Operator Commands

These commands require IRC operator status (`/oper`). See [OPERATORS.md](OPERATORS.md) for details.

| Command   | Syntax                     | Description                  |
| --------- | -------------------------- | ---------------------------- |
| `OPER`    | `OPER <name> <password>`   | Authenticate as IRC operator |
| `KILL`    | `KILL <nick> :<reason>`    | Disconnect a user            |
| `WALLOPS` | `WALLOPS :<message>`       | Broadcast to users with +w   |
| `DIE`     | `DIE`                      | Shutdown the server          |
| `REHASH`  | `REHASH`                   | Reload configuration         |
| `RESTART` | `RESTART`                  | Restart the server           |
| `TRACE`   | `TRACE [target]`           | Trace connection info        |
| `CHGHOST` | `CHGHOST <nick> <newhost>` | Change a user's hostname     |
| `VHOST`   | `VHOST <nick> <vhost>`     | Set a virtual host           |

### Server Admin Commands (SA*)

| Command  | Syntax                      | Description                 |
| -------- | --------------------------- | --------------------------- |
| `SAJOIN` | `SAJOIN <nick> <#channel>`  | Force user to join channel  |
| `SAPART` | `SAPART <nick> <#channel>`  | Force user to leave channel |
| `SAMODE` | `SAMODE <#channel> <modes>` | Set modes without being op  |
| `SANICK` | `SANICK <nick> <newnick>`   | Force nick change           |

### Ban Commands (X-Lines)

| Command   | Syntax                                   | Description                  |
| --------- | ---------------------------------------- | ---------------------------- |
| `KLINE`   | `KLINE [duration] <user@host> :<reason>` | Ban a user@host              |
| `UNKLINE` | `UNKLINE <user@host>`                    | Remove K-Line                |
| `GLINE`   | `GLINE [duration] <user@host> :<reason>` | Network-wide ban             |
| `UNGLINE` | `UNGLINE <user@host>`                    | Remove G-Line                |
| `ZLINE`   | `ZLINE [duration] <ip> :<reason>`        | Ban an IP address            |
| `UNZLINE` | `UNZLINE <ip>`                           | Remove Z-Line                |
| `DLINE`   | `DLINE [duration] <ip> :<reason>`        | Deny connection from IP      |
| `UNDLINE` | `UNDLINE <ip>`                           | Remove D-Line                |
| `RLINE`   | `RLINE [duration] <regex> :<reason>`     | Ban by regex pattern         |
| `UNRLINE` | `UNRLINE <regex>`                        | Remove R-Line                |
| `SHUN`    | `SHUN [duration] <user@host> :<reason>`  | Silence a user (no messages) |
| `UNSHUN`  | `UNSHUN <user@host>`                     | Remove shun                  |

---

## STATS Letters

The `STATS` command accepts single-letter arguments:

| Letter | Description                         |
| ------ | ----------------------------------- |
| `c`    | Show C-lines (server links)         |
| `i`    | Show I-lines (client authorization) |
| `k`    | Show K-lines (bans)                 |
| `l`    | Show connection info                |
| `m`    | Show command usage statistics       |
| `o`    | Show O-lines (operators)            |
| `u`    | Show server uptime                  |
| `y`    | Show class lines                    |

---

## CAP Subcommands

| Subcommand   | Description                 |
| ------------ | --------------------------- |
| `LS [302]`   | List available capabilities |
| `REQ <caps>` | Request capabilities        |
| `END`        | End capability negotiation  |
| `LIST`       | List enabled capabilities   |

---

## CHATHISTORY Subcommands

| Subcommand                               | Description                 |
| ---------------------------------------- | --------------------------- |
| `LATEST <target> * <limit>`              | Get latest messages         |
| `BEFORE <target> <msgid> <limit>`        | Get messages before a point |
| `AFTER <target> <msgid> <limit>`         | Get messages after a point  |
| `AROUND <target> <msgid> <limit>`        | Get messages around a point |
| `BETWEEN <target> <start> <end> <limit>` | Get messages in range       |

---

## Examples

### Joining a Channel
```
NICK alice
USER alice 0 * :Alice Smith
JOIN #general
PRIVMSG #general :Hello everyone!
```

### Setting Channel Modes
```
MODE #general +nt         # No external messages, topic lock
MODE #general +o bob      # Give bob operator status
MODE #general +l 50       # Limit to 50 users
MODE #general +k secret   # Set channel key
```

### Using Services
```
NS REGISTER mypassword email@example.com
NS IDENTIFY mypassword
CS REGISTER #mychannel
```

### Operator Ban
```
OPER admin mypassword
KLINE 1d *@spammer.net :Spamming
```
