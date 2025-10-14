//! Editor handlers for raw text editing interface

use crate::{HttpHandler, HttpRequest, HttpResponse, WebSocketConnection, WebSocketHandler, WebSocketMessage};
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorPosition {
    pub line: usize,
    pub column: usize,
    pub selection_start: Option<usize>,
    pub selection_end: Option<usize>,
}

impl Default for CursorPosition {
    fn default() -> Self {
        Self {
            line: 0,
            column: 0,
            selection_start: None,
            selection_end: None,
        }
    }
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
    SaveRequest {
        session_id: String,
    },
    #[serde(rename = "mode_switch")]
    ModeSwitch {
        session_id: String,
        mode: String,
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
        
        let filename = self.markdown_file
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
        </div>
        <div>Raw Mode</div>
    </div>
    <script>
        const sessionId = '{}';
        const editor = document.getElementById('editor');
        let isDirty = false;
        let ws = null;
        
        function initWebSocket() {{
            const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
            const wsUrl = `${{protocol}}//${{window.location.host}}/ws/editor`;
            ws = new WebSocket(wsUrl);
            ws.onopen = () => console.log('Editor WebSocket connected');
            ws.onclose = () => setTimeout(initWebSocket, 1000);
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
        }}
        
        function saveContent() {{
            const content = editor.value;
            sendMessage({{ type: 'content_update', session_id: sessionId, content: content, cursor_position: {{ line: 0, column: 0 }} }});
            sendMessage({{ type: 'save_request', session_id: sessionId }});
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
        
        let content = match tokio::fs::read_to_string(&self.markdown_file).await {
            Ok(content) => content,
            Err(_) => String::new(),
        };
        
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
}

impl EditorWebSocketHandler {
    /// Create a new editor WebSocket handler
    pub fn new(path: String) -> Self {
        Self {
            path,
            editor_sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Handle content update message
    async fn handle_content_update(&self, session_id: &str, content: String, _cursor_position: CursorPosition) -> Result<()> {
        let mut sessions = self.editor_sessions.write().await;
        
        if let Some(session) = sessions.get_mut(session_id) {
            session.content = content;
            session.is_dirty = true;
        }
        
        Ok(())
    }
    
    /// Handle save request
    async fn handle_save_request(&self, session_id: &str) -> Result<()> {
        let sessions = self.editor_sessions.read().await;
        
        if let Some(session) = sessions.get(session_id) {
            tokio::fs::write(&session.file_path, &session.content).await
                .map_err(|e| RuneError::Server(format!("Failed to save file: {}", e)))?;
            
            drop(sessions);
            let mut sessions = self.editor_sessions.write().await;
            if let Some(session) = sessions.get_mut(session_id) {
                session.is_dirty = false;
            }
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
        Ok(())
    }

    async fn on_message(&self, connection: &WebSocketConnection, message: WebSocketMessage) -> Result<()> {
        if let WebSocketMessage::Text(text) = message {
            match serde_json::from_str::<EditorMessage>(&text) {
                Ok(editor_msg) => {
                    match editor_msg {
                        EditorMessage::ContentUpdate { session_id, content, cursor_position } => {
                            self.handle_content_update(&session_id, content, cursor_position).await?;
                        }
                        EditorMessage::SaveRequest { session_id } => {
                            self.handle_save_request(&session_id).await?;
                            
                            let response = serde_json::json!({
                                "type": "save_complete",
                                "session_id": session_id
                            });
                            connection.send_text(response.to_string()).await?;
                        }
                        EditorMessage::ModeSwitch { session_id, mode } => {
                            tracing::info!("Mode switch requested: {} -> {}", session_id, mode);
                        }
                    }
                }
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