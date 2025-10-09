//! Content renderer plugin for Rune

use async_trait::async_trait;
use rune_core::{Plugin, PluginContext, PluginStatus, Result};

/// Content renderer plugin implementation
pub struct RendererPlugin {
    name: String,
    version: String,
    status: PluginStatus,
}

impl RendererPlugin {
    /// Create a new renderer plugin
    pub fn new() -> Self {
        Self {
            name: "renderer".to_string(),
            version: "0.1.0".to_string(),
            status: PluginStatus::Loading,
        }
    }
}

impl Default for RendererPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for RendererPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn dependencies(&self) -> Vec<&str> {
        vec![] // No dependencies for the renderer
    }

    async fn initialize(&mut self, _context: &PluginContext) -> Result<()> {
        tracing::info!("Initializing renderer plugin");
        self.status = PluginStatus::Active;
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        tracing::info!("Shutting down renderer plugin");
        Ok(())
    }

    fn status(&self) -> PluginStatus {
        self.status.clone()
    }

    fn provided_services(&self) -> Vec<&str> {
        vec!["markdown-rendering", "mermaid-rendering"]
    }
}
