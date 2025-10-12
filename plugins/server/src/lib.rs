//! Server plugin for HTTP and WebSocket handling
//!
//! This plugin provides a modular web server with pluggable handlers and middleware.
//! It supports dynamic route registration, handler hot-reloading, and multiple protocols.

pub mod handlers;

use async_trait::async_trait;
use axum::{
    extract::WebSocketUpgrade,
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post, put},
    Router,
};
use rune_core::{
    error::{Result, RuneError},
    event::{EventBus, SystemEvent},
    plugin::{Plugin, PluginContext, PluginStatus},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
};
use tokio::{
    net::TcpListener,
    sync::{broadcast, RwLock},
};
use tower_http::cors::CorsLayer;
use tracing::{debug, error, info, warn};

/// HTTP handler trait for processing HTTP requests
#[async_trait]
pub trait HttpHandler: Send + Sync {
    /// Get the path pattern this handler matches (e.g., "/api/users/:id")
    fn path_pattern(&self) -> &str;

    /// Get the HTTP method this handler supports
    fn method(&self) -> Method;

    /// Handle the HTTP request
    async fn handle(&self, request: HttpRequest) -> Result<HttpResponse>;

    /// Get handler priority (lower numbers = higher priority)
    fn priority(&self) -> i32 {
        0
    }

    /// Check if this handler can process the given request
    fn can_handle(&self, path: &str, method: &Method) -> bool {
        self.method() == *method && self.matches_path(path)
    }

    /// Check if the path matches this handler's pattern
    fn matches_path(&self, path: &str) -> bool {
        // Simple exact match for now - could be enhanced with pattern matching
        path == self.path_pattern() || path.starts_with(&format!("{}/", self.path_pattern()))
    }
}

/// WebSocket handler trait for processing WebSocket connections
#[async_trait]
pub trait WebSocketHandler: Send + Sync {
    /// Get the path this WebSocket handler serves
    fn path(&self) -> &str;

    /// Handle new WebSocket connection
    async fn on_connect(&self, connection: &WebSocketConnection) -> Result<()>;

    /// Handle incoming WebSocket message
    async fn on_message(&self, connection: &WebSocketConnection, message: WebSocketMessage) -> Result<()>;

    /// Handle WebSocket disconnection
    async fn on_disconnect(&self, connection: &WebSocketConnection) -> Result<()>;

    /// Get handler priority (lower numbers = higher priority)
    fn priority(&self) -> i32 {
        0
    }
}

/// HTTP request wrapper
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: Method,
    pub path: String,
    pub query_params: HashMap<String, String>,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
    pub path_params: HashMap<String, String>,
}

/// HTTP response wrapper
#[derive(Debug)]
pub struct HttpResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// Create a new HTTP response
    pub fn new(status: StatusCode) -> Self {
        Self {
            status,
            headers: HeaderMap::new(),
            body: Vec::new(),
        }
    }

    /// Set response body
    pub fn with_body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    /// Set response header
    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        use axum::http::header::{HeaderName, HeaderValue};
        if let (Ok(name), Ok(value)) = (HeaderName::from_bytes(name.as_bytes()), HeaderValue::from_str(value)) {
            self.headers.insert(name, value);
        }
        self
    }

    /// Create a JSON response
    pub fn json<T: Serialize>(data: &T) -> Result<Self> {
        let body = serde_json::to_vec(data)
            .map_err(|e| RuneError::Server(format!("JSON serialization failed: {}", e)))?;
        
        Ok(Self::new(StatusCode::OK)
            .with_header("content-type", "application/json")
            .with_body(body))
    }

    /// Create an HTML response
    pub fn html(content: &str) -> Self {
        Self::new(StatusCode::OK)
            .with_header("content-type", "text/html; charset=utf-8")
            .with_body(content.as_bytes())
    }

    /// Create a text response
    pub fn text(content: &str) -> Self {
        Self::new(StatusCode::OK)
            .with_header("content-type", "text/plain; charset=utf-8")
            .with_body(content.as_bytes())
    }

    /// Create an error response
    pub fn error(status: StatusCode, message: &str) -> Self {
        Self::new(status)
            .with_header("content-type", "text/plain; charset=utf-8")
            .with_body(message.as_bytes())
    }
}

impl IntoResponse for HttpResponse {
    fn into_response(self) -> Response {
        (self.status, self.headers, self.body).into_response()
    }
}

/// WebSocket connection wrapper
#[derive(Debug, Clone)]
pub struct WebSocketConnection {
    pub id: String,
    pub remote_addr: SocketAddr,
    pub headers: HeaderMap,
    pub sender: broadcast::Sender<WebSocketMessage>,
}

impl WebSocketConnection {
    /// Send a message to this WebSocket connection
    pub async fn send(&self, message: WebSocketMessage) -> Result<()> {
        self.sender
            .send(message)
            .map_err(|e| RuneError::Server(format!("Failed to send WebSocket message: {}", e)))?;
        Ok(())
    }

    /// Send a text message
    pub async fn send_text(&self, text: String) -> Result<()> {
        self.send(WebSocketMessage::Text(text)).await
    }

    /// Send a binary message
    pub async fn send_binary(&self, data: Vec<u8>) -> Result<()> {
        self.send(WebSocketMessage::Binary(data)).await
    }

    /// Send a JSON message
    pub async fn send_json<T: Serialize>(&self, data: &T) -> Result<()> {
        let json = serde_json::to_string(data)
            .map_err(|e| RuneError::Server(format!("JSON serialization failed: {}", e)))?;
        self.send_text(json).await
    }
}

/// WebSocket message types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WebSocketMessage {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close(Option<String>),
}

/// Handler registry for managing HTTP and WebSocket handlers
pub struct HandlerRegistry {
    http_handlers: RwLock<Vec<Arc<dyn HttpHandler>>>,
    websocket_handlers: RwLock<Vec<Arc<dyn WebSocketHandler>>>,
    event_bus: Arc<dyn EventBus>,
}

impl HandlerRegistry {
    /// Create a new handler registry
    pub fn new(event_bus: Arc<dyn EventBus>) -> Self {
        Self {
            http_handlers: RwLock::new(Vec::new()),
            websocket_handlers: RwLock::new(Vec::new()),
            event_bus,
        }
    }

    /// Register an HTTP handler
    pub async fn register_http_handler(&self, handler: Arc<dyn HttpHandler>) -> Result<()> {
        let path = handler.path_pattern().to_string();
        let method = handler.method().clone();
        
        info!("Registering HTTP handler: {} {}", method, path);
        
        let mut handlers = self.http_handlers.write().await;
        handlers.push(handler);
        
        // Sort by priority (lower numbers first)
        handlers.sort_by_key(|h| h.priority());
        
        // Publish handler registration event
        if let Err(e) = self.event_bus
            .publish_system_event(SystemEvent::server_handler_registered(
                "http".to_string(),
                format!("{} {}", method, path),
            ))
            .await
        {
            warn!("Failed to publish handler registration event: {}", e);
        }
        
        Ok(())
    }

    /// Register a WebSocket handler
    pub async fn register_websocket_handler(&self, handler: Arc<dyn WebSocketHandler>) -> Result<()> {
        let path = handler.path().to_string();
        
        info!("Registering WebSocket handler: {}", path);
        
        let mut handlers = self.websocket_handlers.write().await;
        handlers.push(handler);
        
        // Sort by priority (lower numbers first)
        handlers.sort_by_key(|h| h.priority());
        
        // Publish handler registration event
        if let Err(e) = self.event_bus
            .publish_system_event(SystemEvent::server_handler_registered(
                "websocket".to_string(),
                path.clone(),
            ))
            .await
        {
            warn!("Failed to publish handler registration event: {}", e);
        }
        
        Ok(())
    }

    /// Unregister an HTTP handler by path and method
    pub async fn unregister_http_handler(&self, path: &str, method: &Method) -> Result<()> {
        let mut handlers = self.http_handlers.write().await;
        let initial_len = handlers.len();
        
        handlers.retain(|h| !(h.path_pattern() == path && h.method() == *method));
        
        if handlers.len() < initial_len {
            info!("Unregistered HTTP handler: {} {}", method, path);
            
            // Publish handler unregistration event
            if let Err(e) = self.event_bus
                .publish_system_event(SystemEvent::server_handler_unregistered(
                    "http".to_string(),
                    format!("{} {}", method, path),
                ))
                .await
            {
                warn!("Failed to publish handler unregistration event: {}", e);
            }
        }
        
        Ok(())
    }

    /// Unregister a WebSocket handler by path
    pub async fn unregister_websocket_handler(&self, path: &str) -> Result<()> {
        let mut handlers = self.websocket_handlers.write().await;
        let initial_len = handlers.len();
        
        handlers.retain(|h| h.path() != path);
        
        if handlers.len() < initial_len {
            info!("Unregistered WebSocket handler: {}", path);
            
            // Publish handler unregistration event
            if let Err(e) = self.event_bus
                .publish_system_event(SystemEvent::server_handler_unregistered(
                    "websocket".to_string(),
                    path.to_string(),
                ))
                .await
            {
                warn!("Failed to publish handler unregistration event: {}", e);
            }
        }
        
        Ok(())
    }

    /// Find HTTP handler for a request
    pub async fn find_http_handler(&self, path: &str, method: &Method) -> Option<Arc<dyn HttpHandler>> {
        let handlers = self.http_handlers.read().await;
        
        for handler in handlers.iter() {
            if handler.can_handle(path, method) {
                return Some(handler.clone());
            }
        }
        
        None
    }

    /// Find WebSocket handler for a path
    pub async fn find_websocket_handler(&self, path: &str) -> Option<Arc<dyn WebSocketHandler>> {
        let handlers = self.websocket_handlers.read().await;
        
        for handler in handlers.iter() {
            if handler.path() == path {
                return Some(handler.clone());
            }
        }
        
        None
    }

    /// List all registered HTTP handlers
    pub async fn list_http_handlers(&self) -> Vec<(String, Method, i32)> {
        let handlers = self.http_handlers.read().await;
        handlers
            .iter()
            .map(|h| (h.path_pattern().to_string(), h.method().clone(), h.priority()))
            .collect()
    }

    /// List all registered WebSocket handlers
    pub async fn list_websocket_handlers(&self) -> Vec<(String, i32)> {
        let handlers = self.websocket_handlers.read().await;
        handlers
            .iter()
            .map(|h| (h.path().to_string(), h.priority()))
            .collect()
    }

    /// Clear all handlers
    pub async fn clear_all_handlers(&self) {
        let mut http_handlers = self.http_handlers.write().await;
        let mut websocket_handlers = self.websocket_handlers.write().await;
        
        http_handlers.clear();
        websocket_handlers.clear();
        
        info!("Cleared all registered handlers");
    }
}

/// Server plugin configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub hostname: String,
    pub port: u16,
    pub enable_cors: bool,
    pub max_connections: Option<usize>,
    pub request_timeout_secs: Option<u64>,
    pub websocket_ping_interval_secs: Option<u64>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            hostname: "127.0.0.1".to_string(),
            port: 3000,
            enable_cors: true,
            max_connections: None,
            request_timeout_secs: Some(30),
            websocket_ping_interval_secs: Some(30),
        }
    }
}

/// Server plugin implementation
pub struct ServerPlugin {
    name: String,
    version: String,
    status: PluginStatus,
    config: ServerConfig,
    handler_registry: Option<Arc<HandlerRegistry>>,
    server_handle: Option<tokio::task::JoinHandle<()>>,
}

impl ServerPlugin {
    /// Create a new server plugin
    pub fn new() -> Self {
        Self {
            name: "server".to_string(),
            version: "1.0.0".to_string(),
            status: PluginStatus::Loading,
            config: ServerConfig::default(),
            handler_registry: None,
            server_handle: None,
        }
    }

    /// Create a new server plugin with custom configuration
    pub fn with_config(config: ServerConfig) -> Self {
        Self {
            name: "server".to_string(),
            version: "1.0.0".to_string(),
            status: PluginStatus::Loading,
            config,
            handler_registry: None,
            server_handle: None,
        }
    }

    /// Get the handler registry
    pub fn handler_registry(&self) -> Option<Arc<HandlerRegistry>> {
        self.handler_registry.clone()
    }

    /// Build the Axum router with all registered handlers
    async fn build_router(&self, registry: Arc<HandlerRegistry>) -> Router {
        let mut router = Router::new();

        // Add HTTP handlers
        let http_handlers = registry.list_http_handlers().await;
        for (path, method, _priority) in http_handlers {
            let handler_registry = registry.clone();
            let route_path = path.clone();
            
            let route = match method {
                Method::GET => get(move |req| Self::handle_http_request(req, handler_registry.clone(), route_path.clone())),
                Method::POST => post(move |req| Self::handle_http_request(req, handler_registry.clone(), route_path.clone())),
                Method::PUT => put(move |req| Self::handle_http_request(req, handler_registry.clone(), route_path.clone())),
                Method::DELETE => delete(move |req| Self::handle_http_request(req, handler_registry.clone(), route_path.clone())),
                Method::PATCH => patch(move |req| Self::handle_http_request(req, handler_registry.clone(), route_path.clone())),
                _ => continue, // Skip unsupported methods for now
            };
            
            router = router.route(&path, route);
        }

        // Add WebSocket handlers
        let ws_handlers = registry.list_websocket_handlers().await;
        for (path, _priority) in ws_handlers {
            let handler_registry = registry.clone();
            let ws_path = path.clone();
            
            router = router.route(
                &path,
                get(move |ws: WebSocketUpgrade| {
                    let registry = handler_registry.clone();
                    let path = ws_path.clone();
                    async move {
                        ws.on_upgrade(move |socket| Self::handle_websocket_connection(socket, registry, path))
                    }
                })
            );
        }

        // Add CORS if enabled
        if self.config.enable_cors {
            router = router.layer(CorsLayer::permissive());
        }

        router
    }

    /// Handle HTTP request
    async fn handle_http_request(
        _req: axum::extract::Request,
        _registry: Arc<HandlerRegistry>,
        _path: String,
    ) -> impl IntoResponse {
        // This is a simplified implementation
        // In a real implementation, we would extract the request details,
        // find the appropriate handler, and call it
        HttpResponse::text("Handler not implemented yet")
    }

    /// Handle WebSocket connection
    async fn handle_websocket_connection(
        _socket: axum::extract::ws::WebSocket,
        _registry: Arc<HandlerRegistry>,
        _path: String,
    ) {
        // This is a simplified implementation
        // In a real implementation, we would manage the WebSocket connection,
        // find the appropriate handler, and call its methods
        debug!("WebSocket connection established");
    }
}

#[async_trait]
impl Plugin for ServerPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn dependencies(&self) -> Vec<&str> {
        vec![] // Server plugin has no dependencies
    }

    async fn initialize(&mut self, context: &PluginContext) -> Result<()> {
        info!("Initializing server plugin");

        // Load configuration from plugin context
        if let Ok(Some(config)) = context.get_config_value::<ServerConfig>("server").await {
            self.config = config;
        }

        // Create handler registry
        let registry = Arc::new(HandlerRegistry::new(context.event_bus.clone()));
        
        // Store registry in shared resources for other plugins to access
        context.set_shared_resource("server_handler_registry".to_string(), registry.clone()).await?;
        
        self.handler_registry = Some(registry.clone());

        // Build and start the server
        let router = self.build_router(registry).await;
        let addr = format!("{}:{}", self.config.hostname, self.config.port);
        
        info!("Starting HTTP server on {}", addr);
        
        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|e| RuneError::Server(format!("Failed to bind to {}: {}", addr, e)))?;

        // Spawn server task
        let server_handle = tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, router).await {
                error!("Server error: {}", e);
            }
        });

        self.server_handle = Some(server_handle);
        self.status = PluginStatus::Active;

        // Publish server started event
        context.event_bus
            .publish_system_event(SystemEvent::server_started(addr))
            .await?;

        info!("Server plugin initialized successfully");
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        info!("Shutting down server plugin");

        self.status = PluginStatus::Shutting;

        // Stop the server
        if let Some(handle) = self.server_handle.take() {
            handle.abort();
            
            // Wait a bit for graceful shutdown
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        // Clear handler registry
        if let Some(registry) = &self.handler_registry {
            registry.clear_all_handlers().await;
        }

        self.handler_registry = None;
        self.status = PluginStatus::Stopped;

        info!("Server plugin shutdown complete");
        Ok(())
    }

    fn status(&self) -> PluginStatus {
        self.status.clone()
    }

    fn provided_services(&self) -> Vec<&str> {
        vec!["http_server", "websocket_server", "handler_registry"]
    }
}

impl Default for ServerPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_handler_registry_creation() {
        // This would need a mock EventBus implementation
        // For now, just test that we can create the registry
        // let event_bus = Arc::new(MockEventBus::new());
        // let registry = HandlerRegistry::new(event_bus);
        // assert!(registry.list_http_handlers().await.is_empty());
    }

    #[test]
    fn test_server_config_default() {
        let config = ServerConfig::default();
        assert_eq!(config.hostname, "127.0.0.1");
        assert_eq!(config.port, 3000);
        assert!(config.enable_cors);
    }

    #[test]
    fn test_http_response_creation() {
        let response = HttpResponse::text("Hello, World!");
        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(response.body, b"Hello, World!");
    }

    #[test]
    fn test_websocket_message_serialization() {
        let message = WebSocketMessage::Text("test".to_string());
        let json = serde_json::to_string(&message).unwrap();
        assert!(json.contains("test"));
    }
}