//! Simple test for the content renderer plugin

use rune_core::{RenderContext, RendererRegistry};
use rune_renderer::MarkdownRenderer;
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting simple renderer test...");

    // Create a renderer registry
    let registry = Arc::new(RendererRegistry::new());
    println!("Created registry");

    // Register the markdown renderer
    let markdown_renderer = Box::new(MarkdownRenderer::new());
    println!("Created markdown renderer");

    registry.register_renderer(markdown_renderer).await?;
    println!("Registered renderer");

    // Create a render context
    let context = RenderContext::new(
        PathBuf::from("test.md"),
        PathBuf::from("."),
        "default".to_string(),
    );
    println!("Created context");

    // Simple markdown content
    let markdown_content = "# Hello\n\nThis is **bold** text.";
    println!("Rendering content...");

    // Render the content
    match registry.render_content(markdown_content, &context).await {
        Ok(result) => {
            println!("Success! HTML length: {}", result.html.len());
            println!("Renderer: {}", result.metadata.renderer_name);
        }
        Err(e) => {
            println!("Error: {}", e);
        }
    }

    println!("Test complete!");
    Ok(())
}
