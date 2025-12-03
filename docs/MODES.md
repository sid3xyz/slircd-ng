# slircd-ng Mode Reference

This document describes all user modes, channel modes, and channel member prefixes supported by slircd-ng.

---

## User Modes

Set with: `MODE <yournick> +/-<modes>`

| Mode | Name            | Description                                  |
| ---- | --------------- | -------------------------------------------- |
| `i`  | Invisible       | Hide from WHO/WHOIS unless in shared channel |
| `o`  | Operator        | IRC operator status (set via OPER command)   |
| `w`  | Wallops         | Receive WALLOPS messages                     |
| `r`  | Registered      | Identified to NickServ (set by services)     |
| `R`  | Registered-only | Only registered users can PM you             |
| `s`  | Server notices  | Receive server notices (with snomasks)       |
| `Z`  | Secure          | Connected via TLS (set automatically)        |

### Server Notice Masks (+s)

When +s is set, you can specify which notices to receive:

| Mask | Description                       |
| ---- | --------------------------------- |
| `c`  | Client connections/disconnections |
| `k`  | K-line/ban activity               |
| `o`  | Operator activity                 |
| `r`  | Rejected connections              |

Example: `MODE yournick +s +ck` (receive connection and k-line notices)

---

## Channel Modes

Set with: `MODE #channel +/-<modes> [parameters]`

### Basic Modes

| Mode | Name            | Parameters   | Description                            |
| ---- | --------------- | ------------ | -------------------------------------- |
| `i`  | Invite-only     | -            | Only invited users can join            |
| `m`  | Moderated       | -            | Only voiced (+v) or ops can speak      |
| `n`  | No external     | -            | Only channel members can send messages |
| `s`  | Secret          | -            | Channel hidden from LIST and WHOIS     |
| `t`  | Topic lock      | -            | Only ops can change topic              |
| `r`  | Registered-only | -            | Only registered users can join         |
| `k`  | Key             | `<password>` | Require password to join               |
| `l`  | Limit           | `<count>`    | Maximum number of users                |

### Protection Modes

| Mode | Name          | Parameters          | Description                              |
| ---- | ------------- | ------------------- | ---------------------------------------- |
| `f`  | Flood limit   | `<lines>:<seconds>` | Kick users who flood (e.g., `+f 5:10`)   |
| `j`  | Join throttle | `<joins>:<seconds>` | Limit join rate (e.g., `+j 3:10`)        |
| `J`  | Join delay    | `<seconds>`         | New joiners must wait before speaking    |
| `L`  | Redirect      | `<#channel>`        | Redirect excess users when +l is reached |

### Extended Modes

| Mode | Name           | Description                    |
| ---- | -------------- | ------------------------------ |
| `c`  | No colors      | Strip/block color codes        |
| `C`  | No CTCP        | Block CTCP (except ACTION)     |
| `g`  | Free invite    | Anyone can use INVITE          |
| `K`  | No knock       | Disable KNOCK for this channel |
| `N`  | No nick change | Members cannot change nick     |
| `O`  | Oper-only      | Only IRC operators can join    |
| `P`  | Permanent      | Channel persists with 0 users  |
| `T`  | No notice      | Block channel NOTICE           |
| `u`  | No kick        | Disable KICK (peace mode)      |
| `V`  | No invite      | Disable INVITE                 |
| `z`  | TLS-only       | Only TLS clients can join      |

### List Modes

| Mode | Name          | Parameters         | Description                |
| ---- | ------------- | ------------------ | -------------------------- |
| `b`  | Ban           | `<nick!user@host>` | Ban matching users         |
| `e`  | Except        | `<nick!user@host>` | Exempt from bans           |
| `I`  | Invite-except | `<nick!user@host>` | Exempt from +i             |
| `q`  | Quiet         | `<nick!user@host>` | Prevent user from speaking |

#### Extended Ban Syntax

Ban masks starting with `$` have special meaning:

| Prefix | Example          | Description               |
| ------ | ---------------- | ------------------------- |
| `$a:`  | `$a:accountname` | Match by NickServ account |
| `$r:`  | `$r:*bot*`       | Match by realname         |

Example: `MODE #channel +b $a:spammer` (ban by account)

---

## Channel Member Prefixes

| Prefix | Mode | Name     | Permissions                    |
| ------ | ---- | -------- | ------------------------------ |
| `~`    | `+q` | Owner    | Full control, cannot be kicked |
| `&`    | `+a` | Admin    | All ops powers, can op others  |
| `@`    | `+o` | Operator | Kick, ban, set modes           |
| `%`    | `+h` | Half-op  | Kick, ban (limited)            |
| `+`    | `+v` | Voice    | Speak in +m channels           |

Set with: `MODE #channel +o <nick>` (give operator)

---

## Common Mode Combinations

### Typical Public Channel
```
MODE #general +nt
```
No external messages, ops-only topic.

### Registered Users Only
```
MODE #verified +ntr
```
Only NickServ-identified users can join.

### Moderated Discussion
```
MODE #help +ntm
```
Only voiced users and ops can speak.

### Secure Channel
```
MODE #private +intsz
```
Invite-only, secret, TLS-required.

### Anti-Flood Settings
```
MODE #busy +f 5:10 +j 3:5
```
Kick after 5 messages in 10s, limit to 3 joins per 5s.

---

## ISUPPORT Tokens

The server advertises supported modes in the 005 numeric:

```
CHANMODES=beIq,k,flLjJ,imnstrRcCgKNOPTVuz
PREFIX=(qaohv)~&@%+
```

- **Type A** (list): `b`, `e`, `I`, `q`
- **Type B** (param always): `k`
- **Type C** (param on set): `f`, `l`, `L`, `j`, `J`
- **Type D** (no param): `i`, `m`, `n`, `s`, `t`, `r`, `R`, `c`, `C`, `g`, `K`, `N`, `O`, `P`, `T`, `V`, `u`, `z`
