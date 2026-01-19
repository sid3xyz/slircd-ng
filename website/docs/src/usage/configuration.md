# Configuration

SLIRCd is configured via a TOML file. Create `config.toml` in your working directory.

## Example Configuration

```toml
[server]
name = "irc.example.com"
sid = "001"
description = "slircd-ng IRC Server"
port = 6667
tls_port = 6697
password = "your-secret-key"  # Random 32 chars

[server.admin]
line1 = "Admin Name"
line2 = "Admin Email"
line3 = "Admin URL"

[database]
path = "data/irc.db"

[multiclient]
enabled = true
always_on = "opt-in"
```
