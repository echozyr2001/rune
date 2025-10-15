//! Session management for editor instances

use crate::editor_state::{CursorPosition, EditorMode, EditorState};
use crate::render_trigger::{RenderTriggerDetector, TriggerConfig, TriggerEvent};
use crate::syntax_parser::{MarkdownSyntaxParser, SyntaxParser};
use crate::EditorError;
use rune_core::{PluginContext, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::fs;
use uuid::Uuid;

/// Individual editor session representing one file being edited
#[derive(Debug)]
pub struct EditorSession {
    /// Unique session identifier
    pub id: Uuid,
    /// Path to the file being edited
    pub file_path: PathBuf,
    /// Current editor state
    pub state: Arc<EditorState>,
    /// When the session was created
    pub created_at: SystemTime,
    /// When the session was last accessed
    pub last_accessed: SystemTime,
    /// Whether the session is currently active
    pub is_active: bool,
    /// Auto-save configuration for this session
    pub auto_save_config: AutoSaveConfig,
    /// Render trigger detection system
    pub render_trigger_detector: RenderTriggerDetector,
    /// Syntax parser for detecting block elements
    pub syntax_parser: MarkdownSyntaxParser,
}

impl EditorSession {
    /// Create a new editor session
    pub async fn new(file_path: PathBuf) -> Result<Self> {
        let session_id = Uuid::new_v4();

        // Load file content if it exists
        let content = if file_path.exists() {
            fs::read_to_string(&file_path).await.map_err(|e| {
                EditorError::FileOperationFailed(format!("Failed to read file: {}", e))
            })?
        } else {
            String::new()
        };

        let state = Arc::new(EditorState::new(session_id, content));
        let now = SystemTime::now();

        Ok(Self {
            id: session_id,
            file_path,
            state,
            created_at: now,
            last_accessed: now,
            is_active: true,
            auto_save_config: AutoSaveConfig::default(),
            render_trigger_detector: RenderTriggerDetector::with_defaults(),
            syntax_parser: MarkdownSyntaxParser::new(),
        })
    }

    /// Update the last accessed time
    pub fn touch(&mut self) {
        self.last_accessed = SystemTime::now();
    }

    /// Get mutable access to the editor state
    pub fn state_mut(&mut self) -> &mut EditorState {
        Arc::make_mut(&mut self.state)
    }

    /// Save the session content to file
    pub async fn save(&mut self) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                EditorError::FileOperationFailed(format!("Failed to create directory: {}", e))
            })?;
        }

        // Write content to file
        fs::write(&self.file_path, &self.state.content)
            .await
            .map_err(|e| {
                EditorError::FileOperationFailed(format!("Failed to write file: {}", e))
            })?;

        // Update state
        Arc::make_mut(&mut self.state).mark_saved();

        self.touch();
        tracing::info!("Saved session {} to {}", self.id, self.file_path.display());
        Ok(())
    }

    /// Check if the session should be auto-saved
    pub fn should_auto_save(&self) -> bool {
        self.auto_save_config.enabled && self.state.should_auto_save()
    }

    /// Get session age
    pub fn age(&self) -> Option<std::time::Duration> {
        self.created_at.elapsed().ok()
    }

    /// Get time since last access
    pub fn idle_time(&self) -> Option<std::time::Duration> {
        self.last_accessed.elapsed().ok()
    }

    /// Handle space key press for render trigger detection
    pub fn handle_space_key(&mut self, cursor_position: CursorPosition) -> bool {
        self.render_trigger_detector.detect_space_key(cursor_position)
    }

    /// Handle cursor movement for render trigger detection
    pub fn handle_cursor_movement(&mut self, new_position: CursorPosition) -> bool {
        self.render_trigger_detector.detect_cursor_movement(new_position)
    }

    /// Handle content change for render trigger detection
    pub fn handle_content_change(&mut self, new_content: &str, change_start: usize, change_end: usize) -> bool {
        // Parse syntax elements to detect block completion
        let syntax_elements = self.syntax_parser.parse_document(new_content);
        let cursor_pos = self.state.cursor_position.clone();
        
        // Check for block completion
        let block_completed = self.render_trigger_detector.detect_block_completion(
            new_content,
            cursor_pos,
            &syntax_elements,
        );

        // Check for general content change
        let content_changed = self.render_trigger_detector.detect_content_change(
            new_content,
            change_start,
            change_end,
        );

        block_completed || content_changed
    }

    /// Check if rendering should be triggered (debounced)
    pub fn should_trigger_render(&mut self) -> bool {
        self.render_trigger_detector.should_trigger_render()
    }

    /// Get pending trigger events
    pub fn get_pending_trigger_events(&self) -> &[TriggerEvent] {
        self.render_trigger_detector.get_pending_events()
    }

    /// Clear pending trigger events
    pub fn clear_trigger_events(&mut self) {
        self.render_trigger_detector.clear_pending_events()
    }

    /// Force immediate render trigger
    pub fn force_render_trigger(&mut self) -> bool {
        self.render_trigger_detector.force_trigger()
    }

    /// Update render trigger configuration
    pub fn update_trigger_config(&mut self, config: TriggerConfig) {
        self.render_trigger_detector.update_config(config)
    }
}

/// Auto-save configuration for a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoSaveConfig {
    /// Whether auto-save is enabled
    pub enabled: bool,
    /// Delay in seconds before auto-save triggers
    pub delay_seconds: u64,
    /// Maximum number of auto-save attempts
    pub max_attempts: u32,
}

impl Default for AutoSaveConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            delay_seconds: 2,
            max_attempts: 3,
        }
    }
}

/// Session manager that handles multiple editor sessions
pub struct SessionManager {
    /// Active sessions by ID
    sessions: HashMap<Uuid, EditorSession>,
    /// Plugin context for file operations and events
    context: Option<PluginContext>,
    /// Auto-save task handle
    auto_save_handle: Option<tokio::task::JoinHandle<()>>,
}

impl SessionManager {
    /// Create a new session manager
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            context: None,
            auto_save_handle: None,
        }
    }

    /// Initialize the session manager with plugin context
    pub async fn initialize(&mut self, context: PluginContext) -> Result<()> {
        self.context = Some(context);

        // Start auto-save background task
        self.start_auto_save_task().await?;

        tracing::info!("Session manager initialized");
        Ok(())
    }

    /// Shutdown the session manager
    pub async fn shutdown(&mut self) -> Result<()> {
        tracing::info!("Shutting down session manager");

        // Stop auto-save task
        if let Some(handle) = self.auto_save_handle.take() {
            handle.abort();
        }

        // Save all sessions with unsaved changes
        let mut save_errors = Vec::new();
        for (session_id, session) in &mut self.sessions {
            if session.state.is_dirty {
                if let Err(e) = session.save().await {
                    save_errors.push((*session_id, e));
                }
            }
        }

        // Clear all sessions
        self.sessions.clear();

        if !save_errors.is_empty() {
            tracing::warn!("Some sessions failed to save during shutdown:");
            for (id, error) in &save_errors {
                tracing::warn!("  Session {}: {}", id, error);
            }
        }

        tracing::info!("Session manager shutdown complete");
        Ok(())
    }

    /// Create a new editing session
    pub async fn create_session(&mut self, file_path: PathBuf) -> Result<Uuid> {
        let session = EditorSession::new(file_path.clone()).await?;
        let session_id = session.id;

        self.sessions.insert(session_id, session);

        tracing::info!(
            "Created new session {} for {}",
            session_id,
            file_path.display()
        );
        Ok(session_id)
    }

    /// Close an editing session
    pub async fn close_session(&mut self, session_id: Uuid) -> Result<()> {
        if let Some(mut session) = self.sessions.remove(&session_id) {
            // Save if there are unsaved changes
            if session.state.is_dirty {
                session.save().await?;
            }
            tracing::info!("Closed session {}", session_id);
        } else {
            return Err(EditorError::SessionNotFound(session_id).into());
        }
        Ok(())
    }

    /// Get editor state for a session
    pub async fn get_editor_state(&self, session_id: Uuid) -> Result<Arc<EditorState>> {
        let session = self
            .sessions
            .get(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;
        Ok(session.state.clone())
    }

    /// Switch editing mode for a session
    pub async fn switch_mode(&mut self, session_id: Uuid, mode: EditorMode) -> Result<()> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;

        session.state_mut().switch_mode(mode);
        session.touch();

        tracing::debug!(
            "Switched session {} to mode {}",
            session_id,
            session.state.current_mode
        );
        Ok(())
    }

    /// Get content for a session
    pub async fn get_content(&self, session_id: Uuid) -> Result<String> {
        let session = self
            .sessions
            .get(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;
        Ok(session.state.content.clone())
    }

    /// Set content for a session
    pub async fn set_content(&mut self, session_id: Uuid, content: String) -> Result<()> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;

        let old_content_len = session.state.content.len();
        session.state_mut().update_content(content.clone());
        
        // Detect render triggers for content change
        let change_start = 0;
        let change_end = old_content_len;
        let should_render = session.handle_content_change(&content, change_start, change_end);
        
        if should_render {
            tracing::debug!("Content change triggered render for session {}", session_id);
        }
        
        session.touch();

        tracing::debug!("Updated content for session {}", session_id);
        Ok(())
    }

    /// Save content for a session
    pub async fn save_content(&mut self, session_id: Uuid) -> Result<()> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;

        session.save().await?;
        tracing::info!("Saved content for session {}", session_id);
        Ok(())
    }

    /// Update cursor position for a session
    pub async fn update_cursor_position(
        &mut self,
        session_id: Uuid,
        position: CursorPosition,
    ) -> Result<()> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;

        session
            .state_mut()
            .update_cursor_position(position.clone())
            .map_err(|_e| EditorError::InvalidCursorPosition {
                line: position.line,
                column: position.column,
            })?;

        // Detect render triggers for cursor movement
        let should_render = session.handle_cursor_movement(position);
        
        if should_render {
            tracing::debug!("Cursor movement triggered render for session {}", session_id);
        }
        
        session.touch();

        Ok(())
    }

    /// Check if session has unsaved changes
    pub async fn has_unsaved_changes(&self, session_id: Uuid) -> Result<bool> {
        let session = self
            .sessions
            .get(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;
        Ok(session.state.is_dirty)
    }

    /// Set auto-save for a session
    pub async fn set_auto_save(&mut self, session_id: Uuid, enabled: bool) -> Result<()> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;

        session.auto_save_config.enabled = enabled;
        session.state_mut().set_auto_save(enabled);
        session.touch();

        tracing::debug!("Set auto-save {} for session {}", enabled, session_id);
        Ok(())
    }

    /// Get all active session IDs
    pub fn get_active_sessions(&self) -> Vec<Uuid> {
        self.sessions.keys().copied().collect()
    }

    /// Get session information
    pub fn get_session_info(&self, session_id: Uuid) -> Option<&EditorSession> {
        self.sessions.get(&session_id)
    }

    /// Start the auto-save background task
    async fn start_auto_save_task(&mut self) -> Result<()> {
        let _sessions_ref = &self.sessions as *const HashMap<Uuid, EditorSession>;

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));

            loop {
                interval.tick().await;

                // This is a simplified auto-save implementation
                // In a real implementation, we would need proper synchronization
                // and communication with the session manager
                tracing::trace!("Auto-save task tick");
            }
        });

        self.auto_save_handle = Some(handle);
        Ok(())
    }

    /// Perform auto-save for all eligible sessions
    pub async fn perform_auto_save(&mut self) -> Result<Vec<Uuid>> {
        let mut saved_sessions = Vec::new();
        let mut save_errors = Vec::new();

        for (session_id, session) in &mut self.sessions {
            if session.should_auto_save() {
                match session.save().await {
                    Ok(()) => {
                        saved_sessions.push(*session_id);
                        tracing::debug!("Auto-saved session {}", session_id);
                    }
                    Err(e) => {
                        tracing::error!("Auto-save failed for session {}: {}", session_id, e);
                        save_errors.push((*session_id, e));
                    }
                }
            }
        }

        if !save_errors.is_empty() {
            return Err(EditorError::AutoSaveFailed(format!(
                "Failed to auto-save {} sessions",
                save_errors.len()
            ))
            .into());
        }

        Ok(saved_sessions)
    }

    /// Clean up idle sessions
    pub async fn cleanup_idle_sessions(&mut self, max_idle_minutes: u64) -> Result<Vec<Uuid>> {
        let max_idle = std::time::Duration::from_secs(max_idle_minutes * 60);
        let mut closed_sessions = Vec::new();

        let idle_session_ids: Vec<Uuid> = self
            .sessions
            .iter()
            .filter_map(|(id, session)| {
                if let Some(idle_time) = session.idle_time() {
                    if idle_time > max_idle && !session.state.is_dirty {
                        Some(*id)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        for session_id in idle_session_ids {
            if let Err(e) = self.close_session(session_id).await {
                tracing::warn!("Failed to close idle session {}: {}", session_id, e);
            } else {
                closed_sessions.push(session_id);
            }
        }

        if !closed_sessions.is_empty() {
            tracing::info!("Cleaned up {} idle sessions", closed_sessions.len());
        }

        Ok(closed_sessions)
    }

    /// Get session statistics
    pub fn get_session_stats(&self) -> SessionStats {
        let total_sessions = self.sessions.len();
        let active_sessions = self.sessions.values().filter(|s| s.is_active).count();
        let dirty_sessions = self.sessions.values().filter(|s| s.state.is_dirty).count();
        let auto_save_enabled = self
            .sessions
            .values()
            .filter(|s| s.auto_save_config.enabled)
            .count();

        SessionStats {
            total_sessions,
            active_sessions,
            dirty_sessions,
            auto_save_enabled,
        }
    }

    /// Handle space key press for a session
    pub async fn handle_space_key(&mut self, session_id: Uuid, cursor_position: CursorPosition) -> Result<bool> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;

        let should_render = session.handle_space_key(cursor_position);
        session.touch();

        if should_render {
            tracing::debug!("Space key triggered render for session {}", session_id);
        }

        Ok(should_render)
    }

    /// Check if any session should trigger rendering
    pub async fn check_render_triggers(&mut self) -> Result<Vec<Uuid>> {
        let mut sessions_to_render = Vec::new();

        for (session_id, session) in &mut self.sessions {
            if session.should_trigger_render() {
                sessions_to_render.push(*session_id);
                tracing::debug!("Session {} should trigger render", session_id);
            }
        }

        Ok(sessions_to_render)
    }

    /// Get pending trigger events for a session
    pub async fn get_pending_trigger_events(&self, session_id: Uuid) -> Result<Vec<TriggerEvent>> {
        let session = self
            .sessions
            .get(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;

        Ok(session.get_pending_trigger_events().to_vec())
    }

    /// Clear trigger events for a session
    pub async fn clear_trigger_events(&mut self, session_id: Uuid) -> Result<()> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;

        session.clear_trigger_events();
        Ok(())
    }

    /// Force render trigger for a session
    pub async fn force_render_trigger(&mut self, session_id: Uuid) -> Result<bool> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;

        let triggered = session.force_render_trigger();
        session.touch();

        if triggered {
            tracing::debug!("Forced render trigger for session {}", session_id);
        }

        Ok(triggered)
    }

    /// Update render trigger configuration for a session
    pub async fn update_trigger_config(&mut self, session_id: Uuid, config: TriggerConfig) -> Result<()> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;

        session.update_trigger_config(config);
        session.touch();

        tracing::debug!("Updated trigger config for session {}", session_id);
        Ok(())
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Session statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStats {
    pub total_sessions: usize,
    pub active_sessions: usize,
    pub dirty_sessions: usize,
    pub auto_save_enabled: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_session_creation() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("test.md");

        let session = EditorSession::new(file_path.clone()).await.unwrap();

        assert_eq!(session.file_path, file_path);
        assert!(session.is_active);
        assert!(!session.state.is_dirty);
    }

    #[tokio::test]
    async fn test_session_manager_basic_operations() {
        let mut manager = SessionManager::new();
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("test.md");

        // Create session
        let session_id = manager.create_session(file_path.clone()).await.unwrap();
        assert!(manager.sessions.contains_key(&session_id));

        // Update content
        manager
            .set_content(session_id, "Hello, world!".to_string())
            .await
            .unwrap();
        let content = manager.get_content(session_id).await.unwrap();
        assert_eq!(content, "Hello, world!");

        // Check dirty state
        assert!(manager.has_unsaved_changes(session_id).await.unwrap());

        // Save content
        manager.save_content(session_id).await.unwrap();
        assert!(!manager.has_unsaved_changes(session_id).await.unwrap());

        // Close session
        manager.close_session(session_id).await.unwrap();
        assert!(!manager.sessions.contains_key(&session_id));
    }

    #[tokio::test]
    async fn test_cursor_position_updates() {
        let mut manager = SessionManager::new();
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("test.md");

        let session_id = manager.create_session(file_path).await.unwrap();
        manager
            .set_content(session_id, "line 1\nline 2\nline 3".to_string())
            .await
            .unwrap();

        // Update cursor position
        let position = CursorPosition::new(1, 3, 10);
        manager
            .update_cursor_position(session_id, position.clone())
            .await
            .unwrap();

        let state = manager.get_editor_state(session_id).await.unwrap();
        assert_eq!(state.cursor_position.line, 1);
        assert_eq!(state.cursor_position.column, 3);
    }

    #[tokio::test]
    async fn test_mode_switching() {
        let mut manager = SessionManager::new();
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("test.md");

        let session_id = manager.create_session(file_path).await.unwrap();

        // Switch to live mode
        manager
            .switch_mode(session_id, EditorMode::Live)
            .await
            .unwrap();
        let state = manager.get_editor_state(session_id).await.unwrap();
        assert_eq!(state.current_mode, EditorMode::Live);

        // Switch to preview mode
        manager
            .switch_mode(session_id, EditorMode::Preview)
            .await
            .unwrap();
        let state = manager.get_editor_state(session_id).await.unwrap();
        assert_eq!(state.current_mode, EditorMode::Preview);
    }
}
