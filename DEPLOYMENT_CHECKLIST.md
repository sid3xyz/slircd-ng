# Deployment Checklist for slircd-ng

## Pre-Deployment Verification

### Database
- [ ] All migrations (001-006) embedded in binary
- [ ] Migration check logic includes ALL migrations
- [ ] No schema assumptions without migration checks
- [ ] Database path configurable via config.toml
- [ ] Data directory auto-created if missing
- [ ] Connection pool properly configured (5 connections, 5s timeout)

### Configuration
- [ ] config.toml exists with proper values
- [ ] Database path set correctly
- [ ] TLS certificates paths valid (if using TLS)
- [ ] MOTD configured
- [ ] Oper blocks configured
- [ ] Security settings appropriate for deployment

### File System
- [ ] Data directory writable
- [ ] Database file permissions correct
- [ ] TLS cert files readable
- [ ] Log directory writable (if file logging)

### Security
- [ ] Cloaking secret changed from default
- [ ] Oper passwords hashed (not plaintext)
- [ ] Rate limiting configured
- [ ] DNSBL lists configured (if desired)
- [ ] Exempt IPs configured (if needed)

### Build
- [ ] Built with --release flag
- [ ] Binary stripped (optional)
- [ ] All dependencies up to date
- [ ] Clippy passes with -D warnings

## Deployment Steps

1. Build release binary:
   ```bash
   cargo build --release
   ```

2. Create data directory:
   ```bash
   mkdir -p data
   ```

3. Copy config:
   ```bash
   cp config.toml config.production.toml
   # Edit config.production.toml
   ```

4. Test database migrations:
   ```bash
   # Dry run with test config
   ./target/release/slircd config.test.toml
   # Watch logs for "Database migrations applied" messages
   ```

5. Deploy binary and config to server

6. Start service and verify:
   ```bash
   # Check logs for:
   # - "Database connected"
   # - "Database already initialized" or migration messages
   # - "Gateway listening"
   # - No errors
   ```

## Post-Deployment Verification

- [ ] Server accepts connections
- [ ] Registration works (NICK/USER)
- [ ] NickServ commands work
- [ ] ChanServ commands work
- [ ] Channel operations work
- [ ] No database errors in logs
- [ ] Memory usage stable
- [ ] No connection leaks

## Rollback Plan

If deployment fails:
1. Stop new server
2. Restore old binary
3. Check database compatibility
4. Restart old server

## Monitoring

- Database file size growth
- Connection count
- Memory usage
- Error rate in logs
- Response times

