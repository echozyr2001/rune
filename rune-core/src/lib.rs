//! Rune Core - The foundational engine for the Rune markdown live editor
//!
//! This crate provides the core interfaces, event system, and plugin architecture
//! that powers the modular Rune markdown editor.

pub mod config;
pub mod error;
pub mod event;
pub mod plugin;
pub mod renderer;
pub mod state;

#[cfg(test)]
mod event_test;

#[cfg(test)]
mod plugin_test;

#[cfg(test)]
mod plugin_context_test;

// Re-export commonly used types
pub use config::{
    Config, ConfigLoadContext, ConfigMetadata, PluginConfig, RuntimeConfigManager, ServerConfig,
    SystemConfig, ValidationResult,
};
pub use error::{Result, RuneError};
pub use event::{
    Event, EventBus, EventFilter, EventHandler, ExtendedEventBus, InMemoryEventBus, SubscriptionId,
    SystemEvent, SystemEventHandler,
};
pub use plugin::{Plugin, PluginContext, PluginInfo, PluginRegistry, PluginStatus};
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

    /// Initialize the core engine and load plugins
    pub async fn initialize(&mut self) -> Result<()> {
        if self.is_initialized {
            tracing::warn!("Core engine is already initialized");
            return Ok(());
        }

        tracing::info!("Initializing Rune Core Engine");

        // Initialize plugin registry with core services
        let context = PluginContext::new(
            self.event_bus.clone(),
            self.config.clone(),
            self.state_manager.clone(),
        );

        self.plugin_registry.initialize(context.clone()).await?;

        // Discover and load plugins automatically
        self.discover_and_load_plugins(&context).await?;

        self.is_initialized = true;
        tracing::info!("Core Engine initialized successfully");
        Ok(())
    }

    /// Discover and automatically load plugins based on configuration and directories
    async fn discover_and_load_plugins(&mut self, context: &PluginContext) -> Result<()> {
        tracing::info!("Starting plugin discovery and loading");

        // Load built-in plugins first
        self.load_builtin_plugins(context).await?;

        // Load plugins from configuration
        self.load_configured_plugins(context).await?;

        // Discover plugins from directories
        if let Some(plugins_dir) = self.get_plugins_directory() {
            self.discover_plugins_from_directory(&plugins_dir, context)
                .await?;
        }

        tracing::info!("Plugin discovery and loading completed");
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

        for dir in default_dirs {
            if dir.exists() {
                return Some(dir);
            }
        }

        None
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
            if let Err(_) = sender.send(()) {
                tracing::warn!("Failed to send shutdown signal (receiver may have been dropped)");
            }
        }
    }

    /// Shutdown the core engine gracefully
    pub async fn shutdown(&mut self) -> Result<()> {
        tracing::info!("Shutting down Rune Core Engine");

        // Shutdown plugins with timeout
        let shutdown_timeout = Duration::from_secs(30);

        match tokio::time::timeout(shutdown_timeout, self.plugin_registry.shutdown()).await {
            Ok(result) => {
                if let Err(e) = result {
                    tracing::error!("Plugin registry shutdown failed: {}", e);
                }
            }
            Err(_) => {
                tracing::error!(
                    "Plugin registry shutdown timed out after {:?}",
                    shutdown_timeout
                );
            }
        }

        self.is_initialized = false;
        tracing::info!("Core Engine shutdown complete");
        Ok(())
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

    /// Add a file to watch (convenience method)
    pub async fn watch_file(&mut self, file_path: PathBuf) -> Result<()> {
        tracing::info!("Adding file to watch: {}", file_path.display());

        // Update application state
        self.state_manager
            .set_current_file(Some(file_path.clone()))
            .await;

        // Publish file change event to notify plugins
        let event = SystemEvent::file_changed(file_path, event::ChangeType::Modified);
        self.event_bus.publish_system_event(event).await?;

        Ok(())
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
