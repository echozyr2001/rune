//! Pipeline rendering demonstration

use rune_core::{RenderContext, RendererRegistry};
use rune_renderer::{MarkdownRenderer, MermaidRenderer};
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("=== Renderer Pipeline Demo ===\n");

    // Create a renderer registry
    let registry = Arc::new(RendererRegistry::new());

    // Register renderers in order
    let markdown_renderer = Box::new(MarkdownRenderer::new());
    registry.register_renderer(markdown_renderer).await?;

    let mermaid_renderer = Box::new(MermaidRenderer::new());
    registry.register_renderer(mermaid_renderer).await?;

    // Create render context
    let context = RenderContext::new(
        PathBuf::from("pipeline_test.md"),
        PathBuf::from("."),
        "default".to_string(),
    );

    // Test content with multiple mermaid diagrams
    let test_content = r#"
# Pipeline Rendering Test

This document tests the rendering pipeline with multiple Mermaid diagrams.

## Flow Chart

```mermaid
flowchart TD
    A[Christmas] -->|Get money| B(Go shopping)
    B --> C{Let me think}
    C -->|One| D[Laptop]
    C -->|Two| E[iPhone]
    C -->|Three| F[fa:fa-car Car]
```

## Sequence Diagram

```mermaid
sequenceDiagram
    participant Alice
    participant Bob
    Alice->>John: Hello John, how are you?
    loop Healthcheck
        John->>John: Fight against hypochondria
    end
    Note right of John: Rational thoughts <br/>prevail!
    John-->>Alice: Great!
    John->>Bob: How about you?
    Bob-->>John: Jolly good!
```

## Class Diagram

```mermaid
classDiagram
    Animal <|-- Duck
    Animal <|-- Fish
    Animal <|-- Zebra
    Animal : +int age
    Animal : +String gender
    Animal: +isMammal()
    Animal: +mate()
    class Duck{
        +String beakColor
        +swim()
        +quack()
    }
    class Fish{
        -int sizeInFeet
        -canEat()
    }
    class Zebra{
        +bool is_wild
        +run()
    }
```

## Regular Code Block (should not be processed by Mermaid renderer)

```javascript
function hello() {
    console.log("This is regular JavaScript code");
    return "Hello, World!";
}
```

## Summary

This document contains **3 Mermaid diagrams** and **1 regular code block**.
The pipeline should process markdown first, then Mermaid diagrams.
"#;

    println!("Testing pipeline rendering...\n");

    match registry.render_with_pipeline(test_content, &context).await {
        Ok(result) => {
            println!("✅ Pipeline rendering successful!");
            println!("Renderer chain: {}", result.metadata.renderer_name);
            println!(
                "Total render time: {}ms",
                result.metadata.render_time_ms.unwrap_or(0)
            );
            println!(
                "Has interactive content: {}",
                result.has_interactive_content
            );
            println!("Assets required: {}", result.assets.len());

            for asset in &result.assets {
                println!("  📦 {:?}: {}", asset.asset_type, asset.url);
            }

            println!("\n📊 Pipeline Metadata:");
            for (key, value) in &result.metadata.custom_metadata {
                println!("  {}: {}", key, value);
            }

            // Count mermaid divs in output
            let mermaid_count = result.html.matches(r#"<div class="mermaid">"#).count();
            println!("\n🎨 Mermaid diagrams converted: {}", mermaid_count);

            // Check if regular code blocks are preserved
            let code_blocks = result.html.matches("<pre><code").count();
            println!("📝 Regular code blocks preserved: {}", code_blocks);

            // Show structure of rendered HTML
            println!("\n📄 HTML Structure Analysis:");
            if result.html.contains(r#"<div class="mermaid">"#) {
                println!("  ✅ Contains Mermaid diagram divs");
            }
            if result.html.contains("<h1>") {
                println!("  ✅ Contains markdown headers");
            }
            if result
                .html
                .contains("<pre><code class=\"language-javascript\">")
            {
                println!("  ✅ Contains regular code blocks");
            }

            // Save output for inspection
            std::fs::write("pipeline_output.html", &result.html)?;
            println!("\n💾 Full HTML output saved to 'pipeline_output.html'");
        }
        Err(e) => {
            eprintln!("❌ Pipeline rendering failed: {}", e);
        }
    }

    // Compare with single renderer
    println!("\n=== Comparison with Single Renderer ===");
    match registry.render_content(test_content, &context).await {
        Ok(result) => {
            println!("Single renderer: {}", result.metadata.renderer_name);
            println!(
                "Render time: {}ms",
                result.metadata.render_time_ms.unwrap_or(0)
            );
            println!("Has interactive: {}", result.has_interactive_content);
            println!("Assets: {}", result.assets.len());

            // This should only process markdown, not mermaid
            let mermaid_code_blocks = result.html.matches(r#"class="language-mermaid""#).count();
            let mermaid_divs = result.html.matches(r#"<div class="mermaid">"#).count();

            println!("Mermaid code blocks (unprocessed): {}", mermaid_code_blocks);
            println!("Mermaid divs (processed): {}", mermaid_divs);
        }
        Err(e) => {
            eprintln!("Single rendering failed: {}", e);
        }
    }

    Ok(())
}
