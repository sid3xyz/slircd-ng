# slircd-ng Development Roadmap

## Completed Features

### Core IRC
- [x] RFC 1459/2812 compliance
- [x] Zero-copy message parsing (MessageRef)
- [x] Lock-free state (DashMap-based Matrix)
- [x] TCP/TLS/WebSocket listeners
- [x] WEBIRC support

### IRCv3 Capabilities
- [x] CAP negotiation (v301, v302)
- [x] multi-prefix, userhost-in-names
- [x] server-time, message-tags
- [x] echo-message, labeled-response
- [x] SASL (PLAIN, SCRAM-SHA-256)
- [x] batch
- [x] setname, away-notify, account-notify
- [x] extended-join, invite-notify
- [x] chghost, monitor, cap-notify
- [x] chathistory (draft spec)

### Services
- [x] NickServ (REGISTER, IDENTIFY, GHOST, INFO, SET)
- [x] ChanServ (REGISTER, OP, DEOP, VOICE, ACCESS, AKICK)
- [x] Nick enforcement with grace period
- [x] Service aliases (/ns, /cs)

### Security
- [x] Host cloaking (HMAC-SHA256)
- [x] Rate limiting (messages, connections, joins)
- [x] Spam detection (entropy, patterns, URLs)
- [x] Ban cache for fast connection checks
- [x] Extended bans ($a:, $r:, $U, etc.)

### Server Bans
- [x] K-Line (user@host local)
- [x] G-Line (user@host global)
- [x] D-Line (IP)
- [x] Z-Line (IP, no DNS)
- [x] R-Line (realname)
- [x] SHUN (silent ban)

### Operator Commands
- [x] OPER, KILL, DIE, REHASH
- [x] WALLOPS, TRACE
- [x] SAJOIN, SAPART, SAMODE, SANICK
- [x] CHGHOST
- [x] X-line management (add/remove all types)

### Observability
- [x] Prometheus metrics endpoint
- [x] Structured logging (tracing)

### Background Tasks
- [x] Nick enforcement task
- [x] WHOWAS cleanup
- [x] Shun expiry cleanup
- [x] Ban cache pruning
- [x] Rate limiter cleanup
- [x] Message history pruning

---

## Future Development

### Phase 5: Server Linking (S2S)

- [ ] TS6 protocol implementation
- [ ] Server state in Matrix
- [ ] S2S message routing
- [ ] Burst on connect
- [ ] SQUIT handling

### Additional Features

- [ ] SASL EXTERNAL (certificate auth)
- [ ] Channel mode +z (TLS-only)
- [ ] HostServ (virtual hosts)
- [ ] MemoServ (offline messaging)
- [ ] BotServ (channel bots)
- [ ] Global bans sync across linked servers
- [ ] Web admin panel

### Performance

- [ ] Connection pooling for database
- [ ] Message batching for broadcasts
- [ ] Metrics for handler latency

---

## Known Limitations

1. **Single server**: No S2S linking yet
2. **In-memory shuns**: Not persisted to database
3. **No OperServ**: Operator commands are built-in, no separate service

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.


