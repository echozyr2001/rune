//! File watcher plugin for Rune
//!
//! This plugin provides file system watching capabilities with configurable filtering
//! and debouncing. It implements the FileWatcher trait defined in rune-core.

use async_trait::async_trait;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use rune_core::{
    event::{ChangeType, SystemEvent, SystemEventHandler},
    FileFilter, FileWatcher, Plugin, PluginContext, PluginStatus, Result, RuneError, WatcherId,
};
use std::any::Any;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

/// Information about a watched path
#[derive(Debug, Clone)]
struct WatchedPath {
    path: PathBuf,
    recursive: bool,
    filter: Arc<dyn FileFilter>,
}

/// Statistics about file watching activity
#[derive(Debug, Clone)]
pub struct WatchStatistics {
    pub watched_path_count: usize,
    pub pending_events_count: usize,
    pub total_events_processed: u64,
}

/// Debounced file change event
#[derive(Debug, Clone)]
struct DebouncedEvent {
    path: PathBuf,
    change_type: ChangeType,
    last_seen: Instant,
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

    /// Process file system events with debouncing and error recovery
    async fn process_events(
        &self,
        mut event_receiver: mpsc::UnboundedReceiver<notify::Result<Event>>,
    ) {
        let mut debounce_timer = tokio::time::interval(Duration::from_millis(50));
        let mut error_count = 0u32;
        const MAX_CONSECUTIVE_ERRORS: u32 = 10;
        const ERROR_RESET_INTERVAL: Duration = Duration::from_secs(60);
        let mut last_error_reset = Instant::now();

        info!("Starting file watcher event processing loop");

        loop {
            // Reset error count periodically
            if last_error_reset.elapsed() >= ERROR_RESET_INTERVAL {
                if error_count > 0 {
                    debug!("Resetting error count from {} to 0", error_count);
                    error_count = 0;
                }
                last_error_reset = Instant::now();
            }

            tokio::select! {
                // Process incoming file system events
                event_result = event_receiver.recv() => {
                    match event_result {
                        Some(Ok(event)) => {
                            // Reset error count on successful event
                            if error_count > 0 {
                                debug!("Successful event received, resetting error count");
                                error_count = 0;
                            }

                            if let Err(e) = self.handle_file_event(event).await {
                                error!("Failed to handle file event: {}", e);
                                error_count += 1;
                            }
                        }
                        Some(Err(e)) => {
                            error_count += 1;
                            error!("File watcher error (count: {}): {}", error_count, e);

                            // Publish error event for monitoring
                            if let Some(context) = &self.context {
                                let error_event = SystemEvent::error(
                                    "file-watcher".to_string(),
                                    format!("File system watcher error: {}", e),
                                    rune_core::event::ErrorSeverity::High,
                                );

                                if let Err(publish_err) = context.event_bus.publish_system_event(error_event).await {
                                    error!("Failed to publish watcher error event: {}", publish_err);
                                }
                            }

                            // Check if we need to trigger recovery
                            if error_count >= MAX_CONSECUTIVE_ERRORS {
                                error!("Too many consecutive errors ({}), attempting recovery", error_count);
                                if let Err(recovery_err) = self.attempt_watcher_recovery().await {
                                    error!("Watcher recovery failed: {}", recovery_err);
                                } else {
                                    info!("Watcher recovery completed successfully");
                                    error_count = 0;
                                }
                            }
                        }
                        None => {
                            warn!("File watcher event channel closed, attempting to reconnect");
                            if let Err(e) = self.attempt_watcher_recovery().await {
                                error!("Failed to recover from closed channel: {}", e);
                                break;
                            }
                        }
                    }
                }

                // Process debounced events periodically
                _ = debounce_timer.tick() => {
                    if let Err(e) = self.process_debounced_events().await {
                        error!("Failed to process debounced events: {}", e);
                        error_count += 1;
                    }
                }
            }

            // Add a small delay if we're experiencing errors to prevent tight error loops
            if error_count > 0 {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }

        warn!("File watcher event processing loop terminated");
    }

    /// Attempt to recover from watcher failures
    async fn attempt_watcher_recovery(&self) -> Result<()> {
        warn!("Attempting file watcher recovery");

        if let Some(context) = &self.context {
            // Publish recovery attempt event
            let recovery_event = SystemEvent::error(
                "file-watcher".to_string(),
                "Attempting watcher recovery due to failures".to_string(),
                rune_core::event::ErrorSeverity::Medium,
            );

            if let Err(e) = context.event_bus.publish_system_event(recovery_event).await {
                warn!("Failed to publish recovery attempt event: {}", e);
            }
        }

        // Clear any corrupted state
        {
            let mut debounced_events = self.debounced_events.write().await;
            debounced_events.clear();
        }

        // Add a delay to prevent immediate re-failure
        tokio::time::sleep(Duration::from_secs(1)).await;

        info!("File watcher recovery completed");
        Ok(())
    }

    /// Handle a single file system event
    async fn handle_file_event(&self, event: Event) -> Result<()> {
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

        Ok(())
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
    async fn process_debounced_events(&self) -> Result<()> {
        let mut events_to_publish = Vec::new();
        let mut debounced_events = self.debounced_events.write().await;
        let now = Instant::now();

        // Find events that have been debounced long enough
        let mut expired_paths = Vec::new();
        for (path, event) in debounced_events.iter() {
            // Get the appropriate debounce duration from the filter
            let debounce_duration = self.get_debounce_duration_for_path(&event.path).await;

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
                let system_event =
                    SystemEvent::file_changed(event.path.clone(), event.change_type.clone());

                if let Err(e) = context.event_bus.publish_system_event(system_event).await {
                    error!("Failed to publish file change event: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Get the debounce duration for a specific path by finding its filter
    async fn get_debounce_duration_for_path(&self, path: &Path) -> Duration {
        let watched_paths = self.watched_paths.read().await;

        for watched_path in watched_paths.values() {
            if self.path_matches_watch(path, watched_path) {
                return watched_path.filter.debounce_duration();
            }
        }

        // Default debounce duration if no specific filter found
        Duration::from_millis(100)
    }

    /// Get statistics about file watching activity
    pub async fn get_watch_statistics(&self) -> WatchStatistics {
        let watched_paths = self.watched_paths.read().await;
        let debounced_events = self.debounced_events.read().await;

        WatchStatistics {
            watched_path_count: watched_paths.len(),
            pending_events_count: debounced_events.len(),
            total_events_processed: 0, // Would track this in a real implementation
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

        // Start watching the current directory by default
        let current_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

        // Create a default filter for common file types
        let config = rune_core::FileWatcherConfig {
            debounce_ms: 200,
            watch_extensions: vec![
                "md".to_string(),
                "markdown".to_string(),
                "txt".to_string(),
                "html".to_string(),
                "css".to_string(),
                "js".to_string(),
            ],
            ignore_patterns: vec![
                "*.tmp".to_string(),
                "*.swp".to_string(),
                "*~".to_string(),
                ".git/**".to_string(),
                "node_modules/**".to_string(),
                "target/**".to_string(),
                ".DS_Store".to_string(),
            ],
            recursive: false, // Only watch the current directory, not subdirectories
            max_depth: None,
        };

        let filter = Arc::new(rune_core::DefaultFileFilter::new(config));

        // Start watching the current directory
        if let Some(watcher) = &mut self.watcher {
            if let Err(e) = watcher.watch(&current_dir, RecursiveMode::NonRecursive) {
                warn!("Failed to start watching current directory: {}", e);
            } else {
                // Store the watched path
                let watch_id = WatcherId::new();
                let watched_path = WatchedPath {
                    path: current_dir.clone(),
                    recursive: false,
                    filter,
                };

                {
                    let mut watched_paths = self.watched_paths.write().await;
                    watched_paths.insert(watch_id, watched_path);
                }

                info!(
                    "Started watching current directory: {}",
                    current_dir.display()
                );
            }
        }

        self.status = PluginStatus::Active;

        // Subscribe to system events for better integration
        let handler = Arc::new(FileWatcherEventHandler {
            plugin_name: self.name.clone(),
        });

        if let Err(e) = context.event_bus.subscribe_system_events(handler).await {
            warn!("Failed to subscribe to system events: {}", e);
        } else {
            debug!("File watcher subscribed to system events");
        }

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

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
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
            let recursive_mode = if filter.filter_name().contains("recursive") {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            };

            watcher
                .watch(path, recursive_mode)
                .map_err(|e| RuneError::Plugin(format!("Failed to watch path: {}", e)))?;
        }

        // Store the watched path
        let watched_path = WatchedPath {
            path: path.to_path_buf(),
            recursive: true, // Default to recursive for now
            filter,
        };

        {
            let mut watched_paths = self.watched_paths.write().await;
            watched_paths.insert(id, watched_path);
        }

        info!("Successfully started watching path: {}", path.display());
        Ok(id)
    }

    async fn unwatch(&mut self, id: WatcherId) -> Result<()> {
        let path = {
            let mut watched_paths = self.watched_paths.write().await;
            watched_paths.remove(&id).map(|wp| wp.path)
        };

        if let Some(path) = path {
            info!("Stopping watch for path: {}", path.display());

            if let Some(watcher) = &mut self.watcher {
                watcher
                    .unwatch(&path)
                    .map_err(|e| RuneError::Plugin(format!("Failed to unwatch path: {}", e)))?;
            }

            info!("Successfully stopped watching path: {}", path.display());
        } else {
            warn!("Attempted to unwatch unknown ID: {:?}", id);
        }

        Ok(())
    }

    async fn set_filter(&mut self, id: WatcherId, filter: Arc<dyn FileFilter>) -> Result<()> {
        let mut watched_paths = self.watched_paths.write().await;
        if let Some(watched_path) = watched_paths.get_mut(&id) {
            watched_path.filter = filter;
            info!("Updated filter for watch ID: {:?}", id);
            Ok(())
        } else {
            Err(RuneError::Plugin(format!("Watch ID not found: {:?}", id)))
        }
    }

    async fn get_watched_paths(&self) -> Vec<(WatcherId, PathBuf)> {
        let watched_paths = self.watched_paths.read().await;
        watched_paths
            .iter()
            .map(|(id, wp)| (*id, wp.path.clone()))
            .collect()
    }

    async fn is_watching(&self, path: &Path) -> bool {
        let watched_paths = self.watched_paths.read().await;
        watched_paths.values().any(|wp| wp.path == path)
    }
}

/// Event handler for system events
pub struct FileWatcherEventHandler {
    plugin_name: String,
}

#[async_trait]
impl SystemEventHandler for FileWatcherEventHandler {
    async fn handle_system_event(&self, event: &SystemEvent) -> Result<()> {
        match event {
            SystemEvent::Error {
                source,
                message,
                severity,
                ..
            } => {
                // If there's a critical error from another component, we might need to adjust our behavior
                if source == "file-system"
                    && matches!(severity, rune_core::event::ErrorSeverity::Critical)
                {
                    warn!(
                        "Critical file system error detected, may affect file watching: {}",
                        message
                    );
                }
            }
            SystemEvent::PluginLoaded { plugin_name, .. } => {
                debug!(
                    "Plugin {} loaded, file watcher ready for integration",
                    plugin_name
                );
            }
            SystemEvent::FileChanged { path, .. } => {
                debug!(
                    "FileWatcher received file change event for: {}",
                    path.display()
                );
                // The file watcher is already monitoring the directory, so this event
                // was likely generated by our own file monitoring. We don't need to do anything special here.
            }
            _ => {
                // Ignore other events
            }
        }
        Ok(())
    }

    fn handler_name(&self) -> &str {
        &self.plugin_name
    }
}
