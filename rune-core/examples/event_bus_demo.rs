//! Example demonstrating the event bus functionality

use rune_core::{
    Event, EventBus, EventFilter, EventHandler, ExtendedEventBus, InMemoryEventBus, SystemEvent,
    SystemEventHandler,
};
use std::sync::Arc;
use std::time::SystemTime;

// Example custom event
#[derive(Debug, Clone)]
struct CustomEvent {
    message: String,
    timestamp: SystemTime,
}

#[async_trait::async_trait]
impl Event for CustomEvent {
    fn event_type(&self) -> &str {
        "custom_event"
    }

    fn timestamp(&self) -> SystemTime {
        self.timestamp
    }
}

// Example event handler
struct LoggingHandler {
    name: String,
}

#[async_trait::async_trait]
impl EventHandler<CustomEvent> for LoggingHandler {
    async fn handle_event(&self, event: &CustomEvent) -> rune_core::Result<()> {
        println!("[{}] Received custom event: {}", self.name, event.message);
        Ok(())
    }

    fn handler_name(&self) -> &str {
        &self.name
    }
}

// Example system event handler
struct SystemEventLogger;

#[async_trait::async_trait]
impl SystemEventHandler for SystemEventLogger {
    async fn handle_system_event(&self, event: &SystemEvent) -> rune_core::Result<()> {
        println!("System event: {:?}", event);
        Ok(())
    }

    fn handler_name(&self) -> &str {
        "SystemEventLogger"
    }
}

// Example event filter
struct MessageFilter {
    keyword: String,
}

impl EventFilter<CustomEvent> for MessageFilter {
    fn should_handle(&self, event: &CustomEvent) -> bool {
        event.message.contains(&self.keyword)
    }

    fn filter_name(&self) -> &str {
        "MessageFilter"
    }
}

#[tokio::main]
async fn main() -> rune_core::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("Event Bus Demo");
    println!("==============");

    // Create event bus
    let event_bus = InMemoryEventBus::new();

    // Subscribe to custom events with different handlers
    let handler1 = Arc::new(LoggingHandler {
        name: "Handler1".to_string(),
    });
    let handler2 = Arc::new(LoggingHandler {
        name: "Handler2".to_string(),
    });

    // Subscribe without filter
    let _sub1 = event_bus.subscribe(handler1, None).await?;

    // Subscribe with filter
    let filter = Box::new(MessageFilter {
        keyword: "important".to_string(),
    });
    let _sub2 = event_bus.subscribe(handler2, Some(filter)).await?;

    // Subscribe to system events
    let system_handler = Arc::new(SystemEventLogger);
    let _sub3 = event_bus.subscribe_system_events(system_handler).await?;

    println!(
        "Subscriptions created: {}",
        event_bus.subscription_count().await
    );

    // Publish some custom events
    println!("\nPublishing custom events:");

    let event1 = CustomEvent {
        message: "Hello world!".to_string(),
        timestamp: SystemTime::now(),
    };
    event_bus.publish(event1).await?;

    let event2 = CustomEvent {
        message: "This is important news!".to_string(),
        timestamp: SystemTime::now(),
    };
    event_bus.publish(event2).await?;

    let event3 = CustomEvent {
        message: "Just a regular message".to_string(),
        timestamp: SystemTime::now(),
    };
    event_bus.publish(event3).await?;

    // Publish a system event
    println!("\nPublishing system event:");
    let system_event = SystemEvent::PluginLoaded {
        plugin_name: "test-plugin".to_string(),
        version: "1.0.0".to_string(),
        timestamp: SystemTime::now(),
    };
    event_bus.publish_system_event(system_event).await?;

    println!("\nDemo completed successfully!");
    Ok(())
}
