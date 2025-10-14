#!/usr/bin/env rust-script
//! Example demonstrating the new file watcher architecture
//! 
//! This example shows how the FileWatcher trait is now defined in rune-core,
//! breaking the circular dependency and enabling high-performance file watching.

use std::path::Path;
use std::sync::Arc;
use rune_core::{
    Config, CoreEngine, FileWatcher, FileFilter, DefaultFileFilter, FileWatcherConfig, WatcherId,
    Plugin, PluginContext, Result,
};
use rune_file_watcher::FileWatcherPlugin;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::init();

    println!("🚀 Rune File Watcher Architecture Demo");
    println!("=====================================");

    // 1. Create core engine with configuration
    let config = Config::default();
    let mut engine = CoreEngine::new(config)?;

    // 2. Create and register the file watcher plugin
    let mut file_watcher_plugin = FileWatcherPlugin::new();
    let context = engine.create_plugin_context();
    
    // Initialize the plugin
    file_watcher_plugin.initialize(&context).await?;
    
    println!("✅ File watcher plugin initialized");

    // 3. Create a custom file filter for markdown files
    let markdown_config = FileWatcherConfig {
        debounce_ms: 200,
        watch_extensions: vec!["md".to_string(), "markdown".to_string()],
        ignore_patterns: vec![
            "*.tmp".to_string(),
            "*.swp".to_string(),
            "*~".to_string(),
            ".git/**".to_string(),
            "node_modules/**".to_string(),
            "target/**".to_string(),
        ],
        recursive: true,
        max_depth: Some(3),
    };
    
    let markdown_filter = Arc::new(DefaultFileFilter::new(markdown_config));
    
    println!("📁 Created markdown file filter");

    // 4. Start watching the current directory for markdown files
    let current_dir = std::env::current_dir().unwrap();
    let watch_id = file_watcher_plugin.watch(&current_dir, markdown_filter).await?;
    
    println!("👀 Started watching directory: {}", current_dir.display());
    println!("   Watch ID: {:?}", watch_id);

    // 5. Demonstrate the interface
    let watched_paths = file_watcher_plugin.get_watched_paths().await;
    println!("📋 Currently watched paths:");
    for (id, path) in &watched_paths {
        println!("   {:?}: {}", id, path.display());
    }

    // 6. Check if specific files are being watched
    let test_file = current_dir.join("test-readme.md");
    let is_watching = file_watcher_plugin.is_watching(&test_file).await;
    println!("🔍 Is watching test-readme.md: {}", is_watching);

    // 7. Get statistics
    let stats = file_watcher_plugin.get_watch_statistics().await;
    println!("📊 Watch statistics:");
    println!("   Watched paths: {}", stats.watched_path_count);
    println!("   Pending events: {}", stats.pending_events_count);
    println!("   Total events processed: {}", stats.total_events_processed);

    println!("\n🎯 Architecture Benefits:");
    println!("   ✅ No circular dependencies");
    println!("   ✅ FileWatcher trait in rune-core");
    println!("   ✅ Plugin implements the interface");
    println!("   ✅ CoreEngine can use FileWatcher directly");
    println!("   ✅ High-performance event-driven watching");
    println!("   ✅ Modular and replaceable implementation");

    // 8. Simulate some file operations
    println!("\n🔄 Simulating file operations...");
    
    // In a real scenario, file changes would trigger events automatically
    // Here we just demonstrate the interface
    
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // 9. Clean shutdown
    println!("\n🛑 Shutting down...");
    file_watcher_plugin.unwatch(watch_id).await?;
    file_watcher_plugin.shutdown().await?;
    
    println!("✅ File watcher plugin shut down successfully");
    println!("\n🎉 Demo completed!");

    Ok(())
}

/// Example of creating a custom file filter
struct CustomMarkdownFilter {
    debounce_ms: u64,
}

impl CustomMarkdownFilter {
    fn new(debounce_ms: u64) -> Self {
        Self { debounce_ms }
    }
}

#[async_trait::async_trait]
impl FileFilter for CustomMarkdownFilter {
    fn should_watch(&self, path: &Path) -> bool {
        // Only watch markdown files
        if let Some(extension) = path.extension() {
            matches!(extension.to_str(), Some("md") | Some("markdown"))
        } else {
            false
        }
    }

    fn debounce_duration(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.debounce_ms)
    }

    fn filter_name(&self) -> &str {
        "CustomMarkdownFilter"
    }
}

/// Example of how CoreEngine would use FileWatcher in practice
async fn example_core_engine_usage() -> Result<()> {
    let config = Config::default();
    let mut engine = CoreEngine::new(config)?;
    
    // Initialize the engine (this would load plugins)
    engine.initialize().await?;
    
    // Watch a specific file - this now uses the FileWatcher plugin internally
    let file_path = std::path::PathBuf::from("README.md");
    let _watch_id = engine.watch_file(file_path).await?;
    
    // The engine can now receive high-performance file change events
    // without the circular dependency problem
    
    Ok(())
}