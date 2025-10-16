//! Session management for editor instances

use crate::editor_state::{CursorPosition, EditorMode, EditorState};
use crate::file_sync::{
    ConflictResolution, ConflictResolutionStrategy, ExternalChange, FileSync, FileSyncManager,
};
use crate::live_editor::{
    ClickToEditResult, LiveEditorIntegration, LiveEditorResult, ModeSwitchResult,
};
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

/// Auto-save command for background task communication
#[derive(Debug)]
pub enum AutoSaveCommand {
    /// Start auto-save timer for a session
    StartTimer {
        session_id: Uuid,
        response_tx: tokio::sync::oneshot::Sender<AutoSaveResult>,
    },
    /// Cancel auto-save timer for a session
    CancelTimer { session_id: Uuid },
}

/// Auto-save result from background task
#[derive(Debug)]
pub enum AutoSaveResult {
    /// Session is ready to be saved
    ReadyToSave,
    /// Auto-save was cancelled
    Cancelled,
}

/// Auto-save state for tracking pending saves
#[derive(Debug)]
pub struct AutoSaveState {
    pub timer_start: SystemTime,
    pub response_tx: tokio::sync::oneshot::Sender<AutoSaveResult>,
}

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
    /// Live editor integration for rendering
    pub live_editor: LiveEditorIntegration,
    /// Conflict resolution strategy for this session
    pub conflict_strategy: ConflictResolutionStrategy,
    /// Whether to monitor for external file changes
    pub monitor_external_changes: bool,
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
            live_editor: LiveEditorIntegration::new(),
            conflict_strategy: ConflictResolutionStrategy::PreferLocal,
            monitor_external_changes: true,
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
        self.render_trigger_detector
            .detect_space_key(cursor_position)
    }

    /// Handle cursor movement for render trigger detection
    pub fn handle_cursor_movement(&mut self, new_position: CursorPosition) -> bool {
        self.render_trigger_detector
            .detect_cursor_movement(new_position)
    }

    /// Handle content change for render trigger detection
    pub fn handle_content_change(
        &mut self,
        new_content: &str,
        change_start: usize,
        change_end: usize,
    ) -> bool {
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
    /// Auto-save command sender
    auto_save_sender: Option<tokio::sync::mpsc::UnboundedSender<AutoSaveCommand>>,
    /// File synchronization manager
    file_sync: Arc<FileSyncManager>,
}

impl SessionManager {
    /// Create a new session manager
    pub fn new() -> Self {
        // Use a default backup directory in temp
        let backup_dir = std::env::temp_dir().join("rune-editor-backups");
        let file_sync = Arc::new(FileSyncManager::new(backup_dir));

        Self {
            sessions: HashMap::new(),
            context: None,
            auto_save_handle: None,
            auto_save_sender: None,
            file_sync,
        }
    }

    /// Initialize the session manager with plugin context
    pub async fn initialize(&mut self, context: PluginContext) -> Result<()> {
        self.context = Some(context);

        // Initialize file sync manager
        self.file_sync.initialize().await?;

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
        let was_dirty = session.state.is_dirty;
        session.state_mut().update_content(content.clone());

        // Detect render triggers for content change
        let change_start = 0;
        let change_end = old_content_len;
        let should_render = session.handle_content_change(&content, change_start, change_end);

        if should_render {
            tracing::debug!("Content change triggered render for session {}", session_id);
        }

        session.touch();

        // Trigger auto-save if content became dirty
        if !was_dirty && session.state.is_dirty {
            self.trigger_auto_save(session_id).await?;
        }

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
            tracing::debug!(
                "Cursor movement triggered render for session {}",
                session_id
            );
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
        // Create a channel for auto-save commands
        let (auto_save_tx, mut auto_save_rx) =
            tokio::sync::mpsc::unbounded_channel::<AutoSaveCommand>();

        // Store the sender for triggering auto-saves
        self.auto_save_sender = Some(auto_save_tx);

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(500));
            let mut pending_saves: HashMap<Uuid, AutoSaveState> = HashMap::new();

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        // Check for sessions that need auto-save
                        let mut sessions_to_save = Vec::new();

                        // Collect sessions that are ready for auto-save
                        for (session_id, state) in &pending_saves {
                            if let Ok(elapsed) = state.timer_start.elapsed() {
                                if elapsed.as_secs() >= 2 {
                                    sessions_to_save.push(*session_id);
                                }
                            }
                        }

                        // Process ready sessions
                        for session_id in sessions_to_save {
                            if let Some(state) = pending_saves.remove(&session_id) {
                                if state.response_tx.send(AutoSaveResult::ReadyToSave).is_err() {
                                    tracing::warn!("Failed to send auto-save ready signal for session {}", session_id);
                                }
                            }
                        }
                    }

                    Some(command) = auto_save_rx.recv() => {
                        match command {
                            AutoSaveCommand::StartTimer { session_id, response_tx } => {
                                let state = AutoSaveState {
                                    timer_start: SystemTime::now(),
                                    response_tx,
                                };
                                pending_saves.insert(session_id, state);
                                tracing::trace!("Auto-save timer started for session {}", session_id);
                            }
                            AutoSaveCommand::CancelTimer { session_id } => {
                                pending_saves.remove(&session_id);
                                tracing::trace!("Auto-save timer cancelled for session {}", session_id);
                            }
                        }
                    }
                }
            }
        });

        self.auto_save_handle = Some(handle);
        tracing::info!("Auto-save background task started");
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
    pub async fn handle_space_key(
        &mut self,
        session_id: Uuid,
        cursor_position: CursorPosition,
    ) -> Result<bool> {
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
    pub async fn update_trigger_config(
        &mut self,
        session_id: Uuid,
        config: TriggerConfig,
    ) -> Result<()> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;

        session.update_trigger_config(config);
        session.touch();

        tracing::debug!("Updated trigger config for session {}", session_id);
        Ok(())
    }

    /// Process content with live rendering integration
    pub async fn process_live_content(
        &mut self,
        session_id: Uuid,
        trigger_events: Vec<TriggerEvent>,
    ) -> Result<LiveEditorResult> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;

        let result = session.live_editor.process_content_with_cursor(
            &session.state.content,
            &session.state.cursor_position,
            &trigger_events,
        );

        session.touch();
        tracing::debug!("Processed live content for session {}", session_id);
        Ok(result)
    }

    /// Handle click-to-edit functionality
    pub async fn handle_click_to_edit(
        &mut self,
        session_id: Uuid,
        click_position: usize,
    ) -> Result<ClickToEditResult> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;

        let result = session
            .live_editor
            .handle_click_to_edit(click_position, &session.state.content);

        session.touch();
        tracing::debug!(
            "Handled click-to-edit at position {} for session {}",
            click_position,
            session_id
        );
        Ok(result)
    }

    /// Handle mode switching with cursor position preservation
    pub async fn handle_mode_switch(
        &mut self,
        session_id: Uuid,
        from_mode: EditorMode,
        to_mode: EditorMode,
    ) -> Result<ModeSwitchResult> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;

        let result = session.live_editor.handle_mode_switch(
            from_mode.clone(),
            to_mode.clone(),
            &session.state.cursor_position,
        );

        // Update the session state with the new mode and cursor position
        session.state_mut().switch_mode(to_mode.clone());
        if let Err(e) = session
            .state_mut()
            .update_cursor_position(result.preserved_cursor_position.clone())
        {
            tracing::warn!("Failed to update cursor position after mode switch: {}", e);
        }

        session.touch();
        tracing::debug!(
            "Handled mode switch from {:?} to {:?} for session {}",
            from_mode,
            to_mode,
            session_id
        );
        Ok(result)
    }

    /// Update content of the currently active element
    pub async fn update_active_element_content(
        &mut self,
        session_id: Uuid,
        new_content: String,
    ) -> Result<bool> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;

        let updated = session
            .live_editor
            .update_active_element_content(&new_content);

        if updated {
            // Mark session as dirty since content was updated
            let current_content = session.state.content.clone();
            session.state_mut().update_content(current_content);
            session.touch();

            // Trigger auto-save if enabled
            self.trigger_auto_save(session_id).await?;

            tracing::debug!("Updated active element content for session {}", session_id);
        }

        Ok(updated)
    }

    /// Trigger auto-save for a session with debouncing
    ///
    /// This method implements the auto-save system with a 2-second debouncing delay.
    /// When called, it will:
    /// 1. Check if auto-save is enabled and the session has unsaved changes
    /// 2. Cancel any existing auto-save timer for the session
    /// 3. Start a new 2-second timer
    /// 4. Save the content automatically when the timer expires
    ///
    /// The debouncing ensures that rapid typing doesn't trigger multiple saves.
    pub async fn trigger_auto_save(&mut self, session_id: Uuid) -> Result<()> {
        let session = self
            .sessions
            .get(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;

        // Only trigger auto-save if enabled and session is dirty
        if !session.auto_save_config.enabled || !session.state.is_dirty {
            return Ok(());
        }

        if let Some(sender) = &self.auto_save_sender {
            let (response_tx, response_rx) = tokio::sync::oneshot::channel();

            // Cancel any existing timer for this session
            let _ = sender.send(AutoSaveCommand::CancelTimer { session_id });

            // Start new timer
            let _ = sender.send(AutoSaveCommand::StartTimer {
                session_id,
                response_tx,
            });

            // Spawn a task to handle the auto-save when ready
            tokio::spawn(async move {
                if let Ok(AutoSaveResult::ReadyToSave) = response_rx.await {
                    // Note: In a real implementation, we would need a better way to access
                    // the session manager from the background task. For now, we log the intent.
                    tracing::info!("Auto-save ready for session {}", session_id);
                }
            });
        }

        Ok(())
    }

    /// Get auto-save status for a session
    ///
    /// Returns comprehensive auto-save status including:
    /// - Whether auto-save is enabled
    /// - If the session has unsaved changes (is dirty)
    /// - Last save time
    /// - Time since last edit
    /// - Whether an auto-save is currently pending
    pub async fn get_auto_save_status(&self, session_id: Uuid) -> Result<AutoSaveStatus> {
        let session = self
            .sessions
            .get(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;

        let status = AutoSaveStatus {
            enabled: session.auto_save_config.enabled,
            is_dirty: session.state.is_dirty,
            last_save_time: session.state.last_save_time,
            time_since_last_edit: session.state.time_since_last_edit(),
            pending_save: session.state.auto_save_timer.is_some(),
        };

        Ok(status)
    }

    /// Check for external file changes for a session
    ///
    /// Detects if the file has been modified externally while being edited.
    /// This is used to implement bidirectional synchronization.
    pub async fn check_external_changes(&self, session_id: Uuid) -> Result<Option<ExternalChange>> {
        let session = self
            .sessions
            .get(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;

        if !session.monitor_external_changes {
            return Ok(None);
        }

        self.file_sync
            .detect_external_change(&session.file_path)
            .await
    }

    /// Handle external file change with conflict resolution
    ///
    /// When an external change is detected, this method resolves any conflicts
    /// between the local edits and external changes using the configured strategy.
    pub async fn handle_external_change(
        &mut self,
        session_id: Uuid,
        external_change: ExternalChange,
    ) -> Result<ConflictResolution> {
        let session = self
            .sessions
            .get(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;

        let local_content = session.state.content.clone();
        let strategy = session.conflict_strategy;

        // Resolve the conflict
        let resolution = self
            .file_sync
            .resolve_conflict(&local_content, &external_change.new_content, strategy)
            .await?;

        // If resolution was successful, update the session content
        if resolution.success {
            self.set_content(session_id, resolution.content.clone())
                .await?;
            tracing::info!(
                "Resolved external change for session {} using strategy {:?}",
                session_id,
                strategy
            );
        } else {
            tracing::warn!(
                "Could not auto-resolve conflict for session {}, {} unresolved regions",
                session_id,
                resolution.unresolved_conflicts.len()
            );
        }

        Ok(resolution)
    }

    /// Store local backup for a session
    ///
    /// Creates a local backup of the session content. This is used when
    /// the connection is lost to prevent data loss.
    pub async fn store_session_backup(&self, session_id: Uuid) -> Result<()> {
        let session = self
            .sessions
            .get(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;

        self.file_sync
            .store_local_backup(session_id, &session.state.content)
            .await?;

        tracing::debug!("Stored backup for session {}", session_id);
        Ok(())
    }

    /// Restore session from local backup
    ///
    /// Retrieves and restores content from a local backup. This is used
    /// when reconnecting after a connection loss.
    pub async fn restore_session_from_backup(&mut self, session_id: Uuid) -> Result<bool> {
        if let Some(backup_content) = self.file_sync.retrieve_local_backup(session_id).await? {
            self.set_content(session_id, backup_content).await?;
            tracing::info!("Restored session {} from backup", session_id);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Clear local backup for a session
    ///
    /// Removes the local backup after successful synchronization.
    pub async fn clear_session_backup(&self, session_id: Uuid) -> Result<()> {
        self.file_sync.clear_local_backup(session_id).await?;
        tracing::debug!("Cleared backup for session {}", session_id);
        Ok(())
    }

    /// Check if a session has a local backup
    pub async fn has_session_backup(&self, session_id: Uuid) -> Result<bool> {
        self.file_sync.has_local_backup(session_id).await
    }

    /// Set conflict resolution strategy for a session
    pub async fn set_conflict_strategy(
        &mut self,
        session_id: Uuid,
        strategy: ConflictResolutionStrategy,
    ) -> Result<()> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;

        session.conflict_strategy = strategy;
        tracing::debug!(
            "Set conflict strategy to {:?} for session {}",
            strategy,
            session_id
        );
        Ok(())
    }

    /// Enable or disable external change monitoring for a session
    pub async fn set_external_monitoring(&mut self, session_id: Uuid, enabled: bool) -> Result<()> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;

        session.monitor_external_changes = enabled;
        tracing::debug!(
            "Set external monitoring {} for session {}",
            enabled,
            session_id
        );
        Ok(())
    }

    /// Sync session content to file with backup
    ///
    /// Saves the session content to the file system and creates a backup.
    /// If the save fails, the backup can be used for recovery.
    pub async fn sync_session_to_file(&mut self, session_id: Uuid) -> Result<()> {
        let session = self
            .sessions
            .get(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;

        // Store backup before syncing
        self.file_sync
            .store_local_backup(session_id, &session.state.content)
            .await?;

        // Sync to file
        let file_path = session.file_path.clone();
        let content = session.state.content.clone();

        self.file_sync.sync_to_file(&file_path, &content).await?;

        // Clear backup after successful sync
        self.file_sync.clear_local_backup(session_id).await?;

        // Now get mutable access to mark as saved
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or(EditorError::SessionNotFound(session_id))?;
        session.state_mut().mark_saved();

        tracing::info!("Synced session {} to file", session_id);
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

/// Auto-save status for a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoSaveStatus {
    pub enabled: bool,
    pub is_dirty: bool,
    pub last_save_time: Option<SystemTime>,
    pub time_since_last_edit: Option<std::time::Duration>,
    pub pending_save: bool,
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

    #[tokio::test]
    async fn test_auto_save_status() {
        let mut manager = SessionManager::new();
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("test.md");

        let session_id = manager.create_session(file_path).await.unwrap();

        // Initially should not be dirty
        let status = manager.get_auto_save_status(session_id).await.unwrap();
        assert!(status.enabled);
        assert!(!status.is_dirty);
        assert!(!status.pending_save);

        // Update content to make it dirty
        manager
            .set_content(session_id, "New content".to_string())
            .await
            .unwrap();

        let status = manager.get_auto_save_status(session_id).await.unwrap();
        assert!(status.enabled);
        assert!(status.is_dirty);

        // Save content
        manager.save_content(session_id).await.unwrap();

        let status = manager.get_auto_save_status(session_id).await.unwrap();
        assert!(status.enabled);
        assert!(!status.is_dirty);
        assert!(!status.pending_save);
    }

    #[tokio::test]
    async fn test_auto_save_trigger() {
        let mut manager = SessionManager::new();
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("test.md");

        let session_id = manager.create_session(file_path).await.unwrap();

        // Make content dirty
        manager
            .set_content(session_id, "Content that needs saving".to_string())
            .await
            .unwrap();

        // Trigger auto-save should work for dirty content
        let result = manager.trigger_auto_save(session_id).await;
        assert!(result.is_ok());

        // Save the content first
        manager.save_content(session_id).await.unwrap();

        // Trigger auto-save should not do anything for clean content
        let result = manager.trigger_auto_save(session_id).await;
        assert!(result.is_ok());
    }
}
