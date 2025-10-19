//! Rune Core - The foundational engine for the Rune markdown live editor
//!
//! This crate provides the core interfaces, event system, and plugin architecture
//! that powers the modular Rune markdown editor.

pub mod ast;
pub mod config;
pub mod error;
pub mod event;
pub mod file_watcher;
pub mod parser;
pub mod plugin;
pub mod quill;
pub mod render;
pub mod renderer;
pub mod state;

#[cfg(test)]
mod event_test;

#[cfg(test)]
mod plugin_test;

#[cfg(test)]
mod plugin_context_test;

// Re-export commonly used types
pub use ast::{Node, NodeType, ParseOptions, Position, Tree, WalkStatus};
pub use config::{
    Config, ConfigLoadContext, ConfigMetadata, PluginConfig, RuntimeConfigManager, ServerConfig,
    SystemConfig, ValidationResult,
};
pub use error::{Result, RuneError};
pub use event::{
    Event, EventBus, EventFilter, EventHandler, ExtendedEventBus, InMemoryEventBus, SubscriptionId,
    SystemEvent, SystemEventHandler,
};
pub use file_watcher::{DefaultFileFilter, FileFilter, FileWatcher, FileWatcherConfig, WatcherId};
pub use parser::MarkdownParser;
pub use plugin::{Plugin, PluginContext, PluginInfo, PluginRegistry, PluginStatus};
pub use quill::Quill;
pub use render::{render_html, render_wysiwyg, HtmlRenderer, RenderOptions, WysiwygRenderer};
pub use renderer::{
    Asset, AssetType, ContentRenderer, RenderContext, RenderMetadata, RenderResult,
    RendererRegistry,
};
pub use state::{ApplicationState, StateManager};

// CoreEngine is defined in this module, no need to re-export

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;

/// Core engine that orchestrates all plugins and system components
pub struct CoreEngine {
    event_bus: Arc<dyn EventBus>,
    plugin_registry: PluginRegistry,
    state_manager: Arc<StateManager>,
    config: Arc<Config>,
    is_initialized: bool,
    shutdown_signal: Option<tokio::sync::oneshot::Sender<()>>,
}

impl CoreEngine {
    /// Create a new CoreEngine instance
    pub fn new(config: Config) -> Result<Self> {
        let event_bus = Arc::new(event::InMemoryEventBus::new());
        let state_manager = Arc::new(StateManager::new());
        let plugin_registry = PluginRegistry::new();

        Ok(Self {
            event_bus,
            plugin_registry,
            state_manager,
            config: Arc::new(config),
            is_initialized: false,
            shutdown_signal: None,
        })
    }

    /// Initialize the core engine and load plugins with proper dependency ordering
    pub async fn initialize(&mut self) -> Result<()> {
        if self.is_initialized {
            tracing::warn!("Core engine is already initialized");
            return Ok(());
        }

        tracing::info!("Initializing Rune Core Engine");

        // Validate system configuration before initialization
        let validation_result = self.validate_system_pre_init().await?;
        if !validation_result.is_valid {
            return Err(RuneError::config(format!(
                "System validation failed: {}",
                validation_result.errors.join(", ")
            )));
        }

        // Initialize plugin registry with core services
        let context = PluginContext::new(
            self.event_bus.clone(),
            self.config.clone(),
            self.state_manager.clone(),
        );

        // Initialize plugin registry with enhanced error handling
        match self.plugin_registry.initialize(context.clone()).await {
            Ok(()) => {
                tracing::info!("Plugin registry initialized successfully");
            }
            Err(e) => {
                tracing::error!("Plugin registry initialization failed: {}", e);
                return Err(RuneError::Plugin(format!(
                    "Failed to initialize plugin registry: {}",
                    e
                )));
            }
        }

        // Discover and load plugins with dependency-aware ordering
        match self.discover_and_load_plugins_ordered(&context).await {
            Ok(()) => {
                tracing::info!("Plugin discovery and loading completed successfully");
            }
            Err(e) => {
                tracing::error!("Plugin loading failed: {}", e);
                // Attempt partial recovery - continue with successfully loaded plugins
                tracing::warn!("Continuing with partially loaded plugins");
            }
        }

        // Validate system state after initialization
        let post_init_validation = self.validate_system().await?;
        if !post_init_validation.is_valid {
            tracing::warn!(
                "Post-initialization validation warnings: {}",
                post_init_validation.warnings.join(", ")
            );
        }

        self.is_initialized = true;
        tracing::info!(
            "Core Engine initialized successfully with {} active plugins",
            post_init_validation.active_plugin_count
        );
        Ok(())
    }

    /// Discover and automatically load plugins with proper dependency ordering
    async fn discover_and_load_plugins_ordered(&mut self, context: &PluginContext) -> Result<()> {
        tracing::info!("Starting dependency-aware plugin discovery and loading");

        // Build dependency graph from configuration
        let mut dependency_graph = plugin::DependencyGraph::new();
        let mut plugin_configs = Vec::new();

        // Collect all plugin configurations
        for plugin_config in &self.config.plugins {
            if plugin_config.enabled {
                plugin_configs.push(plugin_config.clone());

                // Add dependencies to graph
                for dep in &plugin_config.dependencies {
                    dependency_graph.add_dependency(plugin_config.name.clone(), dep.clone());
                }
            }
        }

        // Resolve load order based on dependencies
        let load_order = match dependency_graph.resolve_load_order() {
            Ok(order) => {
                tracing::info!("Plugin load order resolved: {:?}", order);
                order
            }
            Err(e) => {
                tracing::error!("Failed to resolve plugin dependencies: {}", e);
                // Fallback to simple ordering
                plugin_configs.iter().map(|c| c.name.clone()).collect()
            }
        };

        // Load built-in plugins first (they have no dependencies)
        self.load_builtin_plugins(context).await?;

        // Load plugins in dependency order with error recovery
        let mut successful_loads = 0;
        let mut failed_loads = Vec::new();

        for plugin_name in &load_order {
            if let Some(plugin_config) = plugin_configs.iter().find(|c| &c.name == plugin_name) {
                match self.load_single_plugin(plugin_config, context).await {
                    Ok(()) => {
                        successful_loads += 1;
                        tracing::info!("Successfully loaded plugin: {}", plugin_name);
                    }
                    Err(e) => {
                        tracing::error!("Failed to load plugin {}: {}", plugin_name, e);
                        failed_loads.push((plugin_name.clone(), e));

                        // Check if this is a critical plugin
                        if self.is_critical_plugin(plugin_name) {
                            return Err(RuneError::Plugin(format!(
                                "Critical plugin {} failed to load: {}",
                                plugin_name,
                                failed_loads.last().unwrap().1
                            )));
                        }
                    }
                }
            }
        }

        // Discover plugins from directories
        if let Some(plugins_dir) = self.get_plugins_directory() {
            if let Err(e) = self
                .discover_plugins_from_directory(&plugins_dir, context)
                .await
            {
                tracing::warn!("Plugin directory discovery failed: {}", e);
            }
        }

        // Report loading results
        tracing::info!(
            "Plugin loading completed: {} successful, {} failed",
            successful_loads,
            failed_loads.len()
        );

        if !failed_loads.is_empty() {
            tracing::warn!("Failed plugin loads:");
            for (name, error) in &failed_loads {
                tracing::warn!("  {}: {}", name, error);
            }
        }

        Ok(())
    }

    /// Load built-in plugins that are always available
    /// This method is now a placeholder - plugins should be registered externally
    async fn load_builtin_plugins(&mut self, _context: &PluginContext) -> Result<()> {
        tracing::info!("Built-in plugin loading is handled externally");
        // Built-in plugins are now registered by the CLI or other external code
        // to avoid circular dependencies
        Ok(())
    }

    /// Register a plugin with the core engine
    pub async fn register_plugin(
        &mut self,
        plugin: Box<dyn Plugin>,
        context: &PluginContext,
    ) -> Result<()> {
        self.plugin_registry.register_plugin(plugin, context).await
    }

    /// Get the plugin context for external plugin registration
    pub fn create_plugin_context(&self) -> PluginContext {
        PluginContext::new(
            self.event_bus.clone(),
            self.config.clone(),
            self.state_manager.clone(),
        )
    }

    /// Load plugins specified in configuration
    #[allow(dead_code)]
    async fn load_configured_plugins(&mut self, _context: &PluginContext) -> Result<()> {
        let enabled_plugins = self.config.get_enabled_plugins();

        if enabled_plugins.is_empty() {
            tracing::debug!("No additional plugins configured");
            return Ok(());
        }

        tracing::info!("Loading {} configured plugins", enabled_plugins.len());

        for plugin_config in enabled_plugins {
            tracing::debug!("Processing configured plugin: {}", plugin_config.name);

            // Skip built-in plugins as they're already loaded
            if self.is_builtin_plugin(&plugin_config.name) {
                tracing::debug!("Skipping built-in plugin: {}", plugin_config.name);
                continue;
            }

            // In a real implementation, this would dynamically load plugin libraries
            // For now, we'll just log that we would load them
            tracing::info!(
                "Would load configured plugin: {} (version: {:?})",
                plugin_config.name,
                plugin_config.version
            );
        }

        Ok(())
    }

    /// Check if a plugin name refers to a built-in plugin
    fn is_builtin_plugin(&self, name: &str) -> bool {
        matches!(name, "file-watcher" | "renderer" | "server" | "theme")
    }

    /// Check if a plugin is critical for system operation
    fn is_critical_plugin(&self, name: &str) -> bool {
        // Define critical plugins that must load for the system to function
        matches!(name, "renderer" | "server")
    }

    /// Load a single plugin with error handling and recovery
    async fn load_single_plugin(
        &mut self,
        plugin_config: &crate::config::PluginConfig,
        _context: &PluginContext,
    ) -> Result<()> {
        tracing::debug!("Loading plugin: {}", plugin_config.name);

        // Validate plugin dependencies before loading
        for dep in &plugin_config.dependencies {
            if !self.plugin_registry.is_plugin_active(dep) {
                return Err(RuneError::Plugin(format!(
                    "Plugin {} depends on {}, which is not active",
                    plugin_config.name, dep
                )));
            }
        }

        // Skip built-in plugins as they're loaded separately
        if self.is_builtin_plugin(&plugin_config.name) {
            tracing::debug!("Skipping built-in plugin: {}", plugin_config.name);
            return Ok(());
        }

        // In a real implementation, this would dynamically load plugin libraries
        // For now, we'll just validate the configuration and mark as loaded
        tracing::info!(
            "Would load configured plugin: {} (version: {:?})",
            plugin_config.name,
            plugin_config.version
        );

        Ok(())
    }

    /// Validate system configuration before initialization
    async fn validate_system_pre_init(&self) -> Result<SystemValidationResult> {
        tracing::debug!("Validating system configuration before initialization");

        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        // Validate core configuration
        if let Err(e) = self.config.validate() {
            errors.push(format!("Configuration validation failed: {}", e));
        }

        // Check for circular dependencies in plugin configuration
        let mut dependency_graph = plugin::DependencyGraph::new();
        for plugin_config in &self.config.plugins {
            if plugin_config.enabled {
                for dep in &plugin_config.dependencies {
                    dependency_graph.add_dependency(plugin_config.name.clone(), dep.clone());
                }
            }
        }

        if dependency_graph.has_circular_dependencies() {
            errors.push("Circular dependencies detected in plugin configuration".to_string());
        }

        // Validate server configuration
        if self.config.server.port == 0 {
            errors.push("Server port cannot be 0".to_string());
        }

        if self.config.server.hostname.is_empty() {
            errors.push("Server hostname cannot be empty".to_string());
        }

        // Check for required plugins
        let required_plugins = ["renderer", "server"];
        for required in &required_plugins {
            if !self
                .config
                .plugins
                .iter()
                .any(|p| p.name == *required && p.enabled)
            {
                warnings.push(format!("Required plugin '{}' is not enabled", required));
            }
        }

        Ok(SystemValidationResult {
            is_valid: errors.is_empty(),
            errors,
            warnings,
            system_health: plugin::SystemHealthStatus::Healthy,
            plugin_count: 0,
            active_plugin_count: 0,
        })
    }

    /// Discover plugins from a directory
    async fn discover_plugins_from_directory(
        &mut self,
        dir: &PathBuf,
        _context: &PluginContext,
    ) -> Result<()> {
        if !dir.exists() {
            tracing::debug!("Plugin directory does not exist: {}", dir.display());
            return Ok(());
        }

        tracing::info!("Discovering plugins from directory: {}", dir.display());

        let mut entries = tokio::fs::read_dir(dir)
            .await
            .map_err(|e| RuneError::Plugin(format!("Failed to read plugin directory: {}", e)))?;

        let mut discovered_count = 0;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| RuneError::Plugin(format!("Failed to read directory entry: {}", e)))?
        {
            let path = entry.path();
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();

            // Skip hidden files
            if name.starts_with('.') {
                continue;
            }

            if path.is_dir() {
                // Check for plugin manifest
                let manifest_path = path.join("plugin.json");
                if manifest_path.exists() {
                    tracing::debug!("Found plugin directory with manifest: {}", path.display());
                    discovered_count += 1;
                }
            } else if let Some(extension) = path.extension() {
                match extension.to_string_lossy().as_ref() {
                    "so" | "dll" | "dylib" => {
                        tracing::debug!("Found native plugin: {}", path.display());
                        discovered_count += 1;
                    }
                    "json" if name.contains("plugin") => {
                        tracing::debug!("Found plugin configuration: {}", path.display());
                        discovered_count += 1;
                    }
                    _ => {}
                }
            }
        }

        tracing::info!(
            "Discovered {} potential plugins in {}",
            discovered_count,
            dir.display()
        );
        Ok(())
    }

    /// Get the plugins directory from configuration or default locations
    fn get_plugins_directory(&self) -> Option<PathBuf> {
        // Check global settings first
        if let Some(plugins_dir) = self
            .config
            .get_global_setting::<String>("plugins_directory")
        {
            return Some(PathBuf::from(plugins_dir));
        }

        // Check for default plugin directories
        let mut default_dirs = vec![PathBuf::from("plugins"), PathBuf::from("./plugins")];

        if let Some(config_dir) = dirs::config_dir() {
            default_dirs.push(config_dir.join("rune").join("plugins"));
        }

        default_dirs.into_iter().find(|dir| dir.exists())
    }

    /// Start the core engine and run until shutdown
    pub async fn run(&mut self) -> Result<()> {
        if !self.is_initialized {
            self.initialize().await?;
        }

        tracing::info!("Starting Rune Core Engine");

        // Set up shutdown signal handling
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();
        self.shutdown_signal = Some(shutdown_tx);

        // Spawn signal handler for graceful shutdown
        let shutdown_signal = async {
            let ctrl_c = async {
                signal::ctrl_c()
                    .await
                    .expect("Failed to install Ctrl+C handler");
            };

            #[cfg(unix)]
            let terminate = async {
                signal::unix::signal(signal::unix::SignalKind::terminate())
                    .expect("Failed to install signal handler")
                    .recv()
                    .await;
            };

            #[cfg(not(unix))]
            let terminate = std::future::pending::<()>();

            tokio::select! {
                _ = ctrl_c => {
                    tracing::info!("Received Ctrl+C signal");
                },
                _ = terminate => {
                    tracing::info!("Received terminate signal");
                },
            }
        };

        // Run until shutdown signal
        tokio::select! {
            _ = shutdown_signal => {
                tracing::info!("Shutdown signal received");
            }
            _ = &mut shutdown_rx => {
                tracing::info!("Shutdown requested programmatically");
            }
        }

        // Perform graceful shutdown
        self.shutdown().await?;

        tracing::info!("Rune Core Engine stopped");
        Ok(())
    }

    /// Request shutdown of the core engine
    pub fn request_shutdown(&mut self) {
        if let Some(sender) = self.shutdown_signal.take() {
            if sender.send(()).is_err() {
                tracing::warn!("Failed to send shutdown signal (receiver may have been dropped)");
            }
        }
    }

    /// Shutdown the core engine gracefully with enhanced error handling
    pub async fn shutdown(&mut self) -> Result<()> {
        tracing::info!("Initiating graceful shutdown of Rune Core Engine");

        if !self.is_initialized {
            tracing::warn!("Core engine is not initialized, skipping shutdown");
            return Ok(());
        }

        // Publish system shutdown event
        if let Err(e) = self
            .event_bus
            .publish_system_event(event::SystemEvent::system_shutdown_initiated())
            .await
        {
            tracing::warn!("Failed to publish shutdown event: {}", e);
        }

        // Phase 1: Prepare for shutdown
        tracing::info!("Phase 1: Preparing for shutdown");
        self.prepare_for_shutdown().await?;

        // Phase 2: Shutdown plugins with enhanced error handling
        tracing::info!("Phase 2: Shutting down plugins");
        let shutdown_result = self.shutdown_plugins_gracefully().await;

        // Phase 3: Cleanup system resources
        tracing::info!("Phase 3: Cleaning up system resources");
        self.cleanup_system_resources().await?;

        // Phase 4: Final validation and reporting
        tracing::info!("Phase 4: Final validation");
        let shutdown_report = self.generate_shutdown_report(&shutdown_result).await;

        self.is_initialized = false;

        // Log shutdown summary
        if shutdown_report.successful_shutdowns == shutdown_report.total_plugins {
            tracing::info!(
                "Core Engine shutdown completed successfully ({} plugins)",
                shutdown_report.total_plugins
            );
        } else {
            tracing::warn!(
                "Core Engine shutdown completed with issues: {}/{} plugins shutdown successfully",
                shutdown_report.successful_shutdowns,
                shutdown_report.total_plugins
            );

            if !shutdown_report.failed_shutdowns.is_empty() {
                tracing::warn!("Failed plugin shutdowns:");
                for (plugin, error) in &shutdown_report.failed_shutdowns {
                    tracing::warn!("  {}: {}", plugin, error);
                }
            }
        }

        Ok(())
    }

    /// Prepare the system for shutdown
    async fn prepare_for_shutdown(&mut self) -> Result<()> {
        tracing::debug!("Preparing system for shutdown");

        // Stop accepting new connections or requests
        // This would be implemented by notifying server plugins
        if let Err(e) = self
            .event_bus
            .publish_system_event(event::SystemEvent::system_shutdown_preparing())
            .await
        {
            tracing::warn!("Failed to publish shutdown preparation event: {}", e);
        }

        // Give plugins time to finish current operations
        tokio::time::sleep(Duration::from_millis(500)).await;

        tracing::debug!("System prepared for shutdown");
        Ok(())
    }

    /// Shutdown plugins gracefully with proper error handling
    async fn shutdown_plugins_gracefully(&mut self) -> PluginShutdownResult {
        let shutdown_timeout = Duration::from_secs(30);
        let mut result = PluginShutdownResult::new();

        // Get list of plugins before shutdown
        let plugin_names: Vec<String> = self
            .plugin_registry
            .list_plugins()
            .iter()
            .map(|p| p.name.clone())
            .collect();

        result.total_plugins = plugin_names.len();

        tracing::info!(
            "Shutting down {} plugins with {}s timeout",
            result.total_plugins,
            shutdown_timeout.as_secs()
        );

        // Attempt graceful shutdown with timeout
        match tokio::time::timeout(shutdown_timeout, self.plugin_registry.shutdown()).await {
            Ok(Ok(())) => {
                tracing::info!("All plugins shutdown successfully");
                result.successful_shutdowns = result.total_plugins;
            }
            Ok(Err(e)) => {
                tracing::error!("Plugin registry shutdown failed: {}", e);
                result.registry_error = Some(e.to_string());

                // Try to get individual plugin statuses
                self.collect_individual_plugin_statuses(&mut result).await;
            }
            Err(_) => {
                tracing::error!(
                    "Plugin registry shutdown timed out after {:?}",
                    shutdown_timeout
                );
                result.timed_out = true;

                // Force shutdown remaining plugins
                self.force_shutdown_remaining_plugins(&mut result).await;
            }
        }

        result
    }

    /// Collect individual plugin shutdown statuses
    async fn collect_individual_plugin_statuses(&self, result: &mut PluginShutdownResult) {
        let plugins = self.plugin_registry.list_plugins();

        for plugin in plugins {
            match plugin.status {
                plugin::PluginStatus::Stopped => {
                    result.successful_shutdowns += 1;
                }
                plugin::PluginStatus::Error(ref error) => {
                    result
                        .failed_shutdowns
                        .push((plugin.name.clone(), error.clone()));
                }
                _ => {
                    result.failed_shutdowns.push((
                        plugin.name.clone(),
                        format!("Plugin in unexpected state: {:?}", plugin.status),
                    ));
                }
            }
        }
    }

    /// Force shutdown remaining plugins that didn't shutdown gracefully
    async fn force_shutdown_remaining_plugins(&mut self, result: &mut PluginShutdownResult) {
        tracing::warn!("Force shutting down remaining plugins");

        // In a real implementation, this would forcefully terminate plugin processes
        // For now, we'll just mark them as force-stopped
        let plugins = self.plugin_registry.list_plugins();

        for plugin in plugins {
            if !matches!(plugin.status, plugin::PluginStatus::Stopped) {
                tracing::warn!("Force stopping plugin: {}", plugin.name);
                result.force_stopped.push(plugin.name.clone());
            }
        }
    }

    /// Cleanup system resources after plugin shutdown
    async fn cleanup_system_resources(&mut self) -> Result<()> {
        tracing::debug!("Cleaning up system resources");

        // Clear any remaining event bus subscriptions
        // This would be implemented in the event bus

        // Clear state manager
        self.state_manager.clear_state().await;

        // Publish final shutdown event
        if let Err(e) = self
            .event_bus
            .publish_system_event(event::SystemEvent::system_shutdown_complete())
            .await
        {
            tracing::warn!("Failed to publish final shutdown event: {}", e);
        }

        tracing::debug!("System resources cleaned up");
        Ok(())
    }

    /// Generate a comprehensive shutdown report
    async fn generate_shutdown_report(
        &self,
        shutdown_result: &PluginShutdownResult,
    ) -> ShutdownReport {
        ShutdownReport {
            total_plugins: shutdown_result.total_plugins,
            successful_shutdowns: shutdown_result.successful_shutdowns,
            failed_shutdowns: shutdown_result.failed_shutdowns.clone(),
            force_stopped: shutdown_result.force_stopped.clone(),
            timed_out: shutdown_result.timed_out,
            registry_error: shutdown_result.registry_error.clone(),
        }
    }

    /// Get a reference to the event bus
    pub fn event_bus(&self) -> Arc<dyn EventBus> {
        self.event_bus.clone()
    }

    /// Get a reference to the plugin registry
    pub fn plugin_registry(&self) -> &PluginRegistry {
        &self.plugin_registry
    }

    /// Get a mutable reference to the plugin registry
    pub fn plugin_registry_mut(&mut self) -> &mut PluginRegistry {
        &mut self.plugin_registry
    }

    /// Get a reference to the state manager
    pub fn state_manager(&self) -> Arc<StateManager> {
        self.state_manager.clone()
    }

    /// Get a reference to the configuration
    pub fn config(&self) -> Arc<Config> {
        self.config.clone()
    }

    /// Check if the engine is initialized
    pub fn is_initialized(&self) -> bool {
        self.is_initialized
    }

    /// Get system health status
    pub fn get_system_health(&self) -> plugin::SystemHealthStatus {
        self.plugin_registry.get_system_health()
    }

    /// Get all loaded plugins information
    pub fn get_loaded_plugins(&self) -> Vec<&plugin::PluginInfo> {
        self.plugin_registry.list_plugins()
    }

    /// Reload configuration and restart affected plugins
    pub async fn reload_configuration(&mut self, new_config: Config) -> Result<()> {
        tracing::info!("Reloading configuration");

        // Update configuration
        self.config = Arc::new(new_config);

        // Create new context with updated config
        let context = PluginContext::new(
            self.event_bus.clone(),
            self.config.clone(),
            self.state_manager.clone(),
        );

        // Reload plugin configurations
        context.reload_configurations().await?;

        tracing::info!("Configuration reloaded successfully");
        Ok(())
    }

    /// Add a file to watch using the FileWatcher plugin
    pub async fn watch_file(&mut self, file_path: PathBuf) -> Result<WatcherId> {
        tracing::info!("Adding file to watch: {}", file_path.display());

        // Update application state
        self.state_manager
            .set_current_file(Some(file_path.clone()))
            .await;

        // Publish the file change event for immediate processing
        // The FileWatcher plugin will automatically start monitoring the current directory
        let event = SystemEvent::file_changed(file_path.clone(), event::ChangeType::Modified);
        self.event_bus.publish_system_event(event).await?;

        tracing::info!("File change event published for: {}", file_path.display());
        Ok(WatcherId::new())
    }

    /// Get current watched file
    pub async fn get_current_file(&self) -> Option<PathBuf> {
        let state = self.state_manager.get_state().await;
        state.current_file.clone()
    }

    /// Get server address if server plugin is running
    pub async fn get_server_address(&self) -> Option<String> {
        // Check if server plugin is active
        if let Some(server_info) = self.plugin_registry.get_plugin_info("server") {
            if matches!(server_info.status, plugin::PluginStatus::Active) {
                return Some(format!(
                    "{}:{}",
                    self.config.server.hostname, self.config.server.port
                ));
            }
        }
        None
    }

    /// Validate system configuration and plugin dependencies
    pub async fn validate_system(&self) -> Result<SystemValidationResult> {
        tracing::info!("Validating system configuration and plugin dependencies");

        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        // Validate configuration
        if let Err(e) = self.config.validate() {
            errors.push(format!("Configuration validation failed: {}", e));
        }

        // Validate plugin dependencies
        let plugins = self.plugin_registry.list_plugins();
        let plugin_count = plugins.len();
        let active_plugin_count = plugins
            .iter()
            .filter(|p| matches!(p.status, plugin::PluginStatus::Active))
            .count();

        for plugin in &plugins {
            for dep in &plugin.dependencies {
                if !self.plugin_registry.is_plugin_active(dep) {
                    errors.push(format!(
                        "Plugin '{}' depends on '{}' which is not active",
                        plugin.name, dep
                    ));
                }
            }

            // Check plugin health
            if matches!(plugin.health_status, plugin::PluginHealthStatus::Unhealthy) {
                warnings.push(format!("Plugin '{}' is unhealthy", plugin.name));
            }
        }

        // Check system health
        let system_health = self.get_system_health();
        if matches!(system_health, plugin::SystemHealthStatus::Unhealthy) {
            errors.push("System health is unhealthy".to_string());
        } else if matches!(system_health, plugin::SystemHealthStatus::Degraded) {
            warnings.push("System health is degraded".to_string());
        }

        Ok(SystemValidationResult {
            is_valid: errors.is_empty(),
            errors,
            warnings,
            system_health,
            plugin_count,
            active_plugin_count,
        })
    }
}

/// System validation result
#[derive(Debug, Clone)]
pub struct SystemValidationResult {
    pub is_valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub system_health: plugin::SystemHealthStatus,
    pub plugin_count: usize,
    pub active_plugin_count: usize,
}

/// Plugin shutdown result tracking
#[derive(Debug, Clone)]
struct PluginShutdownResult {
    pub total_plugins: usize,
    pub successful_shutdowns: usize,
    pub failed_shutdowns: Vec<(String, String)>, // (plugin_name, error)
    pub force_stopped: Vec<String>,
    pub timed_out: bool,
    pub registry_error: Option<String>,
}

impl PluginShutdownResult {
    fn new() -> Self {
        Self {
            total_plugins: 0,
            successful_shutdowns: 0,
            failed_shutdowns: Vec::new(),
            force_stopped: Vec::new(),
            timed_out: false,
            registry_error: None,
        }
    }
}

/// Comprehensive shutdown report
#[derive(Debug, Clone)]
pub struct ShutdownReport {
    pub total_plugins: usize,
    pub successful_shutdowns: usize,
    pub failed_shutdowns: Vec<(String, String)>,
    pub force_stopped: Vec<String>,
    pub timed_out: bool,
    pub registry_error: Option<String>,
}
