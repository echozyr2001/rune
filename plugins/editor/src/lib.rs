//! Editor plugin for Rune with WYSIWYG markdown editing capabilities

use async_trait::async_trait;
use rune_core::{
    event::{SystemEvent, SystemEventHandler},
    Plugin, PluginContext, PluginStatus, RenderContext, RendererRegistry, Result, RuneError,
};
use serde::{Deserialize, Serialize};
use std::any::Any;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;
use uuid::Uuid;

pub mod cursor_manager;
pub mod editor_state;
pub mod file_sync;
pub mod inline_renderer;
pub mod keyboard_shortcuts;
pub mod live_editor;
pub mod render_trigger;
pub mod session;
pub mod syntax_highlighter;
pub mod syntax_parser;

pub use cursor_manager::{CursorManager, ElementMapping, MappingStats, PositionMapping};
pub use editor_state::{CursorPosition, EditorMode, EditorState};
pub use file_sync::{
    ConflictRegion, ConflictResolution, ConflictResolutionStrategy, ExternalChange, FileSync,
    FileSyncManager,
};
pub use inline_renderer::{InlineRenderer, MarkdownInlineRenderer, RenderedElement};
pub use keyboard_shortcuts::{
    KeyboardShortcutHandler, ShortcutAction, ShortcutResult, TextSelection,
};
pub use live_editor::{
    ClickToEditResult, LiveEditorIntegration, LiveEditorResult, ModeSwitchResult,
};
pub use render_trigger::{
    RenderTriggerDetector, RenderTriggerHandler, TriggerConfig, TriggerEvent,
};
pub use session::{AutoSaveStatus, EditorSession, SessionManager};
pub use syntax_highlighter::{HighlightToken, SyntaxHighlighter, TokenType};
pub use syntax_parser::{
    MarkdownSyntaxParser, PositionRange, SyntaxElement, SyntaxElementType, SyntaxParser,
};

/// Core editor plugin trait that provides WYSIWYG markdown editing capabilities
#[async_trait]
pub trait EditorPlugin: Plugin {
    /// Get the current editor state
    async fn get_editor_state(&self, session_id: Uuid) -> Result<Arc<EditorState>>;

    /// Switch editing mode for a session
    async fn switch_mode(&self, session_id: Uuid, mode: EditorMode) -> Result<()>;

    /// Save content for a session
    async fn save_content(&self, session_id: Uuid) -> Result<()>;

    /// Get content for a session
    async fn get_content(&self, session_id: Uuid) -> Result<String>;

    /// Set content for a session
    async fn set_content(&self, session_id: Uuid, content: String) -> Result<()>;

    /// Create a new editing session
    async fn create_session(&self, file_path: PathBuf) -> Result<Uuid>;

    /// Close an editing session
    async fn close_session(&self, session_id: Uuid) -> Result<()>;

    /// Get all active sessions
    async fn get_active_sessions(&self) -> Result<Vec<Uuid>>;

    /// Update cursor position for a session
    async fn update_cursor_position(
        &self,
        session_id: Uuid,
        position: CursorPosition,
    ) -> Result<()>;

    /// Check if session has unsaved changes
    async fn has_unsaved_changes(&self, session_id: Uuid) -> Result<bool>;

    /// Enable or disable auto-save for a session
    async fn set_auto_save(&self, session_id: Uuid, enabled: bool) -> Result<()>;

    /// Handle space key press for render trigger detection
    async fn handle_space_key(
        &self,
        session_id: Uuid,
        cursor_position: CursorPosition,
    ) -> Result<bool>;

    /// Check if any sessions should trigger rendering
    async fn check_render_triggers(&self) -> Result<Vec<Uuid>>;

    /// Get pending trigger events for a session
    async fn get_pending_trigger_events(&self, session_id: Uuid) -> Result<Vec<TriggerEvent>>;

    /// Clear trigger events for a session
    async fn clear_trigger_events(&self, session_id: Uuid) -> Result<()>;

    /// Force render trigger for a session
    async fn force_render_trigger(&self, session_id: Uuid) -> Result<bool>;

    /// Update render trigger configuration for a session
    async fn update_trigger_config(&self, session_id: Uuid, config: TriggerConfig) -> Result<()>;

    /// Process content with live rendering integration
    async fn process_live_content(
        &self,
        session_id: Uuid,
        trigger_events: Vec<TriggerEvent>,
    ) -> Result<LiveEditorResult>;

    /// Handle click-to-edit functionality
    async fn handle_click_to_edit(
        &self,
        session_id: Uuid,
        click_position: usize,
    ) -> Result<ClickToEditResult>;

    /// Handle mode switching with cursor position preservation
    async fn handle_mode_switch(
        &self,
        session_id: Uuid,
        from_mode: EditorMode,
        to_mode: EditorMode,
    ) -> Result<ModeSwitchResult>;

    /// Update content of the currently active element
    async fn update_active_element_content(
        &self,
        session_id: Uuid,
        new_content: String,
    ) -> Result<bool>;

    /// Get auto-save status for a session
    async fn get_auto_save_status(&self, session_id: Uuid) -> Result<AutoSaveStatus>;

    /// Trigger auto-save for a session (with debouncing)
    async fn trigger_auto_save(&self, session_id: Uuid) -> Result<()>;

    /// Apply a keyboard shortcut action to a session
    async fn apply_keyboard_shortcut(
        &self,
        session_id: Uuid,
        action: ShortcutAction,
        selection: TextSelection,
    ) -> Result<ShortcutResult>;
}

/// Main editor plugin implementation
pub struct RuneEditorPlugin {
    name: String,
    version: String,
    status: PluginStatus,
    session_manager: Arc<RwLock<SessionManager>>,
    context: Option<PluginContext>,
    renderer_registry: Option<Arc<RendererRegistry>>,
    current_theme: Arc<RwLock<String>>,
}

impl RuneEditorPlugin {
    /// Create a new editor plugin instance
    pub fn new() -> Self {
        Self {
            name: "editor".to_string(),
            version: "0.1.0".to_string(),
            status: PluginStatus::Loading,
            session_manager: Arc::new(RwLock::new(SessionManager::new())),
            context: None,
            renderer_registry: None,
            current_theme: Arc::new(RwLock::new("catppuccin-mocha".to_string())),
        }
    }

    /// Get the session manager
    pub fn session_manager(&self) -> Arc<RwLock<SessionManager>> {
        self.session_manager.clone()
    }

    /// Get the current theme
    pub async fn get_current_theme(&self) -> String {
        self.current_theme.read().await.clone()
    }

    /// Set the current theme
    pub async fn set_current_theme(&self, theme: String) {
        let mut current = self.current_theme.write().await;
        *current = theme;
    }

    /// Trigger rendering for a session's content
    ///
    /// This method integrates with the renderer pipeline to render editor content.
    /// It's called when editor content changes to ensure the preview is updated.
    pub async fn trigger_render_for_session(&self, session_id: Uuid) -> Result<()> {
        let manager = self.session_manager.read().await;
        let content = manager.get_content(session_id).await?;
        let session = manager
            .get_session_info(session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;

        if let Some(registry) = &self.renderer_registry {
            let start_time = std::time::Instant::now();

            // Create render context with current theme
            let theme = self.get_current_theme().await;
            let context = RenderContext::new(
                session.file_path.clone(),
                session
                    .file_path
                    .parent()
                    .unwrap_or(std::path::Path::new("."))
                    .to_path_buf(),
                theme,
            );

            // Render the content through the pipeline
            let render_result = registry.render_with_pipeline(&content, &context).await?;

            let duration = start_time.elapsed();

            // Publish render complete event
            if let Some(context) = &self.context {
                let event =
                    SystemEvent::render_complete(format!("{:x}", session_id.as_u128()), duration);
                context.event_bus.publish_system_event(event).await?;
            }

            tracing::debug!(
                "Rendered content for session {} in {:?}",
                session_id,
                duration
            );

            // Store rendered content in session for preview mode
            // This would be used by the server to serve the preview
            if let Some(context) = &self.context {
                context
                    .set_shared_resource(
                        format!("editor_rendered_content_{}", session_id),
                        render_result.html,
                    )
                    .await?;
            }
        }

        Ok(())
    }

    /// Handle external file change for a session
    ///
    /// When a file is modified externally (e.g., by another editor or git),
    /// this method detects the change and updates the session accordingly.
    #[allow(dead_code)]
    async fn handle_external_file_change(&self, file_path: &PathBuf) -> Result<()> {
        let manager = self.session_manager.read().await;

        // Find sessions editing this file
        let matching_sessions: Vec<Uuid> = manager
            .get_active_sessions()
            .into_iter()
            .filter(|session_id| {
                if let Some(session) = manager.get_session_info(*session_id) {
                    session.file_path == *file_path
                } else {
                    false
                }
            })
            .collect();

        drop(manager);

        // Handle external changes for each matching session
        for session_id in matching_sessions {
            tracing::info!(
                "Detected external change for session {} (file: {})",
                session_id,
                file_path.display()
            );

            let mut manager = self.session_manager.write().await;

            // Check for external changes
            if let Ok(Some(external_change)) = manager.check_external_changes(session_id).await {
                tracing::info!(
                    "External change detected for session {}, resolving conflict",
                    session_id
                );

                // Handle the external change with conflict resolution
                match manager
                    .handle_external_change(session_id, external_change)
                    .await
                {
                    Ok(resolution) => {
                        if resolution.success {
                            tracing::info!(
                                "Successfully resolved external change for session {}",
                                session_id
                            );

                            // Trigger render after resolving external change
                            drop(manager);
                            self.trigger_render_for_session(session_id).await?;
                        } else {
                            tracing::warn!(
                                "Could not auto-resolve conflict for session {}, {} unresolved regions",
                                session_id,
                                resolution.unresolved_conflicts.len()
                            );
                        }
                    }
                    Err(e) => {
                        tracing::error!(
                            "Failed to handle external change for session {}: {}",
                            session_id,
                            e
                        );
                    }
                }
            }
        }

        Ok(())
    }
}

#[async_trait]
impl Plugin for RuneEditorPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn dependencies(&self) -> Vec<&str> {
        vec!["file-watcher", "renderer"]
    }

    async fn initialize(&mut self, context: &PluginContext) -> Result<()> {
        tracing::info!("Initializing editor plugin");

        self.context = Some(context.clone());

        // Get the renderer registry from shared resources
        if let Some(registry) = context
            .get_shared_resource::<Arc<RendererRegistry>>("renderer_registry")
            .await
        {
            self.renderer_registry = Some(registry.as_ref().clone());
            tracing::info!("Editor plugin connected to renderer registry");
        } else {
            tracing::warn!("Renderer registry not found, editor will not trigger rendering");
        }

        // Initialize session manager with context
        {
            let mut manager = self.session_manager.write().await;
            manager.initialize(context.clone()).await?;
        }

        // Subscribe to system events for file changes and theme changes
        let event_handler = Arc::new(EditorEventHandler {
            plugin: Arc::new(RwLock::new(EditorPluginHandle {
                session_manager: self.session_manager.clone(),
                renderer_registry: self.renderer_registry.clone(),
                current_theme: self.current_theme.clone(),
                context: context.clone(),
            })),
        });

        context
            .event_bus
            .subscribe_system_events(event_handler)
            .await?;

        self.status = PluginStatus::Active;
        tracing::info!(
            "Editor plugin initialized successfully with file watcher and renderer integration"
        );
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        tracing::info!("Shutting down editor plugin");

        self.status = PluginStatus::Shutting;

        // Shutdown session manager and save any unsaved changes
        {
            let mut manager = self.session_manager.write().await;
            manager.shutdown().await?;
        }

        self.status = PluginStatus::Stopped;
        tracing::info!("Editor plugin shutdown complete");
        Ok(())
    }

    fn status(&self) -> PluginStatus {
        self.status.clone()
    }

    fn provided_services(&self) -> Vec<&str> {
        vec!["editor", "wysiwyg-editing", "markdown-editing"]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[async_trait]
impl EditorPlugin for RuneEditorPlugin {
    async fn get_editor_state(&self, session_id: Uuid) -> Result<Arc<EditorState>> {
        let manager = self.session_manager.read().await;
        manager.get_editor_state(session_id).await
    }

    async fn switch_mode(&self, session_id: Uuid, mode: EditorMode) -> Result<()> {
        {
            let mut manager = self.session_manager.write().await;
            manager.switch_mode(session_id, mode.clone()).await?;
        }

        // Trigger rendering when switching to preview mode
        if matches!(mode, EditorMode::Preview) {
            self.trigger_render_for_session(session_id).await?;
        }

        Ok(())
    }

    async fn save_content(&self, session_id: Uuid) -> Result<()> {
        {
            let mut manager = self.session_manager.write().await;
            manager.save_content(session_id).await?;
        }

        // Trigger rendering after save to ensure preview is up to date
        self.trigger_render_for_session(session_id).await?;

        Ok(())
    }

    async fn get_content(&self, session_id: Uuid) -> Result<String> {
        let manager = self.session_manager.read().await;
        manager.get_content(session_id).await
    }

    async fn set_content(&self, session_id: Uuid, content: String) -> Result<()> {
        {
            let mut manager = self.session_manager.write().await;
            manager.set_content(session_id, content).await?;
        }

        // Trigger rendering after content change
        self.trigger_render_for_session(session_id).await?;

        Ok(())
    }

    async fn create_session(&self, file_path: PathBuf) -> Result<Uuid> {
        let mut manager = self.session_manager.write().await;
        manager.create_session(file_path).await
    }

    async fn close_session(&self, session_id: Uuid) -> Result<()> {
        let mut manager = self.session_manager.write().await;
        manager.close_session(session_id).await
    }

    async fn get_active_sessions(&self) -> Result<Vec<Uuid>> {
        let manager = self.session_manager.read().await;
        Ok(manager.get_active_sessions())
    }

    async fn update_cursor_position(
        &self,
        session_id: Uuid,
        position: CursorPosition,
    ) -> Result<()> {
        let mut manager = self.session_manager.write().await;
        manager.update_cursor_position(session_id, position).await
    }

    async fn has_unsaved_changes(&self, session_id: Uuid) -> Result<bool> {
        let manager = self.session_manager.read().await;
        manager.has_unsaved_changes(session_id).await
    }

    async fn set_auto_save(&self, session_id: Uuid, enabled: bool) -> Result<()> {
        let mut manager = self.session_manager.write().await;
        manager.set_auto_save(session_id, enabled).await
    }

    async fn handle_space_key(
        &self,
        session_id: Uuid,
        cursor_position: CursorPosition,
    ) -> Result<bool> {
        let mut manager = self.session_manager.write().await;
        manager.handle_space_key(session_id, cursor_position).await
    }

    async fn check_render_triggers(&self) -> Result<Vec<Uuid>> {
        let mut manager = self.session_manager.write().await;
        manager.check_render_triggers().await
    }

    async fn get_pending_trigger_events(&self, session_id: Uuid) -> Result<Vec<TriggerEvent>> {
        let manager = self.session_manager.read().await;
        manager.get_pending_trigger_events(session_id).await
    }

    async fn clear_trigger_events(&self, session_id: Uuid) -> Result<()> {
        let mut manager = self.session_manager.write().await;
        manager.clear_trigger_events(session_id).await
    }

    async fn force_render_trigger(&self, session_id: Uuid) -> Result<bool> {
        let mut manager = self.session_manager.write().await;
        manager.force_render_trigger(session_id).await
    }

    async fn update_trigger_config(&self, session_id: Uuid, config: TriggerConfig) -> Result<()> {
        let mut manager = self.session_manager.write().await;
        manager.update_trigger_config(session_id, config).await
    }

    async fn process_live_content(
        &self,
        session_id: Uuid,
        trigger_events: Vec<TriggerEvent>,
    ) -> Result<LiveEditorResult> {
        let mut manager = self.session_manager.write().await;
        manager
            .process_live_content(session_id, trigger_events)
            .await
    }

    async fn handle_click_to_edit(
        &self,
        session_id: Uuid,
        click_position: usize,
    ) -> Result<ClickToEditResult> {
        let mut manager = self.session_manager.write().await;
        manager
            .handle_click_to_edit(session_id, click_position)
            .await
    }

    async fn handle_mode_switch(
        &self,
        session_id: Uuid,
        from_mode: EditorMode,
        to_mode: EditorMode,
    ) -> Result<ModeSwitchResult> {
        let mut manager = self.session_manager.write().await;
        manager
            .handle_mode_switch(session_id, from_mode, to_mode)
            .await
    }

    async fn update_active_element_content(
        &self,
        session_id: Uuid,
        new_content: String,
    ) -> Result<bool> {
        let mut manager = self.session_manager.write().await;
        manager
            .update_active_element_content(session_id, new_content)
            .await
    }

    async fn get_auto_save_status(&self, session_id: Uuid) -> Result<AutoSaveStatus> {
        let manager = self.session_manager.read().await;
        manager.get_auto_save_status(session_id).await
    }

    async fn trigger_auto_save(&self, session_id: Uuid) -> Result<()> {
        let mut manager = self.session_manager.write().await;
        manager.trigger_auto_save(session_id).await
    }

    async fn apply_keyboard_shortcut(
        &self,
        session_id: Uuid,
        action: ShortcutAction,
        selection: TextSelection,
    ) -> Result<ShortcutResult> {
        let mut manager = self.session_manager.write().await;
        manager
            .apply_keyboard_shortcut(session_id, action, selection)
            .await
    }
}

impl Default for RuneEditorPlugin {
    fn default() -> Self {
        Self::new()
    }
}

/// Editor-specific events for WebSocket communication and system integration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EditorEvent {
    /// Content changed in a session
    ContentChanged {
        session_id: Uuid,
        content: String,
        cursor_position: CursorPosition,
    },
    /// Editor mode changed
    ModeChanged { session_id: Uuid, mode: EditorMode },
    /// Save was requested
    SaveRequested { session_id: Uuid },
    /// Save completed
    SaveCompleted {
        session_id: Uuid,
        success: bool,
        timestamp: SystemTime,
    },
    /// Cursor position moved
    CursorMoved {
        session_id: Uuid,
        position: CursorPosition,
    },
    /// Auto-save triggered
    AutoSaveTriggered { session_id: Uuid },
    /// Session created
    SessionCreated {
        session_id: Uuid,
        file_path: PathBuf,
    },
    /// Session closed
    SessionClosed { session_id: Uuid },
    /// Auto-save status changed
    AutoSaveStatusChanged {
        session_id: Uuid,
        status: AutoSaveStatus,
    },
}

impl EditorEvent {
    /// Get the event type as a string
    pub fn event_type(&self) -> &str {
        match self {
            EditorEvent::ContentChanged { .. } => "content_changed",
            EditorEvent::ModeChanged { .. } => "mode_changed",
            EditorEvent::SaveRequested { .. } => "save_requested",
            EditorEvent::SaveCompleted { .. } => "save_completed",
            EditorEvent::CursorMoved { .. } => "cursor_moved",
            EditorEvent::AutoSaveTriggered { .. } => "auto_save_triggered",
            EditorEvent::SessionCreated { .. } => "session_created",
            EditorEvent::SessionClosed { .. } => "session_closed",
            EditorEvent::AutoSaveStatusChanged { .. } => "auto_save_status_changed",
        }
    }

    /// Get the session ID for this event
    pub fn session_id(&self) -> Uuid {
        match self {
            EditorEvent::ContentChanged { session_id, .. }
            | EditorEvent::ModeChanged { session_id, .. }
            | EditorEvent::SaveRequested { session_id, .. }
            | EditorEvent::SaveCompleted { session_id, .. }
            | EditorEvent::CursorMoved { session_id, .. }
            | EditorEvent::AutoSaveTriggered { session_id, .. }
            | EditorEvent::SessionCreated { session_id, .. }
            | EditorEvent::SessionClosed { session_id, .. }
            | EditorEvent::AutoSaveStatusChanged { session_id, .. } => *session_id,
        }
    }

    /// Serialize the event to JSON for WebSocket transmission
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self)
            .map_err(|e| RuneError::Plugin(format!("Failed to serialize editor event: {}", e)))
    }

    /// Deserialize an event from JSON
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json)
            .map_err(|e| RuneError::Plugin(format!("Failed to deserialize editor event: {}", e)))
    }
}

/// Editor-specific errors
#[derive(Debug, thiserror::Error)]
pub enum EditorError {
    #[error("Session not found: {0}")]
    SessionNotFound(Uuid),

    #[error("File operation failed: {0}")]
    FileOperationFailed(String),

    #[error("Invalid cursor position: line {line}, column {column}")]
    InvalidCursorPosition { line: usize, column: usize },

    #[error("Mode switch failed: {0}")]
    ModeSwitchFailed(String),

    #[error("Auto-save failed: {0}")]
    AutoSaveFailed(String),

    #[error("Content synchronization failed: {0}")]
    ContentSyncFailed(String),
}

impl From<EditorError> for RuneError {
    fn from(err: EditorError) -> Self {
        RuneError::Plugin(err.to_string())
    }
}

/// Handle to editor plugin for event handling
///
/// This structure provides access to the editor plugin's components
/// from the event handler without requiring a full plugin reference.
pub struct EditorPluginHandle {
    session_manager: Arc<RwLock<SessionManager>>,
    renderer_registry: Option<Arc<RendererRegistry>>,
    current_theme: Arc<RwLock<String>>,
    context: PluginContext,
}

impl EditorPluginHandle {
    /// Trigger rendering for a session's content
    async fn trigger_render_for_session(&self, session_id: Uuid) -> Result<()> {
        let manager = self.session_manager.read().await;
        let content = manager.get_content(session_id).await?;
        let session = manager
            .get_session_info(session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;

        if let Some(registry) = &self.renderer_registry {
            let start_time = std::time::Instant::now();

            // Create render context with current theme
            let theme = self.current_theme.read().await.clone();
            let context = RenderContext::new(
                session.file_path.clone(),
                session
                    .file_path
                    .parent()
                    .unwrap_or(std::path::Path::new("."))
                    .to_path_buf(),
                theme,
            );

            // Render the content through the pipeline
            let render_result = registry.render_with_pipeline(&content, &context).await?;

            let duration = start_time.elapsed();

            // Publish render complete event
            let event =
                SystemEvent::render_complete(format!("{:x}", session_id.as_u128()), duration);
            self.context.event_bus.publish_system_event(event).await?;

            tracing::debug!(
                "Rendered content for session {} in {:?}",
                session_id,
                duration
            );

            // Store rendered content in session for preview mode
            self.context
                .set_shared_resource(
                    format!("editor_rendered_content_{}", session_id),
                    render_result.html,
                )
                .await?;
        }

        Ok(())
    }

    /// Handle external file change for a session
    async fn handle_external_file_change(&self, file_path: &PathBuf) -> Result<()> {
        let manager = self.session_manager.read().await;

        // Find sessions editing this file
        let matching_sessions: Vec<Uuid> = manager
            .get_active_sessions()
            .into_iter()
            .filter(|session_id| {
                if let Some(session) = manager.get_session_info(*session_id) {
                    session.file_path == *file_path
                } else {
                    false
                }
            })
            .collect();

        drop(manager);

        // Handle external changes for each matching session
        for session_id in matching_sessions {
            tracing::info!(
                "Detected external change for session {} (file: {})",
                session_id,
                file_path.display()
            );

            let mut manager = self.session_manager.write().await;

            // Check for external changes
            if let Ok(Some(external_change)) = manager.check_external_changes(session_id).await {
                tracing::info!(
                    "External change detected for session {}, resolving conflict",
                    session_id
                );

                // Handle the external change with conflict resolution
                match manager
                    .handle_external_change(session_id, external_change)
                    .await
                {
                    Ok(resolution) => {
                        if resolution.success {
                            tracing::info!(
                                "Successfully resolved external change for session {}",
                                session_id
                            );

                            // Trigger render after resolving external change
                            drop(manager);
                            self.trigger_render_for_session(session_id).await?;
                        } else {
                            tracing::warn!(
                                "Could not auto-resolve conflict for session {}, {} unresolved regions",
                                session_id,
                                resolution.unresolved_conflicts.len()
                            );
                        }
                    }
                    Err(e) => {
                        tracing::error!(
                            "Failed to handle external change for session {}: {}",
                            session_id,
                            e
                        );
                    }
                }
            }
        }

        Ok(())
    }
}

/// Event handler for editor plugin system events
///
/// This handler integrates the editor with:
/// - File watcher: Responds to external file changes
/// - Renderer: Triggers rendering when content changes
/// - Theme system: Updates editor theme when theme changes
pub struct EditorEventHandler {
    plugin: Arc<RwLock<EditorPluginHandle>>,
}

#[async_trait]
impl SystemEventHandler for EditorEventHandler {
    async fn handle_system_event(&self, event: &SystemEvent) -> Result<()> {
        match event {
            SystemEvent::FileChanged {
                path, change_type, ..
            } => {
                tracing::debug!(
                    "Editor received file changed event: {} ({:?})",
                    path.display(),
                    change_type
                );

                // Handle external file changes for active sessions
                let plugin = self.plugin.read().await;
                if let Err(e) = plugin.handle_external_file_change(path).await {
                    tracing::error!("Failed to handle external file change: {}", e);
                }
            }
            SystemEvent::ThemeChanged { theme_name, .. } => {
                tracing::info!("Editor received theme changed event: {}", theme_name);

                // Update current theme
                let plugin = self.plugin.read().await;
                let mut current_theme = plugin.current_theme.write().await;
                *current_theme = theme_name.clone();

                // Trigger re-rendering for all active sessions with new theme
                let manager = plugin.session_manager.read().await;
                let active_sessions = manager.get_active_sessions();
                drop(manager);
                drop(current_theme);

                for session_id in active_sessions {
                    if let Err(e) = plugin.trigger_render_for_session(session_id).await {
                        tracing::error!(
                            "Failed to re-render session {} with new theme: {}",
                            session_id,
                            e
                        );
                    }
                }

                tracing::info!(
                    "Updated editor theme to {} and re-rendered active sessions",
                    theme_name
                );
            }
            SystemEvent::RenderComplete {
                content_hash,
                duration,
                ..
            } => {
                tracing::trace!(
                    "Render completed for content {} in {:?}",
                    content_hash,
                    duration
                );
                // Editor can track render performance if needed
            }
            _ => {
                // Ignore other events
            }
        }
        Ok(())
    }

    fn handler_name(&self) -> &str {
        "editor-event-handler"
    }
}
