//! Content renderer plugin for Rune

use async_trait::async_trait;
use rune_core::{
    Asset, AssetType, ContentRenderer, Plugin, PluginContext, PluginStatus, RenderContext,
    RenderMetadata, RenderResult, RendererRegistry, Result, RuneError,
};

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

/// Markdown content renderer implementation
pub struct MarkdownRenderer {
    name: String,
    version: String,
    status: PluginStatus,
}

impl MarkdownRenderer {
    /// Create a new markdown renderer
    pub fn new() -> Self {
        Self {
            name: "markdown-renderer".to_string(),
            version: "0.1.0".to_string(),
            status: PluginStatus::Loading,
        }
    }

    /// Convert markdown content to HTML
    fn markdown_to_html(&self, content: &str, _context: &RenderContext) -> Result<RenderResult> {
        let start_time = Instant::now();

        // Create GFM options with HTML rendering enabled
        let mut options = markdown::Options::gfm();
        options.compile.allow_dangerous_html = true;

        let html_body = markdown::to_html_with_options(content, &options)
            .map_err(|e| RuneError::Plugin(format!("Markdown parsing failed: {}", e)))?;

        // Check if the HTML contains mermaid code blocks
        let has_mermaid = html_body.contains(r#"class="language-mermaid""#);

        let mut assets = Vec::new();
        let mut custom_metadata = HashMap::new();

        // Add Mermaid asset if needed
        if has_mermaid {
            assets.push(Asset {
                asset_type: AssetType::JavaScript,
                url: "/mermaid.min.js".to_string(),
                is_critical: true,
                integrity: None,
            });
            custom_metadata.insert(
                "has_mermaid".to_string(),
                serde_json::Value::Bool(true),
            );
        }

        // Create metadata
        let metadata = RenderMetadata {
            renderer_name: self.name.clone(),
            renderer_version: self.version.clone(),
            render_time_ms: Some(start_time.elapsed().as_millis() as u64),
            content_hash: Some(format!("{:x}", content.len() as u64)),
            custom_metadata,
        };

        let mut result = RenderResult::new(html_body)
            .with_metadata(metadata);
        
        if has_mermaid {
            result = result.with_interactive_content();
        }

        // Add all assets
        let result = assets.into_iter().fold(result, |acc, asset| acc.with_asset(asset));

        Ok(result)
    }
}

impl Default for MarkdownRenderer {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for MarkdownRenderer {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn dependencies(&self) -> Vec<&str> {
        vec![] // No dependencies for the markdown renderer
    }

    async fn initialize(&mut self, _context: &PluginContext) -> Result<()> {
        tracing::info!("Initializing markdown renderer plugin");
        self.status = PluginStatus::Active;
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        tracing::info!("Shutting down markdown renderer plugin");
        self.status = PluginStatus::Stopped;
        Ok(())
    }

    fn status(&self) -> PluginStatus {
        self.status.clone()
    }

    fn provided_services(&self) -> Vec<&str> {
        vec!["markdown-rendering", "content-rendering"]
    }
}

#[async_trait]
impl ContentRenderer for MarkdownRenderer {
    fn can_render(&self, content_type: &str) -> bool {
        matches!(content_type, "text/markdown" | "text/x-markdown")
    }

    async fn render(&self, content: &str, context: &RenderContext) -> Result<RenderResult> {
        self.markdown_to_html(content, context)
    }

    fn supported_extensions(&self) -> Vec<&str> {
        vec!["md", "markdown"]
    }

    fn priority(&self) -> u32 {
        200 // High priority for markdown files
    }

    fn renderer_metadata(&self) -> RenderMetadata {
        let mut custom_metadata = HashMap::new();
        custom_metadata.insert(
            "features".to_string(),
            serde_json::json!(["gfm", "tables", "code_blocks", "mermaid"]),
        );

        RenderMetadata {
            renderer_name: self.name.clone(),
            renderer_version: self.version.clone(),
            render_time_ms: None,
            content_hash: None,
            custom_metadata,
        }
    }
}

/// Main renderer plugin that manages all content renderers
pub struct RendererPlugin {
    name: String,
    version: String,
    status: PluginStatus,
    registry: Option<Arc<RendererRegistry>>,
}

impl RendererPlugin {
    /// Create a new renderer plugin
    pub fn new() -> Self {
        Self {
            name: "renderer".to_string(),
            version: "0.1.0".to_string(),
            status: PluginStatus::Loading,
            registry: None,
        }
    }

    /// Get the renderer registry
    pub fn registry(&self) -> Option<Arc<RendererRegistry>> {
        self.registry.clone()
    }
}

impl Default for RendererPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for RendererPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn dependencies(&self) -> Vec<&str> {
        vec![] // No dependencies for the renderer plugin
    }

    async fn initialize(&mut self, context: &PluginContext) -> Result<()> {
        tracing::info!("Initializing renderer plugin");

        // Create or get the renderer registry
        let registry = if let Some(existing_registry) = context
            .get_shared_resource::<Arc<RendererRegistry>>("renderer_registry")
            .await
        {
            existing_registry.as_ref().clone()
        } else {
            let new_registry = Arc::new(RendererRegistry::new());
            context
                .set_shared_resource("renderer_registry".to_string(), new_registry.clone())
                .await?;
            new_registry
        };

        // Register built-in markdown renderer
        let markdown_renderer = Box::new(MarkdownRenderer::new());
        registry.register_renderer(markdown_renderer).await?;

        self.registry = Some(registry.clone());
        self.status = PluginStatus::Active;

        tracing::info!("Renderer plugin initialized with markdown renderer");
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        tracing::info!("Shutting down renderer plugin");
        self.registry = None;
        self.status = PluginStatus::Stopped;
        Ok(())
    }

    fn status(&self) -> PluginStatus {
        self.status.clone()
    }

    fn provided_services(&self) -> Vec<&str> {
        vec!["content-rendering", "renderer-registry"]
    }
}
