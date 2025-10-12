//! Concrete handler implementations for the server plugin

use crate::{HttpHandler, HttpRequest, HttpResponse, WebSocketHandler, WebSocketConnection, WebSocketMessage};
use async_trait::async_trait;
use axum::http::{Method, StatusCode};
use rune_core::error::Result;
use std::path::{Path, PathBuf};
use std::fs;
use tracing::{debug, warn};

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
        let requested_path = request.path.strip_prefix(&self.path_pattern)
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
                    warn!("Access denied for path outside base directory: {:?}", canonical_path);
                    return Ok(HttpResponse::error(StatusCode::FORBIDDEN, "Access denied"));
                }

                // Check if file extension is allowed
                if !self.is_allowed_extension(&canonical_path) {
                    return Ok(HttpResponse::error(StatusCode::FORBIDDEN, "File type not allowed"));
                }

                // Try to read and serve the file
                match fs::read(&canonical_path) {
                    Ok(contents) => {
                        let content_type = self.guess_content_type(&canonical_path);
                        debug!("Serving static file: {:?} ({})", canonical_path, content_type);
                        
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

/// Markdown handler for serving rendered markdown content
pub struct MarkdownHandler {
    path_pattern: String,
    markdown_file: PathBuf,
}

impl MarkdownHandler {
    /// Create a new markdown handler
    pub fn new(path_pattern: String, markdown_file: PathBuf) -> Self {
        Self {
            path_pattern,
            markdown_file,
        }
    }

    /// Render markdown content to HTML
    fn render_markdown(&self, content: &str) -> Result<String> {
        // This is a simplified implementation
        // In a real implementation, this would use the renderer plugin
        Ok(format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>Markdown Preview</title>
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; margin: 2rem; }}
        pre {{ background: #f5f5f5; padding: 1rem; border-radius: 4px; }}
        code {{ background: #f5f5f5; padding: 0.2rem 0.4rem; border-radius: 2px; }}
    </style>
</head>
<body>
    <pre>{}</pre>
</body>
</html>"#,
            html_escape::encode_text(content)
        ))
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
        match fs::read_to_string(&self.markdown_file) {
            Ok(content) => {
                let html = self.render_markdown(&content)?;
                debug!("Serving markdown file: {:?}", self.markdown_file);
                Ok(HttpResponse::html(&html))
            }
            Err(e) => {
                warn!("Failed to read markdown file {:?}: {}", self.markdown_file, e);
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
                warn!("Failed to read markdown file {:?}: {}", self.markdown_file, e);
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

/// WebSocket handler for live reload functionality
pub struct LiveReloadHandler {
    path: String,
}

impl LiveReloadHandler {
    /// Create a new live reload handler
    pub fn new(path: String) -> Self {
        Self { path }
    }
}

#[async_trait]
impl WebSocketHandler for LiveReloadHandler {
    fn path(&self) -> &str {
        &self.path
    }

    async fn on_connect(&self, connection: &WebSocketConnection) -> Result<()> {
        debug!("WebSocket client connected: {}", connection.id);
        
        // Send a welcome message
        connection
            .send_json(&serde_json::json!({
                "type": "welcome",
                "message": "Connected to live reload server"
            }))
            .await?;
        
        Ok(())
    }

    async fn on_message(&self, connection: &WebSocketConnection, message: WebSocketMessage) -> Result<()> {
        match message {
            WebSocketMessage::Text(text) => {
                debug!("Received WebSocket message from {}: {}", connection.id, text);
                
                // Try to parse as JSON and handle different message types
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(msg_type) = parsed.get("type").and_then(|t| t.as_str()) {
                        match msg_type {
                            "ping" => {
                                connection
                                    .send_json(&serde_json::json!({
                                        "type": "pong"
                                    }))
                                    .await?;
                            }
                            "request_refresh" => {
                                // In a real implementation, this would trigger a content refresh
                                debug!("Client {} requested refresh", connection.id);
                            }
                            _ => {
                                debug!("Unknown message type: {}", msg_type);
                            }
                        }
                    }
                }
            }
            WebSocketMessage::Ping(data) => {
                connection.send(WebSocketMessage::Pong(data)).await?;
            }
            WebSocketMessage::Close(_) => {
                debug!("WebSocket client {} requested close", connection.id);
            }
            _ => {
                debug!("Received other WebSocket message type from {}", connection.id);
            }
        }
        
        Ok(())
    }

    async fn on_disconnect(&self, connection: &WebSocketConnection) -> Result<()> {
        debug!("WebSocket client disconnected: {}", connection.id);
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

    #[tokio::test]
    async fn test_static_handler_creation() {
        let temp_dir = TempDir::new().unwrap();
        let handler = StaticHandler::new(
            temp_dir.path().to_path_buf(),
            "/static".to_string(),
        );
        
        assert_eq!(handler.path_pattern(), "/static");
        assert_eq!(handler.method(), Method::GET);
        assert_eq!(handler.priority(), 100);
    }

    #[tokio::test]
    async fn test_markdown_handler_creation() {
        let temp_dir = TempDir::new().unwrap();
        let markdown_file = temp_dir.path().join("test.md");
        
        let handler = MarkdownHandler::new(
            "/".to_string(),
            markdown_file,
        );
        
        assert_eq!(handler.path_pattern(), "/");
        assert_eq!(handler.method(), Method::GET);
        assert_eq!(handler.priority(), 10);
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
        let handler = StaticHandler::new(
            temp_dir.path().to_path_buf(),
            "/static".to_string(),
        );
        
        assert_eq!(handler.guess_content_type(Path::new("test.png")), "image/png");
        assert_eq!(handler.guess_content_type(Path::new("test.css")), "text/css");
        assert_eq!(handler.guess_content_type(Path::new("test.js")), "application/javascript");
        assert_eq!(handler.guess_content_type(Path::new("test.unknown")), "application/octet-stream");
    }

    #[test]
    fn test_static_handler_extension_checking() {
        let temp_dir = TempDir::new().unwrap();
        let handler = StaticHandler::new(
            temp_dir.path().to_path_buf(),
            "/static".to_string(),
        );
        
        assert!(handler.is_allowed_extension(Path::new("test.png")));
        assert!(handler.is_allowed_extension(Path::new("test.css")));
        assert!(!handler.is_allowed_extension(Path::new("test.exe")));
        assert!(!handler.is_allowed_extension(Path::new("test")));
    }
}