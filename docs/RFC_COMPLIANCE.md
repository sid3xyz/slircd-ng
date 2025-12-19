# RFC Compliance Requirements

> **slircd-ng: A Baseline Reference Server for the Modern Age**

This document defines the complete set of requirements for a modern IRC server to be considered RFC-compliant and production-ready. It covers the core RFCs (1459, 2810, 2811, 2812, 7194) and essential IRCv3 extensions.

---

## Table of Contents

1. [Message Format](#1-message-format)
2. [Connection Registration](#2-connection-registration)
3. [Channel Operations](#3-channel-operations)
4. [Messaging](#4-messaging)
5. [User Queries](#5-user-queries)
6. [Server Queries](#6-server-queries)
7. [Operator Commands](#7-operator-commands)
8. [Numeric Replies](#8-numeric-replies)
9. [ISUPPORT Tokens](#9-isupport-tokens)
10. [Channel Modes](#10-channel-modes)
11. [User Modes](#11-user-modes)
12. [IRCv3 Capabilities](#12-ircv3-capabilities)
13. [TLS Requirements](#13-tls-requirements)
14. [Implementation Checklist](#14-implementation-checklist)

---

## 1. Message Format

**Source:** RFC 2812 Section 2.3

### 1.1 BNF Grammar

```abnf
message    = [ ":" prefix SPACE ] command [ params ] crlf
prefix     = servername / ( nickname [ [ "!" user ] "@" host ] )
command    = 1*letter / 3digit
params     = *14( SPACE middle ) [ SPACE ":" trailing ]
           / 14( SPACE middle ) [ SPACE [ ":" ] trailing ]
middle     = nospcrlfcl *( ":" / nospcrlfcl )
trailing   = *( ":" / " " / nospcrlfcl )
nospcrlfcl = %x01-09 / %x0B-0C / %x0E-1F / %x21-39 / %x3B-FF
SPACE      = %x20
crlf       = %x0D %x0A
```

### 1.2 Requirements

| Requirement                                                      | RFC             | Priority |
| ---------------------------------------------------------------- | --------------- | -------- |
| Messages MUST be terminated with CRLF                            | 2812 §2.3       | MUST     |
| Messages MUST NOT exceed 512 bytes including CRLF                | 2812 §2.3       | MUST     |
| Messages MUST have at most 15 parameters                         | 2812 §2.3       | MUST     |
| Prefix MUST be valid servername or nick!user@host                | 2812 §2.3       | MUST     |
| Command MUST be letters or 3-digit numeric                       | 2812 §2.3       | MUST     |
| Server SHOULD accept LF-only line endings                        | Modern practice | SHOULD   |
| Server SHOULD accept messages up to 4096 bytes with message-tags | IRCv3           | SHOULD   |

### 1.3 Nickname Format

```abnf
nickname = ( letter / special ) *8( letter / digit / special / "-" )
letter   = %x41-5A / %x61-7A  ; A-Z / a-z
digit    = %x30-39            ; 0-9
special  = %x5B-60 / %x7B-7D  ; [ ] \ ` _ ^ { | }
```

| Requirement                                                 | RFC             | Priority |
| ----------------------------------------------------------- | --------------- | -------- |
| Nicknames MUST start with letter or special                 | 2812 §2.3.1     | MUST     |
| Nicknames MUST NOT exceed NICKLEN                           | 2812 §2.3.1     | MUST     |
| Nicknames MUST NOT contain SPACE, NUL, CR, LF, comma, colon | 2812 §2.3.1     | MUST     |
| Server MUST advertise NICKLEN in ISUPPORT                   | Modern practice | MUST     |

### 1.4 Channel Name Format

```abnf
channel    = ( "#" / "+" / "!" / "&" ) chanstring [ ":" chanstring ]
chanstring = %x01-07 / %x08-09 / %x0B-0C / %x0E-1F / %x21-2B
           / %x2D-39 / %x3B-FF
           ; any octet except NUL, BELL, CR, LF, " ", ",", ":"
```

| Requirement                                                    | RFC             | Priority |
| -------------------------------------------------------------- | --------------- | -------- |
| Channel names MUST start with #, &, +, or !                    | 2811 §2.1       | MUST     |
| Channel names MUST NOT exceed CHANNELLEN                       | 2811 §2.1       | MUST     |
| Channel names MUST NOT contain SPACE, comma, BELL, NUL, CR, LF | 2811 §2.1       | MUST     |
| Server MUST advertise CHANTYPES in ISUPPORT                    | Modern practice | MUST     |

---

## 2. Connection Registration

**Source:** RFC 2812 Section 3.1

### 2.1 Registration Commands

#### PASS
```
Command:  PASS
Params:   <password>
```

| Requirement                                            | RFC             | Priority |
| ------------------------------------------------------ | --------------- | -------- |
| PASS MUST be sent before NICK/USER                     | 2812 §3.1.1     | MUST     |
| Server MUST accept PASS before or after NICK           | Modern practice | SHOULD   |
| Server MUST return 462 if PASS sent after registration | 2812 §3.1.1     | MUST     |

#### NICK
```
Command:  NICK
Params:   <nickname>
```

| Requirement                                              | RFC         | Priority |
| -------------------------------------------------------- | ----------- | -------- |
| Server MUST validate nickname format                     | 2812 §3.1.2 | MUST     |
| Server MUST return 431 ERR_NONICKNAMEGIVEN if no nick    | 2812 §3.1.2 | MUST     |
| Server MUST return 432 ERR_ERRONEUSNICKNAME if invalid   | 2812 §3.1.2 | MUST     |
| Server MUST return 433 ERR_NICKNAMEINUSE if in use       | 2812 §3.1.2 | MUST     |
| Server MUST return 436 ERR_NICKCOLLISION on collision    | 2812 §3.1.2 | MUST     |
| Server MUST broadcast NICK change to all common channels | 2812 §3.1.2 | MUST     |

#### USER
```
Command:  USER
Params:   <user> <mode> <unused> <realname>
```

| Requirement                                                       | RFC         | Priority |
| ----------------------------------------------------------------- | ----------- | -------- |
| Server MUST require USER for registration                         | 2812 §3.1.3 | MUST     |
| Server MUST return 461 ERR_NEEDMOREPARAMS if params missing       | 2812 §3.1.3 | MUST     |
| Server MUST return 462 ERR_ALREADYREGISTRED if already registered | 2812 §3.1.3 | MUST     |
| Server SHOULD apply mode 8 for +i, mode 4 for +w                  | 2812 §3.1.3 | SHOULD   |
| Server MUST use realname as GECOS field                           | 2812 §3.1.3 | MUST     |

#### OPER
```
Command:  OPER
Params:   <name> <password>
```

| Requirement                                          | RFC         | Priority |
| ---------------------------------------------------- | ----------- | -------- |
| Server MUST validate operator credentials            | 2812 §3.1.4 | MUST     |
| Server MUST return 464 ERR_PASSWDMISMATCH on failure | 2812 §3.1.4 | MUST     |
| Server MUST return 381 RPL_YOUREOPER on success      | 2812 §3.1.4 | MUST     |
| Server MUST set user mode +o on success              | 2812 §3.1.4 | MUST     |

#### QUIT
```
Command:  QUIT
Params:   [ <message> ]
```

| Requirement                                       | RFC         | Priority |
| ------------------------------------------------- | ----------- | -------- |
| Server MUST close connection after QUIT           | 2812 §3.1.7 | MUST     |
| Server MUST broadcast QUIT to all common channels | 2812 §3.1.7 | MUST     |
| Server SHOULD include quit message in broadcast   | 2812 §3.1.7 | SHOULD   |
| Server MUST send ERROR before closing connection  | 2812 §3.1.7 | MUST     |

### 2.2 Registration Sequence

```
Client → Server: CAP LS 302
Server → Client: CAP * LS :multi-prefix sasl ...
Client → Server: CAP REQ :multi-prefix sasl
Server → Client: CAP * ACK :multi-prefix sasl
Client → Server: AUTHENTICATE PLAIN
...SASL exchange...
Client → Server: CAP END
Client → Server: NICK mynick
Client → Server: USER myuser 0 * :My Real Name
Server → Client: 001 mynick :Welcome to the ExampleNet IRC Network
Server → Client: 002 mynick :Your host is irc.example.net
Server → Client: 003 mynick :This server was created ...
Server → Client: 004 mynick irc.example.net slircd-1.0 iowsB biklmnopstv
Server → Client: 005 mynick NETWORK=ExampleNet NICKLEN=30 ...
...more 005 lines...
Server → Client: 375 mynick :- irc.example.net Message of the Day -
Server → Client: 372 mynick :- Welcome to ExampleNet!
Server → Client: 376 mynick :End of /MOTD command.
```

| Requirement                                            | RFC             | Priority |
| ------------------------------------------------------ | --------------- | -------- |
| Server MUST send 001-004 upon successful registration  | 2812 §5.1       | MUST     |
| Server MUST send at least one 005 with ISUPPORT tokens | Modern practice | MUST     |
| Server SHOULD send MOTD (375/372/376 or 422)           | 2812 §5.1       | SHOULD   |
| Server MUST process CAP before registration completes  | IRCv3           | MUST     |

---

## 3. Channel Operations

**Source:** RFC 2812 Section 3.2, RFC 2811

### 3.1 JOIN
```
Command:  JOIN
Params:   <channel>{,<channel>} [<key>{,<key>}]
          JOIN 0  ; leave all channels
```

| Requirement                                                    | RFC             | Priority |
| -------------------------------------------------------------- | --------------- | -------- |
| Server MUST add user to channel                                | 2812 §3.2.1     | MUST     |
| Server MUST send JOIN to all channel members                   | 2812 §3.2.1     | MUST     |
| Server MUST send topic (332/331) to joining user               | 2812 §3.2.1     | MUST     |
| Server MUST send NAMES (353/366) to joining user               | 2812 §3.2.1     | MUST     |
| Server MUST return 403 ERR_NOSUCHCHANNEL if invalid name       | 2812 §3.2.1     | MUST     |
| Server MUST return 471 ERR_CHANNELISFULL if +l limit reached   | 2811 §4.3.1     | MUST     |
| Server MUST return 473 ERR_INVITEONLYCHAN if +i without invite | 2811 §4.3.1     | MUST     |
| Server MUST return 474 ERR_BANNEDFROMCHAN if banned            | 2811 §4.3.1     | MUST     |
| Server MUST return 475 ERR_BADCHANNELKEY if wrong key          | 2811 §4.3.1     | MUST     |
| Server SHOULD support JOIN 0 to leave all channels             | 2812 §3.2.1     | SHOULD   |
| Server SHOULD give founder +o on channel creation              | Modern practice | SHOULD   |

### 3.2 PART
```
Command:  PART
Params:   <channel>{,<channel>} [<message>]
```

| Requirement                                                       | RFC         | Priority |
| ----------------------------------------------------------------- | ----------- | -------- |
| Server MUST remove user from channel                              | 2812 §3.2.2 | MUST     |
| Server MUST send PART to all channel members                      | 2812 §3.2.2 | MUST     |
| Server MUST return 403 ERR_NOSUCHCHANNEL if channel doesn't exist | 2812 §3.2.2 | MUST     |
| Server MUST return 442 ERR_NOTONCHANNEL if not on channel         | 2812 §3.2.2 | MUST     |

### 3.3 TOPIC
```
Command:  TOPIC
Params:   <channel> [<topic>]
```

| Requirement                                                  | RFC             | Priority |
| ------------------------------------------------------------ | --------------- | -------- |
| Server MUST return 331 RPL_NOTOPIC if no topic set           | 2812 §3.2.4     | MUST     |
| Server MUST return 332 RPL_TOPIC with current topic          | 2812 §3.2.4     | MUST     |
| Server SHOULD return 333 with topic setter and timestamp     | Modern practice | SHOULD   |
| Server MUST return 482 ERR_CHANOPRIVSNEEDED if +t and not op | 2812 §3.2.4     | MUST     |
| Server MUST broadcast topic change to channel                | 2812 §3.2.4     | MUST     |

### 3.4 NAMES
```
Command:  NAMES
Params:   [<channel>{,<channel>}]
```

| Requirement                                               | RFC         | Priority |
| --------------------------------------------------------- | ----------- | -------- |
| Server MUST return 353 RPL_NAMREPLY with members          | 2812 §3.2.5 | MUST     |
| Server MUST return 366 RPL_ENDOFNAMES                     | 2812 §3.2.5 | MUST     |
| Server MUST prefix nicks with channel status (@, +, etc.) | 2812 §3.2.5 | MUST     |
| Server MUST respect +s/+p visibility rules                | 2811 §4.2.6 | MUST     |
| Server SHOULD support userhost-in-names capability        | IRCv3       | SHOULD   |
| Server SHOULD support multi-prefix capability             | IRCv3       | SHOULD   |

### 3.5 LIST
```
Command:  LIST
Params:   [<channel>{,<channel>}] [<elistconds>]
```

| Requirement                                      | RFC             | Priority |
| ------------------------------------------------ | --------------- | -------- |
| Server MUST return 321 RPL_LISTSTART             | 2812 §3.2.6     | MUST     |
| Server MUST return 322 RPL_LIST for each channel | 2812 §3.2.6     | MUST     |
| Server MUST return 323 RPL_LISTEND               | 2812 §3.2.6     | MUST     |
| Server MUST NOT show +s channels to non-members  | 2811 §4.2.6     | MUST     |
| Server SHOULD support ELIST conditions           | Modern practice | SHOULD   |
| Server SHOULD advertise SAFELIST in ISUPPORT     | Modern practice | SHOULD   |

### 3.6 INVITE
```
Command:  INVITE
Params:   <nickname> <channel>
```

| Requirement                                                  | RFC         | Priority |
| ------------------------------------------------------------ | ----------- | -------- |
| Server MUST send INVITE to target user                       | 2812 §3.2.7 | MUST     |
| Server MUST return 341 RPL_INVITING to inviter               | 2812 §3.2.7 | MUST     |
| Server MUST return 401 ERR_NOSUCHNICK if target not found    | 2812 §3.2.7 | MUST     |
| Server MUST return 442 ERR_NOTONCHANNEL if not on channel    | 2812 §3.2.7 | MUST     |
| Server MUST return 443 ERR_USERONCHANNEL if already on       | 2812 §3.2.7 | MUST     |
| Server MUST return 482 ERR_CHANOPRIVSNEEDED if +i and not op | 2812 §3.2.7 | MUST     |
| Server SHOULD support invite-notify capability               | IRCv3       | SHOULD   |

### 3.7 KICK
```
Command:  KICK
Params:   <channel> <user> [<comment>]
```

| Requirement                                                  | RFC         | Priority |
| ------------------------------------------------------------ | ----------- | -------- |
| Server MUST remove user from channel                         | 2812 §3.2.8 | MUST     |
| Server MUST send KICK to all channel members                 | 2812 §3.2.8 | MUST     |
| Server MUST return 403 ERR_NOSUCHCHANNEL if not found        | 2812 §3.2.8 | MUST     |
| Server MUST return 441 ERR_USERNOTINCHANNEL if target not on | 2812 §3.2.8 | MUST     |
| Server MUST return 442 ERR_NOTONCHANNEL if kicker not on     | 2812 §3.2.8 | MUST     |
| Server MUST return 482 ERR_CHANOPRIVSNEEDED if not op        | 2812 §3.2.8 | MUST     |

---

## 4. Messaging

**Source:** RFC 2812 Section 3.3

### 4.1 PRIVMSG
```
Command:  PRIVMSG
Params:   <target>{,<target>} <text>
```

| Requirement                                                   | RFC             | Priority |
| ------------------------------------------------------------- | --------------- | -------- |
| Server MUST deliver message to target                         | 2812 §3.3.1     | MUST     |
| Server MUST return 401 ERR_NOSUCHNICK if user not found       | 2812 §3.3.1     | MUST     |
| Server MUST return 403 ERR_NOSUCHCHANNEL if channel not found | 2812 §3.3.1     | MUST     |
| Server MUST return 404 ERR_CANNOTSENDTOCHAN if +n/+m blocked  | 2812 §3.3.1     | MUST     |
| Server MUST return 411 ERR_NORECIPIENT if no target           | 2812 §3.3.1     | MUST     |
| Server MUST return 412 ERR_NOTEXTTOSEND if no text            | 2812 §3.3.1     | MUST     |
| Server MUST return 301 RPL_AWAY if target is away             | 2812 §3.3.1     | MUST     |
| Server MUST NOT echo PRIVMSG back unless echo-message enabled | IRCv3           | MUST     |
| Server SHOULD support STATUSMSG (@#channel, +#channel)        | Modern practice | SHOULD   |

### 4.2 NOTICE
```
Command:  NOTICE
Params:   <target>{,<target>} <text>
```

| Requirement                                          | RFC         | Priority |
| ---------------------------------------------------- | ----------- | -------- |
| Server MUST NOT generate automatic replies to NOTICE | 2812 §3.3.2 | MUST     |
| Server MUST NOT return errors for NOTICE             | 2812 §3.3.2 | MUST     |
| Server SHOULD deliver NOTICE same as PRIVMSG         | 2812 §3.3.2 | SHOULD   |

---

## 5. User Queries

**Source:** RFC 2812 Section 3.6

### 5.1 WHO
```
Command:  WHO
Params:   [<mask>] [o]
```

| Requirement                                            | RFC             | Priority |
| ------------------------------------------------------ | --------------- | -------- |
| Server MUST return 352 RPL_WHOREPLY for matching users | 2812 §3.6.1     | MUST     |
| Server MUST return 315 RPL_ENDOFWHO                    | 2812 §3.6.1     | MUST     |
| Server MUST respect +i visibility rules                | 2812 §3.6.1     | MUST     |
| Server SHOULD support WHOX extended format             | Modern practice | SHOULD   |
| Server SHOULD advertise WHOX in ISUPPORT               | Modern practice | SHOULD   |

### 5.2 WHOIS
```
Command:  WHOIS
Params:   [<server>] <nickmask>{,<nickmask>}
```

| Requirement                                           | RFC             | Priority |
| ----------------------------------------------------- | --------------- | -------- |
| Server MUST return 311 RPL_WHOISUSER                  | 2812 §3.6.2     | MUST     |
| Server MUST return 312 RPL_WHOISSERVER                | 2812 §3.6.2     | MUST     |
| Server MUST return 319 RPL_WHOISCHANNELS for channels | 2812 §3.6.2     | MUST     |
| Server MUST return 318 RPL_ENDOFWHOIS                 | 2812 §3.6.2     | MUST     |
| Server MUST return 313 RPL_WHOISOPERATOR for opers    | 2812 §3.6.2     | MUST     |
| Server SHOULD return 301 RPL_AWAY if away             | 2812 §3.6.2     | SHOULD   |
| Server SHOULD return 317 RPL_WHOISIDLE with idle time | 2812 §3.6.2     | SHOULD   |
| Server SHOULD return 330 for authenticated users      | Modern practice | SHOULD   |
| Server SHOULD return 671 for TLS users                | Modern practice | SHOULD   |
| Server SHOULD return 276 for CERTFP                   | Modern practice | SHOULD   |

### 5.3 WHOWAS
```
Command:  WHOWAS
Params:   <nickname> [<count> [<server>]]
```

| Requirement                                           | RFC         | Priority |
| ----------------------------------------------------- | ----------- | -------- |
| Server MUST return 314 RPL_WHOWASUSER                 | 2812 §3.6.3 | MUST     |
| Server MUST return 369 RPL_ENDOFWHOWAS                | 2812 §3.6.3 | MUST     |
| Server MUST return 406 ERR_WASNOSUCHNICK if not found | 2812 §3.6.3 | MUST     |
| Server SHOULD maintain WHOWAS history                 | 2812 §3.6.3 | SHOULD   |

### 5.4 USERHOST
```
Command:  USERHOST
Params:   <nickname>{<space><nickname>}
```

| Requirement                             | RFC         | Priority |
| --------------------------------------- | ----------- | -------- |
| Server MUST return 302 RPL_USERHOST     | 2812 §3.6.4 | MUST     |
| Server MUST support up to 5 nicknames   | 2812 §3.6.4 | MUST     |
| Server MUST include * for opers         | 2812 §3.6.4 | MUST     |
| Server MUST include +/- for away status | 2812 §3.6.4 | MUST     |

### 5.5 ISON
```
Command:  ISON
Params:   <nickname>{<space><nickname>}
```

| Requirement                              | RFC         | Priority |
| ---------------------------------------- | ----------- | -------- |
| Server MUST return 303 RPL_ISON          | 2812 §3.6.5 | MUST     |
| Server MUST return only online nicknames | 2812 §3.6.5 | MUST     |

---

## 6. Server Queries

**Source:** RFC 2812 Section 3.4

### 6.1 MOTD
```
Command:  MOTD
Params:   [<server>]
```

| Requirement                                   | RFC         | Priority |
| --------------------------------------------- | ----------- | -------- |
| Server MUST return 375 RPL_MOTDSTART          | 2812 §3.4.1 | MUST     |
| Server MUST return 372 RPL_MOTD for each line | 2812 §3.4.1 | MUST     |
| Server MUST return 376 RPL_ENDOFMOTD          | 2812 §3.4.1 | MUST     |
| Server MUST return 422 ERR_NOMOTD if no MOTD  | 2812 §3.4.1 | MUST     |

### 6.2 LUSERS
```
Command:  LUSERS
Params:   [<mask> [<server>]]
```

| Requirement                              | RFC         | Priority |
| ---------------------------------------- | ----------- | -------- |
| Server MUST return 251 RPL_LUSERCLIENT   | 2812 §3.4.2 | MUST     |
| Server MUST return 252 RPL_LUSEROP       | 2812 §3.4.2 | MUST     |
| Server MUST return 253 RPL_LUSERUNKNOWN  | 2812 §3.4.2 | MUST     |
| Server MUST return 254 RPL_LUSERCHANNELS | 2812 §3.4.2 | MUST     |
| Server MUST return 255 RPL_LUSERME       | 2812 §3.4.2 | MUST     |

### 6.3 VERSION
```
Command:  VERSION
Params:   [<server>]
```

| Requirement                                | RFC             | Priority |
| ------------------------------------------ | --------------- | -------- |
| Server MUST return 351 RPL_VERSION         | 2812 §3.4.3     | MUST     |
| Server SHOULD include ISUPPORT in response | Modern practice | SHOULD   |

### 6.4 STATS
```
Command:  STATS
Params:   [<query> [<server>]]
```

| Requirement                                                      | RFC         | Priority |
| ---------------------------------------------------------------- | ----------- | -------- |
| Server MUST return 219 RPL_ENDOFSTATS                            | 2812 §3.4.4 | MUST     |
| Server SHOULD support common queries (c, h, i, k, l, m, o, u, y) | 2812 §3.4.4 | SHOULD   |

### 6.5 TIME
```
Command:  TIME
Params:   [<server>]
```

| Requirement                     | RFC         | Priority |
| ------------------------------- | ----------- | -------- |
| Server MUST return 391 RPL_TIME | 2812 §3.4.6 | MUST     |

### 6.6 ADMIN
```
Command:  ADMIN
Params:   [<server>]
```

| Requirement                           | RFC         | Priority |
| ------------------------------------- | ----------- | -------- |
| Server MUST return 256 RPL_ADMINME    | 2812 §3.4.9 | MUST     |
| Server MUST return 257-259 RPL_ADMIN* | 2812 §3.4.9 | MUST     |

### 6.7 INFO
```
Command:  INFO
Params:   [<server>]
```

| Requirement                          | RFC          | Priority |
| ------------------------------------ | ------------ | -------- |
| Server MUST return 371 RPL_INFO      | 2812 §3.4.10 | MUST     |
| Server MUST return 374 RPL_ENDOFINFO | 2812 §3.4.10 | MUST     |

---

## 7. Operator Commands

**Source:** RFC 2812 Section 3.7

### 7.1 KILL
```
Command:  KILL
Params:   <nickname> <comment>
```

| Requirement                                         | RFC         | Priority |
| --------------------------------------------------- | ----------- | -------- |
| Server MUST disconnect target user                  | 2812 §3.7.1 | MUST     |
| Server MUST require operator privileges             | 2812 §3.7.1 | MUST     |
| Server MUST return 481 ERR_NOPRIVILEGES if not oper | 2812 §3.7.1 | MUST     |
| Server MUST return 401 ERR_NOSUCHNICK if not found  | 2812 §3.7.1 | MUST     |

### 7.2 WALLOPS
```
Command:  WALLOPS
Params:   <text>
```

| Requirement                                | RFC         | Priority |
| ------------------------------------------ | ----------- | -------- |
| Server MUST send to all users with +w mode | 2812 §3.7.3 | MUST     |
| Server SHOULD require operator privileges  | 2812 §3.7.3 | SHOULD   |

### 7.3 REHASH
```
Command:  REHASH
Params:   (none)
```

| Requirement                             | RFC         | Priority |
| --------------------------------------- | ----------- | -------- |
| Server MUST reload configuration        | 2812 §3.7.2 | MUST     |
| Server MUST require operator privileges | 2812 §3.7.2 | MUST     |
| Server MUST return 382 RPL_REHASHING    | 2812 §3.7.2 | MUST     |

### 7.4 DIE / RESTART
```
Command:  DIE / RESTART
Params:   (none)
```

| Requirement                             | RFC         | Priority |
| --------------------------------------- | ----------- | -------- |
| Server MUST require operator privileges | 2812 §3.7.4 | MUST     |
| Server MUST terminate/restart process   | 2812 §3.7.4 | MUST     |

---

## 8. Numeric Replies

**Source:** RFC 2812 Section 5

### 8.1 Welcome Numerics (001-005)

| Numeric | Name         | Format                                                                     |
| ------- | ------------ | -------------------------------------------------------------------------- |
| 001     | RPL_WELCOME  | `:Welcome to the <network> IRC Network <nick>!<user>@<host>`               |
| 002     | RPL_YOURHOST | `:Your host is <server>, running version <version>`                        |
| 003     | RPL_CREATED  | `:This server was created <date>`                                          |
| 004     | RPL_MYINFO   | `<servername> <version> <usermodes> <chanmodes> [<chanmodes with params>]` |
| 005     | RPL_ISUPPORT | `<token>... :are supported by this server`                                 |

### 8.2 Command Replies

| Numeric | Name                   | Requirement                      |
| ------- | ---------------------- | -------------------------------- |
| 221     | RPL_UMODEIS            | MUST - response to MODE for user |
| 251-255 | LUSERS                 | MUST - LUSERS response           |
| 256-259 | ADMIN                  | MUST - ADMIN response            |
| 301     | RPL_AWAY               | MUST - user is away              |
| 302     | RPL_USERHOST           | MUST - USERHOST response         |
| 303     | RPL_ISON               | MUST - ISON response             |
| 305/306 | RPL_UNAWAY/RPL_NOWAWAY | MUST - AWAY response             |
| 311-318 | WHOIS                  | MUST - WHOIS response            |
| 314     | RPL_WHOWASUSER         | MUST - WHOWAS response           |
| 321-323 | LIST                   | MUST - LIST response             |
| 324     | RPL_CHANNELMODEIS      | MUST - channel modes             |
| 329     | RPL_CREATIONTIME       | SHOULD - channel creation time   |
| 330     | RPL_WHOISACCOUNT       | SHOULD - logged in account       |
| 331/332 | TOPIC                  | MUST - topic response            |
| 333     | RPL_TOPICWHOTIME       | SHOULD - topic setter/time       |
| 341     | RPL_INVITING           | MUST - INVITE response           |
| 351     | RPL_VERSION            | MUST - VERSION response          |
| 352     | RPL_WHOREPLY           | MUST - WHO response              |
| 353     | RPL_NAMREPLY           | MUST - NAMES response            |
| 354     | RPL_WHOSPCRPL          | SHOULD - WHOX response           |
| 366     | RPL_ENDOFNAMES         | MUST - end of NAMES              |
| 367-369 | BAN/WHOWAS             | MUST - ban list/WHOWAS end       |
| 371/374 | INFO                   | MUST - INFO response             |
| 375-376 | MOTD                   | MUST - MOTD start/end            |
| 381     | RPL_YOUREOPER          | MUST - OPER success              |
| 391     | RPL_TIME               | MUST - TIME response             |

### 8.3 Error Numerics

| Numeric | Name                 | Requirement |
| ------- | -------------------- | ----------- |
| 401     | ERR_NOSUCHNICK       | MUST        |
| 402     | ERR_NOSUCHSERVER     | MUST        |
| 403     | ERR_NOSUCHCHANNEL    | MUST        |
| 404     | ERR_CANNOTSENDTOCHAN | MUST        |
| 405     | ERR_TOOMANYCHANNELS  | MUST        |
| 406     | ERR_WASNOSUCHNICK    | MUST        |
| 407     | ERR_TOOMANYTARGETS   | MUST        |
| 411     | ERR_NORECIPIENT      | MUST        |
| 412     | ERR_NOTEXTTOSEND     | MUST        |
| 421     | ERR_UNKNOWNCOMMAND   | MUST        |
| 422     | ERR_NOMOTD           | MUST        |
| 431     | ERR_NONICKNAMEGIVEN  | MUST        |
| 432     | ERR_ERRONEUSNICKNAME | MUST        |
| 433     | ERR_NICKNAMEINUSE    | MUST        |
| 436     | ERR_NICKCOLLISION    | MUST        |
| 441     | ERR_USERNOTINCHANNEL | MUST        |
| 442     | ERR_NOTONCHANNEL     | MUST        |
| 443     | ERR_USERONCHANNEL    | MUST        |
| 451     | ERR_NOTREGISTERED    | MUST        |
| 461     | ERR_NEEDMOREPARAMS   | MUST        |
| 462     | ERR_ALREADYREGISTRED | MUST        |
| 464     | ERR_PASSWDMISMATCH   | MUST        |
| 465     | ERR_YOUREBANNEDCREEP | SHOULD      |
| 471     | ERR_CHANNELISFULL    | MUST        |
| 472     | ERR_UNKNOWNMODE      | MUST        |
| 473     | ERR_INVITEONLYCHAN   | MUST        |
| 474     | ERR_BANNEDFROMCHAN   | MUST        |
| 475     | ERR_BADCHANNELKEY    | MUST        |
| 476     | ERR_BADCHANMASK      | MUST        |
| 481     | ERR_NOPRIVILEGES     | MUST        |
| 482     | ERR_CHANOPRIVSNEEDED | MUST        |
| 483     | ERR_CANTKILLSERVER   | MUST        |
| 484     | ERR_RESTRICTED       | SHOULD      |
| 491     | ERR_NOOPERHOST       | MUST        |
| 501     | ERR_UMODEUNKNOWNFLAG | MUST        |
| 502     | ERR_USERSDONTMATCH   | MUST        |

---

## 9. ISUPPORT Tokens

**Source:** Modern IRC practice (draft-brocklesby-irc-isupport)

### 9.1 Required Tokens

| Token       | Example                   | Priority |
| ----------- | ------------------------- | -------- |
| NETWORK     | `NETWORK=ExampleNet`      | MUST     |
| CASEMAPPING | `CASEMAPPING=rfc1459`     | MUST     |
| CHANTYPES   | `CHANTYPES=#&`            | MUST     |
| PREFIX      | `PREFIX=(ov)@+`           | MUST     |
| CHANMODES   | `CHANMODES=beI,k,l,imnst` | MUST     |
| NICKLEN     | `NICKLEN=30`              | MUST     |
| CHANNELLEN  | `CHANNELLEN=50`           | MUST     |
| TOPICLEN    | `TOPICLEN=390`            | MUST     |
| KICKLEN     | `KICKLEN=390`             | SHOULD   |
| AWAYLEN     | `AWAYLEN=200`             | SHOULD   |

### 9.2 Recommended Tokens

| Token       | Example                          | Priority                  |
| ----------- | -------------------------------- | ------------------------- |
| MODES       | `MODES=6`                        | SHOULD                    |
| MAXTARGETS  | `MAXTARGETS=4`                   | SHOULD                    |
| MAXLIST     | `MAXLIST=beI:100`                | SHOULD                    |
| TARGMAX     | `TARGMAX=PRIVMSG:4,NOTICE:4,...` | SHOULD                    |
| EXCEPTS     | `EXCEPTS=e`                      | SHOULD                    |
| INVEX       | `INVEX=I`                        | SHOULD                    |
| STATUSMSG   | `STATUSMSG=@+`                   | SHOULD                    |
| ELIST       | `ELIST=CMNTU`                    | SHOULD                    |
| SAFELIST    | `SAFELIST`                       | SHOULD                    |
| MONITOR     | `MONITOR=100`                    | SHOULD                    |
| WHOX        | `WHOX`                           | SHOULD                    |
| BOT         | `BOT=B`                          | SHOULD                    |
| DEAF        | `DEAF=D`                         | MAY                       |
| CHANLIMIT   | `CHANLIMIT=#&:50`                | SHOULD                    |
| MAXCHANNELS | `MAXCHANNELS=50`                 | Deprecated, use CHANLIMIT |
| CHARSET     | `CHARSET=utf-8`                  | MAY                       |

---

## 10. Channel Modes

**Source:** RFC 2811 Section 4

### 10.1 Mode Categories (CHANMODES)

```
CHANMODES=A,B,C,D
```

| Type | Description                        | Examples                       |
| ---- | ---------------------------------- | ------------------------------ |
| A    | List modes (always have parameter) | b (ban), e (except), I (invex) |
| B    | Modes with parameter always        | k (key), o (op), v (voice)     |
| C    | Modes with parameter when setting  | l (limit)                      |
| D    | Modes without parameter            | i, m, n, p, s, t               |

### 10.2 Standard Channel Modes

| Mode | Type | Meaning              | Priority |
| ---- | ---- | -------------------- | -------- |
| b    | A    | Ban mask             | MUST     |
| e    | A    | Ban exception        | SHOULD   |
| I    | A    | Invite exception     | SHOULD   |
| k    | B    | Channel key          | MUST     |
| o    | B    | Channel operator     | MUST     |
| v    | B    | Voice                | MUST     |
| l    | C    | User limit           | MUST     |
| i    | D    | Invite only          | MUST     |
| m    | D    | Moderated            | MUST     |
| n    | D    | No external messages | MUST     |
| p    | D    | Private              | SHOULD   |
| s    | D    | Secret               | MUST     |
| t    | D    | Topic restricted     | MUST     |

### 10.3 Extended Channel Modes (Modern)

| Mode | Type | Meaning                   | Priority |
| ---- | ---- | ------------------------- | -------- |
| q    | B    | Channel owner/founder (~) | SHOULD   |
| a    | B    | Channel admin (&)         | MAY      |
| h    | B    | Half-op (%)               | MAY      |
| r    | D    | Registered only           | MAY      |
| c    | D    | No colors                 | MAY      |
| C    | D    | No CTCPs                  | MAY      |
| S    | D    | SSL/TLS only              | MAY      |

---

## 11. User Modes

**Source:** RFC 2812 Section 3.1.5

### 11.1 Standard User Modes

| Mode | Meaning                | Priority |
| ---- | ---------------------- | -------- |
| i    | Invisible              | MUST     |
| o    | IRC operator           | MUST     |
| w    | Receive wallops        | MUST     |
| s    | Receive server notices | SHOULD   |
| r    | Registered             | SHOULD   |

### 11.2 Extended User Modes (Modern)

| Mode | Meaning                    | Priority |
| ---- | -------------------------- | -------- |
| B    | Bot                        | SHOULD   |
| D    | Deaf (no channel messages) | MAY      |
| x    | Cloaked hostname           | MAY      |
| z    | Using TLS                  | SHOULD   |
| Z    | Available only via TLS     | MAY      |

---

## 12. IRCv3 Capabilities

**Source:** https://ircv3.net/specs/

### 12.1 Core Capabilities (MUST)

| Capability   | Description                      |
| ------------ | -------------------------------- |
| cap-notify   | Notify of capability changes     |
| message-tags | Support arbitrary key-value tags |
| multi-prefix | Show all prefixes in NAMES/WHO   |
| sasl         | SASL authentication              |
| server-time  | Timestamp on messages            |

### 12.2 Recommended Capabilities (SHOULD)

| Capability        | Description                      |
| ----------------- | -------------------------------- |
| account-notify    | Notify of login/logout           |
| account-tag       | Include account in message tags  |
| away-notify       | Notify of AWAY changes           |
| batch             | Group related messages           |
| chghost           | Notify of host changes           |
| echo-message      | Echo sent messages               |
| extended-join     | Include account/realname in JOIN |
| invite-notify     | Notify ops of INVITEs            |
| labeled-response  | Correlate request/response       |
| setname           | Change realname                  |
| userhost-in-names | Include user@host in NAMES       |

### 12.3 Optional Capabilities (MAY)

| Capability       | Description                    |
| ---------------- | ------------------------------ |
| chathistory      | Message history                |
| monitor          | Efficient online notifications |
| standard-replies | FAIL/WARN/NOTE format          |
| tls              | STARTTLS (deprecated)          |
| typing           | Typing notifications (draft)   |
| multiline        | Multi-line messages (draft)    |
| read-marker      | Read position tracking (draft) |

---

## 13. TLS Requirements

**Source:** RFC 7194

### 13.1 Port Assignments

| Port | Usage                | Priority |
| ---- | -------------------- | -------- |
| 6667 | Plaintext (de facto) | SHOULD   |
| 6697 | TLS (official)       | MUST     |

### 13.2 TLS Protocol Requirements

| Requirement                                | Priority |
| ------------------------------------------ | -------- |
| Support TLS 1.2                            | MUST     |
| Support TLS 1.3                            | SHOULD   |
| Disable TLS 1.0/1.1                        | SHOULD   |
| Offer STARTTLS on plaintext ports          | MAY      |
| Verify client certificates (optional mode) | SHOULD   |
| Advertise CERTFP in WHOIS                  | SHOULD   |

### 13.3 Cipher Suite Recommendations

| Suite                        | Priority |
| ---------------------------- | -------- |
| TLS_AES_256_GCM_SHA384       | SHOULD   |
| TLS_CHACHA20_POLY1305_SHA256 | SHOULD   |
| TLS_AES_128_GCM_SHA256       | SHOULD   |
| Disable CBC modes            | SHOULD   |
| Disable RC4, DES, 3DES       | MUST     |

---

## 14. Implementation Checklist

### 14.1 Core Protocol

- [x] Message parsing (512 byte limit, CRLF, 15 params)
  - [slirc-proto/src/message/nom_parser.rs](slirc-proto/src/message/nom_parser.rs#L15): Nom-based zero-copy parser enforcing RFC 2812 limits.
- [x] Message tags support (4096 byte limit with tags)
  - [slirc-proto/src/transport/zero_copy/helpers.rs](slirc-proto/src/transport/zero_copy/helpers.rs#L11): Enforces 4094 byte limit for client-only tags per IRCv3.
- [x] Nickname validation (format, length, uniqueness)
  - [slirc-proto/src/nick.rs](slirc-proto/src/nick.rs#L51): Implements RFC 2812 compliant nickname character and length validation.
- [x] Channel name validation (format, length, types)
  - [slirc-proto/src/chan.rs](slirc-proto/src/chan.rs#L19): Implements RFC 2812 compliant channel name validation for #, &, +, and ! types.
- [x] Casemapping (rfc1459 or ascii)
  - [slirc-proto/src/casemap.rs](slirc-proto/src/casemap.rs#L33): Full implementation of RFC 1459 casemapping (including {}|~).
- [x] Wildcard matching (*, ?)
  - [slirc-proto/src/util.rs](slirc-proto/src/util.rs#L241): Glob-style wildcard matching used for hostmasks and ban lists.

### 14.2 Connection Registration

- [x] PASS command
  - [slircd-ng/src/handlers/connection/pass.rs](slircd-ng/src/handlers/connection/pass.rs#L24): Handles pre-registration password verification.
- [x] NICK command (pre and post registration)
  - [slircd-ng/src/handlers/connection/nick.rs](slircd-ng/src/handlers/connection/nick.rs#L37): Manages nickname changes and collisions across all session states.
- [x] USER command
  - [slircd-ng/src/handlers/connection/user.rs](slircd-ng/src/handlers/connection/user.rs#L24): Collects user identity information required for registration.
- [x] Welcome burst (001-005)
  - [slircd-ng/src/handlers/connection/welcome_burst.rs](slircd-ng/src/handlers/connection/welcome_burst.rs#L240): Sends the standard 001-005 numeric sequence upon successful registration.
- [x] MOTD (375/372/376 or 422)
  - [slircd-ng/src/handlers/server_query/server_info.rs](slircd-ng/src/handlers/server_query/server_info.rs#L20): Streams the Message of the Day from configuration or file.
- [x] CAP negotiation
  - [slircd-ng/src/handlers/cap.rs](slircd-ng/src/handlers/cap.rs#L81): Implements IRCv3 capability negotiation (LS, REQ, ACK, END).
- [x] SASL authentication (PLAIN, EXTERNAL)
  - [slircd-ng/src/handlers/cap.rs](slircd-ng/src/handlers/cap.rs#L448): AuthenticateHandler manages SASL state machine and credential verification.
- [x] QUIT with proper cleanup
  - [slircd-ng/src/handlers/connection/quit.rs](slircd-ng/src/handlers/connection/quit.rs#L12): Ensures clean disconnection and removal from all channels/state.

### 14.3 Channel Operations

- [x] JOIN (single, multiple, with keys)
  - [slircd-ng/src/handlers/channel/join/mod.rs](slircd-ng/src/handlers/channel/join/mod.rs#L40): Handles channel joining, key verification, and member synchronization.
- [x] JOIN 0 (leave all)
  - [slircd-ng/src/handlers/channel/join/mod.rs](slircd-ng/src/handlers/channel/join/mod.rs#L65): Special case for JOIN 0 to leave all currently joined channels.
- [x] PART (single, multiple, with message)
  - [slircd-ng/src/handlers/channel/part.rs](slircd-ng/src/handlers/channel/part.rs#L36): Removes users from channels with optional part messages.
- [x] TOPIC (view, set, restrictions)
  - [slircd-ng/src/handlers/channel/topic.rs](slircd-ng/src/handlers/channel/topic.rs#L43): Manages channel topics with +t mode enforcement.
- [x] NAMES (single, multiple, all)
  - [slircd-ng/src/handlers/channel/names.rs](slircd-ng/src/handlers/channel/names.rs#L26): Lists channel members with appropriate prefixes (~&@%+).
- [x] LIST (all, filtered, ELIST)
  - [slircd-ng/src/handlers/channel/list.rs](slircd-ng/src/handlers/channel/list.rs#L132): Provides channel listing with support for ELIST filtering.
- [x] INVITE (with +i enforcement)
  - [slircd-ng/src/handlers/channel/invite.rs](slircd-ng/src/handlers/channel/invite.rs#L29): Handles user invitations and checks +i (InviteOnly) mode.
- [x] KICK (with comment)
  - [slircd-ng/src/handlers/channel/kick.rs](slircd-ng/src/handlers/channel/kick.rs#L27): Forcibly removes users from channels (requires operator privileges).
- [x] MODE (all channel modes)
  - [slircd-ng/src/handlers/mode/mod.rs](slircd-ng/src/handlers/mode/mod.rs#L23): ModeHandler dispatches to channel mode logic for flags and parameters.

### 14.4 Messaging

- [x] PRIVMSG to user
  - [slircd-ng/src/handlers/messaging/privmsg.rs](slircd-ng/src/handlers/messaging/privmsg.rs#L159): Routes private messages to individual users with history storage.
- [x] PRIVMSG to channel
  - [slircd-ng/src/handlers/messaging/privmsg.rs](slircd-ng/src/handlers/messaging/privmsg.rs#L210): Broadcasts messages to channel members via the Actor model.
- [x] NOTICE to user
  - [slircd-ng/src/handlers/messaging/notice.rs](slircd-ng/src/handlers/messaging/notice.rs#L30): Sends notices to users (silently drops errors per RFC).
- [x] NOTICE to channel
  - [slircd-ng/src/handlers/messaging/notice.rs](slircd-ng/src/handlers/messaging/notice.rs#L30): Broadcasts notices to channel members.
- [x] STATUSMSG (@#channel)
  - [slircd-ng/src/handlers/messaging/privmsg.rs](slircd-ng/src/handlers/messaging/privmsg.rs#L430): Implements STATUSMSG for targeting specific member subsets (e.g., @#channel).
- [x] Away auto-reply
  - [slircd-ng/src/handlers/messaging/common.rs](slircd-ng/src/handlers/messaging/common.rs#L215): Automatically sends RPL_AWAY when messaging a user with an away message set.
- [x] echo-message support
  - [slircd-ng/src/handlers/messaging/privmsg.rs](slircd-ng/src/handlers/messaging/privmsg.rs#L365): Echoes messages back to the sender if the capability is enabled.

### 14.5 User Queries

- [x] WHO (basic, WHOX)
  - [slircd-ng/src/handlers/user_query/who.rs](slircd-ng/src/handlers/user_query/who.rs#L438): Implements standard WHO and IRCv3 WHOX with field selection.
- [x] WHOIS (all numerics)
  - [slircd-ng/src/handlers/user_query/whois/whois_cmd.rs](slircd-ng/src/handlers/user_query/whois/whois_cmd.rs#L64): Returns detailed user info including idle time, signon time, and CERTFP.
- [x] WHOWAS
  - [slircd-ng/src/handlers/user_query/whois/whowas.rs](slircd-ng/src/handlers/user_query/whois/whowas.rs#L44): Queries historical user data from the persistence layer.
- [x] USERHOST
  - [slircd-ng/src/handlers/user_query/whois/userhost.rs](slircd-ng/src/handlers/user_query/whois/userhost.rs#L16): Returns user@host pairs for a list of nicknames.
- [x] ISON
  - [slircd-ng/src/handlers/user_query/whois/ison.rs](slircd-ng/src/handlers/user_query/whois/ison.rs#L16): Quick check for online status of multiple nicknames.
- [x] MONITOR
  - [slircd-ng/src/handlers/monitor.rs](slircd-ng/src/handlers/monitor.rs#L26): Implements IRCv3 MONITOR for presence notifications.

### 14.6 Server Queries

- [x] MOTD
  - [slircd-ng/src/handlers/server_query/server_info.rs](slircd-ng/src/handlers/server_query/server_info.rs#L20): Returns the Message of the Day.
- [x] LUSERS
  - [slircd-ng/src/handlers/server_query/server_info.rs](slircd-ng/src/handlers/server_query/server_info.rs#L296): Provides network-wide user and channel statistics.
- [x] VERSION
  - [slircd-ng/src/handlers/server_query/server_info.rs](slircd-ng/src/handlers/server_query/server_info.rs#L69): Returns server version and build information.
- [x] STATS
  - [slircd-ng/src/handlers/server_query/stats.rs](slircd-ng/src/handlers/server_query/stats.rs#L27): Returns various server statistics (uptime, operators, bans).
- [x] TIME
  - [slircd-ng/src/handlers/server_query/server_info.rs](slircd-ng/src/handlers/server_query/server_info.rs#L109): Returns the current server time.
- [x] ADMIN
  - [slircd-ng/src/handlers/server_query/server_info.rs](slircd-ng/src/handlers/server_query/server_info.rs#L141): Returns administrative contact information.
- [x] INFO
  - [slircd-ng/src/handlers/server_query/server_info.rs](slircd-ng/src/handlers/server_query/server_info.rs#L220): Returns general information about the server and its authors.
- [x] HELP
  - [slircd-ng/src/handlers/server_query/help.rs](slircd-ng/src/handlers/server_query/help.rs#L238): Provides help text for IRC commands.

### 14.7 Operator Commands

- [x] KILL
  - [slircd-ng/src/handlers/oper/kill.rs](slircd-ng/src/handlers/oper/kill.rs#L31): Forcibly disconnects a user from the network.
- [x] DIE
  - [slircd-ng/src/handlers/oper/admin.rs](slircd-ng/src/handlers/oper/admin.rs#L19): Shuts down the server.
- [x] REHASH
  - [slircd-ng/src/handlers/oper/admin.rs](slircd-ng/src/handlers/oper/admin.rs#L61): Reloads server configuration and IP deny lists.
- [x] RESTART
  - [slircd-ng/src/handlers/oper/admin.rs](slircd-ng/src/handlers/oper/admin.rs#L135): Restarts the server process.
- [x] WALLOPS
  - [slircd-ng/src/handlers/oper/wallops.rs](slircd-ng/src/handlers/oper/wallops.rs#L25): Sends a message to all users with the +w (wallops) mode.
- [x] KLINE
  - [slircd-ng/src/handlers/bans/xlines/mod.rs](slircd-ng/src/handlers/bans/xlines/mod.rs#L305): Local ban by user@host mask.
- [x] GLINE
  - [slircd-ng/src/handlers/bans/xlines/mod.rs](slircd-ng/src/handlers/bans/xlines/mod.rs#L322): Global ban by user@host mask.
- [x] ZLINE
  - [slircd-ng/src/handlers/bans/xlines/mod.rs](slircd-ng/src/handlers/bans/xlines/mod.rs#L435): Global IP ban that skips DNS lookups.
- [x] SHUN
  - [slircd-ng/src/handlers/bans/shun.rs](slircd-ng/src/handlers/bans/shun.rs#L22): Silently ignores commands from matching users.

### 14.8 ISUPPORT

- [x] RPL_ISUPPORT (005)
  - [slircd-ng/src/handlers/connection/welcome_burst.rs](slircd-ng/src/handlers/connection/welcome_burst.rs#L284): Dynamically builds and sends ISUPPORT tokens during registration.
- [x] Case Mapping (rfc1459)
  - [slirc-proto/src/casemap.rs](slirc-proto/src/casemap.rs#L1): Implements RFC 1459 compliant case-insensitive string comparisons.
- [x] Channel Types (#&+!)
  - [slircd-ng/src/handlers/connection/welcome_burst.rs](slircd-ng/src/handlers/connection/welcome_burst.rs#L311): Advertises supported channel types.
- [x] Prefix (~&@%+)
  - [slircd-ng/src/handlers/connection/welcome_burst.rs](slircd-ng/src/handlers/connection/welcome_burst.rs#L312): Advertises supported channel status prefixes.

### 14.9 IRCv3

- [x] CAP (Capability Negotiation)
  - [slircd-ng/src/handlers/cap.rs](slircd-ng/src/handlers/cap.rs#L74): Implements CAP LS, LIST, REQ, ACK, NAK, END.
- [x] SASL (PLAIN, EXTERNAL)
  - [slircd-ng/src/handlers/cap.rs](slircd-ng/src/handlers/cap.rs#L458): Implements SASL authentication flow for PLAIN and EXTERNAL mechanisms.
- [x] MONITOR
  - [slircd-ng/src/handlers/monitor.rs](slircd-ng/src/handlers/monitor.rs#L21): Implements presence notification for monitored nicknames.
- [x] CHATHISTORY
  - [slircd-ng/src/handlers/chathistory.rs](slircd-ng/src/handlers/chathistory.rs#L86): Provides message history retrieval for channels and DMs.
- [x] BATCH
  - [slircd-ng/src/handlers/batch/mod.rs](slircd-ng/src/handlers/batch/mod.rs#L28): Implements message batching, including draft/multiline support.
- [x] Message Tags
  - [slirc-proto/src/message/nom_parser.rs](slirc-proto/src/message/nom_parser.rs#L1): Parses IRCv3 message tags from the wire.

### 14.10 TLS

- [x] STARTTLS
  - [slircd-ng/src/handlers/connection/starttls.rs](slircd-ng/src/handlers/connection/starttls.rs#L31): Implements mid-stream TLS upgrade for plaintext connections.
- [x] TLS 1.3 Enforcement
  - [slircd-ng/src/network/gateway.rs](slircd-ng/src/network/gateway.rs#L288): Configures the TLS acceptor to prefer or enforce TLS 1.3.
- [x] CERTFP
  - [slircd-ng/src/state/session.rs](slircd-ng/src/state/session.rs#L109): Stores hex-encoded SHA-256 certificate fingerprints for SASL EXTERNAL.

---

## References

1. RFC 1459 - Internet Relay Chat Protocol
   https://tools.ietf.org/html/rfc1459

2. RFC 2810 - Internet Relay Chat: Architecture
   https://tools.ietf.org/html/rfc2810

3. RFC 2811 - Internet Relay Chat: Channel Management
   https://tools.ietf.org/html/rfc2811

4. RFC 2812 - Internet Relay Chat: Client Protocol
   https://tools.ietf.org/html/rfc2812

5. RFC 2813 - Internet Relay Chat: Server Protocol
   https://tools.ietf.org/html/rfc2813

6. RFC 7194 - Default Port for Internet Relay Chat (IRC) via TLS/SSL
   https://tools.ietf.org/html/rfc7194

7. IRCv3 Specifications
   https://ircv3.net/specs/

8. Modern IRC
   https://modern.ircdocs.horse/

9. IRC Numerics
   https://defs.ircdocs.horse/defs/numerics

10. ISUPPORT Tokens
    https://defs.ircdocs.horse/defs/isupport
