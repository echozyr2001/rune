//! Web server plugin for Rune

use async_trait::async_trait;
use rune_core::{Plugin, PluginContext, PluginStatus, Result};

/// Web server plugin implementation
pub struct ServerPlugin {
    name: String,
    version: String,
    status: PluginStatus,
}

impl ServerPlugin {
    /// Create a new server plugin
    pub fn new() -> Self {
        Self {
            name: "server".to_string(),
            version: "0.1.0".to_string(),
            status: PluginStatus::Loading,
        }
    }
}

impl Default for ServerPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for ServerPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn dependencies(&self) -> Vec<&str> {
        vec!["renderer"] // Depends on renderer for content
    }

    async fn initialize(&mut self, _context: &PluginContext) -> Result<()> {
        tracing::info!("Initializing server plugin");
        self.status = PluginStatus::Active;
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        tracing::info!("Shutting down server plugin");
        Ok(())
    }

    fn status(&self) -> PluginStatus {
        self.status.clone()
    }

    fn provided_services(&self) -> Vec<&str> {
        vec!["http-server", "websocket-server"]
    }
}
