//! Theme management plugin for Rune

use async_trait::async_trait;
use rune_core::{Plugin, PluginContext, PluginStatus, Result};

/// Theme management plugin implementation
pub struct ThemePlugin {
    name: String,
    version: String,
    status: PluginStatus,
}

impl ThemePlugin {
    /// Create a new theme plugin
    pub fn new() -> Self {
        Self {
            name: "theme".to_string(),
            version: "0.1.0".to_string(),
            status: PluginStatus::Loading,
        }
    }
}

impl Default for ThemePlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for ThemePlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn dependencies(&self) -> Vec<&str> {
        vec![] // No dependencies for theme management
    }

    async fn initialize(&mut self, _context: &PluginContext) -> Result<()> {
        tracing::info!("Initializing theme plugin");
        self.status = PluginStatus::Active;
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        tracing::info!("Shutting down theme plugin");
        Ok(())
    }

    fn status(&self) -> PluginStatus {
        self.status.clone()
    }

    fn provided_services(&self) -> Vec<&str> {
        vec!["theme-management", "css-serving"]
    }
}
