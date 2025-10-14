#![allow(dead_code)]
#![allow(unused_variables)]
//! Server plugin for HTTP and WebSocket handling
//!
//! This plugin provides a modular web server with pluggable handlers and middleware.
//! It supports dynamic route registration, handler hot-reloading, and multiple protocols.

pub mod handlers;

use async_trait::async_trait;
use axum::{
    extract::{FromRequest, WebSocketUpgrade},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    Router,
};
use rune_core::{
    error::{Result, RuneError},
    event::{EventBus, SystemEvent},
    plugin::{Plugin, PluginContext, PluginStatus},
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::{
    net::TcpListener,
    sync::{broadcast, RwLock},
};
use tower_http::cors::CorsLayer;
use tracing::{error, info, warn};

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
        let pattern = self.path_pattern();

        // Handle root path specially
        if pattern == "/" {
            return path == "/";
        }

        // For other paths, check exact match or prefix match
        path == pattern || path.starts_with(&format!("{}/", pattern))
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
    async fn on_message(
        &self,
        connection: &WebSocketConnection,
        message: WebSocketMessage,
    ) -> Result<()>;

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
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) {
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
#[serde(tag = "type", content = "data")]
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
        if let Err(e) = self
            .event_bus
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
    pub async fn register_websocket_handler(
        &self,
        handler: Arc<dyn WebSocketHandler>,
    ) -> Result<()> {
        let path = handler.path().to_string();

        info!("Registering WebSocket handler: {}", path);

        let mut handlers = self.websocket_handlers.write().await;
        handlers.push(handler);

        // Sort by priority (lower numbers first)
        handlers.sort_by_key(|h| h.priority());

        // Publish handler registration event
        if let Err(e) = self
            .event_bus
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
            if let Err(e) = self
                .event_bus
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
            if let Err(e) = self
                .event_bus
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
    pub async fn find_http_handler(
        &self,
        path: &str,
        method: &Method,
    ) -> Option<Arc<dyn HttpHandler>> {
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
            .map(|h| {
                (
                    h.path_pattern().to_string(),
                    h.method().clone(),
                    h.priority(),
                )
            })
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
    reload_sender: Option<tokio::sync::broadcast::Sender<handlers::ServerMessage>>,
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
            reload_sender: None,
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
            reload_sender: None,
        }
    }

    /// Get the handler registry
    pub fn handler_registry(&self) -> Option<Arc<HandlerRegistry>> {
        self.handler_registry.clone()
    }

    /// Register core handlers (markdown, static files, etc.)
    async fn register_core_handlers(&self, context: &PluginContext) -> Result<()> {
        if let Some(registry) = &self.handler_registry {
            // Get current file from application state
            let state = context.state_manager.get_state().await;

            if let Some(current_file) = state.current_file {
                self.register_file_handlers(&current_file, context).await?;
            } else {
                info!("No current file set during initialization, will register handlers when file is set");
            }
        }
        Ok(())
    }

    /// Register handlers for a specific file
    async fn register_file_handlers(
        &self,
        current_file: &std::path::Path,
        context: &PluginContext,
    ) -> Result<()> {
        if let Some(registry) = &self.handler_registry {
            info!(
                "Registering markdown handler for file: {}",
                current_file.display()
            );

            // Get renderer registry from shared resources if available
            let renderer_registry = context
                .get_shared_resource::<rune_core::renderer::RendererRegistry>("renderer_registry")
                .await;

            // Register main markdown handler for root path
            let markdown_handler = if let Some(renderer_registry) = renderer_registry {
                Arc::new(handlers::MarkdownHandler::with_renderer_registry(
                    "/".to_string(),
                    current_file.to_path_buf(),
                    renderer_registry,
                ))
            } else {
                Arc::new(handlers::MarkdownHandler::new(
                    "/".to_string(),
                    current_file.to_path_buf(),
                ))
            };

            registry.register_http_handler(markdown_handler).await?;

            // Register raw markdown handler
            let raw_handler = Arc::new(handlers::RawMarkdownHandler::new(
                "/raw".to_string(),
                current_file.to_path_buf(),
            ));
            registry.register_http_handler(raw_handler).await?;

            // Register static file handler for assets in the same directory
            if let Some(base_dir) = current_file.parent() {
                let static_handler = Arc::new(handlers::StaticHandler::new(
                    base_dir.to_path_buf(),
                    "/assets".to_string(),
                ));
                registry.register_http_handler(static_handler).await?;

                // Also register image handler for images in the same directory
                let image_handler = Arc::new(handlers::StaticHandler::new_image_handler(
                    base_dir.to_path_buf(),
                    "/images".to_string(),
                ));
                registry.register_http_handler(image_handler).await?;
            }

            info!(
                "File handlers registered successfully for: {}",
                current_file.display()
            );
        }
        Ok(())
    }

    /// Register WebSocket handlers for live reload
    async fn register_websocket_handlers(&self, event_bus: Arc<dyn EventBus>) -> Result<()> {
        if let Some(registry) = &self.handler_registry {
            // Create a broadcast channel for reload messages
            let (reload_sender, _) = broadcast::channel::<handlers::ServerMessage>(16);

            // Register live reload WebSocket handler
            let live_reload_handler = Arc::new(handlers::LiveReloadHandler::with_reload_sender(
                "/ws".to_string(),
                reload_sender.clone(),
            ));

            registry
                .register_websocket_handler(live_reload_handler.clone())
                .await?;

            // Create and register a file change event handler that will trigger reloads
            let reload_event_handler = Arc::new(LiveReloadEventHandler {
                reload_sender,
                live_reload_handler,
            });

            event_bus
                .subscribe_system_events(reload_event_handler)
                .await?;

            info!("WebSocket handlers registered successfully");
        }
        Ok(())
    }

    /// Register theme asset handlers
    pub async fn register_theme_handlers(&self, event_bus: Arc<dyn EventBus>) -> Result<()> {
        if let Some(registry) = &self.handler_registry {
            // Register theme asset handler
            let theme_asset_handler = Arc::new(handlers::ThemeAssetHandler::with_event_bus(
                "/themes".to_string(),
                event_bus.clone(),
            ));
            registry.register_http_handler(theme_asset_handler).await?;

            // Register theme API handler for POST requests
            let theme_api_handler = Arc::new(handlers::ThemeApiHandler::new(
                "/api/theme".to_string(),
                event_bus.clone(),
            ));
            registry.register_http_handler(theme_api_handler).await?;

            // Register theme API handler for GET requests (separate handler for different method)
            let theme_info_handler = Arc::new(handlers::ThemeInfoHandler::new(
                "/api/theme".to_string(),
                event_bus,
            ));
            registry.register_http_handler(theme_info_handler).await?;

            tracing::info!("Registered theme asset and API handlers");
        }
        Ok(())
    }

    /// Build the Axum router with all registered handlers
    async fn build_router(&self, registry: Arc<HandlerRegistry>) -> Router {
        let registry_clone = registry.clone();

        // Create a catch-all router that dynamically handles requests
        let router = Router::new().fallback(move |req| {
            let registry = registry_clone.clone();
            async move { Self::handle_dynamic_request(req, registry).await }
        });

        // Add CORS if enabled
        if self.config.enable_cors {
            router.layer(CorsLayer::permissive())
        } else {
            router
        }
    }

    /// Handle dynamic HTTP request (catch-all handler)
    async fn handle_dynamic_request(
        req: axum::extract::Request,
        registry: Arc<HandlerRegistry>,
    ) -> Response {
        // Check if this is a WebSocket upgrade request
        if req.headers().get("upgrade").and_then(|v| v.to_str().ok()) == Some("websocket") {
            return Self::handle_websocket_upgrade(req, registry)
                .await
                .into_response();
        }

        Self::handle_http_request(req, registry)
            .await
            .into_response()
    }

    /// Handle WebSocket upgrade request
    async fn handle_websocket_upgrade(
        req: axum::extract::Request,
        registry: Arc<HandlerRegistry>,
    ) -> Response {
        let path = req.uri().path().to_string();

        if let Some(_handler) = registry.find_websocket_handler(&path).await {
            // Handle WebSocket upgrade
            let ws_upgrade = WebSocketUpgrade::from_request(req, &()).await;
            match ws_upgrade {
                Ok(upgrade) => upgrade
                    .on_upgrade(move |socket| {
                        Self::handle_websocket_connection(socket, registry, path)
                    })
                    .into_response(),
                Err(_) => HttpResponse::error(StatusCode::BAD_REQUEST, "Invalid WebSocket upgrade")
                    .into_response(),
            }
        } else {
            HttpResponse::error(StatusCode::NOT_FOUND, "WebSocket handler not found")
                .into_response()
        }
    }

    /// Handle HTTP request
    async fn handle_http_request(
        req: axum::extract::Request,
        registry: Arc<HandlerRegistry>,
    ) -> Response {
        use std::collections::HashMap;

        // Extract request details
        let method = req.method().clone();
        let uri = req.uri().clone();
        let path = uri.path().to_string();
        let headers = req.headers().clone();

        // Extract query parameters
        let query_params: HashMap<String, String> = uri
            .query()
            .map(|q| {
                url::form_urlencoded::parse(q.as_bytes())
                    .into_owned()
                    .collect()
            })
            .unwrap_or_default();

        // Extract body
        let (_parts, body) = req.into_parts();
        let body_bytes = match axum::body::to_bytes(body, usize::MAX).await {
            Ok(bytes) => bytes.to_vec(),
            Err(_) => Vec::new(),
        };

        // Create HttpRequest
        let http_request = HttpRequest {
            method: method.clone(),
            path: path.clone(),
            query_params,
            headers,
            body: body_bytes,
            path_params: HashMap::new(), // TODO: Extract path parameters
        };

        // Find and call the appropriate handler
        if let Some(handler) = registry.find_http_handler(&path, &method).await {
            match handler.handle(http_request).await {
                Ok(response) => response.into_response(),
                Err(e) => {
                    tracing::error!("Handler error for {} {}: {}", method, path, e);
                    HttpResponse::error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
                        .into_response()
                }
            }
        } else {
            tracing::debug!("No handler found for {} {}", method, path);
            HttpResponse::error(StatusCode::NOT_FOUND, "Not found").into_response()
        }
    }

    /// Handle WebSocket connection
    async fn handle_websocket_connection(
        socket: axum::extract::ws::WebSocket,
        registry: Arc<HandlerRegistry>,
        path: String,
    ) {
        use futures_util::{SinkExt, StreamExt};
        use uuid::Uuid;

        // Generate connection ID
        let connection_id = Uuid::new_v4().to_string();

        // Create broadcast channel for this connection
        let (tx, _rx) = broadcast::channel::<WebSocketMessage>(16);

        // Create WebSocketConnection
        let connection = WebSocketConnection {
            id: connection_id.clone(),
            remote_addr: "127.0.0.1:0".parse().unwrap(), // TODO: Get real remote addr
            headers: HeaderMap::new(),
            sender: tx,
        };

        // Find the WebSocket handler
        if let Some(handler) = registry.find_websocket_handler(&path).await {
            // Notify handler of connection
            if let Err(e) = handler.on_connect(&connection).await {
                tracing::error!("WebSocket handler on_connect error: {}", e);
                return;
            }

            let (mut ws_sender, mut ws_receiver) = socket.split();
            let mut rx = connection.sender.subscribe();

            // Spawn task to handle outgoing messages
            let send_task = tokio::spawn(async move {
                while let Ok(msg) = rx.recv().await {
                    let ws_msg = match msg {
                        WebSocketMessage::Text(text) => axum::extract::ws::Message::Text(text),
                        WebSocketMessage::Binary(data) => axum::extract::ws::Message::Binary(data),
                        WebSocketMessage::Ping(data) => axum::extract::ws::Message::Ping(data),
                        WebSocketMessage::Pong(data) => axum::extract::ws::Message::Pong(data),
                        WebSocketMessage::Close(reason) => {
                            axum::extract::ws::Message::Close(reason.map(|r| {
                                axum::extract::ws::CloseFrame {
                                    code: axum::extract::ws::close_code::NORMAL,
                                    reason: r.into(),
                                }
                            }))
                        }
                    };

                    if ws_sender.send(ws_msg).await.is_err() {
                        break;
                    }
                }
            });

            // Handle incoming messages
            while let Some(msg) = ws_receiver.next().await {
                match msg {
                    Ok(axum::extract::ws::Message::Text(text)) => {
                        let ws_msg = WebSocketMessage::Text(text);
                        if let Err(e) = handler.on_message(&connection, ws_msg).await {
                            tracing::error!("WebSocket handler on_message error: {}", e);
                        }
                    }
                    Ok(axum::extract::ws::Message::Binary(data)) => {
                        let ws_msg = WebSocketMessage::Binary(data);
                        if let Err(e) = handler.on_message(&connection, ws_msg).await {
                            tracing::error!("WebSocket handler on_message error: {}", e);
                        }
                    }
                    Ok(axum::extract::ws::Message::Ping(data)) => {
                        let ws_msg = WebSocketMessage::Ping(data);
                        if let Err(e) = handler.on_message(&connection, ws_msg).await {
                            tracing::error!("WebSocket handler on_message error: {}", e);
                        }
                    }
                    Ok(axum::extract::ws::Message::Pong(data)) => {
                        let ws_msg = WebSocketMessage::Pong(data);
                        if let Err(e) = handler.on_message(&connection, ws_msg).await {
                            tracing::error!("WebSocket handler on_message error: {}", e);
                        }
                    }
                    Ok(axum::extract::ws::Message::Close(frame)) => {
                        let reason = frame.map(|f| f.reason.to_string());
                        let ws_msg = WebSocketMessage::Close(reason);
                        if let Err(e) = handler.on_message(&connection, ws_msg).await {
                            tracing::error!("WebSocket handler on_message error: {}", e);
                        }
                        break;
                    }
                    Err(e) => {
                        tracing::error!("WebSocket error: {}", e);
                        break;
                    }
                }
            }

            // Clean up
            send_task.abort();
            if let Err(e) = handler.on_disconnect(&connection).await {
                tracing::error!("WebSocket handler on_disconnect error: {}", e);
            }
        } else {
            tracing::debug!("No WebSocket handler found for path: {}", path);
        }
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
        context
            .set_shared_resource("server_handler_registry".to_string(), registry.clone())
            .await?;

        self.handler_registry = Some(registry.clone());

        // Subscribe to system events to handle file changes
        // Note: We no longer start our own file monitoring - we rely on the FileWatcher plugin
        let server_event_handler = Arc::new(ServerEventHandler {
            plugin_context: context.clone(),
            handler_registry: registry.clone(),
        });

        context
            .event_bus
            .subscribe_system_events(server_event_handler)
            .await?;

        info!("Server plugin will rely on FileWatcher plugin for file change detection");

        // Register core handlers
        self.register_core_handlers(context).await?;

        // Register theme asset handlers
        self.register_theme_handlers(context.event_bus.clone())
            .await?;

        // Register WebSocket handlers
        self.register_websocket_handlers(context.event_bus.clone())
            .await?;

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
        context
            .event_bus
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

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl Default for ServerPlugin {
    fn default() -> Self {
        Self::new()
    }
}

/// Event handler for server plugin to respond to system events
struct ServerEventHandler {
    plugin_context: PluginContext,
    handler_registry: Arc<HandlerRegistry>,
}

#[async_trait]
impl rune_core::event::SystemEventHandler for ServerEventHandler {
    async fn handle_system_event(&self, event: &rune_core::event::SystemEvent) -> Result<()> {
        match event {
            rune_core::event::SystemEvent::FileChanged {
                path, change_type, ..
            } => {
                info!(
                    "Server received file changed event: {} ({:?})",
                    path.display(),
                    change_type
                );

                // File monitoring is now handled by the FileWatcher plugin
                info!("File change detected, relying on FileWatcher plugin for monitoring");

                // Always try to register handlers for the file - this is idempotent
                // The registry will handle duplicates appropriately
                info!("Registering/updating handlers for file: {}", path.display());
                self.register_file_handlers_for_event(path).await?;
            }
            _ => {
                // Ignore other events
            }
        }
        Ok(())
    }

    fn handler_name(&self) -> &str {
        "server-event-handler"
    }
}

/// Event handler for live reload functionality
struct LiveReloadEventHandler {
    reload_sender: broadcast::Sender<handlers::ServerMessage>,
    live_reload_handler: Arc<handlers::LiveReloadHandler>,
}

#[async_trait]
impl rune_core::event::SystemEventHandler for LiveReloadEventHandler {
    async fn handle_system_event(&self, event: &rune_core::event::SystemEvent) -> Result<()> {
        match event {
            rune_core::event::SystemEvent::FileChanged { path, .. } => {
                info!("File changed, triggering live reload: {}", path.display());

                // Broadcast reload message to all connected WebSocket clients
                if let Err(e) = self.live_reload_handler.broadcast_reload().await {
                    warn!("Failed to broadcast reload message: {}", e);
                }
            }
            _ => {
                // Ignore other events
            }
        }
        Ok(())
    }

    fn handler_name(&self) -> &str {
        "live-reload-event-handler"
    }
}

impl ServerEventHandler {
    /// Register file handlers in response to an event
    async fn register_file_handlers_for_event(&self, file_path: &std::path::Path) -> Result<()> {
        info!(
            "Registering markdown handler for file: {}",
            file_path.display()
        );

        // Get renderer registry from shared resources if available
        let renderer_registry = self
            .plugin_context
            .get_shared_resource::<rune_core::renderer::RendererRegistry>("renderer_registry")
            .await;

        // Register main markdown handler for root path
        let markdown_handler = if let Some(renderer_registry) = renderer_registry {
            Arc::new(handlers::MarkdownHandler::with_renderer_registry(
                "/".to_string(),
                file_path.to_path_buf(),
                renderer_registry,
            ))
        } else {
            Arc::new(handlers::MarkdownHandler::new(
                "/".to_string(),
                file_path.to_path_buf(),
            ))
        };

        self.handler_registry
            .register_http_handler(markdown_handler)
            .await?;

        // Register raw markdown handler
        let raw_handler = Arc::new(handlers::RawMarkdownHandler::new(
            "/raw".to_string(),
            file_path.to_path_buf(),
        ));
        self.handler_registry
            .register_http_handler(raw_handler)
            .await?;

        // Register static file handler for assets in the same directory
        if let Some(base_dir) = file_path.parent() {
            let static_handler = Arc::new(handlers::StaticHandler::new(
                base_dir.to_path_buf(),
                "/assets".to_string(),
            ));
            self.handler_registry
                .register_http_handler(static_handler)
                .await?;

            // Also register image handler for images in the same directory
            let image_handler = Arc::new(handlers::StaticHandler::new_image_handler(
                base_dir.to_path_buf(),
                "/images".to_string(),
            ));
            self.handler_registry
                .register_http_handler(image_handler)
                .await?;
        }

        info!(
            "File handlers registered successfully for: {}",
            file_path.display()
        );
        Ok(())
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
