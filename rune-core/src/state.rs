//! State management for the Rune system

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::plugin::PluginInfo;

/// Application state manager
pub struct StateManager {
    state: Arc<RwLock<ApplicationState>>,
}

impl StateManager {
    /// Create a new state manager
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(ApplicationState::default())),
        }
    }

    /// Get the current application state (read-only)
    pub async fn get_state(&self) -> ApplicationState {
        self.state.read().await.clone()
    }

    /// Update the current file being watched
    pub async fn set_current_file(&self, file: Option<PathBuf>) {
        let mut state = self.state.write().await;
        state.current_file = file;
    }

    /// Add a connected client
    pub async fn add_client(&self, client_id: Uuid, info: ClientInfo) {
        let mut state = self.state.write().await;
        state.active_clients.insert(client_id, info);
    }

    /// Remove a connected client
    pub async fn remove_client(&self, client_id: &Uuid) {
        let mut state = self.state.write().await;
        state.active_clients.remove(client_id);
    }

    /// Get all active clients
    pub async fn get_active_clients(&self) -> HashMap<Uuid, ClientInfo> {
        let state = self.state.read().await;
        state.active_clients.clone()
    }

    /// Update plugin information
    pub async fn update_plugin(&self, plugin_info: PluginInfo) {
        let mut state = self.state.write().await;
        state
            .loaded_plugins
            .insert(plugin_info.name.clone(), plugin_info);
    }

    /// Remove plugin information
    pub async fn remove_plugin(&self, plugin_name: &str) {
        let mut state = self.state.write().await;
        state.loaded_plugins.remove(plugin_name);
    }

    /// Add rendered content to cache
    pub async fn cache_render(&self, content_hash: String, cached_render: CachedRender) {
        let mut state = self.state.write().await;

        // Simple LRU eviction - remove oldest if cache is full
        if state.render_cache.len() >= 100 {
            if let Some((oldest_key, _)) = state
                .render_cache
                .iter()
                .min_by_key(|(_, render)| render.timestamp)
            {
                let oldest_key = oldest_key.clone();
                state.render_cache.remove(&oldest_key);
            }
        }

        state.render_cache.insert(content_hash, cached_render);
    }

    /// Get cached render if available
    pub async fn get_cached_render(&self, content_hash: &str) -> Option<CachedRender> {
        let state = self.state.read().await;
        state.render_cache.get(content_hash).cloned()
    }

    /// Update system health status
    pub async fn update_system_health(&self, health: SystemHealth) {
        let mut state = self.state.write().await;
        state.system_health = health;
    }

    /// Get current system health
    pub async fn get_system_health(&self) -> SystemHealth {
        let state = self.state.read().await;
        state.system_health.clone()
    }
}

impl Default for StateManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Main application state
#[derive(Debug, Clone)]
pub struct ApplicationState {
    pub current_file: Option<PathBuf>,
    pub active_clients: HashMap<Uuid, ClientInfo>,
    pub loaded_plugins: HashMap<String, PluginInfo>,
    pub render_cache: HashMap<String, CachedRender>,
    pub system_health: SystemHealth,
}

impl Default for ApplicationState {
    fn default() -> Self {
        Self {
            current_file: None,
            active_clients: HashMap::new(),
            loaded_plugins: HashMap::new(),
            render_cache: HashMap::new(),
            system_health: SystemHealth::default(),
        }
    }
}

/// Information about connected clients
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub user_agent: Option<String>,
    pub ip_address: String,
    pub connected_at: SystemTime,
    pub last_activity: SystemTime,
}

impl ClientInfo {
    /// Create new client info
    pub fn new(ip_address: String, user_agent: Option<String>) -> Self {
        let now = SystemTime::now();
        Self {
            user_agent,
            ip_address,
            connected_at: now,
            last_activity: now,
        }
    }

    /// Update last activity timestamp
    pub fn update_activity(&mut self) {
        self.last_activity = SystemTime::now();
    }
}

/// Cached rendered content
#[derive(Debug, Clone)]
pub struct CachedRender {
    pub content_hash: String,
    pub rendered_html: String,
    pub timestamp: SystemTime,
    pub metadata: RenderMetadata,
}

impl CachedRender {
    /// Create new cached render
    pub fn new(content_hash: String, rendered_html: String, metadata: RenderMetadata) -> Self {
        Self {
            content_hash,
            rendered_html,
            timestamp: SystemTime::now(),
            metadata,
        }
    }

    /// Check if the cached render is still valid
    pub fn is_valid(&self, max_age: std::time::Duration) -> bool {
        self.timestamp.elapsed().unwrap_or_default() < max_age
    }
}

/// Metadata about rendered content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderMetadata {
    pub file_path: Option<PathBuf>,
    pub theme: String,
    pub render_time: std::time::Duration,
    pub content_type: String,
    pub has_mermaid: bool,
}

impl Default for RenderMetadata {
    fn default() -> Self {
        Self {
            file_path: None,
            theme: "default".to_string(),
            render_time: std::time::Duration::from_millis(0),
            content_type: "text/html".to_string(),
            has_mermaid: false,
        }
    }
}

/// System health information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemHealth {
    pub status: HealthStatus,
    pub uptime: std::time::Duration,
    pub memory_usage: Option<u64>,
    pub active_connections: usize,
    pub last_error: Option<String>,
    pub plugin_health: HashMap<String, PluginHealth>,
}

impl Default for SystemHealth {
    fn default() -> Self {
        Self {
            status: HealthStatus::Healthy,
            uptime: std::time::Duration::from_secs(0),
            memory_usage: None,
            active_connections: 0,
            last_error: None,
            plugin_health: HashMap::new(),
        }
    }
}

/// Overall system health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,
    Warning,
    Error,
    Critical,
}

/// Plugin-specific health information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginHealth {
    pub status: HealthStatus,
    pub last_heartbeat: SystemTime,
    pub error_count: u32,
    pub last_error: Option<String>,
}
