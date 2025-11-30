//! Services as Plugin - Phase 3 validation of plugin_api
//!
//! Refactors embedded NickServ/ChanServ to use Plugin trait exclusively.
//! Validates plugin lifecycle, hooks, and state access patterns.

use crate::core::state::ServerState;
use crate::extensions::services::pseudo_client::ServiceBots;
use crate::infrastructure::config::Config;
use crate::plugin_api::{
    Plugin, PluginMetadata,
};
use anyhow::{Context, Result};
use std::sync::Arc;

/// Services Plugin - NickServ + ChanServ as first real plugin
pub struct ServicesPlugin {
    bots: Option<ServiceBots>,
    config: Arc<Config>,
    state: Option<Arc<ServerState>>,
}

impl ServicesPlugin {
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            bots: None,
            config,
            state: None,
        }
    }

    /// Spawn service pseudo-clients
    async fn spawn_services(&mut self, state: Arc<ServerState>) -> Result<()> {
        if !self.config.services.enabled {
            tracing::info!("services disabled in config");
            return Ok(());
        }

        tracing::info!(
            nickserv = %self.config.services.nickserv_name,
            chanserv = %self.config.services.chanserv_name,
            "spawning services pseudo-clients"
        );

        // Use existing spawn_services helper (re-exported from pseudo_client)
        let bots = crate::extensions::services::spawn_services(Arc::clone(&state), Arc::clone(&self.config))
            .await
            .context("failed to spawn services")?;

        self.bots = Some(bots);
        Ok(())
    }
}

#[async_trait::async_trait]
impl Plugin for ServicesPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: "services".to_string(),
            name: "IRC Services".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            author: "SLIRCd Team".to_string(),
            description: "Embedded NickServ and ChanServ services".to_string(),
            dependencies: vec![],
        }
    }

    async fn on_load(&mut self) -> Result<()> {
        tracing::info!("services plugin loaded");
        Ok(())
    }

    async fn on_enable(&mut self) -> Result<()> {
        // State will be provided via registry - for now use stored reference
        if let Some(state) = &self.state {
            self.spawn_services(Arc::clone(state)).await?;
        }
        tracing::info!("services plugin enabled");
        Ok(())
    }

    async fn on_disable(&mut self) -> Result<()> {
        tracing::info!("services plugin disabled");
        Ok(())
    }

    async fn on_unload(&mut self) -> Result<()> {
        if let (Some(state), Some(bots)) = (&self.state, &self.bots) {
            if let Some(nickserv_id) = bots.nickserv_id {
                state.remove_client(nickserv_id).await;
                tracing::info!(client_id = ?nickserv_id, "NickServ removed");
            }
            if let Some(chanserv_id) = bots.chanserv_id {
                state.remove_client(chanserv_id).await;
                tracing::info!(client_id = ?chanserv_id, "ChanServ removed");
            }
        }
        self.bots = None;
        tracing::info!("services plugin unloaded");
        Ok(())
    }

    async fn health_check(&self) -> Result<()> {
        if !self.config.services.enabled {
            return Ok(());
        }
        
        match &self.bots {
            Some(_) => Ok(()),
            None => anyhow::bail!("services not spawned"),
        }
    }
}

impl ServicesPlugin {
    /// Store state reference for plugin operations
    pub fn set_state(&mut self, state: Arc<ServerState>) {
        self.state = Some(state);
    }
}
