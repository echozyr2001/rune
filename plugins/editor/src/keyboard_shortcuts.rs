//! Keyboard shortcut handling for markdown formatting

use crate::editor_state::CursorPosition;
use serde::{Deserialize, Serialize};

/// Keyboard shortcut actions for markdown formatting
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShortcutAction {
    /// Bold text formatting (Ctrl+B / Cmd+B)
    Bold,
    /// Italic text formatting (Ctrl+I / Cmd+I)
    Italic,
    /// Indent list item (Tab)
    IndentList,
    /// Unindent list item (Shift+Tab)
    UnindentList,
    /// Continue list on Enter key
    ContinueList,
}

/// Result of applying a keyboard shortcut
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutResult {
    /// The modified content after applying the shortcut
    pub content: String,
    /// The new cursor position after the modification
    pub cursor_position: CursorPosition,
    /// Whether the shortcut was successfully applied
    pub success: bool,
    /// Optional message about the operation
    pub message: Option<String>,
}

/// Text selection range for keyboard shortcuts
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextSelection {
    /// Start position of the selection (absolute position)
    pub start: usize,
    /// End position of the selection (absolute position)
    pub end: usize,
}

/// Information about a parsed list item
#[derive(Debug, Clone, PartialEq, Eq)]
struct ListItemInfo {
    /// Indentation string (spaces or tabs)
    indentation: String,
    /// List marker (e.g., "- ", "* ", "1. ")
    marker: String,
    /// Whether this is an ordered list
    is_ordered: bool,
    /// Number for ordered lists
    number: usize,
    /// Content after the marker
    content: String,
}

impl TextSelection {
    /// Create a new text selection
    pub fn new(start: usize, end: usize) -> Self {
        // Ensure start is always before end
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        Self { start, end }
    }

    /// Check if the selection is empty (cursor position)
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Get the length of the selection
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// Extract the selected text from content
    pub fn extract_text<'a>(&self, content: &'a str) -> &'a str {
        if self.start >= content.len() {
            return "";
        }
        let end = self.end.min(content.len());
        &content[self.start..end]
    }
}

/// Keyboard shortcut handler for markdown formatting
pub struct KeyboardShortcutHandler;

impl KeyboardShortcutHandler {
    /// Create a new keyboard shortcut handler
    pub fn new() -> Self {
        Self
    }

    /// Apply a keyboard shortcut action to the content
    pub fn apply_shortcut(
        &self,
        action: ShortcutAction,
        content: &str,
        selection: TextSelection,
        cursor_position: CursorPosition,
    ) -> ShortcutResult {
        match action {
            ShortcutAction::Bold => self.apply_bold(content, selection, cursor_position),
            ShortcutAction::Italic => self.apply_italic(content, selection, cursor_position),
            ShortcutAction::IndentList => self.apply_indent_list(content, cursor_position),
            ShortcutAction::UnindentList => self.apply_unindent_list(content, cursor_position),
            ShortcutAction::ContinueList => self.apply_continue_list(content, cursor_position),
        }
    }

    /// Apply bold formatting (wrap with **)
    fn apply_bold(
        &self,
        content: &str,
        selection: TextSelection,
        cursor_position: CursorPosition,
    ) -> ShortcutResult {
        if selection.is_empty() {
            // No selection - insert bold markers at cursor
            let (before, after) = content.split_at(cursor_position.absolute);
            let new_content = format!("{}****{}", before, after);
            let new_absolute = cursor_position.absolute + 2; // Move cursor between **|**

            let new_cursor = self.calculate_cursor_position(&new_content, new_absolute);

            ShortcutResult {
                content: new_content,
                cursor_position: new_cursor,
                success: true,
                message: Some("Inserted bold markers".to_string()),
            }
        } else {
            // Wrap selected text with **
            let selected_text = selection.extract_text(content);
            let before = &content[..selection.start];
            let after = &content[selection.end..];

            let new_content = format!("{}**{}**{}", before, selected_text, after);
            let new_absolute = selection.start + 2 + selected_text.len() + 2; // After closing **

            let new_cursor = self.calculate_cursor_position(&new_content, new_absolute);

            ShortcutResult {
                content: new_content,
                cursor_position: new_cursor,
                success: true,
                message: Some("Applied bold formatting".to_string()),
            }
        }
    }

    /// Apply italic formatting (wrap with *)
    fn apply_italic(
        &self,
        content: &str,
        selection: TextSelection,
        cursor_position: CursorPosition,
    ) -> ShortcutResult {
        if selection.is_empty() {
            // No selection - insert italic markers at cursor
            let (before, after) = content.split_at(cursor_position.absolute);
            let new_content = format!("{}**{}", before, after);
            let new_absolute = cursor_position.absolute + 1; // Move cursor between *|*

            let new_cursor = self.calculate_cursor_position(&new_content, new_absolute);

            ShortcutResult {
                content: new_content,
                cursor_position: new_cursor,
                success: true,
                message: Some("Inserted italic markers".to_string()),
            }
        } else {
            // Wrap selected text with *
            let selected_text = selection.extract_text(content);
            let before = &content[..selection.start];
            let after = &content[selection.end..];

            let new_content = format!("{}*{}*{}", before, selected_text, after);
            let new_absolute = selection.start + 1 + selected_text.len() + 1; // After closing *

            let new_cursor = self.calculate_cursor_position(&new_content, new_absolute);

            ShortcutResult {
                content: new_content,
                cursor_position: new_cursor,
                success: true,
                message: Some("Applied italic formatting".to_string()),
            }
        }
    }

    /// Apply list indentation (add spaces/tabs at line start)
    fn apply_indent_list(&self, content: &str, cursor_position: CursorPosition) -> ShortcutResult {
        let lines: Vec<&str> = content.lines().collect();

        if cursor_position.line >= lines.len() {
            return ShortcutResult {
                content: content.to_string(),
                cursor_position,
                success: false,
                message: Some("Invalid cursor position".to_string()),
            };
        }

        // Check if current line is a list item
        let current_line = lines[cursor_position.line];
        if !self.is_list_line(current_line) {
            return ShortcutResult {
                content: content.to_string(),
                cursor_position,
                success: false,
                message: Some("Not a list item".to_string()),
            };
        }

        // Add 2 spaces at the beginning of the line for indentation
        let mut new_lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
        new_lines[cursor_position.line] = format!("  {}", current_line);

        let new_content = new_lines.join("\n");
        let new_absolute = cursor_position.absolute + 2; // Account for added spaces

        let new_cursor = self.calculate_cursor_position(&new_content, new_absolute);

        ShortcutResult {
            content: new_content,
            cursor_position: new_cursor,
            success: true,
            message: Some("Indented list item".to_string()),
        }
    }

    /// Apply list unindentation (remove spaces/tabs at line start)
    fn apply_unindent_list(
        &self,
        content: &str,
        cursor_position: CursorPosition,
    ) -> ShortcutResult {
        let lines: Vec<&str> = content.lines().collect();

        if cursor_position.line >= lines.len() {
            return ShortcutResult {
                content: content.to_string(),
                cursor_position,
                success: false,
                message: Some("Invalid cursor position".to_string()),
            };
        }

        let current_line = lines[cursor_position.line];

        // Check if line starts with spaces or tabs
        let trimmed = current_line.trim_start();
        let indent_len = current_line.len() - trimmed.len();

        if indent_len == 0 {
            return ShortcutResult {
                content: content.to_string(),
                cursor_position,
                success: false,
                message: Some("No indentation to remove".to_string()),
            };
        }

        // Remove up to 2 spaces or 1 tab
        let spaces_to_remove = if current_line.starts_with('\t') {
            1
        } else {
            indent_len.min(2)
        };

        let mut new_lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
        new_lines[cursor_position.line] = current_line[spaces_to_remove..].to_string();

        let new_content = new_lines.join("\n");
        let new_absolute = cursor_position.absolute.saturating_sub(spaces_to_remove);

        let new_cursor = self.calculate_cursor_position(&new_content, new_absolute);

        ShortcutResult {
            content: new_content,
            cursor_position: new_cursor,
            success: true,
            message: Some("Unindented list item".to_string()),
        }
    }

    /// Apply automatic list continuation on Enter key
    fn apply_continue_list(
        &self,
        content: &str,
        cursor_position: CursorPosition,
    ) -> ShortcutResult {
        let lines: Vec<&str> = content.lines().collect();

        if cursor_position.line >= lines.len() {
            return ShortcutResult {
                content: content.to_string(),
                cursor_position,
                success: false,
                message: Some("Invalid cursor position".to_string()),
            };
        }

        let current_line = lines[cursor_position.line];

        // Check if current line is a list item
        if let Some(list_info) = self.parse_list_item(current_line) {
            // Check if the list item is empty (only has the marker)
            if list_info.content.trim().is_empty() {
                // Empty list item - remove it and exit list mode
                let mut new_lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
                new_lines[cursor_position.line] = String::new();

                let new_content = new_lines.join("\n");
                let new_absolute = cursor_position.absolute - list_info.marker.len();

                let new_cursor = self.calculate_cursor_position(&new_content, new_absolute);

                return ShortcutResult {
                    content: new_content,
                    cursor_position: new_cursor,
                    success: true,
                    message: Some("Exited list mode".to_string()),
                };
            }

            // Split the current line at cursor position
            let line_start = &current_line[..cursor_position.column];
            let line_end = &current_line[cursor_position.column..];

            // Create new list item with same indentation and marker type
            let new_marker = if list_info.is_ordered {
                // Increment the number for ordered lists
                format!("{}. ", list_info.number + 1)
            } else {
                list_info.marker.clone()
            };

            let new_list_item = format!("{}{}", list_info.indentation, new_marker);

            // Build new content
            let mut new_lines: Vec<String> = Vec::new();
            for (i, line) in lines.iter().enumerate() {
                if i == cursor_position.line {
                    new_lines.push(line_start.to_string());
                    new_lines.push(format!("{}{}", new_list_item, line_end));
                } else {
                    new_lines.push(line.to_string());
                }
            }

            let new_content = new_lines.join("\n");

            // Calculate new cursor position (at the end of the new list marker)
            let new_line = cursor_position.line + 1;
            let new_column = new_list_item.len();
            let new_absolute = if let Some(abs) =
                CursorPosition::calculate_absolute(&new_content, new_line, new_column)
            {
                abs
            } else {
                cursor_position.absolute + line_start.len() + 1 + new_list_item.len()
            };

            let new_cursor = CursorPosition::new(new_line, new_column, new_absolute);

            ShortcutResult {
                content: new_content,
                cursor_position: new_cursor,
                success: true,
                message: Some("Continued list".to_string()),
            }
        } else {
            // Not a list line - just insert a newline
            let (before, after) = content.split_at(cursor_position.absolute);
            let new_content = format!("{}\n{}", before, after);
            let new_absolute = cursor_position.absolute + 1;

            let new_cursor = self.calculate_cursor_position(&new_content, new_absolute);

            ShortcutResult {
                content: new_content,
                cursor_position: new_cursor,
                success: true,
                message: Some("Inserted newline".to_string()),
            }
        }
    }

    /// Parse list item information from a line
    fn parse_list_item(&self, line: &str) -> Option<ListItemInfo> {
        let indentation = line.len() - line.trim_start().len();
        let indent_str = &line[..indentation];
        let trimmed = line.trim_start();

        // Check for unordered list markers
        if let Some(content) = trimmed.strip_prefix("- ") {
            return Some(ListItemInfo {
                indentation: indent_str.to_string(),
                marker: "- ".to_string(),
                is_ordered: false,
                number: 0,
                content: content.to_string(),
            });
        } else if let Some(content) = trimmed.strip_prefix("* ") {
            return Some(ListItemInfo {
                indentation: indent_str.to_string(),
                marker: "* ".to_string(),
                is_ordered: false,
                number: 0,
                content: content.to_string(),
            });
        } else if let Some(content) = trimmed.strip_prefix("+ ") {
            return Some(ListItemInfo {
                indentation: indent_str.to_string(),
                marker: "+ ".to_string(),
                is_ordered: false,
                number: 0,
                content: content.to_string(),
            });
        }

        // Check for ordered list markers (e.g., "1. ", "2. ", etc.)
        if let Some(dot_pos) = trimmed.find(". ") {
            let number_str = &trimmed[..dot_pos];
            if let Ok(number) = number_str.parse::<usize>() {
                return Some(ListItemInfo {
                    indentation: indent_str.to_string(),
                    marker: format!("{}. ", number),
                    is_ordered: true,
                    number,
                    content: trimmed[dot_pos + 2..].to_string(),
                });
            }
        }

        None
    }

    /// Check if a line is a list item
    fn is_list_line(&self, line: &str) -> bool {
        self.parse_list_item(line).is_some()
    }

    /// Calculate cursor position from absolute position
    fn calculate_cursor_position(&self, content: &str, absolute: usize) -> CursorPosition {
        if let Some((line, column)) = CursorPosition::calculate_line_column(content, absolute) {
            CursorPosition::new(line, column, absolute)
        } else {
            // Fallback to end of content
            let lines: Vec<&str> = content.lines().collect();
            let last_line = lines.len().saturating_sub(1);
            let last_column = lines.last().map(|l| l.len()).unwrap_or(0);
            CursorPosition::new(last_line, last_column, content.len())
        }
    }
}

impl Default for KeyboardShortcutHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_selection_creation() {
        let selection = TextSelection::new(5, 10);
        assert_eq!(selection.start, 5);
        assert_eq!(selection.end, 10);
        assert!(!selection.is_empty());
        assert_eq!(selection.len(), 5);
    }

    #[test]
    fn test_text_selection_reversed() {
        let selection = TextSelection::new(10, 5);
        assert_eq!(selection.start, 5);
        assert_eq!(selection.end, 10);
    }

    #[test]
    fn test_text_selection_extract() {
        let content = "Hello, world!";
        let selection = TextSelection::new(0, 5);
        assert_eq!(selection.extract_text(content), "Hello");
    }

    #[test]
    fn test_bold_with_selection() {
        let handler = KeyboardShortcutHandler::new();
        let content = "Hello world";
        let selection = TextSelection::new(0, 5); // Select "Hello"
        let cursor = CursorPosition::new(0, 0, 0);

        let result = handler.apply_bold(content, selection, cursor);

        assert!(result.success);
        assert_eq!(result.content, "**Hello** world");
    }

    #[test]
    fn test_bold_without_selection() {
        let handler = KeyboardShortcutHandler::new();
        let content = "Hello world";
        let selection = TextSelection::new(6, 6); // Cursor at position 6
        let cursor = CursorPosition::new(0, 6, 6);

        let result = handler.apply_bold(content, selection, cursor);

        assert!(result.success);
        assert_eq!(result.content, "Hello ****world");
        assert_eq!(result.cursor_position.absolute, 8); // Between **|**
    }

    #[test]
    fn test_italic_with_selection() {
        let handler = KeyboardShortcutHandler::new();
        let content = "Hello world";
        let selection = TextSelection::new(6, 11); // Select "world"
        let cursor = CursorPosition::new(0, 6, 6);

        let result = handler.apply_italic(content, selection, cursor);

        assert!(result.success);
        assert_eq!(result.content, "Hello *world*");
    }

    #[test]
    fn test_italic_without_selection() {
        let handler = KeyboardShortcutHandler::new();
        let content = "Hello world";
        let selection = TextSelection::new(6, 6);
        let cursor = CursorPosition::new(0, 6, 6);

        let result = handler.apply_italic(content, selection, cursor);

        assert!(result.success);
        assert_eq!(result.content, "Hello **world");
        assert_eq!(result.cursor_position.absolute, 7); // Between *|*
    }

    #[test]
    fn test_indent_list_item() {
        let handler = KeyboardShortcutHandler::new();
        let content = "- Item 1\n- Item 2\n- Item 3";
        let cursor = CursorPosition::new(1, 2, 11); // On "Item 2"

        let result = handler.apply_indent_list(content, cursor);

        assert!(result.success);
        assert!(result.content.contains("  - Item 2"));
    }

    #[test]
    fn test_indent_non_list_line() {
        let handler = KeyboardShortcutHandler::new();
        let content = "Regular text\nMore text";
        let cursor = CursorPosition::new(0, 5, 5);

        let result = handler.apply_indent_list(content, cursor);

        assert!(!result.success);
        assert_eq!(result.content, content);
    }

    #[test]
    fn test_unindent_list_item() {
        let handler = KeyboardShortcutHandler::new();
        let content = "- Item 1\n  - Item 2\n- Item 3";
        let cursor = CursorPosition::new(1, 4, 13); // On indented "Item 2"

        let result = handler.apply_unindent_list(content, cursor);

        assert!(result.success);
        assert!(result.content.contains("- Item 2"));
    }

    #[test]
    fn test_unindent_no_indentation() {
        let handler = KeyboardShortcutHandler::new();
        let content = "- Item 1\n- Item 2";
        let cursor = CursorPosition::new(0, 2, 2);

        let result = handler.apply_unindent_list(content, cursor);

        assert!(!result.success);
    }

    #[test]
    fn test_is_list_line() {
        let handler = KeyboardShortcutHandler::new();

        assert!(handler.is_list_line("- Item"));
        assert!(handler.is_list_line("* Item"));
        assert!(handler.is_list_line("+ Item"));
        assert!(handler.is_list_line("1. Item"));
        assert!(handler.is_list_line("  - Indented item"));
        assert!(!handler.is_list_line("Regular text"));
        assert!(!handler.is_list_line("Not a list"));
    }

    #[test]
    fn test_shortcut_action_bold() {
        let handler = KeyboardShortcutHandler::new();
        let content = "test";
        let selection = TextSelection::new(0, 4);
        let cursor = CursorPosition::new(0, 0, 0);

        let result = handler.apply_shortcut(ShortcutAction::Bold, content, selection, cursor);

        assert!(result.success);
        assert_eq!(result.content, "**test**");
    }

    #[test]
    fn test_shortcut_action_italic() {
        let handler = KeyboardShortcutHandler::new();
        let content = "test";
        let selection = TextSelection::new(0, 4);
        let cursor = CursorPosition::new(0, 0, 0);

        let result = handler.apply_shortcut(ShortcutAction::Italic, content, selection, cursor);

        assert!(result.success);
        assert_eq!(result.content, "*test*");
    }

    #[test]
    fn test_continue_unordered_list() {
        let handler = KeyboardShortcutHandler::new();
        let content = "- Item 1";
        let cursor = CursorPosition::new(0, 8, 8); // At end of line

        let result = handler.apply_continue_list(content, cursor);

        assert!(result.success);
        assert!(result.content.contains("- Item 1\n- "));
        assert_eq!(result.cursor_position.line, 1);
    }

    #[test]
    fn test_continue_ordered_list() {
        let handler = KeyboardShortcutHandler::new();
        let content = "1. First item";
        let cursor = CursorPosition::new(0, 13, 13); // At end of line

        let result = handler.apply_continue_list(content, cursor);

        assert!(result.success);
        assert!(result.content.contains("1. First item\n2. "));
        assert_eq!(result.cursor_position.line, 1);
    }

    #[test]
    fn test_continue_indented_list() {
        let handler = KeyboardShortcutHandler::new();
        let content = "  - Indented item";
        let cursor = CursorPosition::new(0, 17, 17); // At end of line

        let result = handler.apply_continue_list(content, cursor);

        assert!(result.success);
        assert!(result.content.contains("  - Indented item\n  - "));
        assert_eq!(result.cursor_position.line, 1);
    }

    #[test]
    fn test_exit_list_on_empty_item() {
        let handler = KeyboardShortcutHandler::new();
        let content = "- Item 1\n- ";
        let cursor = CursorPosition::new(1, 2, 11); // At end of empty list item

        let result = handler.apply_continue_list(content, cursor);

        assert!(result.success);
        assert!(result.content.contains("- Item 1\n"));
        assert!(!result.content.contains("- \n- "));
    }

    #[test]
    fn test_split_list_item_at_cursor() {
        let handler = KeyboardShortcutHandler::new();
        let content = "- Item with more text";
        let cursor = CursorPosition::new(0, 11, 11); // After "Item with"

        let result = handler.apply_continue_list(content, cursor);

        assert!(result.success);
        // When splitting, the first line keeps everything up to cursor
        // and the second line gets a new list marker plus the rest
        assert!(result.content.contains("- Item with"));
        assert!(result.content.contains("\n-  more text"));
    }

    #[test]
    fn test_parse_list_item_unordered() {
        let handler = KeyboardShortcutHandler::new();

        let info = handler.parse_list_item("- Item").unwrap();
        assert_eq!(info.marker, "- ");
        assert!(!info.is_ordered);
        assert_eq!(info.content, "Item");

        let info = handler.parse_list_item("* Item").unwrap();
        assert_eq!(info.marker, "* ");

        let info = handler.parse_list_item("+ Item").unwrap();
        assert_eq!(info.marker, "+ ");
    }

    #[test]
    fn test_parse_list_item_ordered() {
        let handler = KeyboardShortcutHandler::new();

        let info = handler.parse_list_item("1. First").unwrap();
        assert_eq!(info.marker, "1. ");
        assert!(info.is_ordered);
        assert_eq!(info.number, 1);
        assert_eq!(info.content, "First");

        let info = handler.parse_list_item("42. Answer").unwrap();
        assert_eq!(info.number, 42);
    }

    #[test]
    fn test_parse_list_item_with_indentation() {
        let handler = KeyboardShortcutHandler::new();

        let info = handler.parse_list_item("  - Indented").unwrap();
        assert_eq!(info.indentation, "  ");
        assert_eq!(info.marker, "- ");
        assert_eq!(info.content, "Indented");
    }

    #[test]
    fn test_parse_non_list_line() {
        let handler = KeyboardShortcutHandler::new();

        assert!(handler.parse_list_item("Regular text").is_none());
        assert!(handler.parse_list_item("Not a list").is_none());
    }
}
