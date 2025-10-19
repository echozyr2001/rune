//! 修复后的 Live Editor - 基于维护原始 Markdown 内容的简化架构
//!
//! 核心思路：
//! 1. 前端维护原始的 markdown 内容
//! 2. DOM 仅用于渲染显示  
//! 3. 编辑操作直接修改 markdown 文本，而不是依赖 DOM 反向工程

use crate::{HttpHandler, HttpRequest, HttpResponse};
use async_trait::async_trait;
use axum::http::{Method, StatusCode};
use rune_core::{quill::Quill, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// 简化的 Live Editor 处理器
#[derive(Debug, Deserialize)]
struct MarkdownRequest {
    markdown: String,
}

#[derive(Debug, Serialize)]
struct MarkdownResponse {
    html: String,
    success: bool,
    error: Option<String>,
}

pub struct MarkdownRenderHandler {
    quill: Quill,
}

impl Default for MarkdownRenderHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl MarkdownRenderHandler {
    pub fn new() -> Self {
        Self {
            quill: Quill::new(),
        }
    }
}

#[async_trait]
impl HttpHandler for MarkdownRenderHandler {
    async fn handle(&self, request: HttpRequest) -> Result<HttpResponse> {
        if request.method != Method::POST {
            return Ok(HttpResponse::error(
                StatusCode::METHOD_NOT_ALLOWED,
                "Only POST method is allowed",
            ));
        }

        // Parse the JSON request body
        let markdown_request: MarkdownRequest = match serde_json::from_slice(&request.body) {
            Ok(req) => req,
            Err(e) => {
                let error_response = MarkdownResponse {
                    html: String::new(),
                    success: false,
                    error: Some(format!("Invalid JSON: {}", e)),
                };
                return HttpResponse::json(&error_response);
            }
        };

        // Render markdown to HTML using Quill
        let html = self.quill.markdown_to_html(&markdown_request.markdown);

        let response = MarkdownResponse {
            html,
            success: true,
            error: None,
        };

        Ok(HttpResponse::json(&response)?)
    }

    fn path_pattern(&self) -> &str {
        "/api/render-markdown"
    }

    fn method(&self) -> Method {
        Method::POST
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub struct SimpleLiveEditorHandler {
    path_pattern: String,
    markdown_file: PathBuf,
    quill: Quill,
}

impl SimpleLiveEditorHandler {
    pub fn new(path_pattern: String, markdown_file: PathBuf) -> Self {
        Self {
            path_pattern,
            markdown_file,
            quill: Quill::new(),
        }
    }

    /// 使用Quill引擎渲染markdown为HTML
    fn render_markdown(&self, markdown: &str) -> String {
        self.quill.markdown_to_html(markdown)
    }

    /// 生成简化的 Live Editor 界面
    fn generate_simple_live_editor_html(&self, content: &str, session_id: &str) -> String {
        let filename = self
            .markdown_file
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();

        // 转义 JavaScript 字符串中的特殊字符
        let escaped_content = content
            .replace('\\', "\\\\")
            .replace('`', "\\`")
            .replace('$', "\\$");

        format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Simple Live Editor - {}</title>
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
        .btn.secondary {{
            background: #45475a;
            color: #cdd6f4;
        }}
        .editor-container {{ 
            flex: 1; 
            display: flex; 
            overflow: hidden;
        }}
        .markdown-editor {{
            flex: 1;
            background: #1e1e2e;
            color: #cdd6f4;
            border: none;
            outline: none;
            padding: 20px;
            font-family: 'Monaco', 'Menlo', 'Ubuntu Mono', monospace;
            font-size: 14px;
            line-height: 1.6;
            resize: none;
            tab-size: 2;
        }}
        .preview-panel {{
            flex: 1;
            background: #1e1e2e;
            border-left: 1px solid #45475a;
            padding: 20px;
            overflow-y: auto;
        }}
        .status-bar {{
            background: #181825;
            border-top: 1px solid #45475a;
            padding: 8px 16px;
            font-size: 12px;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }}
        .status-info {{ display: flex; gap: 16px; }}
        .dirty-indicator {{ color: #89b4fa; font-weight: bold; }}
        .save-status {{
            padding: 4px 8px;
            border-radius: 4px;
            font-size: 11px;
        }}
        .save-status.saving {{ background: #89b4fa; color: #1e1e2e; }}
        .save-status.saved {{ background: #a6e3a1; color: #1e1e2e; }}
        .save-status.error {{ background: #f38ba8; color: #1e1e2e; }}
        
        /* Markdown 预览样式 */
        .preview-panel h1 {{ font-size: 2em; margin: 0.5em 0; border-bottom: 2px solid #45475a; }}
        .preview-panel h2 {{ font-size: 1.5em; margin: 0.5em 0; border-bottom: 1px solid #45475a; }}
        .preview-panel h3 {{ font-size: 1.17em; margin: 0.5em 0; }}
        .preview-panel h4 {{ font-size: 1em; margin: 0.5em 0; font-weight: bold; }}
        .preview-panel p {{ margin: 1em 0; line-height: 1.6; }}
        .preview-panel ul, .preview-panel ol {{ margin: 1em 0; padding-left: 2em; }}
        .preview-panel li {{ margin: 0.5em 0; }}
        .preview-panel code {{
            background: #181825;
            padding: 2px 4px;
            border-radius: 3px;
            font-family: 'Monaco', 'Menlo', 'Ubuntu Mono', monospace;
        }}
        .preview-panel pre {{
            background: #181825;
            padding: 1em;
            border-radius: 6px;
            overflow-x: auto;
            margin: 1em 0;
        }}
        .preview-panel blockquote {{
            border-left: 4px solid #89b4fa;
            padding-left: 1em;
            margin: 1em 0;
            font-style: italic;
        }}
        .preview-panel a {{ color: #89b4fa; text-decoration: underline; }}
        .preview-panel strong {{ font-weight: bold; }}
        .preview-panel em {{ font-style: italic; }}
    </style>
</head>
<body>
    <div class="editor-header">
        <div class="editor-title">Live Markdown Editor - {}</div>
        <div class="editor-controls">
            <button class="btn secondary" onclick="switchToRaw()">Raw Mode</button>
            <button class="btn" onclick="saveContent()">Save (Ctrl+S)</button>
        </div>
    </div>
    <div class="editor-container">
        <textarea id="markdown-editor" class="markdown-editor" placeholder="Enter your markdown here...">{}</textarea>
        <div id="preview-panel" class="preview-panel"></div>
    </div>
    <div class="status-bar">
        <div class="status-info">
            <span id="cursor-position">Line 1, Column 1</span>
            <span id="word-count">0 words</span>
            <span id="dirty-status" class="dirty-indicator" style="display: none;">● Unsaved changes</span>
        </div>
        <div>
            <span id="save-status" class="save-status">Ready</span>
        </div>
    </div>

    <script>
        const sessionId = '{}';
        const editor = document.getElementById('markdown-editor');
        const previewPanel = document.getElementById('preview-panel');
        let isDirty = false;
        let ws = null;
        let autoSaveTimer = null;
        let originalContent = `{}`;
        
        function initWebSocket() {{
            const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
            const wsUrl = `${{protocol}}//${{window.location.host}}/ws/editor`;
            ws = new WebSocket(wsUrl);
            
            ws.onopen = () => {{
                console.log('Editor WebSocket connected');
                updateSaveStatus('connected', 'Connected');
            }};
            
            ws.onclose = () => {{
                console.log('WebSocket disconnected, reconnecting...');
                updateSaveStatus('error', 'Disconnected');
                setTimeout(initWebSocket, 1000);
            }};
            
            ws.onmessage = handleWebSocketMessage;
            
            ws.onerror = (error) => {{
                console.error('WebSocket error:', error);
                updateSaveStatus('error', 'Connection error');
            }};
        }}
        
        function handleWebSocketMessage(event) {{
            try {{
                const message = JSON.parse(event.data);
                console.log('Received message:', message);
                
                switch (message.type) {{
                    case 'save_complete':
                        if (message.session_id === sessionId) {{
                            setDirty(false);
                            updateSaveStatus('saved', 'Saved');
                            originalContent = editor.value;
                            
                            // Clear save status after 2 seconds
                            setTimeout(() => {{
                                updateSaveStatus('', 'Ready');
                            }}, 2000);
                        }}
                        break;
                    default:
                        console.log('Unknown message type:', message.type);
                }}
            }} catch (e) {{
                console.error('Failed to parse WebSocket message:', e);
            }}
        }}
        
        function sendMessage(message) {{
            if (ws && ws.readyState === WebSocket.OPEN) {{
                ws.send(JSON.stringify(message));
                return true;
            }} else {{
                console.error('WebSocket not connected');
                updateSaveStatus('error', 'Not connected');
                return false;
            }}
        }}
        
        function updateStatus() {{
            const text = editor.value;
            const lines = text.split('\\n');
            const currentLine = getCurrentLine();
            const currentCol = getCurrentColumn();
            
            const words = text.trim() ? text.trim().split(/\\s+/).length : 0;
            
            document.getElementById('cursor-position').textContent = `Line ${{currentLine}}, Column ${{currentCol}}`;
            document.getElementById('word-count').textContent = `${{words}} words`;
            
            // Update preview
            updatePreview(text);
        }}
        
        function getCurrentLine() {{
            const cursorPos = editor.selectionStart;
            const textBeforeCursor = editor.value.substring(0, cursorPos);
            return textBeforeCursor.split('\\n').length;
        }}
        
        function getCurrentColumn() {{
            const cursorPos = editor.selectionStart;
            const textBeforeCursor = editor.value.substring(0, cursorPos);
            const lastLineBreak = textBeforeCursor.lastIndexOf('\\n');
            return cursorPos - lastLineBreak;
        }}
        
        function setDirty(dirty) {{
            isDirty = dirty;
            document.getElementById('dirty-status').style.display = dirty ? 'inline' : 'none';
            
            if (dirty) {{
                startAutoSave();
            }}
        }}
        
        function startAutoSave() {{
            if (autoSaveTimer) {{
                clearTimeout(autoSaveTimer);
            }}
            
            autoSaveTimer = setTimeout(() => {{
                if (isDirty) {{
                    saveContent(true);
                }}
            }}, 2000); // Auto-save after 2 seconds of no changes
        }}
        
        function updateSaveStatus(type, text) {{
            const statusElement = document.getElementById('save-status');
            statusElement.textContent = text;
            statusElement.className = `save-status ${{type}}`;
        }}
        
        function saveContent(isAutoSave = false) {{
            const content = editor.value;
            
            if (!sendMessage({{
                type: 'content_update',
                session_id: sessionId,
                content: content,
                cursor_position: {{
                    line: getCurrentLine(),
                    column: getCurrentColumn()
                }}
            }})) {{
                return;
            }}
            
            if (!sendMessage({{
                type: 'save_request',
                session_id: sessionId
            }})) {{
                return;
            }}
            
            updateSaveStatus('saving', isAutoSave ? 'Auto-saving...' : 'Saving...');
            
            if (autoSaveTimer) {{
                clearTimeout(autoSaveTimer);
                autoSaveTimer = null;
            }}
        }}
        
        function switchToRaw() {{
            window.location.href = '/editor';
        }}
        
        function handleContentChange() {{
            const currentContent = editor.value;
            if (currentContent !== originalContent) {{
                setDirty(true);
            }} else {{
                setDirty(false);
            }}
            updateStatus();
        }}
        
        // 使用服务端Quill引擎渲染markdown 
        async function updatePreview(markdown) {{
            try {{
                const response = await fetch('/api/render-markdown', {{
                    method: 'POST',
                    headers: {{
                        'Content-Type': 'application/json',
                    }},
                    body: JSON.stringify({{ 
                        markdown: markdown
                    }})
                }});
                
                if (response.ok) {{
                    const result = await response.json();
                    if (result.success) {{
                        previewPanel.innerHTML = result.html;
                    }} else {{
                        console.error('API returned error:', result.error);
                        fallbackRender(markdown);
                    }}
                }} else {{
                    console.error('Failed to render markdown:', response.statusText);
                    // 回退到简单渲染
                    fallbackRender(markdown);
                }}
            }} catch (error) {{
                console.error('Error rendering markdown:', error);
                // 回退到简单渲染
                fallbackRender(markdown);
            }}
        }}
        
        // 简单的回退渲染（保留原有逻辑作为备份）
        function fallbackRender(markdown) {{
            let html = markdown;
            
            // Headers
            html = html.replace(/^# (.+$)/gm, '<h1>$1</h1>');
            html = html.replace(/^## (.+$)/gm, '<h2>$1</h2>');
            html = html.replace(/^### (.+$)/gm, '<h3>$1</h3>');
            html = html.replace(/^#### (.+$)/gm, '<h4>$1</h4>');
            
            // Bold and italic
            html = html.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');
            html = html.replace(/\*(.+?)\*/g, '<em>$1</em>');
            
            // Code
            html = html.replace(/`(.+?)`/g, '<code>$1</code>');
            
            // Links
            html = html.replace(/\[(.+?)\]\((.+?)\)/g, '<a href="$2">$1</a>');
            
            // Lists
            html = html.replace(/^- (.+$)/gm, '<li>$1</li>');
            html = html.replace(/(<li>.*<\/li>)/s, '<ul>$1</ul>');
            
            // Paragraphs
            html = html.replace(/^(?!<[h|u|l]).+$/gm, '<p>$&</p>');
            
            // Line breaks
            html = html.replace(/\\n/g, '<br>');
            
            previewPanel.innerHTML = html;
        }}
        
        // Event listeners
        editor.addEventListener('input', handleContentChange);
        editor.addEventListener('keyup', updateStatus);
        editor.addEventListener('click', updateStatus);
        
        // Keyboard shortcuts
        document.addEventListener('keydown', (e) => {{
            if (e.ctrlKey || e.metaKey) {{
                if (e.key === 's') {{
                    e.preventDefault();
                    saveContent();
                }}
            }}
        }});
        
        // Tab key support
        editor.addEventListener('keydown', (e) => {{
            if (e.key === 'Tab') {{
                e.preventDefault();
                const start = editor.selectionStart;
                const end = editor.selectionEnd;
                
                editor.value = editor.value.substring(0, start) + '  ' + editor.value.substring(end);
                editor.selectionStart = editor.selectionEnd = start + 2;
                
                handleContentChange();
            }}
        }});
        
        // Warning for unsaved changes
        window.addEventListener('beforeunload', (e) => {{
            if (isDirty) {{
                const message = 'You have unsaved changes. Are you sure you want to leave?';
                e.preventDefault();
                e.returnValue = message;
                return message;
            }}
        }});
        
        // Initialize
        initWebSocket();
        updateStatus();
        editor.focus();
    </script>
</body>
</html>"#,
            filename, filename, escaped_content, session_id, escaped_content
        )
    }
}

#[async_trait]
impl HttpHandler for SimpleLiveEditorHandler {
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

        let html = self.generate_simple_live_editor_html(&content, &session_id);
        Ok(HttpResponse::html(&html))
    }

    fn priority(&self) -> i32 {
        5 // Higher priority than other editors
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
