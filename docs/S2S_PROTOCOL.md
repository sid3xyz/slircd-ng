# Server-to-Server (S2S) Protocol Specification

This document describes the S2S protocol used by `slircd-ng`. It is primarily based on the **TS6** protocol (used by UnrealIRCd, InspIRCd, and others) with specific extensions for modern features.

## 1. Connection & Handshake

The S2S handshake authenticates two servers and negotiates capabilities.

### Sequence
1. **Initiator** connects to **Listener** (usually on a high-numbered port like 7000+).
2. **Initiator** sends `PASS` and `CAPAB`.
3. **Listener** sends `PASS` and `CAPAB`.
4. **Initiator** sends `SERVER` (introducing itself).
5. **Listener** sends `SERVER`.
6. Both servers send `SVINFO` (protocol version check).
7. Authorization logic runs (password match, IP check).
8. **Burst Phase** begins.

### Commands

#### `PASS`
`PASS <password> TS <ts_version> :<sid>`
- `password`: The shared secret key from `link` block.
- `TS`: Timestamp protocol version (must be 6).
- `sid`: The local server's unique 3-digit Server ID (e.g., `001`).

#### `CAPAB`
`CAPAB :<capabilities>`
Advertises supported features.
- `QS`: Can handle Quit Storms.
- `EX`: Extended messages.
- `CHW`: Channel modes.
- `IE`: Invite exceptions.
- `EOB`: End of Burst support.
- `KLN`: K-Line support.
- `UNKLN`: Un-K-Line support.
- `KNOCK`: Knock command support.
- `TB`: Topic burst support.
- `ENCAP`: Encapsulated commands.
- `SERVICES`: Services integration support.

#### `SERVER`
`SERVER <name> <hopcount> <description>`
- `name`: The server's hostname (e.g., `irc.example.com`).
- `hopcount`: Distance from sender (1 for direct link).
- `description`: Human-readable info.

#### `SVINFO`
`SVINFO <version> <min_version> <serial> :<current_time>`
Used to verify protocol compatibility.
- `version`: Current TS version (6).
- `min_version`: Minimum supported TS version (6).
- `serial`: 0.
- `current_time`: Unix timestamp.

---

## 2. State Synchronization (The Burst)

After handshake, servers exchange their full state. This is called "bursting".

### User Handlers (`UID`)
`:<sid> UID <nick> <hops> <ts> <user> <host> <ip> <uid> <modes> :<realname>`
Introduces a new user to the network.
- `sid`: Source Server ID.
- `uid`: Unique User ID (9 chars, starts with SID).
- `ts`: Nickname TS (for collision resolution).

### Channel Handlers (`SJOIN`)
`:<sid> SJOIN <ts> <channel> <modes> [args...] :<uids>`
Syncs a channel and its members.
- `ts`: Channel creation TS.
- `uids`: List of UIDs to join. Prefixes (e.g., `@`, `+`) denote status.

### Bandlers/X-Lines
- `ADDLINE <type> <mask> <who> <set_time> <expire_time> :<reason>`
    - `type`: `Z` (IP ban), `G` (G-Line), `Q` (Q-Line), `E` (K-Line exception).

---

## 3. Runtime Protocol

Once synced, servers exchange real-time messages.

### Routing
Messages are routed using **UIDs** for users and **SIDs** for servers.
`:<source_uid> PRIVMSG <target_uid> :Hello world`

### Command Extensions (`ENCAP`)
`:<source> ENCAP <target_server_mask> <subcommand> [args...]`
Used for commands that don't have a native TS6 opcode or need to target specific servers.

Supported Subcommands:
- `CHGHOST <uid> <new_host>`: Changes a user's visible hostname.
- `REALHOST <uid> <real_host>`: Updates internal real hostname.
- `LOGIN <uid> <account_name>`: Associates user with services account.
- `CERTFP <uid> <fingerprint>`: Propagates TLS client cert fingerprint.

### Network Topology
- `SQUIT <sid> :<reason>`: Signals a server text-split.
- `PONG <sid>`: Reply to PING from specific server.

---

## 4. Slircd-ng Extensions

Specific behaviors unique to `slircd-ng` or strict checks we enforce.

### Strict SID Format
We strictly enforce the 3-digit SID format (e.g., `001`, `00A`, `XYZ`). Legacy numeric SIDs are supported but mapped to this format.

### UTF-8 Only
All text (nicks, topics, realnames) MUST be valid UTF-8. Invalid sequences are replaced or dropped.

### Atomic Bursts
We buffer the entire initial burst before applying it to the local state to ensure consistency. If `EOB` (End of Burst) is not received within a timeout, the link is dropped.
