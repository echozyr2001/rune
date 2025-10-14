//! Editor state management with cursor tracking and dirty state

use serde::{Deserialize, Serialize};
use std::time::SystemTime;
use uuid::Uuid;

/// Editor modes for different editing experiences
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EditorMode {
    /// Raw text editing mode - plain markdown text
    #[default]
    Raw,
    /// Live WYSIWYG mode - inline rendering with editing
    Live,
    /// Preview-only mode - read-only rendered view
    Preview,
}

impl std::fmt::Display for EditorMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditorMode::Raw => write!(f, "raw"),
            EditorMode::Live => write!(f, "live"),
            EditorMode::Preview => write!(f, "preview"),
        }
    }
}

/// Cursor position in the editor with multiple coordinate systems
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorPosition {
    /// Line number (0-based)
    pub line: usize,
    /// Column number (0-based)
    pub column: usize,
    /// Absolute character position from start of document
    pub absolute: usize,
}

impl CursorPosition {
    /// Create a new cursor position
    pub fn new(line: usize, column: usize, absolute: usize) -> Self {
        Self {
            line,
            column,
            absolute,
        }
    }

    /// Create cursor position at document start
    pub fn start() -> Self {
        Self::new(0, 0, 0)
    }

    /// Check if this position is valid for the given content
    pub fn is_valid_for_content(&self, content: &str) -> bool {
        let lines: Vec<&str> = content.lines().collect();

        // Check line bounds
        if self.line >= lines.len() {
            return false;
        }

        // Check column bounds for the specific line
        if let Some(line_content) = lines.get(self.line) {
            if self.column > line_content.len() {
                return false;
            }
        }

        // Check absolute position bounds
        self.absolute <= content.len()
    }

    /// Calculate absolute position from line and column
    pub fn calculate_absolute(content: &str, line: usize, column: usize) -> Option<usize> {
        let lines: Vec<&str> = content.lines().collect();

        if line >= lines.len() {
            return None;
        }

        let mut absolute = 0;

        // Add lengths of all previous lines (including newlines)
        for i in 0..line {
            if let Some(line_content) = lines.get(i) {
                absolute += line_content.len() + 1; // +1 for newline
            }
        }

        // Add column offset in current line
        if let Some(current_line) = lines.get(line) {
            if column <= current_line.len() {
                absolute += column;
                Some(absolute)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Calculate line and column from absolute position
    pub fn calculate_line_column(content: &str, absolute: usize) -> Option<(usize, usize)> {
        if absolute > content.len() {
            return None;
        }

        let mut current_pos = 0;
        let lines: Vec<&str> = content.lines().collect();

        for (line_idx, line_content) in lines.iter().enumerate() {
            let line_end = current_pos + line_content.len();

            if absolute <= line_end {
                let column = absolute - current_pos;
                return Some((line_idx, column));
            }

            current_pos = line_end + 1; // +1 for newline
        }

        // Handle position at very end of document
        if absolute == content.len() {
            if let Some(last_line_idx) = lines.len().checked_sub(1) {
                if let Some(last_line) = lines.get(last_line_idx) {
                    return Some((last_line_idx, last_line.len()));
                }
            }
        }

        None
    }

    /// Update absolute position based on line and column
    pub fn update_absolute(&mut self, content: &str) {
        if let Some(absolute) = Self::calculate_absolute(content, self.line, self.column) {
            self.absolute = absolute;
        }
    }

    /// Update line and column based on absolute position
    pub fn update_line_column(&mut self, content: &str) {
        if let Some((line, column)) = Self::calculate_line_column(content, self.absolute) {
            self.line = line;
            self.column = column;
        }
    }
}

impl Default for CursorPosition {
    fn default() -> Self {
        Self::start()
    }
}

/// Complete editor state for a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorState {
    /// Current editing mode
    pub current_mode: EditorMode,
    /// Current content being edited
    pub content: String,
    /// Current cursor position
    pub cursor_position: CursorPosition,
    /// Whether the content has unsaved changes
    pub is_dirty: bool,
    /// Whether auto-save is enabled
    pub auto_save_enabled: bool,
    /// Last time content was saved
    pub last_save_time: Option<SystemTime>,
    /// Last time content was modified
    pub last_edit_time: SystemTime,
    /// Session ID this state belongs to
    pub session_id: Uuid,
    /// Original content hash for change detection
    pub original_content_hash: String,
    /// Auto-save timer state
    pub auto_save_timer: Option<SystemTime>,
}

impl EditorState {
    /// Create a new editor state for a session
    pub fn new(session_id: Uuid, content: String) -> Self {
        let content_hash = Self::calculate_content_hash(&content);

        Self {
            current_mode: EditorMode::default(),
            cursor_position: CursorPosition::start(),
            is_dirty: false,
            auto_save_enabled: true,
            last_save_time: Some(SystemTime::now()),
            last_edit_time: SystemTime::now(),
            session_id,
            original_content_hash: content_hash,
            auto_save_timer: None,
            content,
        }
    }

    /// Update content and mark as dirty
    pub fn update_content(&mut self, new_content: String) {
        let content_hash = Self::calculate_content_hash(&new_content);
        self.is_dirty = content_hash != self.original_content_hash;
        self.content = new_content;
        self.last_edit_time = SystemTime::now();

        // Reset auto-save timer
        if self.auto_save_enabled {
            self.auto_save_timer = Some(SystemTime::now());
        }

        // Update cursor position to ensure it's still valid
        self.cursor_position.update_absolute(&self.content);
    }

    /// Update cursor position with validation
    pub fn update_cursor_position(&mut self, position: CursorPosition) -> Result<(), String> {
        if !position.is_valid_for_content(&self.content) {
            return Err(format!(
                "Invalid cursor position: line {}, column {} for content length {}",
                position.line,
                position.column,
                self.content.len()
            ));
        }

        self.cursor_position = position;
        Ok(())
    }

    /// Switch editor mode
    pub fn switch_mode(&mut self, new_mode: EditorMode) {
        if self.current_mode != new_mode {
            self.current_mode = new_mode;
            // Cursor position preservation is handled by the editor plugin
        }
    }

    /// Mark content as saved
    pub fn mark_saved(&mut self) {
        self.is_dirty = false;
        self.last_save_time = Some(SystemTime::now());
        self.original_content_hash = Self::calculate_content_hash(&self.content);
        self.auto_save_timer = None;
    }

    /// Check if auto-save should trigger
    pub fn should_auto_save(&self) -> bool {
        if !self.auto_save_enabled || !self.is_dirty {
            return false;
        }

        if let Some(timer_start) = self.auto_save_timer {
            if let Ok(elapsed) = timer_start.elapsed() {
                return elapsed.as_secs() >= 2; // 2-second delay as per requirements
            }
        }

        false
    }

    /// Enable or disable auto-save
    pub fn set_auto_save(&mut self, enabled: bool) {
        self.auto_save_enabled = enabled;
        if !enabled {
            self.auto_save_timer = None;
        } else if self.is_dirty {
            self.auto_save_timer = Some(SystemTime::now());
        }
    }

    /// Get time since last edit
    pub fn time_since_last_edit(&self) -> Option<std::time::Duration> {
        self.last_edit_time.elapsed().ok()
    }

    /// Get time since last save
    pub fn time_since_last_save(&self) -> Option<std::time::Duration> {
        self.last_save_time?.elapsed().ok()
    }

    /// Calculate a simple hash of content for change detection
    fn calculate_content_hash(content: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    /// Get content statistics
    pub fn get_content_stats(&self) -> ContentStats {
        let lines = self.content.lines().count();
        let characters = self.content.len();
        let words = self.content.split_whitespace().count();

        ContentStats {
            lines,
            characters,
            words,
        }
    }
}

/// Content statistics for the editor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentStats {
    pub lines: usize,
    pub characters: usize,
    pub words: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_position_validation() {
        let content = "line 1\nline 2\nline 3";

        // Valid positions
        assert!(CursorPosition::new(0, 0, 0).is_valid_for_content(content));
        assert!(CursorPosition::new(1, 6, 13).is_valid_for_content(content));
        assert!(CursorPosition::new(2, 6, 20).is_valid_for_content(content));

        // Invalid positions
        assert!(!CursorPosition::new(3, 0, 0).is_valid_for_content(content)); // Line out of bounds
        assert!(!CursorPosition::new(0, 10, 0).is_valid_for_content(content)); // Column out of bounds
        assert!(!CursorPosition::new(0, 0, 100).is_valid_for_content(content)); // Absolute out of bounds
    }

    #[test]
    fn test_absolute_position_calculation() {
        let content = "line 1\nline 2\nline 3";

        assert_eq!(CursorPosition::calculate_absolute(content, 0, 0), Some(0));
        assert_eq!(CursorPosition::calculate_absolute(content, 0, 6), Some(6));
        assert_eq!(CursorPosition::calculate_absolute(content, 1, 0), Some(7));
        assert_eq!(CursorPosition::calculate_absolute(content, 1, 6), Some(13));
        assert_eq!(CursorPosition::calculate_absolute(content, 2, 0), Some(14));
        assert_eq!(CursorPosition::calculate_absolute(content, 2, 6), Some(20));
    }

    #[test]
    fn test_line_column_calculation() {
        let content = "line 1\nline 2\nline 3";

        assert_eq!(
            CursorPosition::calculate_line_column(content, 0),
            Some((0, 0))
        );
        assert_eq!(
            CursorPosition::calculate_line_column(content, 6),
            Some((0, 6))
        );
        assert_eq!(
            CursorPosition::calculate_line_column(content, 7),
            Some((1, 0))
        );
        assert_eq!(
            CursorPosition::calculate_line_column(content, 13),
            Some((1, 6))
        );
        assert_eq!(
            CursorPosition::calculate_line_column(content, 14),
            Some((2, 0))
        );
        assert_eq!(
            CursorPosition::calculate_line_column(content, 20),
            Some((2, 6))
        );
    }

    #[test]
    fn test_editor_state_dirty_tracking() {
        let session_id = Uuid::new_v4();
        let original_content = "original content".to_string();
        let mut state = EditorState::new(session_id, original_content.clone());

        // Initially not dirty
        assert!(!state.is_dirty);

        // Update with same content - should not be dirty
        state.update_content(original_content.clone());
        assert!(!state.is_dirty);

        // Update with different content - should be dirty
        state.update_content("modified content".to_string());
        assert!(state.is_dirty);

        // Mark as saved - should not be dirty
        state.mark_saved();
        assert!(!state.is_dirty);
    }

    #[test]
    fn test_auto_save_timing() {
        let session_id = Uuid::new_v4();
        let mut state = EditorState::new(session_id, "content".to_string());

        // Initially should not auto-save (not dirty)
        assert!(!state.should_auto_save());

        // Make dirty but no timer set
        state.update_content("new content".to_string());
        assert!(state.is_dirty);

        // Should not auto-save immediately
        assert!(!state.should_auto_save());

        // Simulate timer expiry by setting timer to past time
        state.auto_save_timer = Some(SystemTime::now() - std::time::Duration::from_secs(3));
        assert!(state.should_auto_save());
    }
}
