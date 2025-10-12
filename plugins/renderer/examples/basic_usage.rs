//! Basic usage example for the content renderer plugin with pipeline support

use rune_core::{RenderContext, RendererRegistry};
use rune_renderer::{MarkdownRenderer, MermaidRenderer};
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("Creating renderer registry...");
    // Create a renderer registry
    let registry = Arc::new(RendererRegistry::new());

    println!("Registering renderers...");
    // Register the markdown renderer
    let markdown_renderer = Box::new(MarkdownRenderer::new());
    registry.register_renderer(markdown_renderer).await?;

    // Register the mermaid renderer
    let mermaid_renderer = Box::new(MermaidRenderer::new());
    registry.register_renderer(mermaid_renderer).await?;

    println!("Registry setup complete!");

    // Create a render context
    let context = RenderContext::new(
        PathBuf::from("example.md"),
        PathBuf::from("."),
        "default".to_string(),
    );

    // Sample markdown content with mermaid
    let markdown_content = r#"
# Hello World

This is a **markdown** document with some features:

- Lists
- Code blocks
- Tables
- Mermaid diagrams

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

## Another Mermaid Diagram

```mermaid
sequenceDiagram
    participant A as Alice
    participant B as Bob
    A->>B: Hello Bob, how are you?
    B-->>A: Great!
```

## Table

| Name | Age | City |
|------|-----|------|
| John | 30  | NYC  |
| Jane | 25  | LA   |
"#;

    println!("=== Single Renderer Test ===");
    // Test single renderer
    match registry.render_content(markdown_content, &context).await {
        Ok(result) => {
            println!("Single renderer result:");
            println!(
                "Renderer: {} v{}",
                result.metadata.renderer_name, result.metadata.renderer_version
            );
            if let Some(time) = result.metadata.render_time_ms {
                println!("Render time: {}ms", time);
            }
            println!(
                "Has interactive content: {}",
                result.has_interactive_content
            );
            println!("Assets required: {}", result.assets.len());
            for asset in &result.assets {
                println!("  - {:?}: {}", asset.asset_type, asset.url);
            }
        }
        Err(e) => {
            eprintln!("Single rendering failed: {}", e);
        }
    }

    println!("\n=== Pipeline Renderer Test ===");
    // Test pipeline rendering
    match registry
        .render_with_pipeline(markdown_content, &context)
        .await
    {
        Ok(result) => {
            println!("Pipeline renderer result:");
            println!(
                "Renderer: {} v{}",
                result.metadata.renderer_name, result.metadata.renderer_version
            );
            if let Some(time) = result.metadata.render_time_ms {
                println!("Render time: {}ms", time);
            }
            println!(
                "Has interactive content: {}",
                result.has_interactive_content
            );
            println!("Assets required: {}", result.assets.len());
            for asset in &result.assets {
                println!("  - {:?}: {}", asset.asset_type, asset.url);
            }

            println!("\nCustom metadata:");
            for (key, value) in &result.metadata.custom_metadata {
                println!("  {}: {}", key, value);
            }

            // Show a snippet of the rendered HTML
            let html_snippet = if result.html.len() > 500 {
                format!("{}...", &result.html[..500])
            } else {
                result.html.clone()
            };
            println!("\nRendered HTML (snippet):");
            println!("{}", html_snippet);
        }
        Err(e) => {
            eprintln!("Pipeline rendering failed: {}", e);
        }
    }

    println!("\n=== Registry Information ===");
    let renderers = registry.list_renderers().await;
    println!("Registered renderers: {:?}", renderers);

    for renderer_name in renderers {
        if let Some(info) = registry.get_renderer_info(&renderer_name).await {
            println!("Renderer '{}' info:", renderer_name);
            println!("  Version: {}", info.renderer_version);
            println!("  Features: {:?}", info.custom_metadata.get("features"));
        }
    }

    Ok(())
}
