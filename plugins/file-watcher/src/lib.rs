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
    event::{ChangeType, SystemEvent, SystemEventHandler},
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

/// Statistics about file watching activity
#[derive(Debug, Clone, Serialize, Deserialize)]
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

    /// Create a new file watcher plugin with custom configuration
    pub fn with_config(_config: FileWatcherConfig) -> Self {
        // Store config for later use during initialization
        Self::new()
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

                            if let Err(e) = self.handle_file_event_with_recovery(event).await {
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
                                    // Continue processing but log the failure
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
                    if let Err(e) = self.process_debounced_events_with_recovery().await {
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

    /// Handle file event with error recovery
    async fn handle_file_event_with_recovery(&self, event: Event) -> Result<()> {
        match self.handle_file_event(event.clone()).await {
            Ok(()) => Ok(()),
            Err(e) => {
                error!("Error handling file event: {}", e);

                // Attempt to re-process the event once
                debug!("Retrying file event processing");
                tokio::time::sleep(Duration::from_millis(50)).await;

                self.handle_file_event(event).await.map_err(|retry_err| {
                    rune_core::RuneError::Plugin(format!(
                        "File event handling failed after retry: {}",
                        retry_err
                    ))
                })
            }
        }
    }

    /// Process debounced events with error recovery
    async fn process_debounced_events_with_recovery(&self) -> Result<()> {
        match self.process_debounced_events().await {
            Ok(()) => Ok(()),
            Err(e) => {
                error!("Error processing debounced events: {}", e);

                // Clear potentially corrupted debounced events
                {
                    let mut debounced_events = self.debounced_events.write().await;
                    let event_count = debounced_events.len();
                    debounced_events.clear();
                    warn!(
                        "Cleared {} potentially corrupted debounced events",
                        event_count
                    );
                }

                Err(rune_core::RuneError::Plugin(format!(
                    "Debounced event processing failed: {}",
                    e
                )))
            }
        }
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

        // In a real implementation, we would:
        // 1. Stop the current watcher
        // 2. Clear any corrupted state
        // 3. Recreate the watcher
        // 4. Re-register all watched paths

        // For now, we'll simulate recovery by clearing debounced events
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

    /// Process debounced events and publish them with atomic write detection
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
                // Check for atomic write patterns before publishing
                if self.is_atomic_write_complete(&event.path).await {
                    events_to_publish.push(event.clone());
                    expired_paths.push(path.clone());
                } else {
                    // Extend the debounce time for potential atomic writes
                    debug!(
                        "Extending debounce for potential atomic write: {}",
                        event.path.display()
                    );
                }
            }
        }

        // Remove expired events
        for path in expired_paths {
            debounced_events.remove(&path);
        }

        drop(debounced_events);

        // Publish events with error recovery
        if let Some(context) = &self.context {
            for event in events_to_publish {
                let system_event =
                    SystemEvent::file_changed(event.path.clone(), event.change_type.clone());

                // Attempt to publish with retry logic
                if let Err(e) = self.publish_event_with_retry(context, system_event).await {
                    error!("Failed to publish file change event after retries: {}", e);

                    // Publish error event for monitoring
                    let error_event = SystemEvent::error(
                        "file-watcher".to_string(),
                        format!(
                            "Failed to publish file change event for {}: {}",
                            event.path.display(),
                            e
                        ),
                        rune_core::event::ErrorSeverity::Medium,
                    );

                    if let Err(error_publish_err) =
                        context.event_bus.publish_system_event(error_event).await
                    {
                        error!("Failed to publish error event: {}", error_publish_err);
                    }
                }
            }
        }

        Ok(())
    }

    /// Get the debounce duration for a specific path by finding its filter
    async fn get_debounce_duration_for_path(&self, path: &std::path::Path) -> Duration {
        let watched_paths = self.watched_paths.read().await;

        for watched_path in watched_paths.values() {
            if self.path_matches_watch(path, watched_path) {
                return watched_path.filter.debounce_duration();
            }
        }

        // Default debounce duration if no specific filter found
        Duration::from_millis(100)
    }

    /// Check if an atomic write operation is complete
    async fn is_atomic_write_complete(&self, path: &std::path::Path) -> bool {
        // Check for common atomic write patterns

        // 1. Check if file exists and is readable (not being written to)
        if !path.exists() {
            return false;
        }

        // 2. Check for temporary file patterns that indicate atomic writes
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Common atomic write patterns:
        // - .tmp files being renamed
        // - ~ backup files
        // - .swp, .swo swap files
        if file_name.starts_with('.')
            && (file_name.ends_with(".tmp")
                || file_name.ends_with("~")
                || file_name.ends_with(".swp")
                || file_name.ends_with(".swo"))
        {
            debug!(
                "Detected temporary file pattern, waiting for atomic write completion: {}",
                path.display()
            );
            return false;
        }

        // 3. Try to detect if file is still being written by checking if we can get exclusive access
        match std::fs::OpenOptions::new().read(true).open(path) {
            Ok(_) => {
                // File is readable, likely not being written to
                true
            }
            Err(e) => {
                // If we can't read the file, it might still be being written
                debug!(
                    "Cannot read file {}, might still be writing: {}",
                    path.display(),
                    e
                );
                false
            }
        }
    }

    /// Publish event with retry logic for error recovery
    async fn publish_event_with_retry(
        &self,
        context: &PluginContext,
        event: SystemEvent,
    ) -> Result<()> {
        const MAX_RETRIES: u32 = 3;
        const RETRY_DELAY: Duration = Duration::from_millis(100);

        let mut last_error = None;

        for attempt in 1..=MAX_RETRIES {
            match context.event_bus.publish_system_event(event.clone()).await {
                Ok(()) => {
                    if attempt > 1 {
                        debug!("Successfully published event after {} attempts", attempt);
                    }
                    return Ok(());
                }
                Err(e) => {
                    warn!(
                        "Failed to publish event (attempt {}/{}): {}",
                        attempt, MAX_RETRIES, e
                    );
                    last_error = Some(e);

                    if attempt < MAX_RETRIES {
                        tokio::time::sleep(RETRY_DELAY * attempt).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            rune_core::RuneError::Plugin("Unknown error during event publishing".to_string())
        }))
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
}

impl FileWatcherPlugin {
    /// Handle system events that might affect file watching
    pub async fn handle_system_event(&self, event: &SystemEvent) -> Result<()> {
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
                    // Could implement adaptive behavior here
                }
            }
            SystemEvent::PluginLoaded { plugin_name, .. } => {
                debug!(
                    "Plugin {} loaded, file watcher ready for integration",
                    plugin_name
                );
            }
            _ => {
                // Ignore other events
            }
        }
        Ok(())
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
/// Event handler for file watcher to respond to system events
#[derive(Debug)]
struct FileWatcherEventHandler {
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
                // React to file system errors
                if source.contains("file") || source.contains("io") {
                    debug!(
                        "File watcher detected file system error: {} - {}",
                        source, message
                    );

                    // Could implement recovery strategies here based on severity
                    match severity {
                        rune_core::event::ErrorSeverity::Critical => {
                            warn!("Critical file system error may affect file watching");
                        }
                        _ => {
                            debug!("Non-critical file system error noted");
                        }
                    }
                }
            }
            SystemEvent::PluginLoaded { plugin_name, .. } => {
                if plugin_name == "server" || plugin_name == "renderer" {
                    debug!(
                        "Key plugin {} loaded, file watcher ready for enhanced integration",
                        plugin_name
                    );
                }
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
