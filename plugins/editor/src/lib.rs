//! Editor plugin for Rune with WYSIWYG markdown editing capabilities

use async_trait::async_trait;
use rune_core::{Plugin, PluginContext, PluginStatus, Result, RuneError};
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
        }
    }

    /// Get the session manager
    pub fn session_manager(&self) -> Arc<RwLock<SessionManager>> {
        self.session_manager.clone()
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
        self.status = PluginStatus::Active;

        // Initialize session manager with context
        {
            let mut manager = self.session_manager.write().await;
            manager.initialize(context.clone()).await?;
        }

        tracing::info!("Editor plugin initialized successfully");
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
        let mut manager = self.session_manager.write().await;
        manager.switch_mode(session_id, mode).await
    }

    async fn save_content(&self, session_id: Uuid) -> Result<()> {
        let mut manager = self.session_manager.write().await;
        manager.save_content(session_id).await
    }

    async fn get_content(&self, session_id: Uuid) -> Result<String> {
        let manager = self.session_manager.read().await;
        manager.get_content(session_id).await
    }

    async fn set_content(&self, session_id: Uuid, content: String) -> Result<()> {
        let mut manager = self.session_manager.write().await;
        manager.set_content(session_id, content).await
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
