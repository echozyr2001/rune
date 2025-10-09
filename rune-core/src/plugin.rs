//! Plugin system for modular architecture

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use crate::config::Config;
use crate::error::{Result, RuneError};
use crate::event::EventBus;
use crate::state::StateManager;

/// Core plugin trait that all plugins must implement
#[async_trait]
pub trait Plugin: Send + Sync {
    /// Get the plugin name
    fn name(&self) -> &str;

    /// Get the plugin version
    fn version(&self) -> &str;

    /// Get plugin dependencies (other plugin names)
    fn dependencies(&self) -> Vec<&str> {
        Vec::new()
    }

    /// Initialize the plugin with the given context
    async fn initialize(&mut self, context: &PluginContext) -> Result<()>;

    /// Shutdown the plugin gracefully
    async fn shutdown(&mut self) -> Result<()>;

    /// Get plugin status
    fn status(&self) -> PluginStatus {
        PluginStatus::Active
    }

    /// Get services provided by this plugin
    fn provided_services(&self) -> Vec<&str> {
        Vec::new()
    }
}

/// Context provided to plugins during initialization
#[derive(Clone)]
pub struct PluginContext {
    pub event_bus: Arc<dyn EventBus>,
    pub config: Arc<Config>,
    pub state_manager: Arc<StateManager>,
}

impl PluginContext {
    /// Create a new plugin context
    pub fn new(
        event_bus: Arc<dyn EventBus>,
        config: Arc<Config>,
        state_manager: Arc<StateManager>,
    ) -> Self {
        Self {
            event_bus,
            config,
            state_manager,
        }
    }
}

/// Plugin registry for managing loaded plugins
#[allow(dead_code)]
pub struct PluginRegistry {
    plugins: HashMap<String, Box<dyn Plugin>>,
    plugin_info: HashMap<String, PluginInfo>,
    dependencies: DependencyGraph,
    load_order: Vec<String>,
}

impl PluginRegistry {
    /// Create a new plugin registry
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            plugin_info: HashMap::new(),
            dependencies: DependencyGraph::new(),
            load_order: Vec::new(),
        }
    }

    /// Initialize the plugin registry
    pub async fn initialize(&mut self, _context: PluginContext) -> Result<()> {
        tracing::info!("Initializing plugin registry");

        // In a complete implementation, this would:
        // 1. Scan for available plugins
        // 2. Resolve dependencies
        // 3. Load plugins in correct order
        // 4. Initialize each plugin

        Ok(())
    }

    /// Shutdown all plugins
    pub async fn shutdown(&mut self) -> Result<()> {
        tracing::info!("Shutting down plugins");

        // Shutdown plugins in reverse load order
        for plugin_name in self.load_order.iter().rev() {
            if let Some(plugin) = self.plugins.get_mut(plugin_name) {
                if let Err(e) = plugin.shutdown().await {
                    tracing::error!("Failed to shutdown plugin {}: {}", plugin_name, e);
                }
            }
        }

        self.plugins.clear();
        self.plugin_info.clear();
        self.load_order.clear();

        Ok(())
    }

    /// Register a plugin
    pub async fn register_plugin(
        &mut self,
        mut plugin: Box<dyn Plugin>,
        context: &PluginContext,
    ) -> Result<()> {
        let name = plugin.name().to_string();
        let version = plugin.version().to_string();

        tracing::info!("Registering plugin: {} v{}", name, version);

        // Check dependencies
        for dep in plugin.dependencies() {
            if !self.plugins.contains_key(dep) {
                return Err(RuneError::Plugin(format!(
                    "Plugin {} depends on {}, which is not loaded",
                    name, dep
                )));
            }
        }

        // Initialize the plugin
        plugin.initialize(context).await?;

        // Store plugin info
        let info = PluginInfo {
            name: name.clone(),
            version: version.clone(),
            status: plugin.status(),
            load_time: SystemTime::now(),
            dependencies: plugin
                .dependencies()
                .iter()
                .map(|s| s.to_string())
                .collect(),
            provided_services: plugin
                .provided_services()
                .iter()
                .map(|s| s.to_string())
                .collect(),
        };

        self.plugin_info.insert(name.clone(), info);
        self.plugins.insert(name.clone(), plugin);
        self.load_order.push(name);

        Ok(())
    }

    /// Get plugin information
    pub fn get_plugin_info(&self, name: &str) -> Option<&PluginInfo> {
        self.plugin_info.get(name)
    }

    /// List all loaded plugins
    pub fn list_plugins(&self) -> Vec<&PluginInfo> {
        self.plugin_info.values().collect()
    }

    /// Check if a plugin is loaded
    pub fn is_plugin_loaded(&self, name: &str) -> bool {
        self.plugins.contains_key(name)
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about a loaded plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub status: PluginStatus,
    pub load_time: SystemTime,
    pub dependencies: Vec<String>,
    pub provided_services: Vec<String>,
}

/// Plugin status enumeration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PluginStatus {
    Loading,
    Active,
    Error(String),
    Disabled,
}

/// Dependency graph for plugin loading order
#[derive(Debug)]
pub struct DependencyGraph {
    dependencies: HashMap<String, Vec<String>>,
}

impl DependencyGraph {
    /// Create a new dependency graph
    pub fn new() -> Self {
        Self {
            dependencies: HashMap::new(),
        }
    }

    /// Add a dependency relationship
    pub fn add_dependency(&mut self, plugin: String, dependency: String) {
        self.dependencies
            .entry(plugin)
            .or_default()
            .push(dependency);
    }

    /// Resolve load order using topological sort
    pub fn resolve_load_order(&self) -> Result<Vec<String>> {
        // Simplified implementation - in a real system this would do proper topological sorting
        let mut order = Vec::new();

        for plugin in self.dependencies.keys() {
            if !order.contains(plugin) {
                order.push(plugin.clone());
            }
        }

        Ok(order)
    }
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}
