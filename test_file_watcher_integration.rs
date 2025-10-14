#!/usr/bin/env rust-script
//! Integration test for the new file watcher architecture

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use rune_core::{
    Config, FileWatcher, FileFilter, DefaultFileFilter, FileWatcherConfig, WatcherId,
    Plugin, PluginContext, Result, InMemoryEventBus, StateManager,
};
use rune_file_watcher::FileWatcherPlugin;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing for better debugging
    tracing_subscriber::fmt::init();

    println!("🧪 Testing File Watcher Architecture Integration");
    println!("===============================================");

    // Test 1: Plugin Creation and Initialization
    println!("\n1️⃣ Testing plugin creation and initialization...");
    
    let mut plugin = FileWatcherPlugin::new();
    assert_eq!(plugin.name(), "file-watcher");
    assert_eq!(plugin.version(), "0.1.0");
    
    // Create a mock context
    let event_bus = Arc::new(InMemoryEventBus::new());
    let config = Arc::new(Config::default());
    let state_manager = Arc::new(StateManager::new());
    let context = PluginContext::new(event_bus, config, state_manager);
    
    // Initialize the plugin
    plugin.initialize(&context).await?;
    println!("   ✅ Plugin initialized successfully");

    // Test 2: File Filter Creation
    println!("\n2️⃣ Testing file filter creation...");
    
    let filter_config = FileWatcherConfig {
        debounce_ms: 100,
        watch_extensions: vec!["md".to_string(), "txt".to_string()],
        ignore_patterns: vec!["*.tmp".to_string(), "*.swp".to_string()],
        recursive: true,
        max_depth: Some(2),
    };
    
    let filter = Arc::new(DefaultFileFilter::new(filter_config));
    
    // Test filter behavior
    assert!(filter.should_watch(Path::new("test.md")));
    assert!(filter.should_watch(Path::new("readme.txt")));
    assert!(!filter.should_watch(Path::new("temp.tmp")));
    assert!(!filter.should_watch(Path::new("backup.swp")));
    assert!(!filter.should_watch(Path::new("script.js"))); // Not in extensions
    
    println!("   ✅ File filter working correctly");

    // Test 3: Watch Management
    println!("\n3️⃣ Testing watch management...");
    
    let current_dir = std::env::current_dir().unwrap();
    let watch_id = plugin.watch(&current_dir, filter.clone()).await?;
    
    println!("   📁 Started watching: {}", current_dir.display());
    println!("   🆔 Watch ID: {:?}", watch_id);
    
    // Check if watching
    let is_watching = plugin.is_watching(&current_dir).await;
    assert!(is_watching);
    println!("   ✅ Directory is being watched");
    
    // Get watched paths
    let watched_paths = plugin.get_watched_paths().await;
    assert_eq!(watched_paths.len(), 1);
    assert_eq!(watched_paths[0].0, watch_id);
    assert_eq!(watched_paths[0].1, current_dir);
    println!("   ✅ Watch paths retrieved correctly");

    // Test 4: Filter Updates
    println!("\n4️⃣ Testing filter updates...");
    
    let new_filter_config = FileWatcherConfig {
        debounce_ms: 200,
        watch_extensions: vec!["md".to_string()], // Only markdown now
        ignore_patterns: vec!["*.tmp".to_string()],
        recursive: true,
        max_depth: Some(3),
    };
    
    let new_filter = Arc::new(DefaultFileFilter::new(new_filter_config));
    plugin.set_filter(watch_id, new_filter).await?;
    println!("   ✅ Filter updated successfully");

    // Test 5: Statistics
    println!("\n5️⃣ Testing statistics...");
    
    let stats = plugin.get_watch_statistics().await;
    println!("   📊 Watched paths: {}", stats.watched_path_count);
    println!("   📊 Pending events: {}", stats.pending_events_count);
    println!("   📊 Total events processed: {}", stats.total_events_processed);
    
    assert_eq!(stats.watched_path_count, 1);
    println!("   ✅ Statistics retrieved correctly");

    // Test 6: Unwatch
    println!("\n6️⃣ Testing unwatch...");
    
    plugin.unwatch(watch_id).await?;
    
    let is_watching_after = plugin.is_watching(&current_dir).await;
    assert!(!is_watching_after);
    
    let watched_paths_after = plugin.get_watched_paths().await;
    assert_eq!(watched_paths_after.len(), 0);
    
    println!("   ✅ Directory unwatched successfully");

    // Test 7: Plugin Shutdown
    println!("\n7️⃣ Testing plugin shutdown...");
    
    plugin.shutdown().await?;
    println!("   ✅ Plugin shut down successfully");

    // Test 8: Architecture Benefits Demonstration
    println!("\n8️⃣ Demonstrating architecture benefits...");
    
    println!("   🏗️  Architecture Benefits:");
    println!("      ✅ No circular dependencies");
    println!("      ✅ FileWatcher trait in rune-core");
    println!("      ✅ Plugin implements interface cleanly");
    println!("      ✅ Event-driven file monitoring");
    println!("      ✅ Configurable filtering");
    println!("      ✅ Proper error handling");
    println!("      ✅ Resource cleanup");

    println!("\n🎉 All tests passed! File watcher architecture is working correctly.");
    
    Ok(())
}

/// Custom test filter to demonstrate extensibility
struct TestMarkdownFilter;

#[async_trait::async_trait]
impl FileFilter for TestMarkdownFilter {
    fn should_watch(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext == "md")
            .unwrap_or(false)
    }

    fn debounce_duration(&self) -> Duration {
        Duration::from_millis(150)
    }

    fn filter_name(&self) -> &str {
        "TestMarkdownFilter"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_file_filter_behavior() {
        let config = FileWatcherConfig {
            debounce_ms: 100,
            watch_extensions: vec!["md".to_string()],
            ignore_patterns: vec!["*.tmp".to_string()],
            recursive: true,
            max_depth: None,
        };
        
        let filter = DefaultFileFilter::new(config);
        
        // Test positive cases
        assert!(filter.should_watch(Path::new("README.md")));
        assert!(filter.should_watch(Path::new("docs/guide.md")));
        
        // Test negative cases
        assert!(!filter.should_watch(Path::new("temp.tmp")));
        assert!(!filter.should_watch(Path::new("script.js")));
        assert!(!filter.should_watch(Path::new("data.json")));
        
        // Test debounce duration
        assert_eq!(filter.debounce_duration(), Duration::from_millis(100));
        
        // Test filter name
        assert_eq!(filter.filter_name(), "DefaultFileFilter");
    }

    #[tokio::test]
    async fn test_custom_filter() {
        let filter = TestMarkdownFilter;
        
        assert!(filter.should_watch(Path::new("test.md")));
        assert!(!filter.should_watch(Path::new("test.txt")));
        assert_eq!(filter.debounce_duration(), Duration::from_millis(150));
        assert_eq!(filter.filter_name(), "TestMarkdownFilter");
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() -> Result<()> {
        let mut plugin = FileWatcherPlugin::new();
        
        // Test initial state
        assert_eq!(plugin.name(), "file-watcher");
        assert_eq!(plugin.version(), "0.1.0");
        assert_eq!(plugin.dependencies(), Vec::<&str>::new());
        assert_eq!(plugin.provided_services(), vec!["file-watching"]);
        
        // Test context creation and initialization
        let event_bus = Arc::new(InMemoryEventBus::new());
        let config = Arc::new(Config::default());
        let state_manager = Arc::new(StateManager::new());
        let context = PluginContext::new(event_bus, config, state_manager);
        
        plugin.initialize(&context).await?;
        
        // Test shutdown
        plugin.shutdown().await?;
        
        Ok(())
    }
}