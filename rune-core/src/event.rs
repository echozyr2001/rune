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
