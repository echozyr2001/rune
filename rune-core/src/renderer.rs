//! Content renderer system for pluggable content rendering

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::{Result, RuneError};
use crate::plugin::Plugin;

/// Metadata about the rendering process and renderer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RendererMetadata {
    /// Name of the renderer that processed the content
    pub renderer_name: String,
    /// Version of the renderer
    pub renderer_version: String,
    /// Time taken to render (in milliseconds)
    pub render_time_ms: Option<u64>,
    /// Content hash for caching
    pub content_hash: Option<String>,
    /// Additional renderer-specific metadata
    pub custom_metadata: HashMap<String, serde_json::Value>,
}

impl Default for RendererMetadata {
    fn default() -> Self {
        Self {
            renderer_name: "unknown".to_string(),
            renderer_version: "0.0.0".to_string(),
            render_time_ms: None,
            content_hash: None,
            custom_metadata: HashMap::new(),
        }
    }
}

/// Trait for content renderers that can process different content types
#[async_trait]
pub trait ContentRenderer: Plugin {
    /// Check if this renderer can handle the given content type
    fn can_render(&self, content_type: &str) -> bool;

    /// Render content with the given context
    async fn render(&self, content: &str, context: &RenderContext) -> Result<RenderResult>;

    /// Get supported file extensions for this renderer
    fn supported_extensions(&self) -> Vec<&str>;

    /// Get the priority of this renderer (higher priority = preferred)
    fn priority(&self) -> u32 {
        100
    }

    /// Get renderer-specific metadata
    fn renderer_metadata(&self) -> RendererMetadata {
        RendererMetadata::default()
    }
}

/// Context provided to renderers during rendering
#[derive(Debug, Clone)]
pub struct RenderContext {
    /// Path to the file being rendered
    pub file_path: PathBuf,
    /// Base directory for resolving relative paths
    pub base_dir: PathBuf,
    /// Current theme name
    pub theme: String,
    /// Custom data that can be used by renderers
    pub custom_data: HashMap<String, serde_json::Value>,
    /// Content type being rendered
    pub content_type: String,
    /// Original file extension
    pub file_extension: Option<String>,
}

impl RenderContext {
    /// Create a new render context
    pub fn new(file_path: PathBuf, base_dir: PathBuf, theme: String) -> Self {
        let file_extension = file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|s| s.to_lowercase());

        let content_type = match file_extension.as_deref() {
            Some("md") | Some("markdown") => "text/markdown".to_string(),
            Some("html") | Some("htm") => "text/html".to_string(),
            Some("txt") => "text/plain".to_string(),
            _ => "application/octet-stream".to_string(),
        };

        Self {
            file_path,
            base_dir,
            theme,
            custom_data: HashMap::new(),
            content_type,
            file_extension,
        }
    }

    /// Add custom data to the context
    pub fn with_custom_data(mut self, key: String, value: serde_json::Value) -> Self {
        self.custom_data.insert(key, value);
        self
    }

    /// Get custom data from the context
    pub fn get_custom_data(&self, key: &str) -> Option<&serde_json::Value> {
        self.custom_data.get(key)
    }

    /// Set the content type
    pub fn with_content_type(mut self, content_type: String) -> Self {
        self.content_type = content_type;
        self
    }
}

/// Result of content rendering
#[derive(Debug, Clone)]
pub struct RenderResult {
    /// Rendered HTML content
    pub html: String,
    /// Metadata about the rendering process
    pub metadata: RenderMetadata,
    /// Assets required for the rendered content (CSS, JS, etc.)
    pub assets: Vec<Asset>,
    /// Whether the content contains interactive elements
    pub has_interactive_content: bool,
}

impl RenderResult {
    /// Create a new render result
    pub fn new(html: String) -> Self {
        Self {
            html,
            metadata: RenderMetadata::default(),
            assets: Vec::new(),
            has_interactive_content: false,
        }
    }

    /// Add metadata to the result
    pub fn with_metadata(mut self, metadata: RenderMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    /// Add an asset requirement
    pub fn with_asset(mut self, asset: Asset) -> Self {
        self.assets.push(asset);
        self
    }

    /// Mark as having interactive content
    pub fn with_interactive_content(mut self) -> Self {
        self.has_interactive_content = true;
        self
    }
}

/// Alias for RendererMetadata for backward compatibility
pub type RenderMetadata = RendererMetadata;

/// Asset required for rendered content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    /// Type of asset (css, js, font, etc.)
    pub asset_type: AssetType,
    /// URL or path to the asset
    pub url: String,
    /// Whether the asset is critical for rendering
    pub is_critical: bool,
    /// Integrity hash for security
    pub integrity: Option<String>,
}

/// Types of assets that can be required
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AssetType {
    Css,
    JavaScript,
    Font,
    Image,
    Other(String),
}

/// Registry for managing content renderers
pub struct RendererRegistry {
    renderers: Arc<RwLock<HashMap<String, Box<dyn ContentRenderer>>>>,
    render_pipeline: Arc<RwLock<Vec<String>>>,
}

impl RendererRegistry {
    /// Create a new renderer registry
    pub fn new() -> Self {
        Self {
            renderers: Arc::new(RwLock::new(HashMap::new())),
            render_pipeline: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Register a content renderer
    pub async fn register_renderer(&self, renderer: Box<dyn ContentRenderer>) -> Result<()> {
        let name = renderer.name().to_string();
        
        {
            let mut renderers = self.renderers.write().await;
            
            if renderers.contains_key(&name) {
                return Err(RuneError::Plugin(format!(
                    "Renderer '{}' is already registered",
                    name
                )));
            }

            renderers.insert(name.clone(), renderer);
        } // Drop the write lock here
        
        // Update render pipeline order based on priority
        self.update_pipeline_order().await;
        
        tracing::info!("Registered content renderer: {}", name);
        Ok(())
    }

    /// Unregister a content renderer
    pub async fn unregister_renderer(&self, name: &str) -> Result<()> {
        let removed = {
            let mut renderers = self.renderers.write().await;
            renderers.remove(name).is_some()
        }; // Drop the write lock here
        
        if removed {
            self.update_pipeline_order().await;
            tracing::info!("Unregistered content renderer: {}", name);
            Ok(())
        } else {
            Err(RuneError::Plugin(format!(
                "Renderer '{}' is not registered",
                name
            )))
        }
    }

    /// Find the best renderer for the given content type
    pub async fn find_renderer(&self, content_type: &str) -> Option<String> {
        let renderers = self.renderers.read().await;
        let pipeline = self.render_pipeline.read().await;

        // Find the first renderer in the pipeline that can handle this content type
        for renderer_name in pipeline.iter() {
            if let Some(renderer) = renderers.get(renderer_name) {
                if renderer.can_render(content_type) {
                    return Some(renderer_name.clone());
                }
            }
        }

        None
    }

    /// Render content using the appropriate renderer
    pub async fn render_content(
        &self,
        content: &str,
        context: &RenderContext,
    ) -> Result<RenderResult> {
        let renderer_name = self
            .find_renderer(&context.content_type)
            .await
            .ok_or_else(|| {
                RuneError::Plugin(format!(
                    "No renderer found for content type: {}",
                    context.content_type
                ))
            })?;

        let renderers = self.renderers.read().await;
        let renderer = renderers.get(&renderer_name).ok_or_else(|| {
            RuneError::Plugin(format!("Renderer '{}' not found", renderer_name))
        })?;

        let start_time = std::time::Instant::now();
        let mut result = renderer.render(content, context).await?;
        let render_time = start_time.elapsed().as_millis() as u64;

        // Update metadata with timing information
        result.metadata.render_time_ms = Some(render_time);
        result.metadata.renderer_name = renderer_name;
        result.metadata.renderer_version = renderer.version().to_string();

        Ok(result)
    }

    /// Render content using a chained pipeline of renderers
    pub async fn render_with_pipeline(
        &self,
        content: &str,
        context: &RenderContext,
    ) -> Result<RenderResult> {
        let pipeline_start = std::time::Instant::now();
        let mut current_content = content.to_string();
        let mut current_context = context.clone();
        let mut combined_assets = Vec::new();
        let mut combined_metadata = HashMap::new();
        let mut has_interactive = false;
        let mut pipeline_renderers = Vec::new();

        // Get all applicable renderers for the pipeline
        let applicable_renderers = self.get_pipeline_renderers(&context.content_type).await;

        for renderer_name in applicable_renderers {
            let renderers = self.renderers.read().await;
            if let Some(renderer) = renderers.get(&renderer_name) {
                if renderer.can_render(&current_context.content_type) {
                    let render_result = renderer.render(&current_content, &current_context).await?;
                    
                    // Update content for next renderer in pipeline
                    current_content = render_result.html;
                    
                    // Accumulate assets
                    combined_assets.extend(render_result.assets);
                    
                    // Merge metadata
                    for (key, value) in render_result.metadata.custom_metadata {
                        combined_metadata.insert(
                            format!("{}_{}", renderer_name, key),
                            value,
                        );
                    }
                    
                    // Track interactive content
                    if render_result.has_interactive_content {
                        has_interactive = true;
                    }
                    
                    pipeline_renderers.push(renderer_name.clone());
                    
                    // Update context content type if it changed
                    if current_context.content_type.starts_with("text/markdown") {
                        current_context.content_type = "text/html".to_string();
                    }
                }
            }
        }

        let total_time = pipeline_start.elapsed().as_millis() as u64;

        // Create combined metadata
        let metadata = RendererMetadata {
            renderer_name: format!("pipeline({})", pipeline_renderers.join("→")),
            renderer_version: "1.0.0".to_string(),
            render_time_ms: Some(total_time),
            content_hash: Some(format!("{:x}", current_content.len() as u64)),
            custom_metadata: combined_metadata,
        };

        let mut result = RenderResult::new(current_content)
            .with_metadata(metadata);

        if has_interactive {
            result = result.with_interactive_content();
        }

        // Add all accumulated assets
        let result = combined_assets.into_iter().fold(result, |acc, asset| acc.with_asset(asset));

        Ok(result)
    }

    /// Get renderers that should be applied in pipeline order for a content type
    async fn get_pipeline_renderers(&self, content_type: &str) -> Vec<String> {
        let renderers = self.renderers.read().await;
        let pipeline = self.render_pipeline.read().await;

        let mut applicable = Vec::new();

        // For markdown content, we want: markdown → mermaid → any other processors
        if content_type.starts_with("text/markdown") {
            // First, find markdown renderer
            for renderer_name in pipeline.iter() {
                if let Some(renderer) = renderers.get(renderer_name) {
                    if renderer.can_render(content_type) && renderer_name.contains("markdown") {
                        applicable.push(renderer_name.clone());
                        break;
                    }
                }
            }
            
            // Then, find HTML processors (like mermaid)
            for renderer_name in pipeline.iter() {
                if let Some(renderer) = renderers.get(renderer_name) {
                    if renderer.can_render("text/html") && !renderer_name.contains("markdown") {
                        applicable.push(renderer_name.clone());
                    }
                }
            }
        } else {
            // For other content types, just find the first applicable renderer
            for renderer_name in pipeline.iter() {
                if let Some(renderer) = renderers.get(renderer_name) {
                    if renderer.can_render(content_type) {
                        applicable.push(renderer_name.clone());
                        break;
                    }
                }
            }
        }

        applicable
    }

    /// Get all registered renderers
    pub async fn list_renderers(&self) -> Vec<String> {
        let renderers = self.renderers.read().await;
        renderers.keys().cloned().collect()
    }

    /// Get renderer information
    pub async fn get_renderer_info(&self, name: &str) -> Option<RendererMetadata> {
        let renderers = self.renderers.read().await;
        renderers.get(name).map(|r| r.renderer_metadata())
    }

    /// Update the pipeline order based on renderer priorities
    async fn update_pipeline_order(&self) {
        let renderers = self.renderers.read().await;
        let mut pipeline: Vec<(String, u32)> = renderers
            .iter()
            .map(|(name, renderer)| (name.clone(), renderer.priority()))
            .collect();

        // Sort by priority (higher first)
        pipeline.sort_by(|a, b| b.1.cmp(&a.1));

        let mut render_pipeline = self.render_pipeline.write().await;
        *render_pipeline = pipeline.into_iter().map(|(name, _)| name).collect();
    }
}

impl Default for RendererRegistry {
    fn default() -> Self {
        Self::new()
    }
}