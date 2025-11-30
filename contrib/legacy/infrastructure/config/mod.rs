// Infrastructure - Configuration management
// TOML parsing, environment variables, CLI args, hot-reload (SIGHUP)

#[allow(clippy::module_inception)]
pub mod config;
pub mod config_watcher;

pub use config::Config;
// pub use config_watcher::ConfigWatcher; // Not exported
