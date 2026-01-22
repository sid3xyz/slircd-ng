# slircd-ng System Architecture

A visual breakdown of the slircd-ng server architecture and its core subsystems.

```mermaid
graph TD
    User([User Client]) <--> Gateway
    Server([Remote Server]) <--> Gateway

    subgraph "slircd-ng Core"
        Gateway[Gateway Layer]
        
        subgraph "Network Subsystem"
            connection[Connection Manager]
            event_loop[Unified Event Loop]
            dispatch[Message Pipeline]
            Gateway --> connection
            connection --> event_loop
            event_loop --> dispatch
        end

        subgraph "State Subsystem (Matrix)"
            matrix[Matrix Container]
            user_mgr[User Manager]
            chan_mgr[Channel Manager]
            stats_mgr[Stats Manager]
            matrix --> user_mgr
            matrix --> chan_mgr
            matrix --> stats_mgr
        end

        subgraph "Protocol Handlers"
            registry[Command Registry]
            oper_p[Oper Handlers]
            chan_p[Channel Handlers]
            user_p[User Handlers]
            s2s_p[S2S Handlers]
            dispatch --> registry
            registry --> oper_p
            registry --> chan_p
            registry --> user_p
            registry --> s2s_p
        end

        subgraph "Sync Subsystem (S2S)"
            sync_mgr[Sync Manager]
            handshake[Handshake SM]
            topology[Network Topology]
            crdt[State Sync/CRDT]
            s2s_p --> sync_mgr
            sync_mgr --> handshake
            sync_mgr --> topology
            sync_mgr --> crdt
        end

        subgraph "Security Subsystem"
            rbl[RBL Scanner]
            spam[Spam Detector]
            rate_limit[Rate Limiter]
            dispatch --> security_gate[Security Barrier]
            security_gate --> rbl
            security_gate --> spam
            security_gate --> rate_limit
        end

        subgraph "Persistence & Services"
            sqlx[(SQLx / SQLite)]
            redb[(Redb / KV Store)]
            history[History Module]
            services[IRC Services]
            chan_mgr -.-> redb
            user_mgr -.-> sqlx
            history --> redb
            services --> redb
        end
    end

    oper_p --> matrix
    chan_p --> matrix
    user_p --> matrix
    s2s_p --> sync_mgr
```

## Subsystem Descriptions

| Subsystem | Core Responsibilities | Key Components |
|:----------|:----------------------|:---------------|
| **Network** | TCP/TLS termination, decoding, and event distribution. | `gateway.rs`, `event_loop.rs`, `dispatch.rs` |
| **State** | Global DI container and resource management (Users, Channels). | `matrix.rs`, `user_manager.rs`, `channel_manager.rs` |
| **Handlers** | Logic for individual IRC and S2S commands. | `src/handlers/`, `registry.rs` |
| **Sync** | TS6-compatible federation, topology tracking, and state propagation. | `sync/manager.rs`, `handshake.rs`, `topology.rs` |
| **Security** | Admission control, spam mitigation, and rate enforcement. | `security/rbl.rs`, `spam.rs`, `rate_limit.rs` |
| **Persistence** | Fast history storage and durable account/configuration state. | `db/`, `history/`, `persistence.rs` |
| **Services** | Network-level logic for NickServ and ChanServ. | `services/` |
