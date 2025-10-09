//! File watcher plugin for Rune

use async_trait::async_trait;
use rune_core::{Plugin, PluginContext, PluginStatus, Result};

/// File watcher plugin implementation
pub struct FileWatcherPlugin {
    name: String,
    version: String,
    status: PluginStatus,
}

impl FileWatcherPlugin {
    /// Create a new file watcher plugin
    pub fn new() -> Self {
        Self {
            name: "file-watcher".to_string(),
            version: "0.1.0".to_string(),
            status: PluginStatus::Loading,
        }
    }
}

impl Default for FileWatcherPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for FileWatcherPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn dependencies(&self) -> Vec<&str> {
        vec![] // No dependencies for the file watcher
    }

    async fn initialize(&mut self, _context: &PluginContext) -> Result<()> {
        tracing::info!("Initializing file watcher plugin");
        self.status = PluginStatus::Active;
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        tracing::info!("Shutting down file watcher plugin");
        Ok(())
    }

    fn status(&self) -> PluginStatus {
        self.status.clone()
    }

    fn provided_services(&self) -> Vec<&str> {
        vec!["file-watching"]
    }
}
