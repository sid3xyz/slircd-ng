# WebSocket Gateway & IP Cloaking Implementation

## Overview

This document describes the WebSocket support and IP cloaking features added to slircd-ng.

## 1. WebSocket Gateway

### Architecture

The WebSocket gateway allows web-based IRC clients to connect via WebSocket protocol (RFC 6455).

**Components:**
- **Gateway Listener**: Binds to configured WebSocket port and performs HTTP Upgrade handshake
- **Connection Variant**: `ConnectionStream::WebSocket` enum variant for WebSocket streams
- **Configuration**: `[websocket]` section in `config.toml`

### Configuration

Add to `config.toml`:

```toml
[websocket]
address = "0.0.0.0:8080"
allow_origins = ["https://webclient.example.com"]
```

### Files Modified

- `Cargo.toml`: Added `tokio-tungstenite` dependency
- `src/config.rs`: Added `WebSocketConfig` struct
- `src/network/gateway.rs`: Added WebSocket listener task
- `src/network/connection.rs`: Added `new_websocket()` constructor and `WebSocket` stream variant
- `src/main.rs`: Passed WebSocket config to gateway

### Current Limitations

⚠️ **WebSocket Adapter Required**: The WebSocket connection variant is defined but requires a message adapter to convert WebSocket text frames to IRC protocol lines. The current implementation will panic when a WebSocket client connects because `split_stream()` doesn't support WebSocket streams yet.

**Next Steps for Full WebSocket Support:**
1. Implement a WebSocket-to-IRC adapter that:
   - Reads WebSocket text frames
   - Converts them to IRC protocol lines
   - Feeds them into the existing handler pipeline
2. Implement reverse adapter for outgoing messages
3. Handle WebSocket ping/pong frames for keep-alive

## 2. IP Cloaking

### Purpose

IP cloaking protects user privacy by hiding real IP addresses from other users while maintaining internal access for moderation.

### Implementation

**Algorithm**: SHA256(IP + SALT)
- Deterministic: Same IP always produces same cloak
- Unpredictable: Cannot reverse-engineer IP from cloak
- Format: `user-<hash>.cloak` (e.g., `user-a3f4c21d.cloak`)

**Data Model:**
- `User.host`: Real IP/hostname (internal use only)
- `User.visible_host`: Cloaked hostname (shown to other users)

### Files Modified

- `src/state/matrix.rs`: 
  - Added `sha2` import
  - Added `visible_host` field to `User` struct
  - Implemented `cloak_host()` function
  - Updated `User::new()` to initialize cloaked hostname

- `src/handlers/connection.rs`:
  - Updated registration to send `RPL_HOSTHIDDEN` (396)
  - Used cloaked hostname in `RPL_WELCOME`

- `src/handlers/user_query.rs`:
  - Updated `WHO` replies to use `visible_host`
  - Updated `WHOIS` replies to use `visible_host`

- `src/handlers/misc.rs`:
  - Updated `USERHOST` to use `visible_host`

### Security Considerations

✅ **Preserved**:
- Ban checks (`KLINE`, `DLINE`) still use real `host` field
- Channel ban masks still match against real hostname
- Operators can see real IPs via database or logs

✅ **Cloaked**:
- WHO/WHOIS replies
- USERHOST replies
- JOIN/PART/QUIT messages (via visible_host in user mask)
- Welcome message

### Example Output

```
:irc.straylight.net 001 Alice :Welcome to the Straylight IRC Network Alice!alice@user-a3f4c21d.cloak
:irc.straylight.net 396 Alice user-a3f4c21d.cloak :is now your displayed host
```

## 3. Protocol Compliance

### RPL_HOSTHIDDEN (396)

✅ **Verified**: `slirc-proto` has `Response::RPL_HOSTHIDDEN` defined.

Sent after registration to notify user of their cloaked hostname:
```
:server 396 <nick> <cloaked_host> :is now your displayed host
```

## 4. Configuration Notes

**IP Cloak Salt**: Currently hardcoded in `src/state/matrix.rs`:
```rust
const CLOAK_SALT: &str = "slircd-ng-cloak-salt-change-me";
```

**Production Recommendation**: 
- Move salt to `config.toml` as `[security] cloak_salt = "..."`
- Generate unique salt per deployment
- Never change salt after deployment (would change all existing cloaks)

## 5. Testing

### IP Cloaking Test

```bash
# Connect with telnet
telnet localhost 6667
NICK TestUser
USER test 0 * :Test User

# Expected output should show cloaked host:
:irc.straylight.net 001 TestUser :Welcome to the Straylight IRC Network TestUser!test@user-XXXXXXXX.cloak
:irc.straylight.net 396 TestUser user-XXXXXXXX.cloak :is now your displayed host
```

### WebSocket Test (When Adapter Implemented)

```javascript
// JavaScript WebSocket client
const ws = new WebSocket('ws://localhost:8080');
ws.onopen = () => {
    ws.send('NICK TestUser');
    ws.send('USER test 0 * :Test User');
};
ws.onmessage = (event) => {
    console.log('Server:', event.data);
};
```

## 6. Architecture Alignment

### Copilot Instructions Compliance

✅ **Protocol-First Development**:
- Verified `RPL_HOSTHIDDEN` exists in `slirc-proto` before implementation
- No raw strings used; proper Response enum variant

✅ **Zero-Copy Patterns**:
- IP cloaking happens once at User creation
- Cloaked hostname stored as owned String (no runtime allocation)

✅ **Concurrency Safety**:
- DashMap usage preserved
- User struct remains in `Arc<RwLock<User>>`

✅ **RFC Compliance**:
- RPL_HOSTHIDDEN (396) properly formatted
- Hostmask format preserved: `nick!user@visible_host`

## 7. Future Enhancements

### Phase 1: Complete WebSocket Support
- [ ] Implement WebSocket frame adapter
- [ ] Add CORS validation using `allow_origins`
- [ ] Add WebSocket ping/pong handling

### Phase 2: Enhanced Cloaking
- [ ] Make cloak salt configurable
- [ ] Add per-user cloak override for operators
- [ ] Add SETHOST command for opers
- [ ] Consider hostname-based cloaking (e.g., `*.example.com` → `user.example.com`)

### Phase 3: Advanced Features
- [ ] WebSocket over TLS (WSS)
- [ ] Compressed WebSocket frames
- [ ] IRCv3 capability negotiation over WebSocket
