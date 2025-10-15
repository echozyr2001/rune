//! Live editor integration that connects syntax parsing with inline rendering

use crate::cursor_manager::CursorManager;
use crate::editor_state::{CursorPosition, EditorMode};
use crate::inline_renderer::{InlineRenderer, MarkdownInlineRenderer, RenderedElement};
use crate::render_trigger::TriggerEvent;
use crate::syntax_parser::{MarkdownSyntaxParser, SyntaxElement, SyntaxParser};

/// Live editor integration that manages the connection between syntax parsing and rendering
#[derive(Debug)]
pub struct LiveEditorIntegration {
    /// Syntax parser for detecting markdown elements
    syntax_parser: MarkdownSyntaxParser,
    /// Inline renderer for converting syntax to HTML
    inline_renderer: MarkdownInlineRenderer,
    /// Cursor manager for position mapping
    cursor_manager: CursorManager,
    /// Current syntax elements
    current_elements: Vec<SyntaxElement>,
    /// Current rendered elements
    current_rendered: Vec<RenderedElement>,
    /// Element currently being edited (if any)
    active_element_index: Option<usize>,
}

impl LiveEditorIntegration {
    /// Create a new live editor integration
    pub fn new() -> Self {
        Self {
            syntax_parser: MarkdownSyntaxParser::new(),
            inline_renderer: MarkdownInlineRenderer::new(),
            cursor_manager: CursorManager::new(),
            current_elements: Vec::new(),
            current_rendered: Vec::new(),
            active_element_index: None,
        }
    }

    /// Process content and cursor position to determine rendering state
    pub fn process_content_with_cursor(
        &mut self,
        content: &str,
        cursor_position: &CursorPosition,
        trigger_events: &[TriggerEvent],
    ) -> LiveEditorResult {
        // Parse syntax elements from content
        self.current_elements = self.syntax_parser.parse_document(content);

        // Determine which element (if any) should be in editing mode
        self.update_active_element(cursor_position, trigger_events);

        // Render elements with cursor awareness
        self.current_rendered = self
            .inline_renderer
            .render_elements_with_cursor(&self.current_elements, cursor_position);

        // Update cursor manager mappings
        self.cursor_manager.update_element_mappings(
            &self.current_elements,
            &self.current_rendered,
            content,
            &self.generate_rendered_content(),
        );

        // Generate the final rendered content
        let rendered_content = self.generate_mixed_content(content, cursor_position);

        LiveEditorResult {
            rendered_content,
            active_element_index: self.active_element_index,
            syntax_elements: self.current_elements.clone(),
            rendered_elements: self.current_rendered.clone(),
            cursor_mapping: self.cursor_manager.get_mapping_stats(),
        }
    }

    /// Handle click-to-edit functionality
    pub fn handle_click_to_edit(
        &mut self,
        click_position: usize,
        _content: &str,
    ) -> ClickToEditResult {
        // Find which element was clicked
        if let Some(element_index) = self.find_element_at_position(click_position) {
            // Set this element as active for editing
            self.active_element_index = Some(element_index);

            // Update the element to be in editing mode
            if let Some(element) = self.current_elements.get_mut(element_index) {
                element.set_active(true);
            }

            // Update rendered elements
            if let Some(rendered) = self.current_rendered.get_mut(element_index) {
                rendered.set_editing(true);
            }

            // Calculate cursor position within the element
            let element = &self.current_elements[element_index];
            let relative_position = click_position - element.range.start;
            let cursor_position = CursorPosition::new(0, relative_position, click_position);

            ClickToEditResult {
                success: true,
                element_index: Some(element_index),
                raw_content: element.raw_content.clone(),
                cursor_position: Some(cursor_position),
                element_range: (element.range.start, element.range.end),
            }
        } else {
            // Click was not on an element, clear active element
            self.clear_active_element();

            ClickToEditResult {
                success: false,
                element_index: None,
                raw_content: String::new(),
                cursor_position: None,
                element_range: (0, 0),
            }
        }
    }

    /// Handle mode switching between raw and live modes
    pub fn handle_mode_switch(
        &mut self,
        from_mode: EditorMode,
        to_mode: EditorMode,
        current_cursor: &CursorPosition,
    ) -> ModeSwitchResult {
        let preserved_position = match (from_mode.clone(), to_mode.clone()) {
            (EditorMode::Raw, EditorMode::Live) => {
                // Switching from raw to live - map raw position to rendered position
                self.cursor_manager
                    .preserve_position_for_mode_switch(true, false)
                    .unwrap_or_else(|_| current_cursor.clone())
            }
            (EditorMode::Live, EditorMode::Raw) => {
                // Switching from live to raw - map rendered position to raw position
                self.cursor_manager
                    .preserve_position_for_mode_switch(false, true)
                    .unwrap_or_else(|_| current_cursor.clone())
            }
            _ => current_cursor.clone(),
        };

        // Clear active element when switching modes
        if from_mode != to_mode {
            self.clear_active_element();
        }

        ModeSwitchResult {
            preserved_cursor_position: preserved_position,
            needs_rerender: from_mode != to_mode,
        }
    }

    /// Generate mixed content with both raw and rendered elements
    fn generate_mixed_content(&self, content: &str, cursor_position: &CursorPosition) -> String {
        if self.current_elements.is_empty() {
            return self.inline_renderer.render_document(
                content,
                &self.current_elements,
                cursor_position,
            );
        }

        let mut result = String::new();
        let mut last_pos = 0;

        for (i, element) in self.current_elements.iter().enumerate() {
            // Add content before this element
            if element.range.start > last_pos {
                let before_content = &content[last_pos..element.range.start];
                result.push_str(&html_escape(before_content));
            }

            // Render the element based on whether it's active
            if let Some(rendered) = self.current_rendered.get(i) {
                if rendered.is_editing || Some(i) == self.active_element_index {
                    // Show raw content for editing
                    result.push_str(&format!(
                        r#"<span class="editable-element" data-element-index="{}" contenteditable="true">{}</span>"#,
                        i,
                        html_escape(&element.raw_content)
                    ));
                } else {
                    // Show rendered content
                    result.push_str(&rendered.to_html());
                }
            } else {
                // Fallback to raw content
                result.push_str(&html_escape(&element.raw_content));
            }

            last_pos = element.range.end;
        }

        // Add any remaining content
        if last_pos < content.len() {
            let remaining = &content[last_pos..];
            result.push_str(&html_escape(remaining));
        }

        result
    }

    /// Generate fully rendered content for cursor mapping
    fn generate_rendered_content(&self) -> String {
        self.current_rendered
            .iter()
            .map(|r| r.html.clone())
            .collect::<Vec<_>>()
            .join("")
    }

    /// Update which element should be active based on cursor position and triggers
    fn update_active_element(
        &mut self,
        cursor_position: &CursorPosition,
        trigger_events: &[TriggerEvent],
    ) {
        // Check if cursor is within an existing element
        if let Some(element_index) = self.find_element_at_cursor_position(cursor_position) {
            // Check if we should activate this element based on trigger events
            let should_activate = trigger_events.iter().any(|event| match event {
                TriggerEvent::SpaceKey => true,
                TriggerEvent::CursorMovement { .. } => false, // Don't activate on cursor movement alone
                TriggerEvent::BlockElementCompleted { .. } => true,
                TriggerEvent::ContentChange { .. } => false,
            });

            if should_activate {
                self.set_active_element(element_index);
            }
        } else {
            // Cursor is not in any element, check if we should deactivate current element
            let should_deactivate = trigger_events.iter().any(|event| match event {
                TriggerEvent::SpaceKey => true,
                TriggerEvent::CursorMovement { .. } => true,
                TriggerEvent::BlockElementCompleted { .. } => true,
                TriggerEvent::ContentChange { .. } => false,
            });

            if should_deactivate {
                self.clear_active_element();
            }
        }
    }

    /// Find element at a specific position
    fn find_element_at_position(&self, position: usize) -> Option<usize> {
        self.current_elements
            .iter()
            .position(|element| element.range.contains(position))
    }

    /// Find element at cursor position
    fn find_element_at_cursor_position(&self, cursor_position: &CursorPosition) -> Option<usize> {
        self.find_element_at_position(cursor_position.absolute)
    }

    /// Set an element as active for editing
    fn set_active_element(&mut self, element_index: usize) {
        // Clear previous active element
        self.clear_active_element();

        // Set new active element
        self.active_element_index = Some(element_index);

        if let Some(element) = self.current_elements.get_mut(element_index) {
            element.set_active(true);
        }

        if let Some(rendered) = self.current_rendered.get_mut(element_index) {
            rendered.set_editing(true);
        }
    }

    /// Clear the active element
    fn clear_active_element(&mut self) {
        if let Some(index) = self.active_element_index {
            if let Some(element) = self.current_elements.get_mut(index) {
                element.set_active(false);
            }

            if let Some(rendered) = self.current_rendered.get_mut(index) {
                rendered.set_editing(false);
            }
        }

        self.active_element_index = None;
    }

    /// Get the currently active element
    pub fn get_active_element(&self) -> Option<&SyntaxElement> {
        self.active_element_index
            .and_then(|index| self.current_elements.get(index))
    }

    /// Get the currently active rendered element
    pub fn get_active_rendered_element(&self) -> Option<&RenderedElement> {
        self.active_element_index
            .and_then(|index| self.current_rendered.get(index))
    }

    /// Update content of the active element
    pub fn update_active_element_content(&mut self, new_content: &str) -> bool {
        if let Some(index) = self.active_element_index {
            if let Some(element) = self.current_elements.get_mut(index) {
                element.raw_content = new_content.to_string();
                // Re-parse the element content
                // This is a simplified approach - in practice, you might want more sophisticated parsing
                return true;
            }
        }
        false
    }

    /// Get cursor manager for external access
    pub fn cursor_manager(&self) -> &CursorManager {
        &self.cursor_manager
    }

    /// Get cursor manager for external mutation
    pub fn cursor_manager_mut(&mut self) -> &mut CursorManager {
        &mut self.cursor_manager
    }
}

impl Default for LiveEditorIntegration {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of live editor processing
#[derive(Debug, Clone)]
pub struct LiveEditorResult {
    /// The final rendered content with mixed raw/rendered elements
    pub rendered_content: String,
    /// Index of the currently active element (if any)
    pub active_element_index: Option<usize>,
    /// All parsed syntax elements
    pub syntax_elements: Vec<SyntaxElement>,
    /// All rendered elements
    pub rendered_elements: Vec<RenderedElement>,
    /// Cursor mapping statistics
    pub cursor_mapping: crate::cursor_manager::MappingStats,
}

/// Result of click-to-edit operation
#[derive(Debug, Clone)]
pub struct ClickToEditResult {
    /// Whether the click-to-edit was successful
    pub success: bool,
    /// Index of the element that was clicked (if any)
    pub element_index: Option<usize>,
    /// Raw content of the clicked element
    pub raw_content: String,
    /// Suggested cursor position within the element
    pub cursor_position: Option<CursorPosition>,
    /// Range of the element in the original content
    pub element_range: (usize, usize),
}

/// Result of mode switching operation
#[derive(Debug, Clone)]
pub struct ModeSwitchResult {
    /// Preserved cursor position after mode switch
    pub preserved_cursor_position: CursorPosition,
    /// Whether a re-render is needed
    pub needs_rerender: bool,
}

/// HTML escape utility function
fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax_parser::PositionRange;

    #[test]
    fn test_live_editor_integration_creation() {
        let integration = LiveEditorIntegration::new();
        assert!(integration.current_elements.is_empty());
        assert!(integration.current_rendered.is_empty());
        assert_eq!(integration.active_element_index, None);
    }

    #[test]
    fn test_content_processing() {
        let mut integration = LiveEditorIntegration::new();
        let content = "# Header\n\nThis is **bold** text.";
        let cursor_position = CursorPosition::new(0, 0, 0);
        let trigger_events = vec![TriggerEvent::SpaceKey];

        let result =
            integration.process_content_with_cursor(content, &cursor_position, &trigger_events);

        assert!(!result.syntax_elements.is_empty());
        assert!(!result.rendered_elements.is_empty());
        assert!(!result.rendered_content.is_empty());
    }

    #[test]
    fn test_click_to_edit() {
        let mut integration = LiveEditorIntegration::new();
        let content = "This is **bold** text.";
        let cursor_position = CursorPosition::new(0, 0, 0);

        // First process the content to parse elements
        integration.process_content_with_cursor(content, &cursor_position, &[]);

        // Click on the bold element (assuming it's around position 10)
        let click_result = integration.handle_click_to_edit(10, content);

        if click_result.success {
            assert!(click_result.element_index.is_some());
            assert!(!click_result.raw_content.is_empty());
        }
    }

    #[test]
    fn test_mode_switching() {
        let mut integration = LiveEditorIntegration::new();
        let cursor_position = CursorPosition::new(0, 5, 5);

        let result =
            integration.handle_mode_switch(EditorMode::Raw, EditorMode::Live, &cursor_position);

        assert!(result.needs_rerender);
        // Cursor position should be preserved or mapped appropriately
    }

    #[test]
    fn test_active_element_management() {
        let mut integration = LiveEditorIntegration::new();

        // Simulate having parsed elements
        integration.current_elements = vec![crate::syntax_parser::SyntaxElement::new(
            crate::syntax_parser::SyntaxElementType::Bold,
            PositionRange::new(0, 8),
            "**bold**".to_string(),
            "bold".to_string(),
        )];

        // Set element as active
        integration.set_active_element(0);
        assert_eq!(integration.active_element_index, Some(0));
        assert!(integration.get_active_element().is_some());

        // Clear active element
        integration.clear_active_element();
        assert_eq!(integration.active_element_index, None);
        assert!(integration.get_active_element().is_none());
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("&<>\"'"), "&amp;&lt;&gt;&quot;&#x27;");
        assert_eq!(html_escape("normal text"), "normal text");
    }
}
