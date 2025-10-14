//! Concrete handler implementations for the server plugin

use crate::{
    HttpHandler, HttpRequest, HttpResponse, WebSocketConnection, WebSocketHandler, WebSocketMessage,
};
use async_trait::async_trait;
use axum::http::{Method, StatusCode};
use rune_core::{
    error::{Result, RuneError},
    event::{EventBus, SystemEvent},
    renderer::{RenderContext, RendererRegistry},
};
use serde::{Deserialize, Serialize};

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, info, warn};

/// Static file handler for serving files from the filesystem
pub struct StaticHandler {
    base_path: PathBuf,
    path_pattern: String,
    allowed_extensions: Vec<String>,
}

impl StaticHandler {
    /// Create a new static file handler
    pub fn new(base_path: PathBuf, path_pattern: String) -> Self {
        let allowed_extensions = vec![
            "png".to_string(),
            "jpg".to_string(),
            "jpeg".to_string(),
            "gif".to_string(),
            "svg".to_string(),
            "webp".to_string(),
            "bmp".to_string(),
            "ico".to_string(),
            "css".to_string(),
            "js".to_string(),
            "html".to_string(),
            "txt".to_string(),
        ];

        Self {
            base_path,
            path_pattern,
            allowed_extensions,
        }
    }

    /// Create a new static handler specifically for images (like mdserve)
    pub fn new_image_handler(base_path: PathBuf, path_pattern: String) -> Self {
        let allowed_extensions = vec![
            "png".to_string(),
            "jpg".to_string(),
            "jpeg".to_string(),
            "gif".to_string(),
            "svg".to_string(),
            "webp".to_string(),
            "bmp".to_string(),
            "ico".to_string(),
        ];

        Self {
            base_path,
            path_pattern,
            allowed_extensions,
        }
    }

    /// Check if the file extension is allowed
    fn is_allowed_extension(&self, path: &Path) -> bool {
        if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
            self.allowed_extensions.contains(&extension.to_lowercase())
        } else {
            false
        }
    }

    /// Guess the content type based on file extension
    fn guess_content_type(&self, path: &Path) -> String {
        if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
            match extension.to_lowercase().as_str() {
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "gif" => "image/gif",
                "svg" => "image/svg+xml",
                "webp" => "image/webp",
                "bmp" => "image/bmp",
                "ico" => "image/x-icon",
                "css" => "text/css",
                "js" => "application/javascript",
                "html" => "text/html; charset=utf-8",
                "txt" => "text/plain; charset=utf-8",
                _ => "application/octet-stream",
            }
        } else {
            "application/octet-stream"
        }
        .to_string()
    }
}

#[async_trait]
impl HttpHandler for StaticHandler {
    fn path_pattern(&self) -> &str {
        &self.path_pattern
    }

    fn method(&self) -> Method {
        Method::GET
    }

    async fn handle(&self, request: HttpRequest) -> Result<HttpResponse> {
        // Extract the file path from the request path
        let requested_path = request
            .path
            .strip_prefix(&self.path_pattern)
            .unwrap_or(&request.path)
            .trim_start_matches('/');

        if requested_path.is_empty() {
            return Ok(HttpResponse::error(StatusCode::NOT_FOUND, "File not found"));
        }

        // Construct the full file path
        let file_path = self.base_path.join(requested_path);

        // Security check: ensure the resolved path is still within base_path
        match file_path.canonicalize() {
            Ok(canonical_path) => {
                if !canonical_path.starts_with(&self.base_path) {
                    warn!(
                        "Access denied for path outside base directory: {:?}",
                        canonical_path
                    );
                    return Ok(HttpResponse::error(StatusCode::FORBIDDEN, "Access denied"));
                }

                // Check if file extension is allowed
                if !self.is_allowed_extension(&canonical_path) {
                    return Ok(HttpResponse::error(
                        StatusCode::FORBIDDEN,
                        "File type not allowed",
                    ));
                }

                // Try to read and serve the file
                match fs::read(&canonical_path) {
                    Ok(contents) => {
                        let content_type = self.guess_content_type(&canonical_path);
                        debug!(
                            "Serving static file: {:?} ({})",
                            canonical_path, content_type
                        );

                        Ok(HttpResponse::new(StatusCode::OK)
                            .with_header("content-type", &content_type)
                            .with_body(contents))
                    }
                    Err(e) => {
                        warn!("Failed to read file {:?}: {}", canonical_path, e);
                        Ok(HttpResponse::error(StatusCode::NOT_FOUND, "File not found"))
                    }
                }
            }
            Err(e) => {
                warn!("Failed to canonicalize path {:?}: {}", file_path, e);
                Ok(HttpResponse::error(StatusCode::NOT_FOUND, "File not found"))
            }
        }
    }

    fn priority(&self) -> i32 {
        100 // Lower priority than specific handlers
    }

    fn matches_path(&self, path: &str) -> bool {
        path.starts_with(&self.path_pattern)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Markdown handler for serving rendered markdown content with live reload
pub struct MarkdownHandler {
    path_pattern: String,
    markdown_file: PathBuf,
    base_dir: PathBuf,
    renderer_registry: Option<Arc<RendererRegistry>>,
    cached_state: Arc<RwLock<CachedMarkdownState>>,
    template: String,
}

/// Cached state for markdown rendering
#[derive(Debug, Clone)]
struct CachedMarkdownState {
    last_modified: SystemTime,
    cached_html: String,
    content_hash: String,
}

impl CachedMarkdownState {
    fn new() -> Self {
        Self {
            last_modified: SystemTime::UNIX_EPOCH,
            cached_html: String::new(),
            content_hash: String::new(),
        }
    }
}

impl MarkdownHandler {
    /// Create a new markdown handler
    pub fn new(path_pattern: String, markdown_file: PathBuf) -> Self {
        let base_dir = markdown_file
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
            .canonicalize()
            .unwrap_or_else(|_| {
                markdown_file
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .to_path_buf()
            });

        // Use the template from mdserve
        let template = include_str!("../../../template.html").to_string();

        Self {
            path_pattern,
            markdown_file,
            base_dir,
            renderer_registry: None,
            cached_state: Arc::new(RwLock::new(CachedMarkdownState::new())),
            template,
        }
    }

    /// Create a new markdown handler with renderer registry
    pub fn with_renderer_registry(
        path_pattern: String,
        markdown_file: PathBuf,
        renderer_registry: Arc<RendererRegistry>,
    ) -> Self {
        let mut handler = Self::new(path_pattern, markdown_file);
        handler.renderer_registry = Some(renderer_registry);
        handler
    }

    /// Check if the markdown file needs to be refreshed
    async fn refresh_if_needed(&self) -> Result<bool> {
        let metadata = fs::metadata(&self.markdown_file)
            .map_err(|e| RuneError::Server(format!("Failed to read file metadata: {}", e)))?;

        let current_modified = metadata
            .modified()
            .map_err(|e| RuneError::Server(format!("Failed to get modification time: {}", e)))?;

        let mut state = self.cached_state.write().await;

        if current_modified > state.last_modified {
            let content = fs::read_to_string(&self.markdown_file)
                .map_err(|e| RuneError::Server(format!("Failed to read markdown file: {}", e)))?;

            let rendered_html = self.render_markdown(&content).await?;
            let content_hash = format!("{:x}", content.len() as u64);

            state.last_modified = current_modified;
            state.cached_html = rendered_html;
            state.content_hash = content_hash;

            debug!("Refreshed markdown content: {:?}", self.markdown_file);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Render markdown content to HTML using the renderer plugin
    async fn render_markdown(&self, content: &str) -> Result<String> {
        if let Some(registry) = &self.renderer_registry {
            // Create render context with theme support
            let context = RenderContext::new(
                self.markdown_file.clone(),
                self.base_dir.clone(),
                "catppuccin-mocha".to_string(), // Default theme - will be overridden by theme-aware renderer
            );

            // Use the pipeline renderer to apply all transformations including theme
            let result = registry.render_with_pipeline(content, &context).await?;

            // Check if we have mermaid diagrams
            let has_mermaid = result.html.contains(r#"class="language-mermaid""#)
                || result.html.contains(r#"<div class="mermaid""#);

            let mermaid_assets = if has_mermaid {
                r#"<script src="/mermaid.min.js"></script>"#
            } else {
                ""
            };

            // Apply template
            let final_html = self
                .template
                .replace("{CONTENT}", &result.html)
                .replace("<!-- {MERMAID_ASSETS} -->", mermaid_assets);

            Ok(final_html)
        } else {
            // Fallback to simple markdown rendering
            self.render_markdown_fallback(content)
        }
    }

    /// Fallback markdown rendering without renderer plugin
    fn render_markdown_fallback(&self, content: &str) -> Result<String> {
        // Create GFM options with HTML rendering enabled
        let mut options = markdown::Options::gfm();
        options.compile.allow_dangerous_html = true;

        let html_body = markdown::to_html_with_options(content, &options)
            .map_err(|e| RuneError::Server(format!("Markdown parsing failed: {}", e)))?;

        // Check if the HTML contains mermaid code blocks
        let has_mermaid = html_body.contains(r#"class="language-mermaid""#);

        let mermaid_assets = if has_mermaid {
            r#"<script src="/mermaid.min.js"></script>"#
        } else {
            ""
        };

        let final_html = self
            .template
            .replace("{CONTENT}", &html_body)
            .replace("<!-- {MERMAID_ASSETS} -->", mermaid_assets);

        Ok(final_html)
    }

    /// Get the base directory for resolving relative paths
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Render content and push via WebSocket (optimized version)
    pub async fn render_and_push_content(
        &self,
        websocket_handler: &LiveReloadHandler,
    ) -> Result<()> {
        // Check if content needs refresh
        let needs_refresh = self.refresh_if_needed().await?;

        if needs_refresh {
            let state = self.cached_state.read().await;

            // Extract just the content part (without full HTML template)
            let content_html = self.extract_content_only().await?;

            // Create metadata
            let metadata = ContentMetadata {
                title: self.extract_title_from_content(&content_html),
                last_modified: Some(state.last_modified),
                file_path: Some(self.markdown_file.to_string_lossy().to_string()),
                word_count: Some(self.count_words(&content_html)),
            };

            // Push content update via WebSocket
            websocket_handler
                .broadcast_content_update(content_html, None, Some(metadata))
                .await?;

            info!(
                "Pushed content update via WebSocket for: {:?}",
                self.markdown_file
            );
        }

        Ok(())
    }

    /// Extract only the content part without the full HTML template
    async fn extract_content_only(&self) -> Result<String> {
        let content = fs::read_to_string(&self.markdown_file)
            .map_err(|e| RuneError::Server(format!("Failed to read markdown file: {}", e)))?;

        if let Some(registry) = &self.renderer_registry {
            let context = RenderContext::new(
                self.markdown_file.clone(),
                self.base_dir.clone(),
                "catppuccin-mocha".to_string(),
            );

            let result = registry.render_with_pipeline(&content, &context).await?;
            Ok(result.html)
        } else {
            // Fallback rendering
            let mut options = markdown::Options::gfm();
            options.compile.allow_dangerous_html = true;

            let html = markdown::to_html_with_options(&content, &options)
                .map_err(|e| RuneError::Server(format!("Markdown parsing failed: {}", e)))?;

            Ok(html)
        }
    }

    /// Extract title from HTML content
    fn extract_title_from_content(&self, html: &str) -> Option<String> {
        // Simple regex to extract first h1 tag
        if let Some(start) = html.find("<h1") {
            if let Some(content_start) = html[start..].find('>') {
                let content_start = start + content_start + 1;
                if let Some(end) = html[content_start..].find("</h1>") {
                    let title = &html[content_start..content_start + end];
                    // Strip HTML tags from title
                    let title = title.replace(['<', '>'], "");
                    return Some(title.trim().to_string());
                }
            }
        }
        None
    }

    /// Count words in HTML content (approximate)
    fn count_words(&self, html: &str) -> usize {
        // Simple word count by removing HTML tags and counting words
        let text = html.chars().fold(String::new(), |mut acc, c| {
            if c == '<' {
                acc.push(' ');
            } else if c != '>' {
                acc.push(c);
            }
            acc
        });

        text.split_whitespace().count()
    }
}

#[async_trait]
impl HttpHandler for MarkdownHandler {
    fn path_pattern(&self) -> &str {
        &self.path_pattern
    }

    fn method(&self) -> Method {
        Method::GET
    }

    async fn handle(&self, _request: HttpRequest) -> Result<HttpResponse> {
        // Refresh content if needed
        if let Err(e) = self.refresh_if_needed().await {
            warn!("Failed to refresh markdown content: {}", e);
            return Ok(HttpResponse::error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Error refreshing content: {}", e),
            ));
        }

        // Get cached content
        let state = self.cached_state.read().await;
        if state.cached_html.is_empty() {
            return Ok(HttpResponse::error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "No content available",
            ));
        }

        debug!("Serving markdown file: {:?}", self.markdown_file);
        Ok(HttpResponse::html(&state.cached_html))
    }

    fn priority(&self) -> i32 {
        10 // Higher priority than static handler
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Raw markdown handler for serving raw markdown content
pub struct RawMarkdownHandler {
    path_pattern: String,
    markdown_file: PathBuf,
}

impl RawMarkdownHandler {
    /// Create a new raw markdown handler
    pub fn new(path_pattern: String, markdown_file: PathBuf) -> Self {
        Self {
            path_pattern,
            markdown_file,
        }
    }
}

#[async_trait]
impl HttpHandler for RawMarkdownHandler {
    fn path_pattern(&self) -> &str {
        &self.path_pattern
    }

    fn method(&self) -> Method {
        Method::GET
    }

    async fn handle(&self, _request: HttpRequest) -> Result<HttpResponse> {
        match fs::read_to_string(&self.markdown_file) {
            Ok(content) => {
                debug!("Serving raw markdown file: {:?}", self.markdown_file);
                Ok(HttpResponse::text(&content))
            }
            Err(e) => {
                warn!(
                    "Failed to read markdown file {:?}: {}",
                    self.markdown_file, e
                );
                Ok(HttpResponse::error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to read markdown file",
                ))
            }
        }
    }

    fn priority(&self) -> i32 {
        10 // Higher priority than static handler
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Theme API handler for theme management operations
pub struct ThemeApiHandler {
    path_pattern: String,
    event_bus: Arc<dyn EventBus>,
}

impl ThemeApiHandler {
    /// Create a new theme API handler
    pub fn new(path_pattern: String, event_bus: Arc<dyn EventBus>) -> Self {
        Self {
            path_pattern,
            event_bus,
        }
    }

    /// Handle theme switching via POST request
    async fn handle_theme_switch_post(&self, request: &HttpRequest) -> Result<HttpResponse> {
        // Parse JSON body to get theme name
        let body_str = String::from_utf8(request.body.clone())
            .map_err(|e| RuneError::Server(format!("Invalid UTF-8 in request body: {}", e)))?;

        let theme_request: serde_json::Value = serde_json::from_str(&body_str)
            .map_err(|e| RuneError::Server(format!("Invalid JSON in request body: {}", e)))?;

        let theme_name = theme_request
            .get("theme")
            .and_then(|t| t.as_str())
            .ok_or_else(|| RuneError::Server("Missing 'theme' field in request".to_string()))?;

        // Validate theme name
        let valid_themes = vec![
            "light",
            "dark",
            "catppuccin-latte",
            "catppuccin-macchiato",
            "catppuccin-mocha",
        ];
        if !valid_themes.contains(&theme_name) {
            return Ok(HttpResponse::error(
                StatusCode::BAD_REQUEST,
                &format!(
                    "Invalid theme: {}. Valid themes: {:?}",
                    theme_name, valid_themes
                ),
            ));
        }

        // Publish theme change event
        let theme_event = SystemEvent::theme_changed(theme_name.to_string());
        self.event_bus
            .publish_system_event(theme_event)
            .await
            .map_err(|e| {
                RuneError::Server(format!("Failed to publish theme change event: {}", e))
            })?;

        tracing::info!("Theme switched to: {}", theme_name);

        HttpResponse::json(&serde_json::json!({
            "status": "success",
            "theme": theme_name,
            "message": format!("Theme switched to {}", theme_name)
        }))
    }
}

#[async_trait]
impl HttpHandler for ThemeApiHandler {
    fn path_pattern(&self) -> &str {
        &self.path_pattern
    }

    fn method(&self) -> Method {
        Method::POST // Primary method for theme switching
    }

    async fn handle(&self, request: HttpRequest) -> Result<HttpResponse> {
        // Handle theme switching
        self.handle_theme_switch_post(&request).await
    }

    fn priority(&self) -> i32 {
        5 // High priority for API endpoints
    }

    fn can_handle(&self, path: &str, method: &Method) -> bool {
        path == self.path_pattern && *method == Method::POST
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Theme info handler for GET requests to theme API
#[allow(dead_code)]
pub struct ThemeInfoHandler {
    path_pattern: String,
    event_bus: Arc<dyn EventBus>,
}

impl ThemeInfoHandler {
    /// Create a new theme info handler
    pub fn new(path_pattern: String, event_bus: Arc<dyn EventBus>) -> Self {
        Self {
            path_pattern,
            event_bus,
        }
    }

    /// Get current theme information
    async fn handle_theme_info(&self) -> Result<HttpResponse> {
        let themes = vec![
            serde_json::json!({
                "name": "light",
                "display_name": "Light",
                "description": "Classic bright theme",
                "icon": "☀️",
                "is_dark": false
            }),
            serde_json::json!({
                "name": "dark",
                "display_name": "Dark",
                "description": "Classic dark theme",
                "icon": "🌙",
                "is_dark": true
            }),
            serde_json::json!({
                "name": "catppuccin-latte",
                "display_name": "Catppuccin Latte",
                "description": "Warm light theme",
                "icon": "☕",
                "is_dark": false
            }),
            serde_json::json!({
                "name": "catppuccin-macchiato",
                "display_name": "Catppuccin Macchiato",
                "description": "Medium contrast theme",
                "icon": "🥛",
                "is_dark": true
            }),
            serde_json::json!({
                "name": "catppuccin-mocha",
                "display_name": "Catppuccin Mocha",
                "description": "Dark and cozy theme",
                "icon": "🐱",
                "is_dark": true
            }),
        ];

        HttpResponse::json(&serde_json::json!({
            "available_themes": themes,
            "current_theme": "catppuccin-mocha" // Default theme
        }))
    }
}

#[async_trait]
impl HttpHandler for ThemeInfoHandler {
    fn path_pattern(&self) -> &str {
        &self.path_pattern
    }

    fn method(&self) -> Method {
        Method::GET
    }

    async fn handle(&self, _request: HttpRequest) -> Result<HttpResponse> {
        self.handle_theme_info().await
    }

    fn priority(&self) -> i32 {
        5 // High priority for API endpoints
    }

    fn can_handle(&self, path: &str, method: &Method) -> bool {
        path == self.path_pattern && *method == Method::GET
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Theme asset handler for serving theme CSS and assets
pub struct ThemeAssetHandler {
    path_pattern: String,
    event_bus: Option<Arc<dyn EventBus>>,
}

impl ThemeAssetHandler {
    /// Create a new theme asset handler
    pub fn new(path_pattern: String) -> Self {
        Self {
            path_pattern,
            event_bus: None,
        }
    }

    /// Create a new theme asset handler with event bus
    pub fn with_event_bus(path_pattern: String, event_bus: Arc<dyn EventBus>) -> Self {
        Self {
            path_pattern,
            event_bus: Some(event_bus),
        }
    }

    /// Generate CSS for a specific theme
    fn generate_theme_css(&self, theme_name: &str) -> Result<String> {
        let css = match theme_name {
            "light" => {
                r#"
                :root {
                    --bg-color: #fff;
                    --text-color: #333;
                    --border-color: #eaecef;
                    --border-color-light: #dfe2e5;
                    --code-bg: #f6f8fa;
                    --blockquote-color: #6a737d;
                    --link-color: #0366d6;
                    --table-header-bg: #f6f8fa;
                }
            "#
            }
            "dark" => {
                r#"
                :root {
                    --bg-color: #0d1117;
                    --text-color: #e6edf3;
                    --border-color: #30363d;
                    --border-color-light: #21262d;
                    --code-bg: #161b22;
                    --blockquote-color: #8b949e;
                    --link-color: #58a6ff;
                    --table-header-bg: #161b22;
                }
            "#
            }
            "catppuccin-latte" => {
                r#"
                :root {
                    --bg-color: #eff1f5;
                    --text-color: #4c4f69;
                    --border-color: #bcc0cc;
                    --border-color-light: #ccd0da;
                    --code-bg: #e6e9ef;
                    --blockquote-color: #6c6f85;
                    --link-color: #1e66f5;
                    --table-header-bg: #ccd0da;
                }
            "#
            }
            "catppuccin-macchiato" => {
                r#"
                :root {
                    --bg-color: #24273a;
                    --text-color: #cad3f5;
                    --border-color: #494d64;
                    --border-color-light: #363a4f;
                    --code-bg: #1e2030;
                    --blockquote-color: #a5adcb;
                    --link-color: #8aadf4;
                    --table-header-bg: #363a4f;
                }
            "#
            }
            "catppuccin-mocha" => {
                r#"
                :root {
                    --bg-color: #1e1e2e;
                    --text-color: #cdd6f4;
                    --border-color: #45475a;
                    --border-color-light: #313244;
                    --code-bg: #181825;
                    --blockquote-color: #a6adc8;
                    --link-color: #89b4fa;
                    --table-header-bg: #313244;
                }
            "#
            }
            _ => return Err(RuneError::Server(format!("Unknown theme: {}", theme_name))),
        };

        Ok(css.to_string())
    }

    /// Get theme metadata as JSON
    fn get_theme_metadata(&self, theme_name: &str) -> Result<String> {
        let metadata = match theme_name {
            "light" => serde_json::json!({
                "name": "light",
                "display_name": "Light",
                "description": "Classic bright theme",
                "author": "Rune",
                "version": "1.0.0",
                "icon": "☀️",
                "preview_colors": ["#fff", "#333", "#0366d6"],
                "is_dark": false,
                "mermaid_theme": "default"
            }),
            "dark" => serde_json::json!({
                "name": "dark",
                "display_name": "Dark",
                "description": "Classic dark theme",
                "author": "Rune",
                "version": "1.0.0",
                "icon": "🌙",
                "preview_colors": ["#0d1117", "#e6edf3", "#58a6ff"],
                "is_dark": true,
                "mermaid_theme": "dark"
            }),
            "catppuccin-latte" => serde_json::json!({
                "name": "catppuccin-latte",
                "display_name": "Catppuccin Latte",
                "description": "Warm light theme",
                "author": "Rune",
                "version": "1.0.0",
                "icon": "☕",
                "preview_colors": ["#eff1f5", "#4c4f69", "#1e66f5"],
                "is_dark": false,
                "mermaid_theme": "default"
            }),
            "catppuccin-macchiato" => serde_json::json!({
                "name": "catppuccin-macchiato",
                "display_name": "Catppuccin Macchiato",
                "description": "Medium contrast theme",
                "author": "Rune",
                "version": "1.0.0",
                "icon": "🥛",
                "preview_colors": ["#24273a", "#cad3f5", "#8aadf4"],
                "is_dark": true,
                "mermaid_theme": "dark"
            }),
            "catppuccin-mocha" => serde_json::json!({
                "name": "catppuccin-mocha",
                "display_name": "Catppuccin Mocha",
                "description": "Dark and cozy theme",
                "author": "Rune",
                "version": "1.0.0",
                "icon": "🐱",
                "preview_colors": ["#1e1e2e", "#cdd6f4", "#89b4fa"],
                "is_dark": true,
                "mermaid_theme": "dark"
            }),
            _ => return Err(RuneError::Server(format!("Unknown theme: {}", theme_name))),
        };

        serde_json::to_string_pretty(&metadata)
            .map_err(|e| RuneError::Server(format!("Failed to serialize theme metadata: {}", e)))
    }

    /// Handle theme switching request
    async fn handle_theme_switch(&self, theme_name: &str) -> Result<HttpResponse> {
        // Publish theme change event if event bus is available
        if let Some(event_bus) = &self.event_bus {
            let theme_event = SystemEvent::theme_changed(theme_name.to_string());
            if let Err(e) = event_bus.publish_system_event(theme_event).await {
                tracing::warn!("Failed to publish theme change event: {}", e);
            } else {
                tracing::info!("Published theme change event for: {}", theme_name);
            }
        }

        HttpResponse::json(&serde_json::json!({
            "status": "success",
            "theme": theme_name,
            "message": format!("Theme switched to {}", theme_name)
        }))
    }
}

#[async_trait]
impl HttpHandler for ThemeAssetHandler {
    fn path_pattern(&self) -> &str {
        &self.path_pattern
    }

    fn method(&self) -> Method {
        Method::GET
    }

    async fn handle(&self, request: HttpRequest) -> Result<HttpResponse> {
        // Extract the requested path
        let path = request
            .path
            .strip_prefix(&self.path_pattern)
            .unwrap_or(&request.path)
            .trim_start_matches('/');

        if path.is_empty() {
            // Return list of available themes
            let themes = vec![
                "light",
                "dark",
                "catppuccin-latte",
                "catppuccin-macchiato",
                "catppuccin-mocha",
            ];
            return HttpResponse::json(&serde_json::json!({
                "available_themes": themes
            }));
        }

        // Handle different theme asset requests
        let parts: Vec<&str> = path.split('/').collect();

        match parts.as_slice() {
            [theme_name, "css"] => {
                // Serve theme CSS
                let css = self.generate_theme_css(theme_name)?;
                debug!("Serving CSS for theme: {}", theme_name);
                Ok(HttpResponse::new(StatusCode::OK)
                    .with_header("content-type", "text/css")
                    .with_header("cache-control", "public, max-age=3600")
                    .with_body(css.as_bytes()))
            }
            [theme_name, "metadata"] => {
                // Serve theme metadata
                let metadata = self.get_theme_metadata(theme_name)?;
                debug!("Serving metadata for theme: {}", theme_name);
                Ok(HttpResponse::new(StatusCode::OK)
                    .with_header("content-type", "application/json")
                    .with_header("cache-control", "public, max-age=3600")
                    .with_body(metadata.as_bytes()))
            }
            [theme_name, "switch"] => {
                // Handle theme switching
                self.handle_theme_switch(theme_name).await
            }
            [theme_name] => {
                // Default to serving CSS for the theme
                let css = self.generate_theme_css(theme_name)?;
                debug!("Serving default CSS for theme: {}", theme_name);
                Ok(HttpResponse::new(StatusCode::OK)
                    .with_header("content-type", "text/css")
                    .with_header("cache-control", "public, max-age=3600")
                    .with_body(css.as_bytes()))
            }
            _ => Ok(HttpResponse::error(
                StatusCode::NOT_FOUND,
                "Theme asset not found",
            )),
        }
    }

    fn priority(&self) -> i32 {
        5 // High priority for theme assets
    }

    fn matches_path(&self, path: &str) -> bool {
        path.starts_with(&self.path_pattern)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Mermaid.js handler for serving the Mermaid JavaScript library
pub struct MermaidHandler {
    path_pattern: String,
    mermaid_js: &'static str,
    etag: &'static str,
}

impl MermaidHandler {
    /// Create a new Mermaid handler
    pub fn new(path_pattern: String) -> Self {
        Self {
            path_pattern,
            mermaid_js: include_str!("../../../mermaid.min.js"),
            etag: concat!("\"", env!("CARGO_PKG_VERSION"), "\""),
        }
    }

    /// Check if the ETag matches the current version
    fn is_etag_match(&self, request: &HttpRequest) -> bool {
        if let Some(if_none_match) = request.headers.get("if-none-match") {
            if let Ok(etags) = if_none_match.to_str() {
                return etags.split(',').any(|tag| tag.trim() == self.etag);
            }
        }
        false
    }
}

#[async_trait]
impl HttpHandler for MermaidHandler {
    fn path_pattern(&self) -> &str {
        &self.path_pattern
    }

    fn method(&self) -> Method {
        Method::GET
    }

    async fn handle(&self, request: HttpRequest) -> Result<HttpResponse> {
        // Check if client has current version cached
        if self.is_etag_match(&request) {
            debug!("Serving Mermaid.js with 304 Not Modified");
            return Ok(HttpResponse::new(StatusCode::NOT_MODIFIED)
                .with_header("etag", self.etag)
                .with_header("cache-control", "public, no-cache"));
        }

        debug!("Serving Mermaid.js");
        Ok(HttpResponse::new(StatusCode::OK)
            .with_header("content-type", "application/javascript")
            .with_header("etag", self.etag)
            .with_header("cache-control", "public, no-cache")
            .with_body(self.mermaid_js.as_bytes()))
    }

    fn priority(&self) -> i32 {
        5 // High priority for specific asset
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Client message types for WebSocket communication
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum ClientMessage {
    Ping,
    RequestRefresh,
}

/// Server message types for WebSocket communication
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type")]
pub enum ServerMessage {
    /// Traditional reload message (fallback)
    Reload,
    /// Direct content update with rendered HTML
    ContentUpdate {
        html: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        css: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<ContentMetadata>,
    },
    /// Incremental content update for specific elements
    IncrementalUpdate { updates: Vec<ElementUpdate> },
    /// Pong response
    Pong,
    /// Error message
    Error {
        message: String,
        code: Option<String>,
    },
}

/// Metadata about the content
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ContentMetadata {
    pub title: Option<String>,
    pub last_modified: Option<SystemTime>,
    pub file_path: Option<String>,
    pub word_count: Option<usize>,
}

/// Individual element update for incremental updates
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ElementUpdate {
    pub selector: String,
    pub content: String,
    pub update_type: UpdateType,
}

/// Type of update to perform on an element
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum UpdateType {
    /// Replace the entire content
    Replace,
    /// Replace only the inner HTML
    InnerHtml,
    /// Append to existing content
    Append,
    /// Prepend to existing content
    Prepend,
}

/// WebSocket handler for live reload functionality
pub struct LiveReloadHandler {
    path: String,
    reload_sender: Arc<RwLock<Option<broadcast::Sender<ServerMessage>>>>,
}

impl LiveReloadHandler {
    /// Create a new live reload handler
    pub fn new(path: String) -> Self {
        Self {
            path,
            reload_sender: Arc::new(RwLock::new(None)),
        }
    }

    /// Create a new live reload handler with a reload sender
    pub fn with_reload_sender(path: String, sender: broadcast::Sender<ServerMessage>) -> Self {
        Self {
            path,
            reload_sender: Arc::new(RwLock::new(Some(sender))),
        }
    }

    /// Set the reload sender for broadcasting reload messages
    pub async fn set_reload_sender(&self, sender: broadcast::Sender<ServerMessage>) {
        let mut reload_sender = self.reload_sender.write().await;
        *reload_sender = Some(sender);
    }

    /// Get the reload sender
    pub async fn get_reload_sender(&self) -> Option<broadcast::Sender<ServerMessage>> {
        let reload_sender = self.reload_sender.read().await;
        reload_sender.clone()
    }

    /// Broadcast a reload message to all connected clients
    pub async fn broadcast_reload(&self) -> Result<()> {
        if let Some(sender) = self.get_reload_sender().await {
            sender
                .send(ServerMessage::Reload)
                .map_err(|e| RuneError::Server(format!("Failed to broadcast reload: {}", e)))?;
            info!("Broadcasted reload message to WebSocket clients");
        }
        Ok(())
    }

    /// Broadcast rendered content directly to all connected clients
    pub async fn broadcast_content_update(
        &self,
        html: String,
        css: Option<String>,
        metadata: Option<ContentMetadata>,
    ) -> Result<()> {
        if let Some(sender) = self.get_reload_sender().await {
            let message = ServerMessage::ContentUpdate {
                html,
                css,
                metadata,
            };
            sender.send(message).map_err(|e| {
                RuneError::Server(format!("Failed to broadcast content update: {}", e))
            })?;
            info!("Broadcasted content update to WebSocket clients");
        }
        Ok(())
    }

    /// Broadcast incremental updates to specific elements
    pub async fn broadcast_incremental_update(&self, updates: Vec<ElementUpdate>) -> Result<()> {
        if let Some(sender) = self.get_reload_sender().await {
            let update_count = updates.len();
            let message = ServerMessage::IncrementalUpdate { updates };
            sender.send(message).map_err(|e| {
                RuneError::Server(format!("Failed to broadcast incremental update: {}", e))
            })?;
            info!(
                "Broadcasted incremental update to WebSocket clients ({} updates)",
                update_count
            );
        }
        Ok(())
    }

    /// Broadcast error message to all connected clients
    pub async fn broadcast_error(&self, message: String, code: Option<String>) -> Result<()> {
        if let Some(sender) = self.get_reload_sender().await {
            let error_message = ServerMessage::Error { message, code };
            sender
                .send(error_message)
                .map_err(|e| RuneError::Server(format!("Failed to broadcast error: {}", e)))?;
            warn!("Broadcasted error message to WebSocket clients");
        }
        Ok(())
    }
}

#[async_trait]
impl WebSocketHandler for LiveReloadHandler {
    fn path(&self) -> &str {
        &self.path
    }

    async fn on_connect(&self, connection: &WebSocketConnection) -> Result<()> {
        info!(
            "WebSocket client connected: {} from {}",
            connection.id, connection.remote_addr
        );

        // Subscribe this connection to the shared reload sender
        if let Some(reload_sender) = self.get_reload_sender().await {
            let mut rx = reload_sender.subscribe();
            let conn_sender = connection.sender.clone();

            tokio::spawn(async move {
                while let Ok(msg) = rx.recv().await {
                    // Convert ServerMessage to WebSocketMessage
                    if let Ok(text) = serde_json::to_string(&msg) {
                        if conn_sender.send(WebSocketMessage::Text(text)).is_err() {
                            // Connection closed
                            break;
                        }
                    }
                }
            });
        }

        // Send a welcome message
        connection
            .send_json(&serde_json::json!({
                "type": "welcome",
                "message": "Connected to live reload server"
            }))
            .await?;

        Ok(())
    }

    async fn on_message(
        &self,
        connection: &WebSocketConnection,
        message: WebSocketMessage,
    ) -> Result<()> {
        match message {
            WebSocketMessage::Text(text) => {
                debug!(
                    "Received WebSocket message from {}: {}",
                    connection.id, text
                );

                // Try to parse as ClientMessage
                if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                    match client_msg {
                        ClientMessage::Ping => {
                            connection.send_json(&ServerMessage::Pong).await?;
                        }
                        ClientMessage::RequestRefresh => {
                            debug!("Client {} requested refresh", connection.id);
                            // In a real implementation, this could trigger a content refresh
                            // For now, we just acknowledge the request
                        }
                    }
                } else {
                    // Try to parse as generic JSON for backward compatibility
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) {
                        if let Some(msg_type) = parsed.get("type").and_then(|t| t.as_str()) {
                            match msg_type {
                                "ping" => {
                                    connection.send_json(&ServerMessage::Pong).await?;
                                }
                                "request_refresh" => {
                                    debug!(
                                        "Client {} requested refresh (legacy format)",
                                        connection.id
                                    );
                                }
                                _ => {
                                    debug!("Unknown message type: {}", msg_type);
                                }
                            }
                        }
                    }
                }
            }
            WebSocketMessage::Ping(data) => {
                connection.send(WebSocketMessage::Pong(data)).await?;
            }
            WebSocketMessage::Close(reason) => {
                debug!(
                    "WebSocket client {} requested close: {:?}",
                    connection.id, reason
                );
            }
            _ => {
                debug!(
                    "Received other WebSocket message type from {}",
                    connection.id
                );
            }
        }

        Ok(())
    }

    async fn on_disconnect(&self, connection: &WebSocketConnection) -> Result<()> {
        info!(
            "WebSocket client disconnected: {} from {}",
            connection.id, connection.remote_addr
        );
        Ok(())
    }

    fn priority(&self) -> i32 {
        1 // High priority for WebSocket handler
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs;

    #[tokio::test]
    async fn test_static_handler_creation() {
        let temp_dir = TempDir::new().unwrap();
        let handler = StaticHandler::new(temp_dir.path().to_path_buf(), "/static".to_string());

        assert_eq!(handler.path_pattern(), "/static");
        assert_eq!(handler.method(), Method::GET);
        assert_eq!(handler.priority(), 100);
    }

    #[tokio::test]
    async fn test_static_image_handler_creation() {
        let temp_dir = TempDir::new().unwrap();
        let handler =
            StaticHandler::new_image_handler(temp_dir.path().to_path_buf(), "/*path".to_string());

        assert_eq!(handler.path_pattern(), "/*path");
        assert_eq!(handler.method(), Method::GET);
        assert_eq!(handler.priority(), 100);

        // Should only allow image extensions
        assert!(handler.is_allowed_extension(Path::new("test.png")));
        assert!(handler.is_allowed_extension(Path::new("test.jpg")));
        assert!(!handler.is_allowed_extension(Path::new("test.css")));
        assert!(!handler.is_allowed_extension(Path::new("test.js")));
    }

    #[tokio::test]
    async fn test_markdown_handler_creation() {
        let temp_dir = TempDir::new().unwrap();
        let markdown_file = temp_dir.path().join("test.md");

        // Create a test markdown file
        fs::write(&markdown_file, "# Test\n\nThis is a test.")
            .await
            .unwrap();

        let handler = MarkdownHandler::new("/".to_string(), markdown_file);

        assert_eq!(handler.path_pattern(), "/");
        assert_eq!(handler.method(), Method::GET);
        assert_eq!(handler.priority(), 10);
    }

    #[tokio::test]
    async fn test_mermaid_handler_creation() {
        let handler = MermaidHandler::new("/mermaid.min.js".to_string());

        assert_eq!(handler.path_pattern(), "/mermaid.min.js");
        assert_eq!(handler.method(), Method::GET);
        assert_eq!(handler.priority(), 5);
    }

    #[tokio::test]
    async fn test_live_reload_handler_creation() {
        let handler = LiveReloadHandler::new("/ws".to_string());

        assert_eq!(handler.path(), "/ws");
        assert_eq!(handler.priority(), 1);
    }

    #[test]
    fn test_static_handler_content_type_guessing() {
        let temp_dir = TempDir::new().unwrap();
        let handler = StaticHandler::new(temp_dir.path().to_path_buf(), "/static".to_string());

        assert_eq!(
            handler.guess_content_type(Path::new("test.png")),
            "image/png"
        );
        assert_eq!(
            handler.guess_content_type(Path::new("test.css")),
            "text/css"
        );
        assert_eq!(
            handler.guess_content_type(Path::new("test.js")),
            "application/javascript"
        );
        assert_eq!(
            handler.guess_content_type(Path::new("test.unknown")),
            "application/octet-stream"
        );
    }

    #[test]
    fn test_static_handler_extension_checking() {
        let temp_dir = TempDir::new().unwrap();
        let handler = StaticHandler::new(temp_dir.path().to_path_buf(), "/static".to_string());

        assert!(handler.is_allowed_extension(Path::new("test.png")));
        assert!(handler.is_allowed_extension(Path::new("test.css")));
        assert!(!handler.is_allowed_extension(Path::new("test.exe")));
        assert!(!handler.is_allowed_extension(Path::new("test")));
    }

    #[test]
    fn test_client_message_serialization() {
        let ping_msg = ClientMessage::Ping;
        let json = serde_json::to_string(&ping_msg).unwrap();
        assert!(json.contains("Ping"));

        let refresh_msg = ClientMessage::RequestRefresh;
        let json = serde_json::to_string(&refresh_msg).unwrap();
        assert!(json.contains("RequestRefresh"));
    }

    #[test]
    fn test_server_message_serialization() {
        let reload_msg = ServerMessage::Reload;
        let json = serde_json::to_string(&reload_msg).unwrap();
        assert!(json.contains("Reload"));

        let pong_msg = ServerMessage::Pong;
        let json = serde_json::to_string(&pong_msg).unwrap();
        assert!(json.contains("Pong"));
    }
}
