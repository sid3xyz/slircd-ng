# Phase 2: Operational Maturity

> **Target Duration:** 2-3 weeks
> **Primary Agent:** server-engineer
> **Exit Criteria:** Event sourcing capturing all changes, plugin system loading handlers, state replay working

---

## Overview

Phase 2 adds the operational infrastructure that differentiates a production server from a development prototype. These features enable debugging, auditing, and extensibility.

---

## 2.1 Event Sourcing

**Priority:** Critical
**Agent:** server-engineer (supported by: observability-engineer)
**Estimated Effort:** 1 week

### Objective

Capture every state-changing operation in an append-only log. This enables audit trails, debugging, and state reconstruction.

### Event Types

| Category      | Events                                                               |
| ------------- | -------------------------------------------------------------------- |
| User          | UserConnect, UserDisconnect, UserNickChange, UserModeChange          |
| Channel       | ChannelCreate, ChannelDestroy, ChannelJoin, ChannelPart, ChannelKick |
| Channel State | TopicChange, ModeChange, BanAdd, BanRemove                           |
| Message       | PrivmsgSent, NoticeSent (optional, high volume)                      |
| Services      | NickRegister, NickIdentify, ChanRegister, ChanFlags                  |
| Oper          | OperLogin, Kill, Kline, Rehash                                       |

### Database Schema

```sql
CREATE TABLE events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,           -- ISO 8601
    sequence INTEGER NOT NULL,         -- Logical clock
    event_type TEXT NOT NULL,          -- e.g., "channel.join"
    actor_uid TEXT,                    -- Who caused the event
    target TEXT,                       -- Channel name, nick, etc.
    payload TEXT NOT NULL,             -- JSON details
    session_id TEXT                    -- Server instance ID
);

CREATE INDEX idx_events_timestamp ON events(timestamp);
CREATE INDEX idx_events_type ON events(event_type);
CREATE INDEX idx_events_target ON events(target);
```

### Implementation Steps

```
STEP 1: Create event types
  FILE: src/events/mod.rs (new)
  - Define ServerEvent enum with all event variants
  - Implement Serialize for JSON encoding
  - Include LamportClock timestamp from slirc-proto CRDT

STEP 2: Create event store
  FILE: src/events/store.rs (new)
  - EventStore struct wrapping SqlitePool
  - async fn append(event: ServerEvent) -> Result<i64>
  - async fn query_range(start: DateTime, end: DateTime) -> Vec<ServerEvent>
  - async fn query_by_target(target: &str) -> Vec<ServerEvent>

STEP 3: Create event bus
  FILE: src/events/bus.rs (new)
  - Use tokio::sync::broadcast for in-memory fanout
  - EventBus::publish(event) -> writes to store + broadcast
  - Subscribers: metrics, WebSocket admin, state machines

STEP 4: Instrument channel actor
  FILE: src/state/actor/mod.rs
  - Inject EventBus into ChannelActor
  - Emit events for: Join, Part, Kick, Topic, Mode, Message

STEP 5: Instrument connection handler
  FILE: src/connection/mod.rs
  - Emit UserConnect on successful registration
  - Emit UserDisconnect on connection close
  - Emit UserNickChange on NICK command

STEP 6: Instrument services
  FILE: src/services/nickserv/mod.rs
  FILE: src/services/chanserv/mod.rs
  - Emit NickRegister, ChanRegister, etc.

STEP 7: Add admin query endpoint
  FILE: src/http/admin.rs (new)
  - GET /admin/events?from=X&to=Y
  - Requires admin authentication
  - Returns JSON array of events
```

### Verification

```bash
# Join a channel, check events
sqlite3 slircd.db "SELECT * FROM events WHERE event_type = 'channel.join' LIMIT 5;"

# Query via admin API
curl -u admin:password http://localhost:8080/admin/events?from=2025-01-01
```

### Files to Create/Modify

| File                         | Action | Lines (est.) |
| ---------------------------- | ------ | ------------ |
| src/events/mod.rs            | Create | 150          |
| src/events/store.rs          | Create | 200          |
| src/events/bus.rs            | Create | 80           |
| src/state/actor/mod.rs       | Modify | +50          |
| src/connection/mod.rs        | Modify | +30          |
| src/services/nickserv/mod.rs | Modify | +20          |
| src/services/chanserv/mod.rs | Modify | +20          |
| src/http/admin.rs            | Create | 100          |
| migrations/003_events.sql    | Create | 20           |

---

## 2.2 Plugin System

**Priority:** High
**Agent:** server-engineer (supported by: security-ops)
**Estimated Effort:** 1 week

### Objective

Allow extending server functionality without modifying core code. Plugins can add commands, services, and event handlers.

### Plugin Architecture

```
┌─────────────────────────────────────────────────────────┐
│                      slircd-ng                          │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐     │
│  │ Core        │  │ Plugin      │  │ Plugin      │     │
│  │ Handlers    │  │ Manager     │  │ Sandbox     │     │
│  └─────────────┘  └─────────────┘  └─────────────┘     │
│         │                │                │             │
│         └────────────────┴────────────────┘             │
│                          │                              │
│  ┌───────────────────────┴───────────────────────────┐ │
│  │                    Plugin API                      │ │
│  │  - register_command(name, handler)                 │ │
│  │  - register_event_listener(event_type, handler)   │ │
│  │  - get_user(uid) -> Option<UserSnapshot>          │ │
│  │  - get_channel(name) -> Option<ChannelSnapshot>   │ │
│  │  - send_message(target, message)                  │ │
│  └────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
           │              │              │
    ┌──────┴──────┐ ┌─────┴─────┐ ┌─────┴─────┐
    │  Lua Plugin │ │ WASM      │ │ Native    │
    │  (mlua)     │ │ (wasmtime)│ │ (dylib)   │
    └─────────────┘ └───────────┘ └───────────┘
```

### Implementation Strategy

Start with **Lua plugins** (mlua crate) for safety and simplicity. Native dylib plugins are Phase 3 scope.

### Plugin API

```rust
trait Plugin: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn on_load(&mut self, api: &PluginApi) -> Result<()>;
    fn on_unload(&mut self) -> Result<()>;
}

struct PluginApi {
    // Registration
    fn register_command(&self, name: &str, handler: CommandHandler);
    fn register_event_listener(&self, event: EventType, handler: EventHandler);

    // Read-only queries
    fn get_user(&self, uid: &str) -> Option<UserSnapshot>;
    fn get_channel(&self, name: &str) -> Option<ChannelSnapshot>;
    fn list_channels(&self) -> Vec<String>;

    // Actions (capability-gated)
    fn send_privmsg(&self, target: &str, message: &str) -> Result<()>;
    fn send_notice(&self, target: &str, message: &str) -> Result<()>;
    fn kick_user(&self, channel: &str, nick: &str, reason: &str) -> Result<()>;
}
```

### Implementation Steps

```
STEP 1: Add mlua dependency
  FILE: Cargo.toml
  - Add: mlua = { version = "0.9", features = ["lua54", "async", "send"] }

STEP 2: Define Plugin trait
  FILE: src/plugins/mod.rs (new)
  - Plugin trait definition
  - PluginManager struct
  - PluginApi struct

STEP 3: Implement PluginManager
  FILE: src/plugins/manager.rs (new)
  - load_plugin(path: &Path) -> Result<()>
  - unload_plugin(name: &str) -> Result<()>
  - list_plugins() -> Vec<PluginInfo>
  - Plugin lifecycle management

STEP 4: Implement Lua bindings
  FILE: src/plugins/lua.rs (new)
  - LuaPlugin struct implementing Plugin
  - Expose PluginApi to Lua runtime
  - Sandbox: disable os.execute, io.*, etc.

STEP 5: Create plugin directory structure
  CONFIG: plugins/ directory
  - Each plugin: plugins/myplugin/init.lua
  - Optional: plugins/myplugin/config.toml

STEP 6: Hook into command dispatch
  FILE: src/handlers/core/registry.rs
  - Before core dispatch, check plugin commands
  - Plugin commands have lower priority than core

STEP 7: Hook into event bus
  FILE: src/events/bus.rs
  - Notify plugin event listeners
  - Run in separate task (don't block main event loop)

STEP 8: Add PLUGIN admin commands
  FILE: src/handlers/oper/plugin.rs (new)
  - PLUGIN LOAD <name>
  - PLUGIN UNLOAD <name>
  - PLUGIN LIST
  - PLUGIN RELOAD <name>
```

### Example Lua Plugin

```lua
-- plugins/welcome/init.lua
local plugin = {}

function plugin.on_load(api)
    api.register_event_listener("user.connect", function(event)
        api.send_notice(event.nick, "Welcome to our IRC network!")
    end)

    api.register_command("GREET", function(ctx, args)
        local target = args[1] or ctx.nick
        api.send_privmsg(ctx.channel, "Hello, " .. target .. "!")
    end)
end

function plugin.on_unload()
    -- Cleanup
end

return plugin
```

### Verification

```bash
# Create test plugin
mkdir -p plugins/test
cat > plugins/test/init.lua << 'EOF'
local p = {}
function p.on_load(api) print("Test plugin loaded!") end
return p
EOF

# Load via OPER command
/plugin load test
# Check logs for "Test plugin loaded!"
```

### Files to Create/Modify

| File                          | Action | Lines (est.) |
| ----------------------------- | ------ | ------------ |
| src/plugins/mod.rs            | Create | 100          |
| src/plugins/manager.rs        | Create | 200          |
| src/plugins/lua.rs            | Create | 300          |
| src/handlers/oper/plugin.rs   | Create | 80           |
| src/handlers/core/registry.rs | Modify | +30          |
| src/events/bus.rs             | Modify | +20          |
| Cargo.toml                    | Modify | +3           |
| plugins/example/init.lua      | Create | 30           |

---

## 2.3 State Replay

**Priority:** Medium
**Agent:** server-engineer (supported by: observability-engineer)
**Estimated Effort:** 3 days

### Objective

Reconstruct server state at any point in time by replaying the event log. Essential for debugging production issues.

### Implementation Steps

```
STEP 1: Define ReplayableState trait
  FILE: src/replay/mod.rs (new)
  - trait ReplayableState { fn apply(event: &ServerEvent) -> Result<()> }
  - Implement for ChannelState, UserState

STEP 2: Implement StateReconstructor
  FILE: src/replay/reconstructor.rs (new)
  - reconstruct_at(timestamp: DateTime) -> ServerSnapshot
  - Read events up to timestamp
  - Apply in order to fresh state

STEP 3: Add replay CLI command
  FILE: src/bin/slircd-replay.rs (new)
  - slircd-replay --db slircd.db --at "2025-01-01T12:00:00Z"
  - Output: JSON snapshot of state at that time

STEP 4: Add debug admin endpoint
  FILE: src/http/admin.rs
  - GET /admin/replay?at=2025-01-01T12:00:00Z
  - Returns state snapshot as JSON

STEP 5: Add diff capability
  FILE: src/replay/diff.rs (new)
  - diff_states(before: DateTime, after: DateTime) -> StateDiff
  - Show what changed between two points
```

### Verification

```bash
# Replay to specific time
./target/debug/slircd-replay --db slircd.db --at "2025-01-01T12:00:00Z" | jq .

# Via admin API
curl http://localhost:8080/admin/replay?at=2025-01-01T12:00:00Z | jq .
```

### Files to Create/Modify

| File                        | Action | Lines (est.) |
| --------------------------- | ------ | ------------ |
| src/replay/mod.rs           | Create | 50           |
| src/replay/reconstructor.rs | Create | 150          |
| src/replay/diff.rs          | Create | 100          |
| src/bin/slircd-replay.rs    | Create | 80           |
| src/http/admin.rs           | Modify | +30          |

---

## 2.4 Production Deployment Guide

**Priority:** Medium
**Agent:** release-manager
**Estimated Effort:** 2 days

### Deliverables

1. **systemd Unit File**
   ```ini
   [Unit]
   Description=SLIRC IRC Daemon
   After=network.target

   [Service]
   Type=notify
   ExecStart=/usr/local/bin/slircd /etc/slircd/config.toml
   ExecReload=/bin/kill -HUP $MAINPID
   Restart=on-failure
   User=slircd
   Group=slircd

   [Install]
   WantedBy=multi-user.target
   ```

2. **Docker Compose**
   ```yaml
   version: '3.8'
   services:
     slircd:
       image: slircd:latest
       ports:
         - "6667:6667"
         - "6697:6697"
       volumes:
         - ./config:/etc/slircd
         - ./data:/var/lib/slircd
       restart: unless-stopped
   ```

3. **Prometheus/Grafana Dashboards**
   - IRC Server Overview dashboard
   - Channel Activity dashboard
   - User Session dashboard

4. **Runbook**
   - Startup/shutdown procedures
   - Log analysis
   - Common issues and fixes
   - Scaling considerations

### Files to Create

| File                             | Action | Lines (est.) |
| -------------------------------- | ------ | ------------ |
| deploy/slircd.service            | Create | 25           |
| deploy/docker-compose.yml        | Create | 30           |
| deploy/Dockerfile                | Create | 40           |
| deploy/grafana/dashboards/*.json | Create | 500          |
| docs/RUNBOOK.md                  | Create | 200          |

---

## Phase 2 Completion Checklist

- [ ] 2.1 Event Sourcing
  - [ ] ServerEvent enum defined
  - [ ] EventStore with SQLite backend
  - [ ] EventBus with broadcast
  - [ ] Channel actor instrumentation
  - [ ] Connection instrumentation
  - [ ] Services instrumentation
  - [ ] Admin query endpoint
  - [ ] Database migrations

- [ ] 2.2 Plugin System
  - [ ] Plugin trait defined
  - [ ] PluginManager implementation
  - [ ] Lua bindings via mlua
  - [ ] PluginApi with read/write methods
  - [ ] Command registration hooks
  - [ ] Event listener hooks
  - [ ] PLUGIN admin commands
  - [ ] Example plugin

- [ ] 2.3 State Replay
  - [ ] ReplayableState trait
  - [ ] StateReconstructor implementation
  - [ ] slircd-replay CLI tool
  - [ ] Admin replay endpoint
  - [ ] State diff capability

- [ ] 2.4 Deployment Guide
  - [ ] systemd unit file
  - [ ] Docker Compose setup
  - [ ] Grafana dashboards
  - [ ] Operations runbook

---

## Agent Handoff Notes

When assigning this phase to AI agents:

1. **Event Sourcing first** - Plugin system depends on EventBus
2. **Use existing CRDT types** - LamportClock from slirc-proto for event ordering
3. **Security for plugins** - Lua sandbox is critical, no arbitrary code execution
4. **Test event replay** - Verify idempotency (replaying twice = same result)
5. **Metrics integration** - Hook event counts into existing Prometheus metrics

### Recommended Prompts for GPT-5.1-codex-max

```
TASK: Implement event sourcing for slircd-ng
CONTEXT: Read PHASE_2_OPERATIONAL.md section 2.1
FILES TO READ FIRST:
  - src/state/actor/mod.rs (channel events)
  - src/connection/mod.rs (user events)
  - slirc-proto/src/crdt/clock.rs (LamportClock)
CONSTRAINTS:
  - Use SQLite for persistence (existing pattern)
  - Use tokio::sync::broadcast for in-memory fanout
  - Events must be serializable to JSON
OUTPUT: Implementation with tests, database migration
```
