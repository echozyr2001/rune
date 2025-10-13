//! Test theme integration with the renderer system

use rune_core::{
    config::Config,
    event::{EventBus, InMemoryEventBus, SystemEvent},
    plugin::{Plugin, PluginContext},
    renderer::RenderContext,
    state::StateManager,
};
use rune_renderer::RendererPlugin;
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("🎨 Testing theme integration with renderer system");

    // Create event bus
    let event_bus = Arc::new(InMemoryEventBus::new());

    // Create plugin context
    let config = Arc::new(Config::default());
    let state_manager = Arc::new(StateManager::new());
    let context = PluginContext::new(event_bus.clone(), config, state_manager);

    // Create and initialize renderer plugin
    let mut renderer_plugin = RendererPlugin::new();
    renderer_plugin.initialize(&context).await?;

    // Get the renderer registry
    let registry = renderer_plugin.registry().unwrap();

    // Test markdown rendering with theme
    let markdown_content = r#"
# Theme Integration Test

This is a test of the theme-aware rendering system.

## Features

- **Theme-aware rendering**: Content is processed with theme information
- **Event-driven updates**: Theme changes trigger re-rendering
- **Pipeline processing**: Multiple renderers work together

```rust
fn main() {
    println!("Hello, themed world!");
}
```

```mermaid
graph TD
    A[Markdown] --> B[Theme-Aware Renderer]
    B --> C[Themed HTML]
    D[Theme Change Event] --> B
```
"#;

    // Create render context with theme
    let context = RenderContext::new(
        PathBuf::from("test.md"),
        PathBuf::from("."),
        "catppuccin-mocha".to_string(),
    );

    println!("📝 Rendering markdown with theme: {}", context.theme);

    // Render using pipeline (includes theme processing)
    let result = registry
        .render_with_pipeline(markdown_content, &context)
        .await?;

    println!("✅ Rendered {} characters of HTML", result.html.len());
    println!("🔧 Renderer: {}", result.metadata.renderer_name);
    println!("⏱️  Render time: {:?}ms", result.metadata.render_time_ms);
    println!(
        "🎯 Has interactive content: {}",
        result.has_interactive_content
    );
    println!("📦 Assets required: {}", result.assets.len());

    // Print metadata
    if !result.metadata.custom_metadata.is_empty() {
        println!("📊 Custom metadata:");
        for (key, value) in &result.metadata.custom_metadata {
            println!("   {}: {}", key, value);
        }
    }

    // Test theme change event
    println!("\n🔄 Testing theme change event...");

    let theme_event = SystemEvent::theme_changed("catppuccin-latte".to_string());
    event_bus.publish_system_event(theme_event).await?;

    // Wait a moment for event processing
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Render again with new theme context
    let new_context = RenderContext::new(
        PathBuf::from("test.md"),
        PathBuf::from("."),
        "catppuccin-latte".to_string(),
    );

    let new_result = registry
        .render_with_pipeline(markdown_content, &new_context)
        .await?;

    println!("✅ Re-rendered with new theme: {}", new_context.theme);
    println!("🔧 Renderer: {}", new_result.metadata.renderer_name);

    // Check if theme metadata was applied
    if let Some(applied_theme) = new_result.metadata.custom_metadata.get("applied_theme") {
        println!("🎨 Applied theme: {}", applied_theme);
    }

    println!("\n🎉 Theme integration test completed successfully!");

    Ok(())
}
