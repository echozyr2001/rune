//! Basic usage example for the content renderer plugin

use rune_core::{RenderContext, RendererRegistry};
use rune_renderer::MarkdownRenderer;
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("Creating renderer registry...");
    // Create a renderer registry
    let registry = Arc::new(RendererRegistry::new());

    println!("Registering markdown renderer...");
    // Register the markdown renderer
    let markdown_renderer = Box::new(MarkdownRenderer::new());
    registry.register_renderer(markdown_renderer).await?;
    
    println!("Registry setup complete!");

    // Create a render context
    let context = RenderContext::new(
        PathBuf::from("example.md"),
        PathBuf::from("."),
        "default".to_string(),
    );

    // Sample markdown content
    let markdown_content = r#"
# Hello World

This is a **markdown** document with some features:

- Lists
- Code blocks
- Tables

## Code Example

```rust
fn main() {
    println!("Hello, world!");
}
```

## Mermaid Diagram

```mermaid
graph TD
    A[Start] --> B{Is it?}
    B -->|Yes| C[OK]
    B -->|No| D[End]
```

## Table

| Name | Age | City |
|------|-----|------|
| John | 30  | NYC  |
| Jane | 25  | LA   |
"#;

    println!("Starting to render content...");
    // Render the content
    match registry.render_content(markdown_content, &context).await {
        Ok(result) => {
            println!("Rendered HTML:");
            println!("{}", result.html);
            println!("\nMetadata:");
            println!("Renderer: {} v{}", result.metadata.renderer_name, result.metadata.renderer_version);
            if let Some(time) = result.metadata.render_time_ms {
                println!("Render time: {}ms", time);
            }
            println!("Has interactive content: {}", result.has_interactive_content);
            println!("Assets required: {}", result.assets.len());
            for asset in &result.assets {
                println!("  - {:?}: {}", asset.asset_type, asset.url);
            }
        }
        Err(e) => {
            eprintln!("Rendering failed: {}", e);
        }
    }

    Ok(())
}