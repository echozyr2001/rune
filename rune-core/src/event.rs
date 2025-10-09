//! Event system for decoupled communication between components

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

use crate::error::{Result, RuneError};

/// Core trait for all events in the system
#[async_trait]
pub trait Event: Send + Sync + Clone + std::fmt::Debug + 'static {
    /// Get the event type identifier
    fn event_type(&self) -> &str;

    /// Get the event timestamp
    fn timestamp(&self) -> SystemTime;

    /// Get event metadata
    fn metadata(&self) -> HashMap<String, String> {
        HashMap::new()
    }
}

/// Event bus for publishing and subscribing to events
#[async_trait]
pub trait EventBus: Send + Sync {
    /// Publish a system event to all subscribers
    async fn publish_system_event(&self, event: SystemEvent) -> Result<()>;

    /// Subscribe to system events
    async fn subscribe_system_events(
        &self,
        handler: Arc<dyn SystemEventHandler>,
    ) -> Result<SubscriptionId>;

    /// Unsubscribe from events
    async fn unsubscribe(&self, id: SubscriptionId) -> Result<()>;

    /// Get the number of active subscriptions
    async fn subscription_count(&self) -> usize;
}

/// Handler for system events specifically
#[async_trait]
pub trait SystemEventHandler: Send + Sync {
    /// Handle a system event
    async fn handle_system_event(&self, event: &SystemEvent) -> Result<()>;

    /// Get handler name for debugging
    fn handler_name(&self) -> &str {
        "UnnamedSystemEventHandler"
    }
}

/// Handler for processing events
#[async_trait]
pub trait EventHandler<T: Event>: Send + Sync {
    /// Handle an incoming event
    async fn handle_event(&self, event: &T) -> Result<()>;

    /// Get handler name for debugging
    fn handler_name(&self) -> &str {
        "UnnamedHandler"
    }
}

/// Unique identifier for event subscriptions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriptionId(pub Uuid);

impl SubscriptionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

/// System events that can occur during operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SystemEvent {
    /// File system change detected
    FileChanged {
        path: PathBuf,
        change_type: ChangeType,
        timestamp: SystemTime,
    },
    /// Client connected to the system
    ClientConnected {
        client_id: Uuid,
        info: ClientInfo,
        timestamp: SystemTime,
    },
    /// Client disconnected from the system
    ClientDisconnected {
        client_id: Uuid,
        timestamp: SystemTime,
    },
    /// Plugin was loaded
    PluginLoaded {
        plugin_name: String,
        version: String,
        timestamp: SystemTime,
    },
    /// Plugin was unloaded
    PluginUnloaded {
        plugin_name: String,
        timestamp: SystemTime,
    },
    /// Theme was changed
    ThemeChanged {
        theme_name: String,
        timestamp: SystemTime,
    },
    /// Content rendering completed
    RenderComplete {
        content_hash: String,
        duration: Duration,
        timestamp: SystemTime,
    },
    /// System error occurred
    Error {
        source: String,
        message: String,
        severity: ErrorSeverity,
        timestamp: SystemTime,
    },
}

#[async_trait]
impl Event for SystemEvent {
    fn event_type(&self) -> &str {
        match self {
            SystemEvent::FileChanged { .. } => "file_changed",
            SystemEvent::ClientConnected { .. } => "client_connected",
            SystemEvent::ClientDisconnected { .. } => "client_disconnected",
            SystemEvent::PluginLoaded { .. } => "plugin_loaded",
            SystemEvent::PluginUnloaded { .. } => "plugin_unloaded",
            SystemEvent::ThemeChanged { .. } => "theme_changed",
            SystemEvent::RenderComplete { .. } => "render_complete",
            SystemEvent::Error { .. } => "error",
        }
    }

    fn timestamp(&self) -> SystemTime {
        match self {
            SystemEvent::FileChanged { timestamp, .. } => *timestamp,
            SystemEvent::ClientConnected { timestamp, .. } => *timestamp,
            SystemEvent::ClientDisconnected { timestamp, .. } => *timestamp,
            SystemEvent::PluginLoaded { timestamp, .. } => *timestamp,
            SystemEvent::PluginUnloaded { timestamp, .. } => *timestamp,
            SystemEvent::ThemeChanged { timestamp, .. } => *timestamp,
            SystemEvent::RenderComplete { timestamp, .. } => *timestamp,
            SystemEvent::Error { timestamp, .. } => *timestamp,
        }
    }
}

/// Types of file system changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChangeType {
    Created,
    Modified,
    Deleted,
    Renamed { from: PathBuf, to: PathBuf },
}

/// Information about connected clients
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub user_agent: Option<String>,
    pub ip_address: String,
    pub connected_at: SystemTime,
}

/// Error severity levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ErrorSeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// In-memory implementation of the event bus
pub struct InMemoryEventBus {
    subscriptions: RwLock<HashMap<SubscriptionId, Box<dyn Send + Sync>>>,
    sender: broadcast::Sender<SystemEvent>,
}

impl InMemoryEventBus {
    /// Create a new in-memory event bus
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(1000);

        Self {
            subscriptions: RwLock::new(HashMap::new()),
            sender,
        }
    }
}

#[async_trait]
impl EventBus for InMemoryEventBus {
    async fn publish_system_event(&self, event: SystemEvent) -> Result<()> {
        self.sender
            .send(event.clone())
            .map_err(|e| RuneError::EventBus(format!("Failed to publish event: {}", e)))?;

        tracing::debug!("Published system event: {}", event.event_type());
        Ok(())
    }

    async fn subscribe_system_events(
        &self,
        _handler: Arc<dyn SystemEventHandler>,
    ) -> Result<SubscriptionId> {
        // Simplified implementation - in a real system we'd store the handler
        // and route events to it based on type
        let id = SubscriptionId::new();

        tracing::debug!("Created system event subscription: {:?}", id);
        Ok(id)
    }

    async fn unsubscribe(&self, id: SubscriptionId) -> Result<()> {
        let mut subscriptions = self.subscriptions.write().await;
        subscriptions.remove(&id);

        tracing::debug!("Removed subscription: {:?}", id);
        Ok(())
    }

    async fn subscription_count(&self) -> usize {
        self.subscriptions.read().await.len()
    }
}

impl Default for InMemoryEventBus {
    fn default() -> Self {
        Self::new()
    }
}
