# slircd-ng Services Guide

This document covers the built-in IRC services: NickServ and ChanServ.

## NickServ

NickServ handles nickname registration and account management.

### Commands

| Command    | Usage                         | Description                          |
| ---------- | ----------------------------- | ------------------------------------ |
| `REGISTER` | `REGISTER <password> [email]` | Register your current nickname       |
| `IDENTIFY` | `IDENTIFY <password>`         | Log in to your account               |
| `GHOST`    | `GHOST <nick>`                | Disconnect a session using your nick |
| `INFO`     | `INFO <nick>`                 | View account information             |
| `SET`      | `SET <option> <value>`        | Configure account settings           |

### Registration

```
/msg NickServ REGISTER mypassword user@example.com
```

After registering, your nickname is protected. Other users will be forced to change their nick if they don't identify within the grace period.

### Identification

```
/msg NickServ IDENTIFY mypassword
```

You can also use SASL for automatic identification during connection.

### Ghost

If your connection drops and someone (or a ghost session) is using your nick:

```
/msg NickServ GHOST mynick
```

This disconnects the other session if you provide the correct password.

### Account Settings

```
/msg NickServ SET ENFORCE ON
/msg NickServ SET HIDE EMAIL ON
```

| Option       | Values | Description                         |
| ------------ | ------ | ----------------------------------- |
| `ENFORCE`    | ON/OFF | Force nick change if not identified |
| `HIDE EMAIL` | ON/OFF | Hide email in INFO output           |

### Nick Enforcement

When `ENFORCE` is enabled:
1. User connects with registered nick
2. NickServ sends "Please identify within N seconds"
3. If not identified in time, user is renamed to `Guest<random>`

## ChanServ

ChanServ handles channel registration and access control.

### Commands

| Command    | Usage                          | Description                     |
| ---------- | ------------------------------ | ------------------------------- |
| `REGISTER` | `REGISTER #channel`            | Register a channel you're op in |
| `INFO`     | `INFO #channel`                | View channel information        |
| `OP`       | `OP #channel [nick]`           | Grant operator status           |
| `DEOP`     | `DEOP #channel [nick]`         | Remove operator status          |
| `VOICE`    | `VOICE #channel [nick]`        | Grant voice                     |
| `DEVOICE`  | `DEVOICE #channel [nick]`      | Remove voice                    |
| `ACCESS`   | `ACCESS #channel <subcommand>` | Manage access list              |
| `AKICK`    | `AKICK #channel <subcommand>`  | Manage auto-kick list           |

### Channel Registration

First, join the channel and become operator:
```
/join #mychannel
```

Then register it:
```
/msg ChanServ REGISTER #mychannel
```

You become the channel founder with full control.

### Access Levels

ChanServ uses a flags-based access system:

| Flag | Meaning                        |
| ---- | ------------------------------ |
| `+o` | Can be auto-opped              |
| `+v` | Can be auto-voiced             |
| `+O` | Can use OP/DEOP commands       |
| `+V` | Can use VOICE/DEVOICE commands |
| `+A` | Can modify access list         |
| `+F` | Founder (full control)         |

### Managing Access

Add someone to access list:
```
/msg ChanServ ACCESS #channel ADD user +oOv
```

List access:
```
/msg ChanServ ACCESS #channel LIST
```

Remove access:
```
/msg ChanServ ACCESS #channel DEL user
```

### Auto-Kick (AKICK)

Add a mask to auto-kick:
```
/msg ChanServ AKICK #channel ADD *!*@spammer.host Spamming
```

List AKICK entries:
```
/msg ChanServ AKICK #channel LIST
```

Remove AKICK:
```
/msg ChanServ AKICK #channel DEL *!*@spammer.host
```

### Quick Op/Voice

```
/msg ChanServ OP #channel              # Op yourself
/msg ChanServ OP #channel someuser     # Op another user
/msg ChanServ VOICE #channel           # Voice yourself
```

## Service Aliases

For convenience, you can use short aliases:

| Alias | Target   |
| ----- | -------- |
| `/ns` | NickServ |
| `/cs` | ChanServ |

Example:
```
/ns identify mypassword
/cs op #channel
```

## Extended Bans

ChanServ and channel modes support extended bans that match beyond `nick!user@host`:

| Pattern         | Matches                                             |
| --------------- | --------------------------------------------------- |
| `$a:account`    | Users logged into specific account                  |
| `$a`            | Any logged-in user (inverted: `$~a` = unregistered) |
| `$r:realname*`  | Users with matching realname (GECOS)                |
| `$U`            | Unregistered users                                  |
| `$o`            | IRC operators                                       |
| `$O:opertype`   | Specific operator type                              |
| `$c:#channel`   | Users in specific channel                           |
| `$z`            | TLS users                                           |
| `$s:servername` | Users on specific server                            |
| `$j:#channel`   | Users matching ban in another channel               |

Examples:
```
# Ban unregistered users
/mode #channel +b $U

# Ban everyone from #badchannel
/mode #channel +b $c:#badchannel

# Only allow registered users (quiet unregistered)
/mode #channel +q $~a
```

## SASL Authentication

For automatic identification during connection, use SASL:

### PLAIN Mechanism

1. Client sends: `CAP REQ :sasl`
2. Client sends: `AUTHENTICATE PLAIN`
3. Client sends: `AUTHENTICATE <base64(authzid\0authcid\0password)>`

### SCRAM-SHA-256 Mechanism

More secure than PLAIN, recommended when available:

1. Client sends: `CAP REQ :sasl`
2. Client sends: `AUTHENTICATE SCRAM-SHA-256`
3. (SCRAM challenge-response exchange)

Most modern IRC clients handle SASL automatically when you configure server credentials.

## Database

Services data is stored in SQLite:

- **accounts** - NickServ accounts and settings
- **nicknames** - Registered nicknames linked to accounts
- **channels** - ChanServ channel registrations
- **channel_access** - Channel access lists
- **channel_akick** - Channel auto-kick lists

The database is automatically created at the path specified in `config.toml`.
