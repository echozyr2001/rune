//! Render trigger detection system for live markdown editing

use crate::editor_state::CursorPosition;
use crate::syntax_parser::{SyntaxElement, SyntaxElementType};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Types of events that can trigger rendering
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerEvent {
    /// Space key was pressed
    SpaceKey,
    /// Cursor moved to a different position
    CursorMovement {
        from: CursorPosition,
        to: CursorPosition,
    },
    /// Block element was completed (header, list item, etc.)
    BlockElementCompleted {
        element_type: SyntaxElementType,
        position: usize,
    },
    /// Content changed significantly
    ContentChange {
        change_start: usize,
        change_end: usize,
    },
}

/// Configuration for render trigger detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerConfig {
    /// Debounce delay in milliseconds
    pub debounce_delay_ms: u64,
    /// Whether to trigger on space key
    pub trigger_on_space: bool,
    /// Whether to trigger on cursor movement
    pub trigger_on_cursor_movement: bool,
    /// Whether to trigger on block element completion
    pub trigger_on_block_completion: bool,
    /// Minimum cursor movement distance to trigger
    pub min_cursor_movement_distance: usize,
}

impl Default for TriggerConfig {
    fn default() -> Self {
        Self {
            debounce_delay_ms: 150, // 150ms debounce as per requirements
            trigger_on_space: true,
            trigger_on_cursor_movement: true,
            trigger_on_block_completion: true,
            min_cursor_movement_distance: 1,
        }
    }
}

/// State for debounced rendering
#[derive(Debug, Default)]
struct DebounceState {
    last_trigger_time: Option<Instant>,
    pending_events: Vec<TriggerEvent>,
    is_render_scheduled: bool,
}

/// Render trigger detection system
#[derive(Debug)]
pub struct RenderTriggerDetector {
    config: TriggerConfig,
    debounce_state: DebounceState,
    last_cursor_position: Option<CursorPosition>,
    last_content: String,
}

impl RenderTriggerDetector {
    /// Create a new render trigger detector
    pub fn new(config: TriggerConfig) -> Self {
        Self {
            config,
            debounce_state: DebounceState::default(),
            last_cursor_position: None,
            last_content: String::new(),
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(TriggerConfig::default())
    }

    /// Update configuration
    pub fn update_config(&mut self, config: TriggerConfig) {
        self.config = config;
    }

    /// Detect space key trigger
    pub fn detect_space_key(&mut self, _cursor_position: CursorPosition) -> bool {
        if !self.config.trigger_on_space {
            return false;
        }

        let event = TriggerEvent::SpaceKey;
        self.add_trigger_event(event)
    }

    /// Detect cursor movement trigger
    pub fn detect_cursor_movement(&mut self, new_position: CursorPosition) -> bool {
        if !self.config.trigger_on_cursor_movement {
            return false;
        }

        if let Some(last_pos) = &self.last_cursor_position {
            let distance = self.calculate_cursor_distance(last_pos, &new_position);

            if distance >= self.config.min_cursor_movement_distance {
                let event = TriggerEvent::CursorMovement {
                    from: last_pos.clone(),
                    to: new_position.clone(),
                };

                self.last_cursor_position = Some(new_position);
                return self.add_trigger_event(event);
            }
        } else {
            self.last_cursor_position = Some(new_position);
        }

        false
    }

    /// Detect block element completion
    pub fn detect_block_completion(
        &mut self,
        content: &str,
        cursor_position: CursorPosition,
        syntax_elements: &[SyntaxElement],
    ) -> bool {
        if !self.config.trigger_on_block_completion {
            return false;
        }

        // Check if we just completed a block element
        if let Some(completed_element) =
            self.find_completed_block_element(content, cursor_position.absolute, syntax_elements)
        {
            let event = TriggerEvent::BlockElementCompleted {
                element_type: completed_element.element_type.clone(),
                position: cursor_position.absolute,
            };

            return self.add_trigger_event(event);
        }

        false
    }

    /// Detect content changes that should trigger rendering
    pub fn detect_content_change(
        &mut self,
        new_content: &str,
        change_start: usize,
        change_end: usize,
    ) -> bool {
        let event = TriggerEvent::ContentChange {
            change_start,
            change_end,
        };

        self.last_content = new_content.to_string();
        self.add_trigger_event(event)
    }

    /// Check if rendering should be triggered (debounced)
    pub fn should_trigger_render(&mut self) -> bool {
        let now = Instant::now();

        // If no events are pending, no need to render
        if self.debounce_state.pending_events.is_empty() {
            return false;
        }

        // Check if enough time has passed since last trigger
        if let Some(last_trigger) = self.debounce_state.last_trigger_time {
            let elapsed = now.duration_since(last_trigger);
            let debounce_duration = Duration::from_millis(self.config.debounce_delay_ms);

            if elapsed >= debounce_duration {
                // Clear pending events and trigger render
                self.debounce_state.pending_events.clear();
                self.debounce_state.is_render_scheduled = false;
                return true;
            }
        }

        false
    }

    /// Get pending trigger events
    pub fn get_pending_events(&self) -> &[TriggerEvent] {
        &self.debounce_state.pending_events
    }

    /// Clear all pending events
    pub fn clear_pending_events(&mut self) {
        self.debounce_state.pending_events.clear();
        self.debounce_state.is_render_scheduled = false;
    }

    /// Force immediate render trigger
    pub fn force_trigger(&mut self) -> bool {
        if !self.debounce_state.pending_events.is_empty() {
            self.debounce_state.pending_events.clear();
            self.debounce_state.is_render_scheduled = false;
            return true;
        }
        false
    }

    /// Add a trigger event to the debounce queue
    fn add_trigger_event(&mut self, event: TriggerEvent) -> bool {
        let now = Instant::now();

        self.debounce_state.pending_events.push(event);
        self.debounce_state.last_trigger_time = Some(now);

        if !self.debounce_state.is_render_scheduled {
            self.debounce_state.is_render_scheduled = true;
            return true;
        }

        false
    }

    /// Calculate distance between cursor positions
    fn calculate_cursor_distance(&self, from: &CursorPosition, to: &CursorPosition) -> usize {
        // Use absolute position difference as distance metric
        from.absolute.abs_diff(to.absolute)
    }

    /// Find a completed block element at the cursor position
    fn find_completed_block_element<'a>(
        &self,
        content: &str,
        cursor_position: usize,
        syntax_elements: &'a [SyntaxElement],
    ) -> Option<&'a SyntaxElement> {
        // Look for block elements that end near the cursor position
        for element in syntax_elements {
            match &element.element_type {
                SyntaxElementType::Header { .. }
                | SyntaxElementType::UnorderedListItem { .. }
                | SyntaxElementType::OrderedListItem { .. } => {
                    // Check if cursor is at the end of this block element
                    if self.is_cursor_at_block_end(content, cursor_position, element) {
                        return Some(element);
                    }
                }
                _ => continue,
            }
        }

        None
    }

    /// Check if cursor is at the end of a block element
    fn is_cursor_at_block_end(
        &self,
        content: &str,
        cursor_position: usize,
        element: &SyntaxElement,
    ) -> bool {
        // For block elements, check if we're at the end of the line
        let chars: Vec<char> = content.chars().collect();

        // Check if cursor is within or just after the element
        if cursor_position < element.range.start || cursor_position > element.range.end + 1 {
            return false;
        }

        // Check if we're at a line boundary (newline or end of document)
        if cursor_position >= chars.len() {
            return true; // End of document
        }

        if cursor_position > 0 && chars[cursor_position - 1] == '\n' {
            return true; // Just after a newline
        }

        if cursor_position < chars.len() && chars[cursor_position] == '\n' {
            return true; // Just before a newline
        }

        false
    }
}

/// Trait for components that can respond to render triggers
pub trait RenderTriggerHandler {
    /// Handle a render trigger event
    fn handle_trigger(&mut self, events: &[TriggerEvent]) -> Result<(), String>;

    /// Check if the handler is ready to process triggers
    fn is_ready(&self) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax_parser::{PositionRange, SyntaxElement};

    #[test]
    fn test_space_key_detection() {
        let mut detector = RenderTriggerDetector::with_defaults();
        let cursor_pos = CursorPosition::new(0, 5, 5);

        assert!(detector.detect_space_key(cursor_pos));
        assert_eq!(detector.get_pending_events().len(), 1);

        if let TriggerEvent::SpaceKey = &detector.get_pending_events()[0] {
            // Expected
        } else {
            panic!("Expected SpaceKey event");
        }
    }

    #[test]
    fn test_cursor_movement_detection() {
        let mut detector = RenderTriggerDetector::with_defaults();

        let pos1 = CursorPosition::new(0, 0, 0);
        let pos2 = CursorPosition::new(0, 5, 5);

        // First position - no trigger
        assert!(!detector.detect_cursor_movement(pos1.clone()));

        // Second position - should trigger
        assert!(detector.detect_cursor_movement(pos2.clone()));
        assert_eq!(detector.get_pending_events().len(), 1);

        if let TriggerEvent::CursorMovement { from, to } = &detector.get_pending_events()[0] {
            assert_eq!(from, &pos1);
            assert_eq!(to, &pos2);
        } else {
            panic!("Expected CursorMovement event");
        }
    }

    #[test]
    fn test_block_completion_detection() {
        let mut detector = RenderTriggerDetector::with_defaults();
        let content = "# Header\n";
        let cursor_pos = CursorPosition::new(0, 8, 8);

        let header_element = SyntaxElement::new(
            SyntaxElementType::Header { level: 1 },
            PositionRange::new(0, 8),
            "# Header".to_string(),
            "Header".to_string(),
        );

        let elements = vec![header_element];

        assert!(detector.detect_block_completion(content, cursor_pos, &elements));
        assert_eq!(detector.get_pending_events().len(), 1);
    }

    #[test]
    fn test_debounce_timing() {
        let config = TriggerConfig {
            debounce_delay_ms: 50, // Short delay for testing
            ..Default::default()
        };

        let mut detector = RenderTriggerDetector::new(config);
        let cursor_pos = CursorPosition::new(0, 0, 0);

        // Add trigger event
        detector.detect_space_key(cursor_pos);

        // Should not trigger immediately
        assert!(!detector.should_trigger_render());

        // Wait for debounce period
        std::thread::sleep(Duration::from_millis(60));

        // Should trigger now
        assert!(detector.should_trigger_render());

        // Should not trigger again
        assert!(!detector.should_trigger_render());
    }

    #[test]
    fn test_cursor_distance_calculation() {
        let detector = RenderTriggerDetector::with_defaults();

        let pos1 = CursorPosition::new(0, 0, 0);
        let pos2 = CursorPosition::new(0, 5, 5);
        let pos3 = CursorPosition::new(1, 0, 10);

        assert_eq!(detector.calculate_cursor_distance(&pos1, &pos2), 5);
        assert_eq!(detector.calculate_cursor_distance(&pos2, &pos1), 5);
        assert_eq!(detector.calculate_cursor_distance(&pos1, &pos3), 10);
    }

    #[test]
    fn test_config_updates() {
        let mut detector = RenderTriggerDetector::with_defaults();

        let new_config = TriggerConfig {
            trigger_on_space: false,
            debounce_delay_ms: 300,
            ..Default::default()
        };

        detector.update_config(new_config);

        let cursor_pos = CursorPosition::new(0, 0, 0);

        // Should not trigger on space since it's disabled
        assert!(!detector.detect_space_key(cursor_pos));
        assert_eq!(detector.get_pending_events().len(), 0);
    }

    #[test]
    fn test_force_trigger() {
        let mut detector = RenderTriggerDetector::with_defaults();
        let cursor_pos = CursorPosition::new(0, 0, 0);

        // Add some events
        detector.detect_space_key(cursor_pos);
        assert_eq!(detector.get_pending_events().len(), 1);

        // Force trigger should clear events and return true
        assert!(detector.force_trigger());
        assert_eq!(detector.get_pending_events().len(), 0);

        // Force trigger with no events should return false
        assert!(!detector.force_trigger());
    }
}
