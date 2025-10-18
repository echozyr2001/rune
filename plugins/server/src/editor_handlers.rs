//! Editor handlers for raw text editing interface

use crate::{
    HttpHandler, HttpRequest, HttpResponse, WebSocketConnection, WebSocketHandler, WebSocketMessage,
};
use async_trait::async_trait;
use axum::http::Method;
use rune_core::{Result, RuneError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Raw text editor handler for serving the editor interface
pub struct RawEditorHandler {
    path_pattern: String,
    markdown_file: PathBuf,
    editor_sessions: Arc<RwLock<HashMap<String, EditorSession>>>,
}

/// Editor session data
#[derive(Debug, Clone)]
pub struct EditorSession {
    pub session_id: String,
    pub file_path: PathBuf,
    pub content: String,
    pub cursor_position: CursorPosition,
    pub is_dirty: bool,
}

/// Cursor position in the editor
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CursorPosition {
    pub line: usize,
    pub column: usize,
    pub selection_start: Option<usize>,
    pub selection_end: Option<usize>,
}

/// Editor API messages for WebSocket communication
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EditorMessage {
    #[serde(rename = "content_update")]
    ContentUpdate {
        session_id: String,
        content: String,
        cursor_position: CursorPosition,
    },
    #[serde(rename = "save_request")]
    SaveRequest { session_id: String },
    #[serde(rename = "mode_switch")]
    ModeSwitch { session_id: String, mode: String },
    #[serde(rename = "click_to_edit")]
    ClickToEdit {
        session_id: String,
        click_position: usize,
    },
    #[serde(rename = "trigger_render")]
    TriggerRender {
        session_id: String,
        trigger_events: Vec<String>,
    },
    #[serde(rename = "render_markdown")]
    RenderMarkdown { session_id: String, content: String },
    #[serde(rename = "markdown_rendered")]
    MarkdownRendered { session_id: String, html: String },
    #[serde(rename = "update_element")]
    UpdateElement {
        session_id: String,
        element_content: String,
    },
    #[serde(rename = "auto_save_status")]
    AutoSaveStatus {
        session_id: String,
        enabled: bool,
        is_dirty: bool,
        pending_save: bool,
        last_save_time: Option<String>,
    },
    #[serde(rename = "save_complete")]
    SaveComplete {
        session_id: String,
        success: bool,
        timestamp: String,
    },
}

impl RawEditorHandler {
    /// Create a new raw editor handler
    pub fn new(path_pattern: String, markdown_file: PathBuf) -> Self {
        Self {
            path_pattern,
            markdown_file,
            editor_sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Generate the raw text editor HTML interface
    fn generate_editor_html(&self, content: &str, session_id: &str) -> String {
        let escaped_content = content
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");

        let filename = self
            .markdown_file
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();

        format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Raw Text Editor</title>
    <style>
        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        body {{ 
            font-family: 'Monaco', 'Menlo', 'Ubuntu Mono', monospace;
            background: var(--bg-color, #1e1e2e);
            color: var(--text-color, #cdd6f4);
            height: 100vh;
            display: flex;
            flex-direction: column;
        }}
        .editor-header {{
            background: var(--code-bg, #181825);
            border-bottom: 1px solid var(--border-color, #45475a);
            padding: 8px 16px;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }}
        .editor-title {{ font-weight: bold; font-size: 14px; }}
        .editor-controls {{ display: flex; gap: 8px; }}
        .btn {{
            background: var(--link-color, #89b4fa);
            color: var(--bg-color, #1e1e2e);
            border: none;
            padding: 4px 12px;
            border-radius: 4px;
            cursor: pointer;
            font-size: 12px;
            font-weight: 500;
        }}
        .btn:hover {{ opacity: 0.8; }}
        .btn.secondary {{
            background: var(--border-color, #45475a);
            color: var(--text-color, #cdd6f4);
        }}
        .editor-container {{ flex: 1; display: flex; flex-direction: column; }}
        .editor-textarea {{
            flex: 1;
            width: 100%;
            background: var(--bg-color, #1e1e2e);
            color: var(--text-color, #cdd6f4);
            border: none;
            outline: none;
            padding: 16px;
            font-family: inherit;
            font-size: 14px;
            line-height: 1.5;
            resize: none;
            tab-size: 4;
        }}
        .status-bar {{
            background: var(--code-bg, #181825);
            border-top: 1px solid var(--border-color, #45475a);
            padding: 4px 16px;
            font-size: 12px;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }}
        .status-info {{ display: flex; gap: 16px; }}
        .dirty-indicator {{ color: var(--link-color, #89b4fa); font-weight: bold; }}
        .auto-save-indicator {{ 
            color: var(--text-color, #cdd6f4); 
            font-size: 11px; 
            opacity: 0.7;
        }}
        .auto-save-indicator.saving {{ 
            color: var(--link-color, #89b4fa); 
            opacity: 1;
        }}
        .auto-save-indicator.saved {{ 
            color: #a6e3a1; 
            opacity: 1;
        }}
    </style>
</head>
<body>
    <div class="editor-header">
        <div class="editor-title">Raw Text Editor - {}</div>
        <div class="editor-controls">
            <button class="btn secondary" onclick="switchToLive()">Live Mode</button>
            <button class="btn" onclick="saveContent()">Save (Ctrl+S)</button>
        </div>
    </div>
    <div class="editor-container">
        <textarea id="editor" class="editor-textarea" spellcheck="false">{}</textarea>
    </div>
    <div class="status-bar">
        <div class="status-info">
            <span id="cursor-position">Line 1, Column 1</span>
            <span id="word-count">0 words</span>
            <span id="dirty-status" class="dirty-indicator" style="display: none;">●</span>
            <span id="auto-save-status" class="auto-save-indicator">Auto-save enabled</span>
        </div>
        <div>Raw Mode</div>
    </div>
    <script>
        const sessionId = '{}';
        const editor = document.getElementById('editor');
        let isDirty = false;
        let ws = null;
        let autoSaveEnabled = true;
        let autoSaveTimer = null;
        let lastSaveTime = null;
        
        function initWebSocket() {{
            const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
            const wsUrl = `${{protocol}}//${{window.location.host}}/ws/editor`;
            ws = new WebSocket(wsUrl);
            ws.onopen = () => console.log('Editor WebSocket connected');
            ws.onclose = () => setTimeout(initWebSocket, 1000);
            ws.onmessage = handleWebSocketMessage;
        }}
        
        function handleWebSocketMessage(event) {{
            try {{
                const message = JSON.parse(event.data);
                switch (message.type) {{
                    case 'save_complete':
                        if (message.session_id === sessionId) {{
                            setDirty(false);
                            updateAutoSaveStatus('saved');
                            lastSaveTime = new Date(message.timestamp);
                        }}
                        break;
                    case 'auto_save_status':
                        if (message.session_id === sessionId) {{
                            autoSaveEnabled = message.enabled;
                            updateAutoSaveStatus(message.pending_save ? 'saving' : 'idle');
                        }}
                        break;
                }}
            }} catch (e) {{
                console.error('Failed to parse WebSocket message:', e);
            }}
        }}
        
        function sendMessage(message) {{
            if (ws && ws.readyState === WebSocket.OPEN) {{
                ws.send(JSON.stringify(message));
            }}
        }}
        
        function updateStatus() {{
            const text = editor.value;
            const cursorPos = editor.selectionStart;
            const textBeforeCursor = text.substring(0, cursorPos);
            const lines = textBeforeCursor.split('\\n');
            const line = lines.length;
            const column = lines[lines.length - 1].length + 1;
            
            document.getElementById('cursor-position').textContent = `Line ${{line}}, Column ${{column}}`;
            const words = text.trim() ? text.trim().split(/\\s+/).length : 0;
            document.getElementById('word-count').textContent = `${{words}} words`;
        }}
        
        function setDirty(dirty) {{
            isDirty = dirty;
            document.getElementById('dirty-status').style.display = dirty ? 'inline' : 'none';
            
            if (dirty && autoSaveEnabled) {{
                startAutoSaveTimer();
            }} else if (!dirty) {{
                clearAutoSaveTimer();
            }}
        }}
        
        function startAutoSaveTimer() {{
            clearAutoSaveTimer();
            updateAutoSaveStatus('pending');
            
            autoSaveTimer = setTimeout(() => {{
                if (isDirty && autoSaveEnabled) {{
                    updateAutoSaveStatus('saving');
                    saveContent(true); // true indicates auto-save
                }}
            }}, 2000); // 2-second delay as per requirements
        }}
        
        function clearAutoSaveTimer() {{
            if (autoSaveTimer) {{
                clearTimeout(autoSaveTimer);
                autoSaveTimer = null;
            }}
        }}
        
        function updateAutoSaveStatus(status) {{
            const statusElement = document.getElementById('auto-save-status');
            statusElement.className = 'auto-save-indicator';
            
            switch (status) {{
                case 'pending':
                    statusElement.textContent = 'Auto-save in 2s...';
                    break;
                case 'saving':
                    statusElement.textContent = 'Saving...';
                    statusElement.classList.add('saving');
                    break;
                case 'saved':
                    statusElement.textContent = 'Saved';
                    statusElement.classList.add('saved');
                    setTimeout(() => {{
                        statusElement.textContent = 'Auto-save enabled';
                        statusElement.className = 'auto-save-indicator';
                    }}, 2000);
                    break;
                case 'idle':
                default:
                    statusElement.textContent = autoSaveEnabled ? 'Auto-save enabled' : 'Auto-save disabled';
                    break;
            }}
        }}
        
        function saveContent(isAutoSave = false) {{
            const content = editor.value;
            sendMessage({{ type: 'content_update', session_id: sessionId, content: content, cursor_position: {{ line: 0, column: 0 }} }});
            sendMessage({{ type: 'save_request', session_id: sessionId }});
            
            if (!isAutoSave) {{
                clearAutoSaveTimer();
            }}
        }}
        
        function switchToLive() {{
            sendMessage({{ type: 'mode_switch', session_id: sessionId, mode: 'live' }});
            window.location.href = '/';
        }}
        
        editor.addEventListener('input', () => {{
            setDirty(true);
            updateStatus();
        }});
        
        editor.addEventListener('click', updateStatus);
        editor.addEventListener('keyup', updateStatus);
        
        document.addEventListener('keydown', (e) => {{
            if (e.ctrlKey || e.metaKey) {{
                if (e.key === 's') {{ e.preventDefault(); saveContent(); }}
                if (e.key === 'l') {{ e.preventDefault(); switchToLive(); }}
            }}
            if (e.key === 'Tab') {{
                e.preventDefault();
                const start = editor.selectionStart;
                const end = editor.selectionEnd;
                editor.value = editor.value.substring(0, start) + '\\t' + editor.value.substring(end);
                editor.selectionStart = editor.selectionEnd = start + 1;
                setDirty(true);
                updateStatus();
            }}
        }});
        
        // Browser warning for unsaved changes
        window.addEventListener('beforeunload', (e) => {{
            if (isDirty) {{
                const message = 'You have unsaved changes. Are you sure you want to leave?';
                e.preventDefault();
                e.returnValue = message;
                return message;
            }}
        }});
        
        initWebSocket();
        updateStatus();
        editor.focus();
    </script>
</body>
</html>"#,
            filename, escaped_content, session_id
        )
    }
}

#[async_trait]
impl HttpHandler for RawEditorHandler {
    fn path_pattern(&self) -> &str {
        &self.path_pattern
    }

    fn method(&self) -> Method {
        Method::GET
    }

    async fn handle(&self, _request: HttpRequest) -> Result<HttpResponse> {
        let session_id = Uuid::new_v4().to_string();

        let content = tokio::fs::read_to_string(&self.markdown_file)
            .await
            .unwrap_or_default();

        let session = EditorSession {
            session_id: session_id.clone(),
            file_path: self.markdown_file.clone(),
            content: content.clone(),
            cursor_position: CursorPosition::default(),
            is_dirty: false,
        };

        {
            let mut sessions = self.editor_sessions.write().await;
            sessions.insert(session_id.clone(), session);
        }

        let html = self.generate_editor_html(&content, &session_id);
        Ok(HttpResponse::html(&html))
    }

    fn priority(&self) -> i32 {
        5
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// WebSocket handler for editor communication
pub struct EditorWebSocketHandler {
    path: String,
    editor_sessions: Arc<RwLock<HashMap<String, EditorSession>>>,
    /// Broadcast channel for editor events
    event_sender: Arc<RwLock<Option<tokio::sync::broadcast::Sender<EditorBroadcastMessage>>>>,
    /// Current markdown file being edited
    markdown_file: Arc<RwLock<Option<PathBuf>>>,
}

/// Broadcast message for editor events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorBroadcastMessage {
    pub session_id: String,
    pub event: EditorMessage,
    pub timestamp: String,
}

impl EditorWebSocketHandler {
    /// Create a new editor WebSocket handler
    pub fn new(path: String) -> Self {
        let (event_sender, _) = tokio::sync::broadcast::channel(100);
        Self {
            path,
            editor_sessions: Arc::new(RwLock::new(HashMap::new())),
            event_sender: Arc::new(RwLock::new(Some(event_sender))),
            markdown_file: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the current markdown file being edited
    pub async fn set_markdown_file(&self, file_path: PathBuf) {
        let mut markdown_file = self.markdown_file.write().await;
        *markdown_file = Some(file_path);
    }

    /// Get the event broadcast sender
    pub async fn get_event_sender(
        &self,
    ) -> Option<tokio::sync::broadcast::Sender<EditorBroadcastMessage>> {
        let sender = self.event_sender.read().await;
        sender.clone()
    }

    /// Broadcast an editor event to all connected clients
    pub async fn broadcast_editor_event(
        &self,
        session_id: String,
        event: EditorMessage,
    ) -> Result<()> {
        if let Some(sender) = self.get_event_sender().await {
            let broadcast_msg = EditorBroadcastMessage {
                session_id,
                event,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
                    .to_string(),
            };

            sender.send(broadcast_msg).map_err(|e| {
                RuneError::Server(format!("Failed to broadcast editor event: {}", e))
            })?;

            tracing::debug!("Broadcasted editor event to all clients");
        }
        Ok(())
    }

    /// Handle content update message
    async fn handle_content_update(
        &self,
        session_id: &str,
        content: String,
        cursor_position: CursorPosition,
    ) -> Result<()> {
        let mut sessions = self.editor_sessions.write().await;

        // Get the current markdown file path
        let markdown_file = self.markdown_file.read().await;
        let file_path = markdown_file
            .as_ref()
            .ok_or_else(|| RuneError::Server("No markdown file set for editor".to_string()))?;

        // Create or update session
        if let Some(session) = sessions.get_mut(session_id) {
            // Update existing session
            session.content = content;
            session.is_dirty = true;
            session.cursor_position = cursor_position;
        } else {
            // Create new session
            tracing::info!("Creating new editor session: {}", session_id);
            let session = EditorSession {
                session_id: session_id.to_string(),
                file_path: file_path.clone(),
                content,
                cursor_position,
                is_dirty: true,
            };
            sessions.insert(session_id.to_string(), session);
        }

        Ok(())
    }

    /// Handle save request
    async fn handle_save_request(&self, session_id: &str) -> Result<()> {
        // Get the current markdown file path
        let markdown_file = self.markdown_file.read().await;
        let file_path = markdown_file
            .as_ref()
            .ok_or_else(|| RuneError::Server("No markdown file set for editor".to_string()))?;

        // Get the content from the session
        let sessions = self.editor_sessions.read().await;
        let content = if let Some(session) = sessions.get(session_id) {
            session.content.clone()
        } else {
            // If session doesn't exist, try to get content from the most recent session
            // or return an error
            return Err(RuneError::Server(format!(
                "Session not found: {}. Available sessions: {:?}",
                session_id,
                sessions.keys().collect::<Vec<_>>()
            )));
        };

        drop(sessions);

        // Write content to file
        tokio::fs::write(file_path, &content)
            .await
            .map_err(|e| RuneError::Server(format!("Failed to save file: {}", e)))?;

        tracing::info!("✅ Saved content to file: {:?}", file_path);

        // Mark session as not dirty
        let mut sessions = self.editor_sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.is_dirty = false;
        }

        Ok(())
    }
}

#[async_trait]
impl WebSocketHandler for EditorWebSocketHandler {
    fn path(&self) -> &str {
        &self.path
    }

    async fn on_connect(&self, connection: &WebSocketConnection) -> Result<()> {
        tracing::info!("Editor WebSocket client connected: {}", connection.id);

        // Subscribe this connection to the editor event broadcast
        if let Some(event_sender) = self.get_event_sender().await {
            let mut rx = event_sender.subscribe();
            let conn_sender = connection.sender.clone();
            let conn_id = connection.id.clone();

            tokio::spawn(async move {
                while let Ok(broadcast_msg) = rx.recv().await {
                    // Convert broadcast message to JSON and send to client
                    if let Ok(text) = serde_json::to_string(&broadcast_msg.event) {
                        if conn_sender.send(WebSocketMessage::Text(text)).is_err() {
                            tracing::debug!("Connection {} closed, stopping broadcast", conn_id);
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
                "message": "Connected to editor WebSocket server"
            }))
            .await?;

        Ok(())
    }

    async fn on_message(
        &self,
        connection: &WebSocketConnection,
        message: WebSocketMessage,
    ) -> Result<()> {
        if let WebSocketMessage::Text(text) = message {
            match serde_json::from_str::<EditorMessage>(&text) {
                Ok(editor_msg) => match editor_msg.clone() {
                    EditorMessage::ContentUpdate {
                        ref session_id,
                        ref content,
                        ref cursor_position,
                    } => {
                        self.handle_content_update(
                            session_id,
                            content.clone(),
                            cursor_position.clone(),
                        )
                        .await?;

                        // Broadcast content update to all other clients
                        self.broadcast_editor_event(session_id.clone(), editor_msg)
                            .await?;

                        tracing::debug!(
                            "Content updated and broadcasted for session {}",
                            session_id
                        );
                    }
                    EditorMessage::SaveRequest { ref session_id } => {
                        self.handle_save_request(session_id).await?;

                        let save_complete_msg = EditorMessage::SaveComplete {
                            session_id: session_id.clone(),
                            success: true,
                            timestamp: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs()
                                .to_string(),
                        };

                        // Broadcast save complete to all clients
                        self.broadcast_editor_event(session_id.clone(), save_complete_msg.clone())
                            .await?;

                        tracing::info!("Save completed and broadcasted for session {}", session_id);
                    }
                    EditorMessage::ModeSwitch {
                        ref session_id,
                        ref mode,
                    } => {
                        tracing::info!("Mode switch requested: {} -> {}", session_id, mode);

                        // Broadcast mode switch to all clients
                        self.broadcast_editor_event(session_id.clone(), editor_msg)
                            .await?;
                    }
                    EditorMessage::ClickToEdit {
                        ref session_id,
                        click_position,
                    } => {
                        tracing::debug!(
                            "Click-to-edit at position {} for session {}",
                            click_position,
                            session_id
                        );

                        // In a real implementation, you would:
                        // 1. Get the editor plugin from the plugin context
                        // 2. Call handle_click_to_edit
                        // 3. Send back the result to update the UI

                        let response = serde_json::json!({
                            "type": "click_to_edit_result",
                            "session_id": session_id,
                            "success": true
                        });
                        let _ = connection.send_text(response.to_string()).await;
                    }
                    EditorMessage::TriggerRender {
                        ref session_id,
                        ref trigger_events,
                    } => {
                        tracing::debug!(
                            "Render trigger for session {} with events: {:?}",
                            session_id,
                            trigger_events
                        );

                        // In a real implementation, you would:
                        // 1. Convert trigger_events to TriggerEvent enum
                        // 2. Call process_live_content on the editor plugin
                        // 3. Send back the rendered content

                        let response = serde_json::json!({
                            "type": "live_content_update",
                            "session_id": session_id,
                            "rendered_content": "<p>Live rendered content would go here</p>"
                        });
                        let _ = connection.send_text(response.to_string()).await;
                    }
                    EditorMessage::RenderMarkdown {
                        ref session_id,
                        ref content,
                    } => {
                        tracing::debug!(
                            "Render markdown request for session {}, content length: {}",
                            session_id,
                            content.len()
                        );

                        // Render markdown to HTML using markdown crate
                        let html_output = markdown::to_html_with_options(
                            content,
                            &markdown::Options {
                                compile: markdown::CompileOptions {
                                    allow_dangerous_html: true,
                                    allow_dangerous_protocol: false,
                                    ..markdown::CompileOptions::default()
                                },
                                parse: markdown::ParseOptions {
                                    constructs: markdown::Constructs {
                                        attention: true,
                                        autolink: true,
                                        block_quote: true,
                                        character_escape: true,
                                        character_reference: true,
                                        code_fenced: true,
                                        code_indented: true,
                                        code_text: true,
                                        definition: true,
                                        frontmatter: false,
                                        gfm_autolink_literal: true,
                                        gfm_footnote_definition: true,
                                        gfm_label_start_footnote: true,
                                        gfm_strikethrough: true,
                                        gfm_table: true,
                                        gfm_task_list_item: true,
                                        hard_break_escape: true,
                                        hard_break_trailing: true,
                                        heading_atx: true,
                                        heading_setext: true,
                                        html_flow: true,
                                        html_text: true,
                                        label_start_image: true,
                                        label_start_link: true,
                                        label_end: true,
                                        list_item: true,
                                        math_flow: false,
                                        math_text: false,
                                        mdx_esm: false,
                                        mdx_expression_flow: false,
                                        mdx_expression_text: false,
                                        mdx_jsx_flow: false,
                                        mdx_jsx_text: false,
                                        thematic_break: true,
                                    },
                                    ..markdown::ParseOptions::default()
                                },
                            },
                        )
                        .unwrap_or_else(|e| {
                            tracing::error!("Failed to render markdown: {}", e);
                            format!(
                                "<p>Error rendering markdown: {}</p>",
                                html_escape::encode_text(&e.to_string())
                            )
                        });

                        // Send rendered HTML back to client
                        let response_msg = EditorMessage::MarkdownRendered {
                            session_id: session_id.clone(),
                            html: html_output,
                        };

                        // Broadcast to all clients
                        self.broadcast_editor_event(session_id.clone(), response_msg)
                            .await?;

                        tracing::debug!("Markdown rendered and sent for session {}", session_id);
                    }
                    EditorMessage::MarkdownRendered { .. } => {
                        // This message is sent from server to client, not expected from client
                        tracing::debug!(
                            "Received markdown_rendered message from client (unexpected)"
                        );
                    }
                    EditorMessage::UpdateElement {
                        ref session_id,
                        ref element_content,
                    } => {
                        tracing::debug!("Update element content for session {}", session_id);

                        // In a real implementation, you would:
                        // 1. Call update_active_element_content on the editor plugin
                        // 2. Trigger re-rendering if successful

                        // Broadcast element update to all clients
                        self.broadcast_editor_event(session_id.clone(), editor_msg)
                            .await?;

                        let response = serde_json::json!({
                            "type": "element_update_result",
                            "session_id": session_id,
                            "success": true
                        });
                        let _ = connection.send_text(response.to_string()).await;
                    }
                    EditorMessage::AutoSaveStatus { ref session_id, .. } => {
                        // Broadcast auto-save status to all clients
                        self.broadcast_editor_event(session_id.clone(), editor_msg)
                            .await?;
                    }
                    EditorMessage::SaveComplete { .. } => {
                        // Save complete messages are typically sent from server to client
                        tracing::debug!("Received save complete message from client (unexpected)");
                    }
                },
                Err(e) => {
                    tracing::warn!("Failed to parse editor message: {}", e);
                }
            }
        }

        Ok(())
    }

    async fn on_disconnect(&self, connection: &WebSocketConnection) -> Result<()> {
        tracing::info!("Editor WebSocket client disconnected: {}", connection.id);
        Ok(())
    }

    fn priority(&self) -> i32 {
        5
    }
}

/*
pub struct LiveEditorHandler {
    path_pattern: String,
    markdown_file: PathBuf,
    editor_sessions: Arc<RwLock<HashMap<String, EditorSession>>>,
}

impl LiveEditorHandler {
    /// Create a new live editor handler
    pub fn new(path_pattern: String, markdown_file: PathBuf) -> Self {
        Self {
            path_pattern,
            markdown_file,
            editor_sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Convert markdown content to editable HTML
    fn markdown_to_editable_html(&self, markdown: &str) -> String {
        // Basic markdown to HTML conversion for contenteditable
        let mut html = String::new();
        let lines: Vec<&str> = markdown.split('\n').collect();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i].trim();

            if line.is_empty() {
                html.push_str("<br>");
                i += 1;
                continue;
            }

            // Headers
            if let Some(text) = line.strip_prefix("# ") {
                html.push_str(&format!("<h1>{}</h1>", Self::escape_html(text)));
            } else if let Some(text) = line.strip_prefix("## ") {
                html.push_str(&format!("<h2>{}</h2>", Self::escape_html(text)));
            } else if let Some(text) = line.strip_prefix("### ") {
                html.push_str(&format!("<h3>{}</h3>", Self::escape_html(text)));
            } else if let Some(text) = line.strip_prefix("#### ") {
                html.push_str(&format!("<h4>{}</h4>", Self::escape_html(text)));
            } else if let Some(text) = line.strip_prefix("##### ") {
                html.push_str(&format!("<h5>{}</h5>", Self::escape_html(text)));
            } else if let Some(text) = line.strip_prefix("###### ") {
                html.push_str(&format!("<h6>{}</h6>", Self::escape_html(text)));
            }
            // Lists
            else if line.starts_with("- ") || line.starts_with("* ") {
                // Start of unordered list
                html.push_str("<ul>");
                while i < lines.len() && (lines[i].trim().starts_with("- ") || lines[i].trim().starts_with("* ")) {
                    let item = lines[i].trim();
                    let text = if let Some(t) = item.strip_prefix("- ") {
                        t
                    } else if let Some(t) = item.strip_prefix("* ") {
                        t
                    } else {
                        item // fallback
                    };
                    html.push_str(&format!("<li>{}</li>", Self::escape_html(text)));
                    i += 1;
                }
                html.push_str("</ul>");
                continue;
            }
            // Numbered lists
            else if line.chars().next().is_some_and(|c| c.is_ascii_digit()) && line.contains(". ") {
                html.push_str("<ol>");
                while i < lines.len() {
                    let item = lines[i].trim();
                    if let Some(dot_pos) = item.find(". ") {
                        if item.chars().take(dot_pos).all(|c| c.is_ascii_digit()) {
                            let text = &item[dot_pos + 2..];
                            html.push_str(&format!("<li>{}</li>", Self::escape_html(text)));
                            i += 1;
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                html.push_str("</ol>");
                continue;
            }
            // Code blocks
            else if let Some(lang) = line.strip_prefix("```") {
                html.push_str("<pre><code>");
                i += 1;
                while i < lines.len() && !lines[i].trim().starts_with("```") {
                    html.push_str(&Self::escape_html(lines[i]));
                    html.push('\n');
                    i += 1;
                }
                html.push_str("</code></pre>");
            }
            // Blockquotes
            else if let Some(text) = line.strip_prefix("> ") {
                html.push_str(&format!("<blockquote>{}</blockquote>", Self::escape_html(text)));
            }
            // Regular paragraphs
            else {
                // Process inline formatting
                let processed_line = self.process_inline_formatting(line);
                html.push_str(&format!("<p>{}</p>", processed_line));
            }

            i += 1;
        }

        html
    }

    /// Process inline markdown formatting (bold, italic, code, links)
    fn process_inline_formatting(&self, text: &str) -> String {
        let mut result = Self::escape_html(text);

        // Bold (**text** or __text__)
        result = result.replace("**", "<strong>").replace("</strong>**", "</strong>");

        // Italic (*text* or _text_)
        // This is simplified - in a real implementation you'd need proper parsing

        // Inline code (`code`)
        while let (Some(start), Some(end)) = (result.find('`'), result.rfind('`')) {
            if start < end {
                let before = &result[..start];
                let code = &result[start + 1..end];
                let after = &result[end + 1..];
                result = format!("{}<code>{}</code>{}", before, code, after);
            } else {
                break;
            }
        }

        result
    }

    /// Escape HTML special characters
    fn escape_html(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#39;")
    }



        format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Live WYSIWYG Editor</title>
    <style>
        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: var(--bg-color, #1e1e2e);
            color: var(--text-color, #cdd6f4);
            height: 100vh;
            display: flex;
            flex-direction: column;
        }}
        .editor-header {{
            background: var(--code-bg, #181825);
            border-bottom: 1px solid var(--border-color, #45475a);
            padding: 8px 16px;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }}
        .editor-title {{ font-weight: bold; font-size: 14px; }}
        .editor-controls {{ display: flex; gap: 8px; }}
        .btn {{
            background: var(--link-color, #89b4fa);
            color: var(--bg-color, #1e1e2e);
            border: none;
            padding: 4px 12px;
            border-radius: 4px;
            cursor: pointer;
            font-size: 12px;
            font-weight: 500;
        }}
        .btn:hover {{ opacity: 0.8; }}
        .btn.secondary {{
            background: var(--border-color, #45475a);
            color: var(--text-color, #cdd6f4);
        }}
        .editor-container {{
            flex: 1;
            display: flex;
            flex-direction: column;
            overflow: hidden;
        }}
        .live-editor {{
            flex: 1;
            width: 100%;
            background: var(--bg-color, #1e1e2e);
            color: var(--text-color, #cdd6f4);
            border: none;
            outline: none;
            padding: 16px;
            font-family: inherit;
            font-size: 14px;
            line-height: 1.6;
            overflow-y: auto;
            white-space: pre-wrap;
        }}
        .live-editor:focus {{ outline: none; }}

        /* Live editor element styles */
        .editable-element {{
            display: inline;
            background: rgba(137, 180, 250, 0.1);
            border-radius: 3px;
            padding: 1px 2px;
            cursor: text;
        }}
        .editable-element:hover {{
            background: rgba(137, 180, 250, 0.2);
        }}
        .editable-element[contenteditable="true"] {{
            background: rgba(137, 180, 250, 0.3);
            outline: 1px solid var(--link-color, #89b4fa);
        }}

        /* Markdown element styles */
        .md-header {{ font-weight: bold; margin: 0.5em 0; }}
        .md-header-1 {{ font-size: 2em; }}
        .md-header-2 {{ font-size: 1.5em; }}
        .md-header-3 {{ font-size: 1.17em; }}
        .md-bold {{ font-weight: bold; }}
        .md-italic {{ font-style: italic; }}
        .md-code {{
            background: var(--code-bg, #181825);
            padding: 2px 4px;
            border-radius: 3px;
            font-family: 'Monaco', 'Menlo', 'Ubuntu Mono', monospace;
        }}
        .md-link {{ color: var(--link-color, #89b4fa); text-decoration: underline; }}
        .md-list-item {{ margin: 0.2em 0; }}

        .status-bar {{
            background: var(--code-bg, #181825);
            border-top: 1px solid var(--border-color, #45475a);
            padding: 4px 16px;
            font-size: 12px;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }}
        .status-info {{ display: flex; gap: 16px; }}
        .dirty-indicator {{ color: var(--link-color, #89b4fa); font-weight: bold; }}
        .auto-save-indicator {{
            color: var(--text-color, #cdd6f4);
            font-size: 11px;
            opacity: 0.7;
        }}
        .auto-save-indicator.saving {{
            color: var(--link-color, #89b4fa);
            opacity: 1;
        }}
        .auto-save-indicator.saved {{
            color: #a6e3a1;
            opacity: 1;
        }}
    </style>
</head>
<body>
    <div class="editor-header">
        <div class="editor-title">Live WYSIWYG Editor - {}</div>
        <div class="editor-controls">
            <button class="btn secondary" onclick="switchToRaw()">Raw Mode</button>
            <button class="btn" onclick="saveContent()">Save (Ctrl+S)</button>
        </div>
    </div>
    <div class="editor-container">
        <div id="live-editor" class="live-editor" contenteditable="true">{}</div>
    </div>
    <div class="status-bar">
        <div class="status-info">
            <span id="cursor-position">Line 1, Column 1</span>
            <span id="word-count">0 words</span>
            <span id="dirty-status" class="dirty-indicator" style="display: none;">●</span>
            <span id="auto-save-status" class="auto-save-indicator">Auto-save enabled</span>
        </div>
        <div>Live Mode</div>
    </div>
    <script>
        const sessionId = '{}';
        const editor = document.getElementById('live-editor');
        let isDirty = false;
        let ws = null;
        let lastContent = '';
        let autoSaveEnabled = true;
        let autoSaveTimer = null;
        let lastSaveTime = null;

        function initWebSocket() {{
            const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
            const wsUrl = `${{protocol}}//${{window.location.host}}/ws/editor`;
            ws = new WebSocket(wsUrl);
            ws.onopen = () => console.log('Live Editor WebSocket connected');
            ws.onclose = () => setTimeout(initWebSocket, 1000);
            ws.onmessage = handleWebSocketMessage;
        }}

        function handleWebSocketMessage(event) {{
            try {{
                const message = JSON.parse(event.data);
                switch (message.type) {{
                    case 'live_content_update':
                        editor.innerHTML = message.rendered_content;
                        updateStatus();
                        break;
                    case 'save_complete':
                        if (message.session_id === sessionId) {{
                            setDirty(false);
                            updateAutoSaveStatus('saved');
                            lastSaveTime = new Date(message.timestamp);
                        }}
                        break;
                    case 'auto_save_status':
                        if (message.session_id === sessionId) {{
                            autoSaveEnabled = message.enabled;
                            updateAutoSaveStatus(message.pending_save ? 'saving' : 'idle');
                        }}
                        break;
                }}
            }} catch (e) {{
                console.error('Failed to parse WebSocket message:', e);
            }}
        }}

        function sendMessage(message) {{
            if (ws && ws.readyState === WebSocket.OPEN) {{
                ws.send(JSON.stringify(message));
            }}
        }}

        function updateStatus() {{
            const text = editor.textContent || editor.innerText || '';
            const words = text.trim() ? text.trim().split(/\\s+/).length : 0;
            document.getElementById('word-count').textContent = `${{words}} words`;
        }}

        function setDirty(dirty) {{
            isDirty = dirty;
            document.getElementById('dirty-status').style.display = dirty ? 'inline' : 'none';

            if (dirty && autoSaveEnabled) {{
                startAutoSaveTimer();
            }} else if (!dirty) {{
                clearAutoSaveTimer();
            }}
        }}

        function startAutoSaveTimer() {{
            clearAutoSaveTimer();
            updateAutoSaveStatus('pending');

            autoSaveTimer = setTimeout(() => {{
                if (isDirty && autoSaveEnabled) {{
                    updateAutoSaveStatus('saving');
                    saveContent(true); // true indicates auto-save
                }}
            }}, 2000); // 2-second delay as per requirements
        }}

        function clearAutoSaveTimer() {{
            if (autoSaveTimer) {{
                clearTimeout(autoSaveTimer);
                autoSaveTimer = null;
            }}
        }}

        function updateAutoSaveStatus(status) {{
            const statusElement = document.getElementById('auto-save-status');
            statusElement.className = 'auto-save-indicator';

            switch (status) {{
                case 'pending':
                    statusElement.textContent = 'Auto-save in 2s...';
                    break;
                case 'saving':
                    statusElement.textContent = 'Saving...';
                    statusElement.classList.add('saving');
                    break;
                case 'saved':
                    statusElement.textContent = 'Saved';
                    statusElement.classList.add('saved');
                    setTimeout(() => {{
                        statusElement.textContent = 'Auto-save enabled';
                        statusElement.className = 'auto-save-indicator';
                    }}, 2000);
                    break;
                case 'idle':
                default:
                    statusElement.textContent = autoSaveEnabled ? 'Auto-save enabled' : 'Auto-save disabled';
                    break;
            }}
        }}

        function saveContent(isAutoSave = false) {{
            sendMessage({{ type: 'save_request', session_id: sessionId }});

            if (!isAutoSave) {{
                clearAutoSaveTimer();
            }}
        }}

        function switchToRaw() {{
            sendMessage({{ type: 'mode_switch', session_id: sessionId, mode: 'raw' }});
            window.location.href = '/editor';
        }}

        function handleContentChange() {{
            // Extract markdown content from the contenteditable div
            // This is a critical fix - we need to preserve markdown formatting
            const currentContent = extractMarkdownFromEditor();
            if (currentContent !== lastContent) {{
                setDirty(true);
                lastContent = currentContent;

                // Send content update with proper markdown content
                sendMessage({{
                    type: 'content_update',
                    session_id: sessionId,
                    content: currentContent,
                    cursor_position: getCursorPosition()
                }});

                // Trigger live rendering
                sendMessage({{
                    type: 'trigger_render',
                    session_id: sessionId,
                    trigger_events: ['content_change']
                }});
            }}
        }}

        // Extract markdown content from the contenteditable editor
        function extractMarkdownFromEditor() {{
            // Get the innerHTML and convert back to markdown
            // This preserves the original structure better than textContent
            const html = editor.innerHTML;
            return htmlToMarkdown(html);
        }}

        // Convert HTML content back to markdown
        function htmlToMarkdown(html) {{
            // Create a temporary element to parse HTML
            const temp = document.createElement('div');
            temp.innerHTML = html;

            let markdown = '';

            // Walk through all child nodes and convert them
            for (let node of temp.childNodes) {{
                markdown += nodeToMarkdown(node);
            }}

            return markdown.trim();
        }}

        // Convert a DOM node to markdown
        function nodeToMarkdown(node) {{
            if (node.nodeType === Node.TEXT_NODE) {{
                return node.textContent;
            }}

            if (node.nodeType === Node.ELEMENT_NODE) {{
                const tag = node.tagName.toLowerCase();
                const text = node.textContent;

                switch (tag) {{
                    case 'h1':
                        return '# ' + text + '\\n\\n';
                    case 'h2':
                        return '## ' + text + '\\n\\n';
                    case 'h3':
                        return '### ' + text + '\\n\\n';
                    case 'h4':
                        return '#### ' + text + '\\n\\n';
                    case 'h5':
                        return '##### ' + text + '\\n\\n';
                    case 'h6':
                        return '###### ' + text + '\\n\\n';
                    case 'p':
                        return text + '\\n\\n';
                    case 'strong':
                    case 'b':
                        return '**' + text + '**';
                    case 'em':
                    case 'i':
                        return '*' + text + '*';
                    case 'code':
                        return '`' + text + '`';
                    case 'a':
                        const href = node.getAttribute('href') || '#';
                        return '[' + text + '](' + href + ')';
                    case 'ul':
                        let ulResult = '';
                        for (let li of node.children) {{
                            if (li.tagName.toLowerCase() === 'li') {{
                                ulResult += '- ' + li.textContent + '\\n';
                            }}
                        }}
                        return ulResult + '\\n';
                    case 'ol':
                        let olResult = '';
                        let index = 1;
                        for (let li of node.children) {{
                            if (li.tagName.toLowerCase() === 'li') {{
                                olResult += index + '. ' + li.textContent + '\\n';
                                index++;
                            }}
                        }}
                        return olResult + '\\n';
                    case 'blockquote':
                        return '> ' + text + '\\n\\n';
                    case 'pre':
                        const codeNode = node.querySelector('code');
                        if (codeNode) {{
                            const lang = codeNode.className.replace('language-', '') || '';
                            return '```' + lang + '\\n' + codeNode.textContent + '\\n```\\n\\n';
                        }}
                        return '```\\n' + text + '\\n```\\n\\n';
                    case 'br':
                        return '\\n';
                    case 'div':
                        // Handle div elements recursively
                        let divResult = '';
                        for (let child of node.childNodes) {{
                            divResult += nodeToMarkdown(child);
                        }}
                        return divResult;
                    default:
                        // For unknown elements, just return the text content
                        return text;
                }}
            }}

            return '';
        }}

        // Get cursor position in the contenteditable div
        function getCursorPosition() {{
            const selection = window.getSelection();
            if (selection.rangeCount > 0) {{
                const range = selection.getRangeAt(0);
                const preCaretRange = range.cloneRange();
                preCaretRange.selectNodeContents(editor);
                preCaretRange.setEnd(range.endContainer, range.endOffset);
                const textBeforeCursor = preCaretRange.toString();
                const lines = textBeforeCursor.split('\\n');
                return {{
                    line: lines.length,
                    column: lines[lines.length - 1].length,
                    selection_start: null,
                    selection_end: null
                }};
            }}
            return {{ line: 1, column: 1, selection_start: null, selection_end: null }};
        }}

        function handleClick(event) {{
            const clickPosition = getClickPosition(event);
            if (clickPosition !== null) {{
                sendMessage({{
                    type: 'click_to_edit',
                    session_id: sessionId,
                    click_position: clickPosition
                }});
            }}
        }}

        function getClickPosition(event) {{
            const selection = window.getSelection();
            if (selection.rangeCount > 0) {{
                const range = selection.getRangeAt(0);
                const preCaretRange = range.cloneRange();
                preCaretRange.selectNodeContents(editor);
                preCaretRange.setEnd(range.endContainer, range.endOffset);
                return preCaretRange.toString().length;
            }}
            return null;
        }}

        function handleKeyPress(event) {{
            if (event.key === ' ') {{
                // Space key pressed - trigger rendering
                setTimeout(() => {{
                    sendMessage({{
                        type: 'trigger_render',
                        session_id: sessionId,
                        trigger_events: ['space_key']
                    }});
                }}, 50);
            }}
        }}

        // Event listeners
        editor.addEventListener('input', handleContentChange);
        editor.addEventListener('click', handleClick);
        editor.addEventListener('keypress', handleKeyPress);

        document.addEventListener('keydown', (e) => {{
            if (e.ctrlKey || e.metaKey) {{
                if (e.key === 's') {{ e.preventDefault(); saveContent(); }}
                if (e.key === 'r') {{ e.preventDefault(); switchToRaw(); }}
            }}
        }});

        // Browser warning for unsaved changes
        window.addEventListener('beforeunload', (e) => {{
            if (isDirty) {{
                const message = 'You have unsaved changes. Are you sure you want to leave?';
                e.preventDefault();
                e.returnValue = message;
                return message;
            }}
        }});

        initWebSocket();
        updateStatus();
        editor.focus();
        // Initialize lastContent with the original markdown content
        lastContent = extractMarkdownFromEditor();
    </script>
</body>
</html>"#,
            filename, html_content, session_id
        )
    }

    /// Generate the live WYSIWYG editor HTML interface
    fn generate_live_editor_html(&self, content: &str, session_id: &str) -> String {
        let html_content = self.markdown_to_editable_html(content);
        let filename = self
            .markdown_file
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();

        format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Live Editor - {}</title>
    <style>
        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: #1e1e2e;
            color: #cdd6f4;
            height: 100vh;
            display: flex;
            flex-direction: column;
        }}
        .editor-header {{
            background: #181825;
            border-bottom: 1px solid #45475a;
            padding: 12px 16px;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }}
        .editor-title {{ font-weight: bold; }}
        .editor-controls {{ display: flex; gap: 8px; }}
        .btn {{
            background: #89b4fa;
            color: #1e1e2e;
            border: none;
            padding: 8px 16px;
            border-radius: 6px;
            cursor: pointer;
            font-size: 14px;
            font-weight: 500;
        }}
        .btn:hover {{ opacity: 0.8; }}
        .editor-container {{
            flex: 1;
            padding: 20px;
            overflow-y: auto;
        }}
        .live-editor {{
            width: 100%;
            min-height: 100%;
            background: transparent;
            color: inherit;
            border: none;
            outline: none;
            font-family: inherit;
            font-size: 16px;
            line-height: 1.6;
        }}
        .live-editor:focus {{ outline: none; }}
        .status-bar {{
            background: #181825;
            border-top: 1px solid #45475a;
            padding: 8px 16px;
            font-size: 12px;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }}
        .dirty-indicator {{ color: #89b4fa; font-weight: bold; }}
    </style>
</head>
<body>
    <div class="editor-header">
        <div class="editor-title">Live Editor - {}</div>
        <div class="editor-controls">
            <button class="btn" onclick="saveContent()">Save (Ctrl+S)</button>
        </div>
    </div>
    <div class="editor-container">
        <div id="live-editor" class="live-editor" contenteditable="true">{}</div>
    </div>
    <div class="status-bar">
        <div>
            <span id="dirty-status" class="dirty-indicator" style="display: none;">● Unsaved changes</span>
        </div>
        <div>Live Mode</div>
    </div>

    <script>
        const sessionId = '{}';
        const editor = document.getElementById('live-editor');
        let isDirty = false;
        let ws = null;
        let originalContent = editor.innerHTML;

        function initWebSocket() {{
            const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
            const wsUrl = `${{protocol}}//${{window.location.host}}/ws/editor`;
            ws = new WebSocket(wsUrl);
            ws.onopen = () => console.log('Editor WebSocket connected');
            ws.onclose = () => setTimeout(initWebSocket, 1000);
            ws.onmessage = handleWebSocketMessage;
        }}

        function handleWebSocketMessage(event) {{
            try {{
                const message = JSON.parse(event.data);
                if (message.type === 'save_complete' && message.session_id === sessionId) {{
                    setDirty(false);
                    originalContent = editor.innerHTML;
                }}
            }} catch (e) {{
                console.error('Failed to parse WebSocket message:', e);
            }}
        }}

        function sendMessage(message) {{
            if (ws && ws.readyState === WebSocket.OPEN) {{
                ws.send(JSON.stringify(message));
            }}
        }}

        function setDirty(dirty) {{
            isDirty = dirty;
            document.getElementById('dirty-status').style.display = dirty ? 'inline' : 'none';
        }}

        function saveContent() {{
            // For now, just save the innerHTML - this is where we'd need proper conversion
            const content = editor.innerHTML;

            sendMessage({{
                type: 'content_update',
                session_id: sessionId,
                content: content,
                cursor_position: {{ line: 0, column: 0 }}
            }});

            sendMessage({{
                type: 'save_request',
                session_id: sessionId
            }});
        }}

        function handleContentChange() {{
            if (editor.innerHTML !== originalContent) {{
                setDirty(true);
            }}
        }}

        // Event listeners
        editor.addEventListener('input', handleContentChange);

        document.addEventListener('keydown', (e) => {{
            if (e.ctrlKey || e.metaKey) {{
                if (e.key === 's') {{
                    e.preventDefault();
                    saveContent();
                }}
            }}
        }});

        initWebSocket();
    </script>
</body>
</html>"#,
            filename, filename, html_content, session_id
        )
    }
}

#[async_trait]
*/
