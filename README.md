# slircd-ng

**Status**: AI research experiment. Not production ready.

## Metrics (from code)

| Metric | Value |
|--------|-------|
| Source files | 175 |
| Lines of Rust | 32,182 |
| Commands | 81 (6 universal, 4 pre-reg, 71 post-reg) |
| IRCv3 Caps | 21 |
| Migrations | 7 |

### Quality (Phase 2)

| Metric | Value |
|--------|-------|
| Clippy allows | 19 (from 104) |
| Capacity hints | 47 |
| Deep nesting | 0 files >8 levels |
| TODOs/FIXMEs | 0 |

### Compliance (Phase 3)

| Metric | Value |
|--------|-------|
| irctest passed | 269 |
| Pass rate | 100% (on applicable tests) |
| Skipped | 36 (SASL=TLS, ascii casemapping, optional) |
| XFailed | 6 (deprecated RFCs) |
| Failed | 1 (upstream test bug in deprecated test) |

## Commands

### Universal (any state)
QUIT, PING, PONG, NICK, CAP, REGISTER

### Pre-Registration
USER, PASS, WEBIRC, AUTHENTICATE

### Post-Registration
JOIN, PART, CYCLE, TOPIC, NAMES, MODE, KICK, LIST, INVITE, KNOCK,
PRIVMSG, NOTICE, TAGMSG, BATCH,
WHO, WHOIS, WHOWAS, USERHOST, ISON,
VERSION, TIME, ADMIN, INFO, LUSERS, STATS, MOTD, MAP, RULES, USERIP, LINKS, HELP, SUMMON, USERS,
SERVICE, SERVLIST, SQUERY,
AWAY, SETNAME, SILENCE, MONITOR, CHATHISTORY,
NICKSERV, NS, CHANSERV, CS,
OPER, KILL, WALLOPS, GLOBOPS, DIE, REHASH, RESTART, CHGHOST, CHGIDENT, VHOST, TRACE,
KLINE, DLINE, GLINE, ZLINE, RLINE, SHUN,
UNKLINE, UNDLINE, UNGLINE, UNZLINE, UNRLINE, UNSHUN,
SAJOIN, SAPART, SANICK, SAMODE

## IRCv3 Capabilities

multi-prefix, userhost-in-names, server-time, echo-message, sasl,
batch, message-tags, labeled-response, setname, away-notify,
account-notify, extended-join, invite-notify, chghost, monitor,
cap-notify, account-tag, draft/multiline, draft/account-registration,
draft/chathistory, draft/event-playback

Note: SASL is only advertised over TLS connections for security.

## ISUPPORT Tokens

NETWORK, CASEMAPPING=rfc1459, CHANTYPES=#&+!, PREFIX=(qaohv)~&@%+,
CHANMODES=beIq,k,l,imnrst, NICKLEN=30, CHANNELLEN=50, TOPICLEN=390,
KICKLEN=390, AWAYLEN=200, MODES=6, MAXTARGETS=4, MONITOR=100,
EXCEPTS=e, INVEX=I, ELIST=MNU, STATUSMSG=~&@%+, BOT=B, WHOX

## Services

### NickServ
REGISTER, IDENTIFY, GHOST, INFO, SET, DROP, GROUP, UNGROUP, CERT

### ChanServ
REGISTER, ACCESS, INFO, SET, DROP, OP, DEOP, VOICE, DEVOICE, AKICK, CLEAR

## Security Modules

| Module | File |
|--------|------|
| DNSBL | src/security/dnsbl.rs |
| Reputation | src/security/reputation.rs |
| Heuristics | src/security/heuristics.rs |
| Spam | src/security/spam.rs |
| X-lines | src/security/xlines.rs |
| Cloaking | src/security/cloaking.rs |
| Rate Limit | src/security/rate_limit.rs |
| Ban Cache | src/security/ban_cache.rs |

## Persistence

SQLite via sqlx. Migrations:
- 001_init.sql — accounts, channels, channel_access
- 002_shuns.sql — shuns table
- 002_xlines.sql — xlines table (k/g/d/z/r-lines)
- 003_history.sql — message history metadata
- 004_certfp.sql — certificate fingerprint storage
- 005_channel_topics.sql — persistent topics
- 006_reputation.sql — user reputation scores
- 007_bans.sql — channel ban persistence

## Build

\`\`\`bash
cargo build -p slircd-ng
cargo test -p slircd-ng
cargo clippy -p slircd-ng -- -D warnings
cargo run -p slircd-ng -- config.toml
\`\`\`

## License

Unlicense
