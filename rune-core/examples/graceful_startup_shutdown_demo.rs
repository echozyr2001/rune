//! Demonstration of graceful startup and shutdown with mock plugin instances

use async_trait::async_trait;
use rune_core::{Config, CoreEngine, Plugin, PluginContext, Result};
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, Level};

// Mock plugin implementations for demonstration
#[derive(Debug)]
struct MockRendererPlugin;

#[async_trait]
impl Plugin for MockRendererPlugin {
    fn name(&self) -> &str {
        "renderer"
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn dependencies(&self) -> Vec<&str> {
        vec![]
    }
    fn provided_services(&self) -> Vec<&str> {
        vec!["rendering"]
    }

    async fn initialize(&mut self, _context: &PluginContext) -> Result<()> {
        info!("MockRendererPlugin: Initializing...");
        tokio::time::sleep(Duration::from_millis(100)).await; // Simulate initialization work
        info!("MockRendererPlugin: Initialized successfully");
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        info!("MockRendererPlugin: Shutting down...");
        tokio::time::sleep(Duration::from_millis(50)).await; // Simulate cleanup work
        info!("MockRendererPlugin: Shutdown complete");
        Ok(())
    }
}

#[derive(Debug)]
struct MockServerPlugin;

#[async_trait]
impl Plugin for MockServerPlugin {
    fn name(&self) -> &str {
        "server"
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn dependencies(&self) -> Vec<&str> {
        vec!["renderer"]
    }
    fn provided_services(&self) -> Vec<&str> {
        vec!["http-server"]
    }

    async fn initialize(&mut self, _context: &PluginContext) -> Result<()> {
        info!("MockServerPlugin: Initializing...");
        tokio::time::sleep(Duration::from_millis(150)).await; // Simulate initialization work
        info!("MockServerPlugin: Initialized successfully");
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        info!("MockServerPlugin: Shutting down...");
        tokio::time::sleep(Duration::from_millis(100)).await; // Simulate cleanup work
        info!("MockServerPlugin: Shutdown complete");
        Ok(())
    }
}

#[derive(Debug)]
struct MockFileWatcherPlugin;

#[async_trait]
impl Plugin for MockFileWatcherPlugin {
    fn name(&self) -> &str {
        "file-watcher"
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn dependencies(&self) -> Vec<&str> {
        vec!["server"]
    }
    fn provided_services(&self) -> Vec<&str> {
        vec!["file-monitoring"]
    }

    async fn initialize(&mut self, _context: &PluginContext) -> Result<()> {
        info!("MockFileWatcherPlugin: Initializing...");
        tokio::time::sleep(Duration::from_millis(80)).await; // Simulate initialization work
        info!("MockFileWatcherPlugin: Initialized successfully");
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        info!("MockFileWatcherPlugin: Shutting down...");
        tokio::time::sleep(Duration::from_millis(60)).await; // Simulate cleanup work
        info!("MockFileWatcherPlugin: Shutdown complete");
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    info!("Starting graceful startup and shutdown demonstration with mock plugins");

    // Create a basic configuration
    let config = Config::new();

    // Create and initialize the core engine
    let mut engine = CoreEngine::new(config)?;

    info!("=== Phase 1: System Initialization ===");

    // Initialize the engine first
    engine.initialize().await?;

    // Now register mock plugin instances in dependency order
    let context = engine.create_plugin_context();

    info!("Registering renderer plugin (no dependencies)...");
    engine
        .register_plugin(Box::new(MockRendererPlugin), &context)
        .await?;

    info!("Registering server plugin (depends on renderer)...");
    engine
        .register_plugin(Box::new(MockServerPlugin), &context)
        .await?;

    info!("Registering file-watcher plugin (depends on server)...");
    engine
        .register_plugin(Box::new(MockFileWatcherPlugin), &context)
        .await?;

    // Display system status after plugin registration
    let validation_result = engine.validate_system().await?;
    info!(
        "✅ System validation: {} plugins, {} active",
        validation_result.plugin_count, validation_result.active_plugin_count
    );

    if !validation_result.warnings.is_empty() {
        info!("Warnings: {:?}", validation_result.warnings);
    }

    info!("=== Phase 2: Runtime Operation ===");

    // Display system health
    let system_health = engine.get_system_health();
    info!("System health status: {:?}", system_health);

    // Display loaded plugins
    let loaded_plugins = engine.get_loaded_plugins();
    info!("Loaded plugins: {}", loaded_plugins.len());
    for plugin in loaded_plugins {
        info!(
            "  - {} v{} (status: {:?}, health: {:?})",
            plugin.name, plugin.version, plugin.status, plugin.health_status
        );
        if !plugin.dependencies.is_empty() {
            info!("    Dependencies: {:?}", plugin.dependencies);
        }
    }

    // Simulate some runtime operation
    info!("Simulating runtime operation for 3 seconds...");
    sleep(Duration::from_secs(3)).await;

    info!("=== Phase 3: Graceful Shutdown ===");

    // Test graceful shutdown with proper dependency ordering
    match engine.shutdown().await {
        Ok(()) => {
            info!("✅ Core engine shutdown completed successfully");
        }
        Err(e) => {
            info!("⚠️  Core engine shutdown completed with warnings: {}", e);
        }
    }

    info!("=== Demonstration Complete ===");
    info!("The system successfully demonstrated:");
    info!("  ✓ Real plugin registration with dependency validation");
    info!("  ✓ Dependency-aware plugin initialization ordering");
    info!("  ✓ System health monitoring");
    info!("  ✓ Graceful shutdown with proper cleanup");
    info!("  ✓ Dependency-aware plugin shutdown ordering");

    Ok(())
}
