//! File watcher plugin for Rune
//!
//! This plugin provides file system watching capabilities with configurable filtering
//! and debouncing. It extends the core Plugin trait with FileWatcher functionality.
//!
//! # Features
//!
//! - **Debounced file change detection**: Prevents duplicate events for rapid file changes
//! - **Configurable file filtering**: Watch only specific file types or patterns
//! - **Recursive directory watching**: Monitor entire directory trees
//! - **Event bus integration**: Publishes file change events to the system event bus
//! - **Multiple watch management**: Track multiple watched paths with unique IDs
//!
//! # Example Usage
//!
//! ```rust,no_run
//! use std::sync::Arc;
//! use std::path::Path;
//! use rune_file_watcher::{FileWatcherPlugin, FileWatcher, DefaultFileFilter, FileWatcherConfig};
//! use rune_core::{Plugin, PluginContext};
//!
//! # async fn example() -> rune_core::Result<()> {
//! // Create and initialize the plugin
//! let mut plugin = FileWatcherPlugin::new();
//! // let context = PluginContext::new(...); // Initialize with actual context
//! // plugin.initialize(&context).await?;
//!
//! // Create a filter for markdown files
//! let config = FileWatcherConfig {
//!     debounce_ms: 200,
//!     watch_extensions: vec!["md".to_string(), "markdown".to_string()],
//!     ignore_patterns: vec!["*.tmp".to_string()],
//!     recursive: true,
//!     max_depth: Some(3),
//! };
//! let filter = Arc::new(DefaultFileFilter::new(config));
//!
//! // Start watching a directory
//! let watch_id = plugin.watch(Path::new("./docs"), filter).await?;
//!
//! // Later, stop watching
//! plugin.unwatch(watch_id).await?;
//! # Ok(())
//! # }
//! ```

use async_trait::async_trait;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use rune_core::{
    event::{ChangeType, SystemEvent},
    Plugin, PluginContext, PluginStatus, Result, RuneError,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

/// Unique identifier for file watchers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WatcherId(pub u64);

impl WatcherId {
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for WatcherId {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration for file filtering and debouncing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileWatcherConfig {
    /// Debounce duration in milliseconds
    pub debounce_ms: u64,
    /// File extensions to watch (empty means all files)
    pub watch_extensions: Vec<String>,
    /// File patterns to ignore (glob patterns)
    pub ignore_patterns: Vec<String>,
    /// Whether to watch directories recursively
    pub recursive: bool,
    /// Maximum depth for recursive watching (None means unlimited)
    pub max_depth: Option<usize>,
}

impl Default for FileWatcherConfig {
    fn default() -> Self {
        Self {
            debounce_ms: 100,
            watch_extensions: vec![],
            ignore_patterns: vec![
                "*.tmp".to_string(),
                "*.swp".to_string(),
                "*~".to_string(),
                ".git/**".to_string(),
                "node_modules/**".to_string(),
                "target/**".to_string(),
            ],
            recursive: true,
            max_depth: None,
        }
    }
}

/// Filter for determining which files should be watched
#[async_trait]
pub trait FileFilter: Send + Sync + std::fmt::Debug {
    /// Check if a file should be watched
    fn should_watch(&self, path: &Path) -> bool;

    /// Get the debounce duration for this filter
    fn debounce_duration(&self) -> Duration;

    /// Get filter name for debugging
    fn filter_name(&self) -> &str {
        "UnnamedFilter"
    }
}

/// Default file filter implementation based on configuration
#[derive(Debug, Clone)]
pub struct DefaultFileFilter {
    config: FileWatcherConfig,
}

impl DefaultFileFilter {
    pub fn new(config: FileWatcherConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl FileFilter for DefaultFileFilter {
    fn should_watch(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();

        // Check ignore patterns
        for pattern in &self.config.ignore_patterns {
            if glob_match(pattern, &path_str) {
                debug!("Ignoring file {} due to pattern {}", path_str, pattern);
                return false;
            }
        }

        // Check extensions if specified
        if !self.config.watch_extensions.is_empty() {
            if let Some(ext) = path.extension() {
                let ext_str = ext.to_string_lossy().to_lowercase();
                if !self
                    .config
                    .watch_extensions
                    .iter()
                    .any(|e| e.to_lowercase() == ext_str)
                {
                    debug!("Ignoring file {} due to extension filter", path_str);
                    return false;
                }
            } else {
                // No extension, but we have extension filters
                debug!("Ignoring file {} (no extension)", path_str);
                return false;
            }
        }

        true
    }

    fn debounce_duration(&self) -> Duration {
        Duration::from_millis(self.config.debounce_ms)
    }

    fn filter_name(&self) -> &str {
        "DefaultFileFilter"
    }
}

/// Simple glob pattern matching
fn glob_match(pattern: &str, text: &str) -> bool {
    // Simple implementation - in production, use a proper glob library
    if pattern.contains("**") {
        // Handle recursive patterns
        let parts: Vec<&str> = pattern.split("**").collect();
        if parts.len() == 2 {
            let prefix = parts[0];
            let suffix = parts[1];
            return text.starts_with(prefix) && text.ends_with(suffix);
        }
    }

    if pattern.contains('*') {
        // Handle single-level wildcards
        let parts: Vec<&str> = pattern.split('*').collect();
        if parts.len() == 2 {
            let prefix = parts[0];
            let suffix = parts[1];
            return text.starts_with(prefix) && text.ends_with(suffix);
        }
    }

    // Exact match
    pattern == text
}

/// Information about a watched path
#[derive(Debug, Clone)]
struct WatchedPath {
    path: PathBuf,
    recursive: bool,
    filter: Arc<dyn FileFilter>,
}

/// Debounced file change event
#[derive(Debug, Clone)]
struct DebouncedEvent {
    path: PathBuf,
    change_type: ChangeType,
    last_seen: Instant,
}

/// File watcher trait extending Plugin interface
#[async_trait]
pub trait FileWatcher: Plugin {
    /// Start watching a path with the given filter
    async fn watch(&mut self, path: &Path, filter: Arc<dyn FileFilter>) -> Result<WatcherId>;

    /// Stop watching a path by ID
    async fn unwatch(&mut self, id: WatcherId) -> Result<()>;

    /// Update the filter for a watched path
    async fn set_filter(&mut self, id: WatcherId, filter: Arc<dyn FileFilter>) -> Result<()>;

    /// Get all currently watched paths
    async fn get_watched_paths(&self) -> Vec<(WatcherId, PathBuf)>;

    /// Check if a path is being watched
    async fn is_watching(&self, path: &Path) -> bool;
}

/// File watcher plugin implementation using notify
pub struct FileWatcherPlugin {
    name: String,
    version: String,
    status: PluginStatus,
    context: Option<PluginContext>,
    watcher: Option<RecommendedWatcher>,
    watched_paths: Arc<RwLock<HashMap<WatcherId, WatchedPath>>>,
    debounced_events: Arc<RwLock<HashMap<PathBuf, DebouncedEvent>>>,
    event_sender: Option<mpsc::UnboundedSender<notify::Result<Event>>>,
}

impl FileWatcherPlugin {
    /// Create a new file watcher plugin
    pub fn new() -> Self {
        Self {
            name: "file-watcher".to_string(),
            version: "0.1.0".to_string(),
            status: PluginStatus::Loading,
            context: None,
            watcher: None,
            watched_paths: Arc::new(RwLock::new(HashMap::new())),
            debounced_events: Arc::new(RwLock::new(HashMap::new())),
            event_sender: None,
        }
    }

    /// Process file system events with debouncing
    async fn process_events(
        &self,
        mut event_receiver: mpsc::UnboundedReceiver<notify::Result<Event>>,
    ) {
        let mut debounce_timer = tokio::time::interval(Duration::from_millis(50));

        loop {
            tokio::select! {
                // Process incoming file system events
                event_result = event_receiver.recv() => {
                    match event_result {
                        Some(Ok(event)) => {
                            self.handle_file_event(event).await;
                        }
                        Some(Err(e)) => {
                            error!("File watcher error: {}", e);
                        }
                        None => {
                            debug!("File watcher event channel closed");
                            break;
                        }
                    }
                }

                // Process debounced events periodically
                _ = debounce_timer.tick() => {
                    self.process_debounced_events().await;
                }
            }
        }
    }

    /// Handle a single file system event
    async fn handle_file_event(&self, event: Event) {
        for path in event.paths {
            // Check if any watched path should handle this event
            let watched_paths = self.watched_paths.read().await;
            let mut should_process = false;

            for watched_path in watched_paths.values() {
                if self.path_matches_watch(&path, watched_path)
                    && watched_path.filter.should_watch(&path)
                {
                    should_process = true;
                    break;
                }
            }

            drop(watched_paths);

            if should_process {
                let change_type = match event.kind {
                    notify::EventKind::Create(_) => ChangeType::Created,
                    notify::EventKind::Modify(_) => ChangeType::Modified,
                    notify::EventKind::Remove(_) => ChangeType::Deleted,
                    _ => ChangeType::Modified, // Default to modified for other events
                };

                // Add to debounced events
                let mut debounced_events = self.debounced_events.write().await;
                debounced_events.insert(
                    path.clone(),
                    DebouncedEvent {
                        path: path.clone(),
                        change_type,
                        last_seen: Instant::now(),
                    },
                );
            }
        }
    }

    /// Check if a path matches a watched path configuration
    fn path_matches_watch(&self, path: &Path, watched_path: &WatchedPath) -> bool {
        if watched_path.recursive {
            path.starts_with(&watched_path.path)
        } else {
            path.parent() == Some(&watched_path.path) || path == watched_path.path
        }
    }

    /// Process debounced events and publish them
    async fn process_debounced_events(&self) {
        let mut events_to_publish = Vec::new();
        let mut debounced_events = self.debounced_events.write().await;
        let now = Instant::now();

        // Find events that have been debounced long enough
        let mut expired_paths = Vec::new();
        for (path, event) in debounced_events.iter() {
            // Use a default debounce duration if we can't find the specific filter
            let debounce_duration = Duration::from_millis(100);

            if now.duration_since(event.last_seen) >= debounce_duration {
                events_to_publish.push(event.clone());
                expired_paths.push(path.clone());
            }
        }

        // Remove expired events
        for path in expired_paths {
            debounced_events.remove(&path);
        }

        drop(debounced_events);

        // Publish events
        if let Some(context) = &self.context {
            for event in events_to_publish {
                let system_event = SystemEvent::file_changed(event.path, event.change_type);

                if let Err(e) = context.event_bus.publish_system_event(system_event).await {
                    error!("Failed to publish file change event: {}", e);
                }
            }
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

    async fn initialize(&mut self, context: &PluginContext) -> Result<()> {
        info!("Initializing file watcher plugin");

        self.context = Some(context.clone());

        // Create event channel for file system events
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        self.event_sender = Some(event_sender.clone());

        // Create the notify watcher
        let watcher = RecommendedWatcher::new(
            move |res| {
                if let Err(e) = event_sender.send(res) {
                    error!("Failed to send file watcher event: {}", e);
                }
            },
            Config::default(),
        )
        .map_err(|e| RuneError::Plugin(format!("Failed to create file watcher: {}", e)))?;

        self.watcher = Some(watcher);

        // Start event processing task
        let plugin_clone = self.watched_paths.clone();
        let debounced_events_clone = self.debounced_events.clone();
        let context_clone = context.clone();

        tokio::spawn(async move {
            let temp_plugin = FileWatcherPlugin {
                name: "file-watcher".to_string(),
                version: "0.1.0".to_string(),
                status: PluginStatus::Active,
                context: Some(context_clone),
                watcher: None,
                watched_paths: plugin_clone,
                debounced_events: debounced_events_clone,
                event_sender: None,
            };
            temp_plugin.process_events(event_receiver).await;
        });

        self.status = PluginStatus::Active;
        info!("File watcher plugin initialized successfully");
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        info!("Shutting down file watcher plugin");

        // Stop watching all paths
        let watched_paths: Vec<WatcherId> = {
            let paths = self.watched_paths.read().await;
            paths.keys().copied().collect()
        };

        for id in watched_paths {
            if let Err(e) = self.unwatch(id).await {
                warn!("Failed to unwatch path during shutdown: {}", e);
            }
        }

        // Drop the watcher
        self.watcher = None;
        self.event_sender = None;
        self.context = None;

        self.status = PluginStatus::Stopped;
        info!("File watcher plugin shutdown complete");
        Ok(())
    }

    fn status(&self) -> PluginStatus {
        self.status.clone()
    }

    fn provided_services(&self) -> Vec<&str> {
        vec!["file-watching"]
    }
}

#[async_trait]
impl FileWatcher for FileWatcherPlugin {
    async fn watch(&mut self, path: &Path, filter: Arc<dyn FileFilter>) -> Result<WatcherId> {
        let id = WatcherId::new();

        info!(
            "Starting to watch path: {} with filter: {}",
            path.display(),
            filter.filter_name()
        );

        // Add to watcher if we have one
        if let Some(watcher) = &mut self.watcher {
            let recursive_mode = if filter.as_ref().filter_name().contains("recursive") {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            };

            watcher.watch(path, recursive_mode).map_err(|e| {
                RuneError::Plugin(format!("Failed to watch path {}: {}", path.display(), e))
            })?;
        }

        // Store the watched path info
        let watched_path = WatchedPath {
            path: path.to_path_buf(),
            recursive: true, // Default to recursive for now
            filter,
        };

        let mut watched_paths = self.watched_paths.write().await;
        watched_paths.insert(id, watched_path);

        debug!(
            "Successfully added watch for path: {} (ID: {:?})",
            path.display(),
            id
        );
        Ok(id)
    }

    async fn unwatch(&mut self, id: WatcherId) -> Result<()> {
        let mut watched_paths = self.watched_paths.write().await;

        if let Some(watched_path) = watched_paths.remove(&id) {
            info!(
                "Stopping watch for path: {} (ID: {:?})",
                watched_path.path.display(),
                id
            );

            // Remove from notify watcher if we have one
            if let Some(watcher) = &mut self.watcher {
                if let Err(e) = watcher.unwatch(&watched_path.path) {
                    warn!(
                        "Failed to unwatch path {}: {}",
                        watched_path.path.display(),
                        e
                    );
                }
            }

            debug!(
                "Successfully removed watch for path: {}",
                watched_path.path.display()
            );
            Ok(())
        } else {
            Err(RuneError::Plugin(format!("Watch ID {:?} not found", id)))
        }
    }

    async fn set_filter(&mut self, id: WatcherId, filter: Arc<dyn FileFilter>) -> Result<()> {
        let mut watched_paths = self.watched_paths.write().await;

        if let Some(watched_path) = watched_paths.get_mut(&id) {
            info!(
                "Updating filter for path: {} (ID: {:?}) to: {}",
                watched_path.path.display(),
                id,
                filter.filter_name()
            );
            watched_path.filter = filter;
            Ok(())
        } else {
            Err(RuneError::Plugin(format!("Watch ID {:?} not found", id)))
        }
    }

    async fn get_watched_paths(&self) -> Vec<(WatcherId, PathBuf)> {
        let watched_paths = self.watched_paths.read().await;
        watched_paths
            .iter()
            .map(|(id, watched_path)| (*id, watched_path.path.clone()))
            .collect()
    }

    async fn is_watching(&self, path: &Path) -> bool {
        let watched_paths = self.watched_paths.read().await;
        watched_paths
            .values()
            .any(|watched_path| watched_path.path == path)
    }
}
