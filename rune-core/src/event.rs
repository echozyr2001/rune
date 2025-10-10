//! Event system for decoupled communication between components

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::Result;

/// Event serialization utilities for persistence and debugging
pub mod serialization {
    use super::*;
    use std::io::Write;

    /// Serialize an event to JSON string
    pub fn serialize_event(event: &SystemEvent) -> Result<String> {
        serde_json::to_string(event).map_err(crate::error::RuneError::Json)
    }

    /// Serialize an event to pretty JSON string for debugging
    pub fn serialize_event_pretty(event: &SystemEvent) -> Result<String> {
        serde_json::to_string_pretty(event).map_err(crate::error::RuneError::Json)
    }

    /// Deserialize an event from JSON string
    pub fn deserialize_event(json: &str) -> Result<SystemEvent> {
        serde_json::from_str(json).map_err(crate::error::RuneError::Json)
    }

    /// Write event to a writer in JSON format
    pub fn write_event<W: Write>(writer: &mut W, event: &SystemEvent) -> Result<()> {
        let json = serialize_event(event)?;
        writer.write_all(json.as_bytes())?;
        writer.write_all(b"\n")?;
        Ok(())
    }

    /// Event batch serialization for efficient storage
    pub fn serialize_event_batch(events: &[SystemEvent]) -> Result<String> {
        serde_json::to_string(events).map_err(crate::error::RuneError::Json)
    }

    /// Deserialize a batch of events
    pub fn deserialize_event_batch(json: &str) -> Result<Vec<SystemEvent>> {
        serde_json::from_str(json).map_err(crate::error::RuneError::Json)
    }

    /// Format event for logging with timestamp
    pub fn format_event_for_log(event: &SystemEvent) -> String {
        let timestamp = event
            .timestamp()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        format!(
            "[{}] {}: {}",
            timestamp,
            event.event_type().to_uppercase(),
            event.description()
        )
    }

    /// Create a compact event representation for debugging
    pub fn event_debug_string(event: &SystemEvent) -> String {
        let metadata = event.metadata();
        let metadata_str = if metadata.is_empty() {
            String::new()
        } else {
            format!(
                " ({})",
                metadata
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };

        format!("{}{}", event.description(), metadata_str)
    }
}

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
/// This trait is object-safe by avoiding generic methods
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

/// Extended event bus trait with generic methods for type-safe event handling
/// This trait is not object-safe but provides the full API
#[async_trait]
pub trait ExtendedEventBus: EventBus {
    /// Publish any event that implements the Event trait
    async fn publish<T: Event>(&self, event: T) -> Result<()>;

    /// Subscribe to events of a specific type with optional filtering
    async fn subscribe<T: Event>(
        &self,
        handler: Arc<dyn EventHandler<T>>,
        filter: Option<Box<dyn EventFilter<T>>>,
    ) -> Result<SubscriptionId>;

    /// Get the number of subscriptions for a specific event type
    async fn subscription_count_for_type<T: Event>(&self) -> usize;
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

/// Filter for events to determine if they should be delivered to a handler
pub trait EventFilter<T: Event>: Send + Sync {
    /// Check if the event should be delivered to the handler
    fn should_handle(&self, event: &T) -> bool;

    /// Get filter name for debugging
    fn filter_name(&self) -> &str {
        "UnnamedFilter"
    }
}

/// Adapter to make SystemEventHandler work with the generic EventHandler interface
struct SystemEventHandlerAdapter {
    handler: Arc<dyn SystemEventHandler>,
}

#[async_trait]
impl EventHandler<SystemEvent> for SystemEventHandlerAdapter {
    async fn handle_event(&self, event: &SystemEvent) -> Result<()> {
        self.handler.handle_system_event(event).await
    }

    fn handler_name(&self) -> &str {
        self.handler.handler_name()
    }
}

/// Unique identifier for event subscriptions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriptionId(pub Uuid);

impl Default for SubscriptionId {
    fn default() -> Self {
        Self::new()
    }
}

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
    /// Plugin is loading
    PluginLoading {
        plugin_name: String,
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
    /// Plugin health check result
    PluginHealthCheck {
        plugin_name: String,
        status: crate::plugin::PluginHealthStatus,
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
            SystemEvent::PluginLoading { .. } => "plugin_loading",
            SystemEvent::PluginLoaded { .. } => "plugin_loaded",
            SystemEvent::PluginUnloaded { .. } => "plugin_unloaded",
            SystemEvent::PluginHealthCheck { .. } => "plugin_health_check",
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
            SystemEvent::PluginLoading { timestamp, .. } => *timestamp,
            SystemEvent::PluginLoaded { timestamp, .. } => *timestamp,
            SystemEvent::PluginUnloaded { timestamp, .. } => *timestamp,
            SystemEvent::PluginHealthCheck { timestamp, .. } => *timestamp,
            SystemEvent::ThemeChanged { timestamp, .. } => *timestamp,
            SystemEvent::RenderComplete { timestamp, .. } => *timestamp,
            SystemEvent::Error { timestamp, .. } => *timestamp,
        }
    }

    fn metadata(&self) -> HashMap<String, String> {
        let mut metadata = HashMap::new();

        match self {
            SystemEvent::FileChanged {
                path, change_type, ..
            } => {
                metadata.insert("path".to_string(), path.display().to_string());
                metadata.insert("change_type".to_string(), format!("{:?}", change_type));
            }
            SystemEvent::ClientConnected {
                client_id, info, ..
            } => {
                metadata.insert("client_id".to_string(), client_id.to_string());
                metadata.insert("ip_address".to_string(), info.ip_address.clone());
                if let Some(user_agent) = &info.user_agent {
                    metadata.insert("user_agent".to_string(), user_agent.clone());
                }
            }
            SystemEvent::ClientDisconnected { client_id, .. } => {
                metadata.insert("client_id".to_string(), client_id.to_string());
            }
            SystemEvent::PluginLoading { plugin_name, .. } => {
                metadata.insert("plugin_name".to_string(), plugin_name.clone());
            }
            SystemEvent::PluginLoaded {
                plugin_name,
                version,
                ..
            } => {
                metadata.insert("plugin_name".to_string(), plugin_name.clone());
                metadata.insert("version".to_string(), version.clone());
            }
            SystemEvent::PluginUnloaded { plugin_name, .. } => {
                metadata.insert("plugin_name".to_string(), plugin_name.clone());
            }
            SystemEvent::PluginHealthCheck {
                plugin_name,
                status,
                ..
            } => {
                metadata.insert("plugin_name".to_string(), plugin_name.clone());
                metadata.insert("health_status".to_string(), format!("{:?}", status));
            }
            SystemEvent::ThemeChanged { theme_name, .. } => {
                metadata.insert("theme_name".to_string(), theme_name.clone());
            }
            SystemEvent::RenderComplete {
                content_hash,
                duration,
                ..
            } => {
                metadata.insert("content_hash".to_string(), content_hash.clone());
                metadata.insert("duration_ms".to_string(), duration.as_millis().to_string());
            }
            SystemEvent::Error {
                source,
                message,
                severity,
                ..
            } => {
                metadata.insert("source".to_string(), source.clone());
                metadata.insert("message".to_string(), message.clone());
                metadata.insert("severity".to_string(), format!("{:?}", severity));
            }
        }

        metadata
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

impl SystemEvent {
    /// Create a new file changed event with current timestamp
    pub fn file_changed(path: PathBuf, change_type: ChangeType) -> Self {
        Self::FileChanged {
            path,
            change_type,
            timestamp: SystemTime::now(),
        }
    }

    /// Create a new client connected event with current timestamp
    pub fn client_connected(client_id: Uuid, info: ClientInfo) -> Self {
        Self::ClientConnected {
            client_id,
            info,
            timestamp: SystemTime::now(),
        }
    }

    /// Create a new client disconnected event with current timestamp
    pub fn client_disconnected(client_id: Uuid) -> Self {
        Self::ClientDisconnected {
            client_id,
            timestamp: SystemTime::now(),
        }
    }

    /// Create a new plugin loading event with current timestamp
    pub fn plugin_loading(plugin_name: String) -> Self {
        Self::PluginLoading {
            plugin_name,
            timestamp: SystemTime::now(),
        }
    }

    /// Create a new plugin loaded event with current timestamp
    pub fn plugin_loaded(plugin_name: String, version: String) -> Self {
        Self::PluginLoaded {
            plugin_name,
            version,
            timestamp: SystemTime::now(),
        }
    }

    /// Create a new plugin unloaded event with current timestamp
    pub fn plugin_unloaded(plugin_name: String) -> Self {
        Self::PluginUnloaded {
            plugin_name,
            timestamp: SystemTime::now(),
        }
    }

    /// Create a new plugin health check event with current timestamp
    pub fn plugin_health_check(
        plugin_name: String,
        status: crate::plugin::PluginHealthStatus,
    ) -> Self {
        Self::PluginHealthCheck {
            plugin_name,
            status,
            timestamp: SystemTime::now(),
        }
    }

    /// Create a new theme changed event with current timestamp
    pub fn theme_changed(theme_name: String) -> Self {
        Self::ThemeChanged {
            theme_name,
            timestamp: SystemTime::now(),
        }
    }

    /// Create a new render complete event with current timestamp
    pub fn render_complete(content_hash: String, duration: Duration) -> Self {
        Self::RenderComplete {
            content_hash,
            duration,
            timestamp: SystemTime::now(),
        }
    }

    /// Create a new error event with current timestamp
    pub fn error(source: String, message: String, severity: ErrorSeverity) -> Self {
        Self::Error {
            source,
            message,
            severity,
            timestamp: SystemTime::now(),
        }
    }

    /// Get a human-readable description of the event
    pub fn description(&self) -> String {
        match self {
            SystemEvent::FileChanged {
                path, change_type, ..
            } => {
                format!("File {} was {:?}", path.display(), change_type)
            }
            SystemEvent::ClientConnected {
                client_id, info, ..
            } => {
                format!("Client {} connected from {}", client_id, info.ip_address)
            }
            SystemEvent::ClientDisconnected { client_id, .. } => {
                format!("Client {} disconnected", client_id)
            }
            SystemEvent::PluginLoading { plugin_name, .. } => {
                format!("Plugin {} is loading", plugin_name)
            }
            SystemEvent::PluginLoaded {
                plugin_name,
                version,
                ..
            } => {
                format!("Plugin {} v{} loaded", plugin_name, version)
            }
            SystemEvent::PluginUnloaded { plugin_name, .. } => {
                format!("Plugin {} unloaded", plugin_name)
            }
            SystemEvent::PluginHealthCheck {
                plugin_name,
                status,
                ..
            } => {
                format!("Plugin {} health check: {:?}", plugin_name, status)
            }
            SystemEvent::ThemeChanged { theme_name, .. } => {
                format!("Theme changed to {}", theme_name)
            }
            SystemEvent::RenderComplete {
                content_hash,
                duration,
                ..
            } => {
                format!("Rendered content {} in {:?}", content_hash, duration)
            }
            SystemEvent::Error {
                source,
                message,
                severity,
                ..
            } => {
                format!("{:?} error from {}: {}", severity, source, message)
            }
        }
    }

    /// Check if this is an error event
    pub fn is_error(&self) -> bool {
        matches!(self, SystemEvent::Error { .. })
    }

    /// Check if this is a file system event
    pub fn is_file_event(&self) -> bool {
        matches!(self, SystemEvent::FileChanged { .. })
    }

    /// Check if this is a client event
    pub fn is_client_event(&self) -> bool {
        matches!(
            self,
            SystemEvent::ClientConnected { .. } | SystemEvent::ClientDisconnected { .. }
        )
    }

    /// Check if this is a plugin event
    pub fn is_plugin_event(&self) -> bool {
        matches!(
            self,
            SystemEvent::PluginLoading { .. }
                | SystemEvent::PluginLoaded { .. }
                | SystemEvent::PluginUnloaded { .. }
                | SystemEvent::PluginHealthCheck { .. }
        )
    }
}

/// Subscription information stored in the event bus
#[allow(dead_code)]
struct Subscription {
    id: SubscriptionId,
    event_type_id: TypeId,
    handler: Box<dyn Any + Send + Sync>,
    filter: Option<Box<dyn Any + Send + Sync>>,
}

/// In-memory implementation of the event bus with async message handling
pub struct InMemoryEventBus {
    subscriptions: RwLock<HashMap<SubscriptionId, Subscription>>,
    type_subscriptions: RwLock<HashMap<TypeId, Vec<SubscriptionId>>>,
}

impl InMemoryEventBus {
    /// Create a new in-memory event bus
    pub fn new() -> Self {
        Self {
            subscriptions: RwLock::new(HashMap::new()),
            type_subscriptions: RwLock::new(HashMap::new()),
        }
    }

    /// Route an event to all matching subscribers
    async fn route_event<T: Event>(&self, event: &T) -> Result<()> {
        let type_id = TypeId::of::<T>();

        // Get all subscription IDs for this event type
        let subscription_ids = {
            let type_subs = self.type_subscriptions.read().await;
            type_subs.get(&type_id).cloned().unwrap_or_default()
        };

        if subscription_ids.is_empty() {
            tracing::trace!("No subscribers for event type: {}", event.event_type());
            return Ok(());
        }

        // Process each subscription
        let subscriptions = self.subscriptions.read().await;
        let mut handlers_called = 0;

        for sub_id in subscription_ids {
            if let Some(subscription) = subscriptions.get(&sub_id) {
                // Downcast the handler to the correct type
                if let Some(handler) = subscription
                    .handler
                    .downcast_ref::<Arc<dyn EventHandler<T>>>()
                {
                    // Check filter if present
                    let should_handle = if let Some(filter_any) = &subscription.filter {
                        if let Some(filter) = filter_any.downcast_ref::<Box<dyn EventFilter<T>>>() {
                            filter.should_handle(event)
                        } else {
                            true // If filter downcast fails, allow the event
                        }
                    } else {
                        true // No filter means handle all events
                    };

                    if should_handle {
                        // Handle the event asynchronously
                        if let Err(e) = handler.handle_event(event).await {
                            tracing::error!(
                                "Handler {} failed to process event {}: {}",
                                handler.handler_name(),
                                event.event_type(),
                                e
                            );
                        } else {
                            handlers_called += 1;
                            tracing::trace!(
                                "Handler {} processed event {}",
                                handler.handler_name(),
                                event.event_type()
                            );
                        }
                    }
                }
            }
        }

        tracing::debug!(
            "Routed event {} to {} handlers",
            event.event_type(),
            handlers_called
        );

        Ok(())
    }
}

#[async_trait]
impl EventBus for InMemoryEventBus {
    async fn publish_system_event(&self, event: SystemEvent) -> Result<()> {
        self.publish(event).await
    }

    async fn subscribe_system_events(
        &self,
        handler: Arc<dyn SystemEventHandler>,
    ) -> Result<SubscriptionId> {
        let adapter = SystemEventHandlerAdapter { handler };
        self.subscribe(Arc::new(adapter), None).await
    }

    async fn unsubscribe(&self, id: SubscriptionId) -> Result<()> {
        // Remove from main subscriptions
        let subscription = {
            let mut subscriptions = self.subscriptions.write().await;
            subscriptions.remove(&id)
        };

        if let Some(subscription) = subscription {
            // Remove from type index
            let mut type_subs = self.type_subscriptions.write().await;
            if let Some(ids) = type_subs.get_mut(&subscription.event_type_id) {
                ids.retain(|&sub_id| sub_id != id);
                if ids.is_empty() {
                    type_subs.remove(&subscription.event_type_id);
                }
            }

            tracing::debug!("Removed subscription: {:?}", id);
        } else {
            tracing::warn!("Attempted to remove non-existent subscription: {:?}", id);
        }

        Ok(())
    }

    async fn subscription_count(&self) -> usize {
        self.subscriptions.read().await.len()
    }
}

#[async_trait]
impl ExtendedEventBus for InMemoryEventBus {
    async fn publish<T: Event>(&self, event: T) -> Result<()> {
        tracing::debug!("Publishing event: {}", event.event_type());

        // Route the event to all matching subscribers
        self.route_event(&event).await?;

        Ok(())
    }

    async fn subscribe<T: Event>(
        &self,
        handler: Arc<dyn EventHandler<T>>,
        filter: Option<Box<dyn EventFilter<T>>>,
    ) -> Result<SubscriptionId> {
        let id = SubscriptionId::new();
        let type_id = TypeId::of::<T>();

        let subscription = Subscription {
            id,
            event_type_id: type_id,
            handler: Box::new(handler.clone()),
            filter: filter.map(|f| Box::new(f) as Box<dyn Any + Send + Sync>),
        };

        // Store the subscription
        {
            let mut subscriptions = self.subscriptions.write().await;
            subscriptions.insert(id, subscription);
        }

        // Add to type index
        {
            let mut type_subs = self.type_subscriptions.write().await;
            type_subs.entry(type_id).or_default().push(id);
        }

        tracing::debug!(
            "Created subscription {:?} for handler {} on type {}",
            id,
            handler.handler_name(),
            std::any::type_name::<T>()
        );

        Ok(id)
    }

    async fn subscription_count_for_type<T: Event>(&self) -> usize {
        let type_id = TypeId::of::<T>();
        let type_subs = self.type_subscriptions.read().await;
        type_subs.get(&type_id).map(|ids| ids.len()).unwrap_or(0)
    }
}

impl Default for InMemoryEventBus {
    fn default() -> Self {
        Self::new()
    }
}
