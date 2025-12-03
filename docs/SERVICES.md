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

| Command    | Usage                            | Description                     |
| ---------- | -------------------------------- | ------------------------------- |
| `REGISTER` | `REGISTER #channel`              | Register a channel you're op in |
| `DROP`     | `DROP #channel`                  | Unregister a channel            |
| `INFO`     | `INFO #channel`                  | View channel information        |
| `SET`      | `SET #channel <option> <value>`  | Change channel settings         |
| `OP`       | `OP #channel [nick]`             | Grant operator status           |
| `DEOP`     | `DEOP #channel [nick]`           | Remove operator status          |
| `VOICE`    | `VOICE #channel [nick]`          | Grant voice                     |
| `DEVOICE`  | `DEVOICE #channel [nick]`        | Remove voice                    |
| `ACCESS`   | `ACCESS #channel <subcommand>`   | Manage access list              |
| `AKICK`    | `AKICK #channel <subcommand>`    | Manage auto-kick list           |

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

### Channel Settings (SET)

Configure channel options with the SET command:

```
/msg ChanServ SET #channel <option> <value>
```

| Option        | Values      | Description                              |
| ------------- | ----------- | ---------------------------------------- |
| `DESCRIPTION` | text        | Channel description shown in INFO        |
| `MLOCK`       | mode string | Modes to lock (e.g., `+nt-s`)            |
| `KEEPTOPIC`   | ON/OFF      | Preserve topic when channel empties      |

### Mode Lock (MLOCK)

MLOCK forces certain modes to always be set or unset on a channel:

```
/msg ChanServ SET #channel MLOCK +nt-s
```

This ensures:
- `+n` (no external messages) is always set
- `+t` (topic lock) is always set  
- `-s` (secret) is always unset

MLOCK is enforced when the channel is created (first user joins a registered channel).

Supported MLOCK modes:
- Simple flags: `n`, `t`, `s`, `i`, `m`, `r`, `c`, `C`, `N`
- Parameter modes: `k` (key), `l` (limit)

Example with key:
```
/msg ChanServ SET #channel MLOCK +ntk secretkey
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

| Pattern          | Matches                                  |
| ---------------- | ---------------------------------------- |
| `$a:account`     | Users logged into specific account       |
| `$U`             | Unregistered (not identified) users      |
| `$r:pattern`     | Users with matching realname (GECOS)     |
| `$s:servername`  | Users on specific server                 |
| `$c:#channel`    | Users in specific channel                |
| `$o:opertype`    | IRC operators of specific type           |
| `$x:fingerprint` | Users with matching TLS certificate      |
| `$z:mechanism`   | Users authenticated via SASL mechanism   |
| `$j:pattern`     | Users matching join pattern              |

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
