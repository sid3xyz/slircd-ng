# Deployment Checklist

Before deploying `slircd-ng` to a production environment, complete the following verification steps.

## 1. Security Configuration
- [ ] **Change the Cloak Secret**: Generate a new random secret for `security.cloak_secret` in `config.toml`.
      ```bash
      openssl rand -hex 32
      ```
- [ ] **Set a Strong Password**: Ensure `server.password` is complex if set.
- [ ] **Configure TLS**: Obtain and verify certificates (`cert_path` and `key_path`).
      - Recommended: Let's Encrypt / Certbot.
- [ ] **Review Permissions**: Ensure the database file (`slircd.db`) is not readable by others.

## 2. Network & System
- [ ] **Firewall**: Open ports 6667 (TCP) and 6697 (TLS) only.
- [ ] **File Descriptors**: Increase `ulimit -n` for the user running the daemon (recommend 10,000+).
- [ ] **Reverse DNS**: Ensure the server IP has a valid PTR record matching its hostname (helps with deliverability and abuse prevention).

## 3. Configuration Tuning
- [ ] **Review Limits**: Adjust `[security.rate_limits]` based on expected traffic.
- [ ] **Operators**: Define operator accounts in `config.toml` with secure passwords and restricted hostmasks.
- [ ] **MOTD**: Customize `motd.txt` with server rules and contact info.

## 4. Monitoring
- [ ] **Metrics**: Configure a Prometheus scraper for the `metrics_port` (default 9090).
- [ ] **Logs**: redirect standard output/error to a log manager (systemd/journald recommended).
      ```ini
      [Unit]
      Description=slircd-ng IRC Server
      After=network.target

      [Service]
      User=slircd
      ExecStart=/usr/local/bin/slircd /etc/slircd/config.toml
      Restart=always

      [Install]
      WantedBy=multi-user.target
      ```

## 5. Persistence
- [ ] **Backup**: Schedule regular backups of `slircd.db` and `history.db`.
- [ ] **Storage**: Ensure sufficient disk space for `history.db` if `CHATHISTORY` is heavily used.
