//! Basic server plugin usage example
//!
//! This example demonstrates how to use the server plugin interface
//! to create a simple web server with custom handlers.

use rune_core::{
    config::Config,
    event::InMemoryEventBus,
    plugin::{PluginContext, PluginRegistry},
    state::StateManager,
};
use rune_server::{
    handlers::{LiveReloadHandler, MarkdownHandler, RawMarkdownHandler, StaticHandler},
    ServerConfig, ServerPlugin,
};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::fs;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("🚀 Starting server plugin example");

    // Create temporary directory for demo files
    let temp_dir = TempDir::new()?;
    let base_path = temp_dir.path().to_path_buf();

    // Create a sample markdown file
    let markdown_file = base_path.join("example.md");
    fs::write(
        &markdown_file,
        r#"# Hello, World!

This is a **sample markdown file** being served by the Rune server plugin.

## Features

- Static file serving
- Markdown rendering
- WebSocket live reload
- Modular handler system

## Code Example

```rust
fn main() {
    println!("Hello from Rune!");
}
```

> This content is served dynamically by the server plugin!
"#,
    )
    .await?;

    // Create a sample CSS file
    let css_file = base_path.join("style.css");
    fs::write(
        &css_file,
        r#"body {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
    max-width: 800px;
    margin: 0 auto;
    padding: 2rem;
    line-height: 1.6;
}

h1, h2 {
    color: #333;
}

code {
    background: #f5f5f5;
    padding: 0.2rem 0.4rem;
    border-radius: 2px;
    font-family: 'Monaco', 'Consolas', monospace;
}

pre {
    background: #f5f5f5;
    padding: 1rem;
    border-radius: 4px;
    overflow-x: auto;
}

blockquote {
    border-left: 4px solid #ddd;
    margin: 0;
    padding-left: 1rem;
    color: #666;
}
"#,
    )
    .await?;

    // Create system components
    let config = Arc::new(Config::default());
    let event_bus = Arc::new(InMemoryEventBus::new());
    let state_manager = Arc::new(StateManager::new());

    // Create plugin context
    let context = PluginContext::new(event_bus.clone(), config.clone(), state_manager.clone());

    // Create and initialize plugin registry
    let mut registry = PluginRegistry::new();
    registry.initialize(context.clone()).await?;

    // Create server plugin with custom configuration
    let server_config = ServerConfig {
        hostname: "127.0.0.1".to_string(),
        port: 3030,
        enable_cors: true,
        max_connections: Some(100),
        request_timeout_secs: Some(30),
        websocket_ping_interval_secs: Some(30),
    };

    let server_plugin = ServerPlugin::with_config(server_config);

    // Register the server plugin
    registry
        .register_plugin(Box::new(server_plugin), &context)
        .await?;

    // Get the handler registry from shared resources
    if let Some(handler_registry) = context
        .get_shared_resource::<rune_server::HandlerRegistry>("server_handler_registry")
        .await
    {
        println!("📝 Registering handlers...");

        // Register markdown handler for the root path
        let markdown_handler =
            Arc::new(MarkdownHandler::new("/".to_string(), markdown_file.clone()));
        handler_registry
            .register_http_handler(markdown_handler)
            .await?;

        // Register raw markdown handler
        let raw_handler = Arc::new(RawMarkdownHandler::new("/raw".to_string(), markdown_file));
        handler_registry.register_http_handler(raw_handler).await?;

        // Register static file handler for assets
        let static_handler = Arc::new(StaticHandler::new(base_path, "/static".to_string()));
        handler_registry
            .register_http_handler(static_handler)
            .await?;

        // Register WebSocket handler for live reload
        let ws_handler = Arc::new(LiveReloadHandler::new("/ws".to_string()));
        handler_registry
            .register_websocket_handler(ws_handler)
            .await?;

        println!("✅ All handlers registered successfully!");

        // List registered handlers
        let http_handlers = handler_registry.list_http_handlers().await;
        let ws_handlers = handler_registry.list_websocket_handlers().await;

        println!("\n📋 Registered HTTP handlers:");
        for (path, method, priority) in http_handlers {
            println!("  {} {} (priority: {})", method, path, priority);
        }

        println!("\n🔌 Registered WebSocket handlers:");
        for (path, priority) in ws_handlers {
            println!("  {} (priority: {})", path, priority);
        }

        println!("\n🌐 Server is running at:");
        println!("  📄 Markdown content: http://127.0.0.1:3030/");
        println!("  📝 Raw markdown: http://127.0.0.1:3030/raw");
        println!("  🎨 CSS file: http://127.0.0.1:3030/static/style.css");
        println!("  🔌 WebSocket: ws://127.0.0.1:3030/ws");
        println!("\n💡 Try opening these URLs in your browser!");
        println!("   Press Ctrl+C to stop the server");

        // Keep the server running
        tokio::signal::ctrl_c().await?;
        println!("\n🛑 Shutting down server...");
    } else {
        eprintln!("❌ Failed to get handler registry from server plugin");
    }

    // Shutdown the registry
    registry.shutdown().await?;

    println!("✅ Server shutdown complete");
    Ok(())
}
