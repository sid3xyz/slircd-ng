//! Infrastructure - Observability
//!
//! Core metrics tracking (always compiled, always collecting).
//! 
//! # Architecture (Big 4 Pattern)
//!
//! - **Core**: Metrics collection (this module) - ALWAYS ENABLED
//! - **Plugin**: Export backend (extensions/prometheus/) - OPTIONAL
//!
//! This matches UnrealIRCd, InspIRCd, Ergo, Solanum:
//! - Core always tracks connections, commands, errors
//! - Plugins expose metrics via HTTP/UDP/etc
//!
//! # Competitive Analysis
//!
//! - **UnrealIRCd**: Core stats always available, m_prometheus.so optional
//! - **InspIRCd**: Core /STATS commands always work, m_httpd_stats.so optional
//! - **Ergo**: Built-in metrics always collected, HTTP endpoint configurable
//! - **Solanum**: Core tracking always active, external exporters optional
//! - **SLIRCd**: MATCHES BIG 4 - core tracks, plugin exports

pub mod metrics;
pub use metrics::*;
