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
pub use config::{Config, PluginConfig, SystemConfig};
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

use std::sync::Arc;

/// Core engine that orchestrates all plugins and system components
pub struct CoreEngine {
    event_bus: Arc<dyn EventBus>,
    plugin_registry: PluginRegistry,
    state_manager: Arc<StateManager>,
    config: Arc<Config>,
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
        })
    }

    /// Initialize the core engine and load plugins
    pub async fn initialize(&mut self) -> Result<()> {
        tracing::info!("Initializing Rune Core Engine");

        // Initialize plugin registry with core services
        let context = PluginContext::new(
            self.event_bus.clone(),
            self.config.clone(),
            self.state_manager.clone(),
        );

        self.plugin_registry.initialize(context).await?;

        tracing::info!("Core Engine initialized successfully");
        Ok(())
    }

    /// Shutdown the core engine gracefully
    pub async fn shutdown(&mut self) -> Result<()> {
        tracing::info!("Shutting down Rune Core Engine");

        self.plugin_registry.shutdown().await?;

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

    /// Get a reference to the state manager
    pub fn state_manager(&self) -> Arc<StateManager> {
        self.state_manager.clone()
    }

    /// Get a reference to the configuration
    pub fn config(&self) -> Arc<Config> {
        self.config.clone()
    }
}
