//! Concrete handler implementations for the server plugin

use crate::{
    HttpHandler, HttpRequest, HttpResponse, WebSocketConnection, WebSocketHandler, WebSocketMessage,
};
use async_trait::async_trait;
use axum::http::{Method, StatusCode};
use rune_core::{
    error::{Result, RuneError},
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
            // Create render context
            let context = RenderContext::new(
                self.markdown_file.clone(),
                self.base_dir.clone(),
                "catppuccin-mocha".to_string(), // Default theme
            );

            // Render markdown to HTML
            let result = registry.render_content(content, &context).await?;

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
    Reload,
    Pong,
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
