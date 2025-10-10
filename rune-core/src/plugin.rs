//! Plugin system for modular architecture

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::time::interval;
use tracing::{debug, error, info, warn};

use crate::config::Config;
use crate::error::{Result, RuneError};
use crate::event::{EventBus, SystemEvent};
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

/// Plugin registry for managing loaded plugins with lifecycle management
pub struct PluginRegistry {
    plugins: HashMap<String, Box<dyn Plugin>>,
    plugin_info: HashMap<String, PluginInfo>,
    dependencies: DependencyGraph,
    load_order: Vec<String>,
    health_monitor: PluginHealthMonitor,
    context: Option<PluginContext>,
}

impl PluginRegistry {
    /// Create a new plugin registry
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            plugin_info: HashMap::new(),
            dependencies: DependencyGraph::new(),
            load_order: Vec::new(),
            health_monitor: PluginHealthMonitor::new(),
            context: None,
        }
    }

    /// Initialize the plugin registry with context and start health monitoring
    pub async fn initialize(&mut self, context: PluginContext) -> Result<()> {
        info!("Initializing plugin registry");

        self.context = Some(context.clone());

        // Start health monitoring
        self.health_monitor
            .start_monitoring(context.clone())
            .await?;

        // Load plugins from configuration
        self.load_plugins_from_config(&context).await?;

        info!("Plugin registry initialized successfully");
        Ok(())
    }

    /// Load plugins from configuration
    async fn load_plugins_from_config(&mut self, context: &PluginContext) -> Result<()> {
        let config = &context.config;

        // Build dependency graph from configuration
        for plugin_config in &config.plugins {
            if plugin_config.enabled {
                for dep in &plugin_config.dependencies {
                    self.dependencies
                        .add_dependency(plugin_config.name.clone(), dep.clone());
                }
            }
        }

        // Resolve load order
        let load_order = self.dependencies.resolve_load_order()?;

        info!("Plugin load order resolved: {:?}", load_order);

        // Load plugins in dependency order
        for plugin_name in load_order {
            if let Some(plugin_config) = config.get_plugin_config(&plugin_name) {
                if plugin_config.enabled {
                    // In a real implementation, this would dynamically load plugin libraries
                    // For now, we'll just track the configuration
                    debug!("Would load plugin: {}", plugin_name);
                }
            }
        }

        Ok(())
    }

    /// Shutdown all plugins gracefully
    pub async fn shutdown(&mut self) -> Result<()> {
        info!("Shutting down plugin registry");

        // Stop health monitoring first
        self.health_monitor.stop_monitoring().await;

        // Shutdown plugins in reverse load order for proper dependency cleanup
        for plugin_name in self.load_order.iter().rev() {
            if let Some(mut plugin) = self.plugins.remove(plugin_name) {
                info!("Shutting down plugin: {}", plugin_name);

                // Update status to indicate shutdown in progress
                if let Some(info) = self.plugin_info.get_mut(plugin_name) {
                    info.status = PluginStatus::Shutting;
                }

                // Attempt graceful shutdown with timeout
                match tokio::time::timeout(Duration::from_secs(30), plugin.shutdown()).await {
                    Ok(Ok(())) => {
                        info!("Plugin {} shutdown successfully", plugin_name);
                        if let Some(info) = self.plugin_info.get_mut(plugin_name) {
                            info.status = PluginStatus::Stopped;
                        }
                    }
                    Ok(Err(e)) => {
                        error!("Plugin {} shutdown failed: {}", plugin_name, e);
                        if let Some(info) = self.plugin_info.get_mut(plugin_name) {
                            info.status = PluginStatus::Error(format!("Shutdown failed: {}", e));
                        }
                    }
                    Err(_) => {
                        error!("Plugin {} shutdown timed out", plugin_name);
                        if let Some(info) = self.plugin_info.get_mut(plugin_name) {
                            info.status = PluginStatus::Error("Shutdown timeout".to_string());
                        }
                    }
                }
            }
        }

        // Clear all data structures
        self.plugins.clear();
        self.plugin_info.clear();
        self.load_order.clear();
        self.dependencies = DependencyGraph::new();

        info!("Plugin registry shutdown complete");
        Ok(())
    }

    /// Register and initialize a plugin with full lifecycle management
    pub async fn register_plugin(
        &mut self,
        mut plugin: Box<dyn Plugin>,
        context: &PluginContext,
    ) -> Result<()> {
        let name = plugin.name().to_string();
        let version = plugin.version().to_string();

        info!("Registering plugin: {} v{}", name, version);

        // Check if plugin is already registered
        if self.plugins.contains_key(&name) {
            return Err(RuneError::Plugin(format!(
                "Plugin {} is already registered",
                name
            )));
        }

        // Validate dependencies
        self.validate_dependencies(plugin.as_ref())?;

        // Create initial plugin info with loading status
        let mut info = PluginInfo {
            name: name.clone(),
            version: version.clone(),
            status: PluginStatus::Loading,
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
            health_status: PluginHealthStatus::Unknown,
            last_health_check: SystemTime::now(),
            restart_count: 0,
        };

        self.plugin_info.insert(name.clone(), info.clone());

        // Publish plugin loading event
        if let Err(e) = context
            .event_bus
            .publish_system_event(SystemEvent::plugin_loading(name.clone()))
            .await
        {
            warn!("Failed to publish plugin loading event: {}", e);
        }

        // Initialize the plugin with timeout
        match tokio::time::timeout(Duration::from_secs(60), plugin.initialize(context)).await {
            Ok(Ok(())) => {
                info!("Plugin {} initialized successfully", name);
                info.status = PluginStatus::Active;
                info.health_status = PluginHealthStatus::Healthy;
            }
            Ok(Err(e)) => {
                error!("Plugin {} initialization failed: {}", name, e);
                info.status = PluginStatus::Error(format!("Initialization failed: {}", e));
                info.health_status = PluginHealthStatus::Unhealthy;
                self.plugin_info.insert(name.clone(), info);
                return Err(RuneError::Plugin(format!(
                    "Failed to initialize plugin {}: {}",
                    name, e
                )));
            }
            Err(_) => {
                error!("Plugin {} initialization timed out", name);
                info.status = PluginStatus::Error("Initialization timeout".to_string());
                info.health_status = PluginHealthStatus::Unhealthy;
                self.plugin_info.insert(name.clone(), info);
                return Err(RuneError::Plugin(format!(
                    "Plugin {} initialization timed out",
                    name
                )));
            }
        }

        // Update plugin info and store plugin
        self.plugin_info.insert(name.clone(), info);
        self.plugins.insert(name.clone(), plugin);
        self.load_order.push(name.clone());

        // Register plugin for health monitoring
        self.health_monitor.register_plugin(name.clone());

        // Publish plugin loaded event
        if let Err(e) = context
            .event_bus
            .publish_system_event(SystemEvent::plugin_loaded(name.clone(), version.clone()))
            .await
        {
            warn!("Failed to publish plugin loaded event: {}", e);
        }

        info!("Plugin {} registered and initialized successfully", name);
        Ok(())
    }

    /// Validate plugin dependencies are satisfied
    fn validate_dependencies(&self, plugin: &dyn Plugin) -> Result<()> {
        for dep in plugin.dependencies() {
            if !self.is_plugin_active(dep) {
                return Err(RuneError::Plugin(format!(
                    "Plugin {} depends on {}, which is not active",
                    plugin.name(),
                    dep
                )));
            }
        }
        Ok(())
    }

    /// Unregister a plugin
    pub async fn unregister_plugin(&mut self, name: &str) -> Result<()> {
        info!("Unregistering plugin: {}", name);

        // Check if any other plugins depend on this one
        for (plugin_name, plugin_info) in &self.plugin_info {
            if plugin_info.dependencies.contains(&name.to_string()) && plugin_name != name {
                return Err(RuneError::Plugin(format!(
                    "Cannot unregister plugin {} because {} depends on it",
                    name, plugin_name
                )));
            }
        }

        // Remove from health monitoring
        self.health_monitor.unregister_plugin(name);

        // Shutdown and remove plugin
        if let Some(mut plugin) = self.plugins.remove(name) {
            if let Some(info) = self.plugin_info.get_mut(name) {
                info.status = PluginStatus::Shutting;
            }

            match tokio::time::timeout(Duration::from_secs(30), plugin.shutdown()).await {
                Ok(Ok(())) => {
                    info!("Plugin {} unregistered successfully", name);
                }
                Ok(Err(e)) => {
                    error!(
                        "Plugin {} shutdown failed during unregistration: {}",
                        name, e
                    );
                }
                Err(_) => {
                    error!("Plugin {} shutdown timed out during unregistration", name);
                }
            }
        }

        // Remove from data structures
        self.plugin_info.remove(name);
        self.load_order.retain(|n| n != name);

        // Publish plugin unloaded event
        if let Some(context) = &self.context {
            if let Err(e) = context
                .event_bus
                .publish_system_event(SystemEvent::plugin_unloaded(name.to_string()))
                .await
            {
                warn!("Failed to publish plugin unloaded event: {}", e);
            }
        }

        Ok(())
    }

    /// Restart a plugin
    pub async fn restart_plugin(&mut self, name: &str) -> Result<()> {
        info!("Restarting plugin: {}", name);

        if self.context.is_some() {
            // This is a simplified restart - in a real implementation,
            // we would need to preserve the plugin instance or reload it
            if let Some(info) = self.plugin_info.get_mut(name) {
                info.restart_count += 1;
                info.status = PluginStatus::Loading;
                info.health_status = PluginHealthStatus::Unknown;
                info.last_health_check = SystemTime::now();

                // In a real implementation, we would reload and reinitialize the plugin here
                info!(
                    "Plugin {} restart completed (restart count: {})",
                    name, info.restart_count
                );
                info.status = PluginStatus::Active;
                info.health_status = PluginHealthStatus::Healthy;
            }
        }

        Ok(())
    }

    /// Get plugin information
    pub fn get_plugin_info(&self, name: &str) -> Option<&PluginInfo> {
        self.plugin_info.get(name)
    }

    /// Get mutable plugin information
    pub fn get_plugin_info_mut(&mut self, name: &str) -> Option<&mut PluginInfo> {
        self.plugin_info.get_mut(name)
    }

    /// List all loaded plugins
    pub fn list_plugins(&self) -> Vec<&PluginInfo> {
        self.plugin_info.values().collect()
    }

    /// List plugins by status
    pub fn list_plugins_by_status(&self, status: &PluginStatus) -> Vec<&PluginInfo> {
        self.plugin_info
            .values()
            .filter(|info| std::mem::discriminant(&info.status) == std::mem::discriminant(status))
            .collect()
    }

    /// Check if a plugin is loaded
    pub fn is_plugin_loaded(&self, name: &str) -> bool {
        self.plugins.contains_key(name)
    }

    /// Check if a plugin is active and healthy
    pub fn is_plugin_active(&self, name: &str) -> bool {
        self.plugin_info
            .get(name)
            .map(|info| matches!(info.status, PluginStatus::Active))
            .unwrap_or(false)
    }

    /// Get plugin health status
    pub fn get_plugin_health(&self, name: &str) -> Option<PluginHealthStatus> {
        self.plugin_info
            .get(name)
            .map(|info| info.health_status.clone())
    }

    /// Get overall system health
    pub fn get_system_health(&self) -> SystemHealthStatus {
        let total_plugins = self.plugin_info.len();
        if total_plugins == 0 {
            return SystemHealthStatus::Healthy;
        }

        let unhealthy_count = self
            .plugin_info
            .values()
            .filter(|info| matches!(info.health_status, PluginHealthStatus::Unhealthy))
            .count();

        if unhealthy_count == 0 {
            SystemHealthStatus::Healthy
        } else if unhealthy_count < total_plugins / 2 {
            SystemHealthStatus::Degraded
        } else {
            SystemHealthStatus::Unhealthy
        }
    }

    /// Get dependency information for a plugin
    pub fn get_plugin_dependencies(&self, name: &str) -> Vec<String> {
        self.plugin_info
            .get(name)
            .map(|info| info.dependencies.clone())
            .unwrap_or_default()
    }

    /// Get plugins that depend on the given plugin
    pub fn get_dependent_plugins(&self, name: &str) -> Vec<String> {
        self.plugin_info
            .iter()
            .filter(|(_, info)| info.dependencies.contains(&name.to_string()))
            .map(|(plugin_name, _)| plugin_name.clone())
            .collect()
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about a loaded plugin with health monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub status: PluginStatus,
    pub load_time: SystemTime,
    pub dependencies: Vec<String>,
    pub provided_services: Vec<String>,
    pub health_status: PluginHealthStatus,
    pub last_health_check: SystemTime,
    pub restart_count: u32,
}

/// Plugin status enumeration with lifecycle states
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PluginStatus {
    Loading,
    Active,
    Shutting,
    Stopped,
    Error(String),
    Disabled,
}

/// Plugin health status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PluginHealthStatus {
    Unknown,
    Healthy,
    Unhealthy,
    Recovering,
}

/// System-wide health status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SystemHealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

/// Dependency graph for plugin loading order with proper topological sorting
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

    /// Resolve load order using topological sort (Kahn's algorithm)
    pub fn resolve_load_order(&self) -> Result<Vec<String>> {
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut graph: HashMap<String, Vec<String>> = HashMap::new();
        let mut all_nodes: HashSet<String> = HashSet::new();

        // Build the graph and calculate in-degrees
        for (plugin, deps) in &self.dependencies {
            all_nodes.insert(plugin.clone());
            in_degree.entry(plugin.clone()).or_insert(0);

            for dep in deps {
                all_nodes.insert(dep.clone());
                graph.entry(dep.clone()).or_default().push(plugin.clone());
                *in_degree.entry(plugin.clone()).or_insert(0) += 1;
            }
        }

        // Initialize in-degree for nodes that are only dependencies
        for node in &all_nodes {
            in_degree.entry(node.clone()).or_insert(0);
        }

        // Kahn's algorithm
        let mut queue: VecDeque<String> = VecDeque::new();
        let mut result: Vec<String> = Vec::new();

        // Find all nodes with no incoming edges
        for (node, &degree) in &in_degree {
            if degree == 0 {
                queue.push_back(node.clone());
            }
        }

        while let Some(node) = queue.pop_front() {
            result.push(node.clone());

            // For each neighbor of the current node
            if let Some(neighbors) = graph.get(&node) {
                for neighbor in neighbors {
                    if let Some(degree) = in_degree.get_mut(neighbor) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push_back(neighbor.clone());
                        }
                    }
                }
            }
        }

        // Check for circular dependencies
        if result.len() != all_nodes.len() {
            let remaining: Vec<String> = all_nodes
                .into_iter()
                .filter(|node| !result.contains(node))
                .collect();
            return Err(RuneError::Plugin(format!(
                "Circular dependency detected involving plugins: {:?}",
                remaining
            )));
        }

        Ok(result)
    }

    /// Check if there are any circular dependencies
    pub fn has_circular_dependencies(&self) -> bool {
        self.resolve_load_order().is_err()
    }

    /// Get direct dependencies of a plugin
    pub fn get_dependencies(&self, plugin: &str) -> Vec<String> {
        self.dependencies.get(plugin).cloned().unwrap_or_default()
    }

    /// Get all plugins that depend on the given plugin
    pub fn get_dependents(&self, plugin: &str) -> Vec<String> {
        self.dependencies
            .iter()
            .filter(|(_, deps)| deps.contains(&plugin.to_string()))
            .map(|(name, _)| name.clone())
            .collect()
    }
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

/// Plugin health monitoring system
#[derive(Debug)]
pub struct PluginHealthMonitor {
    monitored_plugins: HashSet<String>,
    monitoring_active: bool,
    health_check_interval: Duration,
}

impl PluginHealthMonitor {
    /// Create a new health monitor
    pub fn new() -> Self {
        Self {
            monitored_plugins: HashSet::new(),
            monitoring_active: false,
            health_check_interval: Duration::from_secs(30), // Check every 30 seconds
        }
    }

    /// Start health monitoring
    pub async fn start_monitoring(&mut self, context: PluginContext) -> Result<()> {
        if self.monitoring_active {
            return Ok(());
        }

        info!("Starting plugin health monitoring");
        self.monitoring_active = true;

        // Spawn health monitoring task
        let plugins = self.monitored_plugins.clone();
        let interval_duration = self.health_check_interval;
        let event_bus = context.event_bus.clone();

        tokio::spawn(async move {
            let mut interval = interval(interval_duration);

            loop {
                interval.tick().await;

                // In a real implementation, this would check actual plugin health
                // For now, we'll just log that we're monitoring
                if !plugins.is_empty() {
                    debug!("Health check for {} plugins", plugins.len());

                    // Simulate health check events
                    for plugin_name in &plugins {
                        if let Err(e) = event_bus
                            .publish_system_event(SystemEvent::plugin_health_check(
                                plugin_name.clone(),
                                PluginHealthStatus::Healthy,
                            ))
                            .await
                        {
                            warn!("Failed to publish health check event: {}", e);
                        }
                    }
                }
            }
        });

        Ok(())
    }

    /// Stop health monitoring
    pub async fn stop_monitoring(&mut self) {
        if !self.monitoring_active {
            return;
        }

        info!("Stopping plugin health monitoring");
        self.monitoring_active = false;
        // In a real implementation, we would cancel the monitoring task
    }

    /// Register a plugin for health monitoring
    pub fn register_plugin(&mut self, plugin_name: String) {
        debug!("Registering plugin {} for health monitoring", plugin_name);
        self.monitored_plugins.insert(plugin_name);
    }

    /// Unregister a plugin from health monitoring
    pub fn unregister_plugin(&mut self, plugin_name: &str) {
        debug!(
            "Unregistering plugin {} from health monitoring",
            plugin_name
        );
        self.monitored_plugins.remove(plugin_name);
    }

    /// Set health check interval
    pub fn set_health_check_interval(&mut self, interval: Duration) {
        self.health_check_interval = interval;
    }

    /// Get monitored plugins
    pub fn get_monitored_plugins(&self) -> &HashSet<String> {
        &self.monitored_plugins
    }

    /// Check if monitoring is active
    pub fn is_monitoring_active(&self) -> bool {
        self.monitoring_active
    }
}

impl Default for PluginHealthMonitor {
    fn default() -> Self {
        Self::new()
    }
}
