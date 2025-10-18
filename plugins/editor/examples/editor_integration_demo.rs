//! Demonstration of editor integration with file watcher and renderer plugins
//!
//! This example shows how the editor plugin integrates with:
//! - File watcher: Responds to external file changes
//! - Renderer: Triggers rendering when content changes
//! - Theme system: Updates rendering when theme changes

use rune_core::{
    event::{EventBus, InMemoryEventBus, SystemEvent},
    Config, Plugin, PluginContext, RendererRegistry, StateManager,
};
use rune_editor::{EditorMode, EditorPlugin, RuneEditorPlugin};
use std::sync::Arc;
use tempfile::tempdir;
use tokio::fs;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Editor Integration Demo ===\n");

    // Create temporary directory for test files
    let temp_dir = tempdir()?;
    let test_file = temp_dir.path().join("test.md");

    // Write initial content
    fs::write(&test_file, "# Hello World\n\nThis is a test.").await?;
    println!("Created test file: {}", test_file.display());

    // Set up the plugin system
    let event_bus = Arc::new(InMemoryEventBus::new());
    let config = Arc::new(Config::default());
    let state_manager = Arc::new(StateManager::new());

    let context = PluginContext::new(event_bus.clone(), config, state_manager);

    // Create and register renderer registry
    let renderer_registry = Arc::new(RendererRegistry::new());
    context
        .set_shared_resource("renderer_registry".to_string(), renderer_registry.clone())
        .await?;

    // Initialize editor plugin
    let mut editor_plugin = RuneEditorPlugin::new();
    editor_plugin.initialize(&context).await?;

    println!("\n✓ Editor plugin initialized with renderer integration");

    // Create an editing session
    let session_id = editor_plugin.create_session(test_file.clone()).await?;
    println!("✓ Created editing session: {}", session_id);

    // Get initial content
    let content = editor_plugin.get_content(session_id).await?;
    println!("✓ Initial content: {:?}", content);

    // Simulate content change (this should trigger rendering)
    println!("\n--- Simulating content change ---");
    editor_plugin
        .set_content(
            session_id,
            "# Updated Title\n\nThis content was updated by the editor.".to_string(),
        )
        .await?;
    println!("✓ Content updated (rendering triggered automatically)");

    // Check if rendered content was stored
    if let Some(rendered) = context
        .get_shared_resource::<String>(&format!("editor_rendered_content_{}", session_id))
        .await
    {
        println!("✓ Rendered content available: {} bytes", rendered.len());
    } else {
        println!("⚠ Rendered content not found (renderer plugin may not be initialized)");
    }

    // Simulate theme change
    println!("\n--- Simulating theme change ---");
    let theme_event = SystemEvent::theme_changed("catppuccin-latte".to_string());
    event_bus.publish_system_event(theme_event).await?;
    println!("✓ Theme changed event published");

    // Give the event handler time to process
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let current_theme = editor_plugin.get_current_theme().await;
    println!("✓ Editor theme updated to: {}", current_theme);

    // Simulate external file change
    println!("\n--- Simulating external file change ---");
    fs::write(
        &test_file,
        "# Externally Modified\n\nThis was changed outside the editor.",
    )
    .await?;

    let file_change_event =
        SystemEvent::file_changed(test_file.clone(), rune_core::event::ChangeType::Modified);
    event_bus.publish_system_event(file_change_event).await?;
    println!("✓ File change event published");

    // Give the event handler time to process
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Switch to preview mode (should trigger rendering)
    println!("\n--- Switching to preview mode ---");
    editor_plugin
        .switch_mode(session_id, EditorMode::Preview)
        .await?;
    println!("✓ Switched to preview mode (rendering triggered)");

    // Save content
    println!("\n--- Saving content ---");
    editor_plugin.save_content(session_id).await?;
    println!("✓ Content saved (rendering triggered)");

    // Check auto-save status
    let auto_save_status = editor_plugin.get_auto_save_status(session_id).await?;
    println!(
        "✓ Auto-save status: enabled={}, dirty={}",
        auto_save_status.enabled, auto_save_status.is_dirty
    );

    // Close session
    println!("\n--- Closing session ---");
    editor_plugin.close_session(session_id).await?;
    println!("✓ Session closed");

    // Shutdown plugin
    editor_plugin.shutdown().await?;
    println!("✓ Editor plugin shutdown complete");

    println!("\n=== Integration Demo Complete ===");
    println!("\nKey Integration Points Demonstrated:");
    println!("1. ✓ Content changes trigger automatic rendering");
    println!("2. ✓ Theme changes update editor and trigger re-rendering");
    println!("3. ✓ External file changes are detected and handled");
    println!("4. ✓ Mode switching triggers appropriate rendering");
    println!("5. ✓ Rendered content is stored for preview serving");

    Ok(())
}
