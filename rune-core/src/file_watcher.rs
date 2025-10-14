//! File watcher traits and types for Rune

use crate::{Plugin, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// Unique identifier for file watchers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    pub debounce_ms: u64,
    pub watch_extensions: Vec<String>,
    pub ignore_patterns: Vec<String>,
    pub recursive: bool,
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
    fn should_watch(&self, path: &Path) -> bool;
    fn debounce_duration(&self) -> Duration;
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

        for pattern in &self.config.ignore_patterns {
            if glob_match(pattern, &path_str) {
                return false;
            }
        }

        if !self.config.watch_extensions.is_empty() {
            if let Some(ext) = path.extension() {
                let ext_str = ext.to_string_lossy().to_lowercase();
                if !self
                    .config
                    .watch_extensions
                    .iter()
                    .any(|e| e.to_lowercase() == ext_str)
                {
                    return false;
                }
            } else {
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
pub fn glob_match(pattern: &str, text: &str) -> bool {
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

/// File watcher trait extending Plugin interface
#[async_trait]
pub trait FileWatcher: Plugin {
    async fn watch(&mut self, path: &Path, filter: Arc<dyn FileFilter>) -> Result<WatcherId>;
    async fn unwatch(&mut self, id: WatcherId) -> Result<()>;
    async fn set_filter(&mut self, id: WatcherId, filter: Arc<dyn FileFilter>) -> Result<()>;
    async fn get_watched_paths(&self) -> Vec<(WatcherId, PathBuf)>;
    async fn is_watching(&self, path: &Path) -> bool;
}
