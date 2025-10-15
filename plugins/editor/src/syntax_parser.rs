//! Syntax parser for real-time markdown element detection

use serde::{Deserialize, Serialize};
use std::ops::Range;

/// Position range within the document
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PositionRange {
    /// Start position (inclusive)
    pub start: usize,
    /// End position (exclusive)
    pub end: usize,
}

impl PositionRange {
    /// Create a new position range
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Check if this range contains a position
    pub fn contains(&self, position: usize) -> bool {
        position >= self.start && position < self.end
    }

    /// Check if this range overlaps with another range
    pub fn overlaps(&self, other: &PositionRange) -> bool {
        self.start < other.end && other.start < self.end
    }

    /// Get the length of this range
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// Check if this range is empty
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

impl From<Range<usize>> for PositionRange {
    fn from(range: Range<usize>) -> Self {
        Self::new(range.start, range.end)
    }
}

impl From<PositionRange> for Range<usize> {
    fn from(pos_range: PositionRange) -> Self {
        pos_range.start..pos_range.end
    }
}

/// Types of markdown syntax elements
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyntaxElementType {
    /// Header elements (# ## ### etc.)
    Header { level: u8 },
    /// Bold text (**text** or __text__)
    Bold,
    /// Italic text (*text* or _text_)
    Italic,
    /// Inline code (`code`)
    InlineCode,
    /// Code block (```code```)
    CodeBlock { language: Option<String> },
    /// Link ([text](url))
    Link { url: String, title: Option<String> },
    /// Unordered list item (- item or * item)
    UnorderedListItem { level: u8 },
    /// Ordered list item (1. item)
    OrderedListItem { level: u8, number: u32 },
}

/// A syntax element with its position and content
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyntaxElement {
    /// Type of the syntax element
    pub element_type: SyntaxElementType,
    /// Position range in the document
    pub range: PositionRange,
    /// Raw content including markdown syntax
    pub raw_content: String,
    /// Rendered content without markdown syntax
    pub rendered_content: String,
    /// Whether this element is currently being edited
    pub is_active: bool,
}

impl SyntaxElement {
    /// Create a new syntax element
    pub fn new(
        element_type: SyntaxElementType,
        range: PositionRange,
        raw_content: String,
        rendered_content: String,
    ) -> Self {
        Self {
            element_type,
            range,
            raw_content,
            rendered_content,
            is_active: false,
        }
    }

    /// Check if this element contains a cursor position
    pub fn contains_cursor(&self, cursor_position: usize) -> bool {
        self.range.contains(cursor_position)
    }

    /// Set the active state of this element
    pub fn set_active(&mut self, active: bool) {
        self.is_active = active;
    }
}

/// Trait for parsing markdown syntax elements in real-time
pub trait SyntaxParser {
    /// Parse the entire document and return all syntax elements
    fn parse_document(&self, content: &str) -> Vec<SyntaxElement>;

    /// Parse a specific line and return syntax elements found
    fn parse_line(&self, line: &str, line_start_offset: usize) -> Vec<SyntaxElement>;

    /// Parse incrementally from a cursor position
    fn parse_incremental(
        &self,
        content: &str,
        cursor_position: usize,
        change_range: Option<PositionRange>,
    ) -> Vec<SyntaxElement>;

    /// Find syntax element at a specific position
    fn find_element_at_position<'a>(
        &self,
        elements: &'a [SyntaxElement],
        position: usize,
    ) -> Option<&'a SyntaxElement>;

    /// Update syntax elements after content change
    fn update_elements_after_change(
        &self,
        elements: &mut Vec<SyntaxElement>,
        change_range: PositionRange,
        new_content: &str,
    );
}

/// Default implementation of the markdown syntax parser
#[derive(Debug, Default)]
pub struct MarkdownSyntaxParser;

impl MarkdownSyntaxParser {
    /// Create a new markdown syntax parser
    pub fn new() -> Self {
        Self
    }

    /// Parse headers (# ## ### etc.)
    fn parse_headers(&self, content: &str, offset: usize) -> Vec<SyntaxElement> {
        let mut elements = Vec::new();
        let lines: Vec<&str> = content.lines().collect();
        let mut current_offset = offset;

        for line in lines {
            if let Some(element) = self.parse_header_line(line, current_offset) {
                elements.push(element);
            }
            current_offset += line.len() + 1; // +1 for newline
        }

        elements
    }

    /// Parse a single header line
    fn parse_header_line(&self, line: &str, line_offset: usize) -> Option<SyntaxElement> {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('#') {
            return None;
        }

        let mut level = 0;
        let mut chars = trimmed.chars();

        // Count consecutive # characters
        for ch in chars.by_ref() {
            if ch == '#' && level < 6 {
                level += 1;
            } else {
                break;
            }
        }

        if level == 0 || level > 6 {
            return None;
        }

        // Find the start of the header text
        let remaining: String = chars.collect();
        let header_text = remaining.trim_start();

        if header_text.is_empty() {
            return None;
        }

        let start_pos = line_offset + (line.len() - line.trim_start().len());
        let end_pos = line_offset + line.len();

        Some(SyntaxElement::new(
            SyntaxElementType::Header { level },
            PositionRange::new(start_pos, end_pos),
            line.to_string(),
            header_text.to_string(),
        ))
    }

    /// Parse inline formatting (bold, italic, code)
    fn parse_inline_formatting(&self, content: &str, offset: usize) -> Vec<SyntaxElement> {
        let mut elements = Vec::new();
        let mut chars = content.char_indices().peekable();

        for (i, ch) in chars {
            match ch {
                '*' | '_' => {
                    if let Some(element) = self.parse_emphasis(&content[i..], offset + i) {
                        elements.push(element);
                    }
                }
                '`' => {
                    if let Some(element) = self.parse_inline_code(&content[i..], offset + i) {
                        elements.push(element);
                    }
                }
                _ => {}
            }
        }

        elements
    }

    /// Parse emphasis (bold/italic)
    fn parse_emphasis(&self, content: &str, offset: usize) -> Option<SyntaxElement> {
        let chars: Vec<char> = content.chars().collect();
        if chars.is_empty() {
            return None;
        }

        let marker = chars[0];
        if marker != '*' && marker != '_' {
            return None;
        }

        // Check for double marker (bold)
        let is_double = chars.len() > 1 && chars[1] == marker;
        let marker_len = if is_double { 2 } else { 1 };

        // Find closing marker
        let mut i = marker_len;
        while i < chars.len() {
            if chars[i] == marker {
                if is_double && i + 1 < chars.len() && chars[i + 1] == marker {
                    // Found closing double marker
                    let raw_content = chars[0..i + 2].iter().collect::<String>();
                    let rendered_content = chars[marker_len..i].iter().collect::<String>();

                    return Some(SyntaxElement::new(
                        SyntaxElementType::Bold,
                        PositionRange::new(offset, offset + i + 2),
                        raw_content,
                        rendered_content,
                    ));
                } else if !is_double {
                    // Found closing single marker
                    let raw_content = chars[0..i + 1].iter().collect::<String>();
                    let rendered_content = chars[marker_len..i].iter().collect::<String>();

                    return Some(SyntaxElement::new(
                        SyntaxElementType::Italic,
                        PositionRange::new(offset, offset + i + 1),
                        raw_content,
                        rendered_content,
                    ));
                }
            }
            i += 1;
        }

        None
    }

    /// Parse inline code
    fn parse_inline_code(&self, content: &str, offset: usize) -> Option<SyntaxElement> {
        let chars: Vec<char> = content.chars().collect();
        if chars.is_empty() || chars[0] != '`' {
            return None;
        }

        // Find closing backtick
        for i in 1..chars.len() {
            if chars[i] == '`' {
                let raw_content = chars[0..i + 1].iter().collect::<String>();
                let rendered_content = chars[1..i].iter().collect::<String>();

                return Some(SyntaxElement::new(
                    SyntaxElementType::InlineCode,
                    PositionRange::new(offset, offset + i + 1),
                    raw_content,
                    rendered_content,
                ));
            }
        }

        None
    }

    /// Parse links
    fn parse_links(&self, content: &str, offset: usize) -> Vec<SyntaxElement> {
        let mut elements = Vec::new();
        let mut chars = content.char_indices().peekable();

        for (i, ch) in chars {
            if ch == '[' {
                if let Some(element) = self.parse_link(&content[i..], offset + i) {
                    elements.push(element);
                }
            }
        }

        elements
    }

    /// Parse a single link
    fn parse_link(&self, content: &str, offset: usize) -> Option<SyntaxElement> {
        let chars: Vec<char> = content.chars().collect();
        if chars.is_empty() || chars[0] != '[' {
            return None;
        }

        // Find closing bracket for link text
        let mut text_end = None;
        for i in 1..chars.len() {
            if chars[i] == ']' {
                text_end = Some(i);
                break;
            }
        }

        let text_end = text_end?;

        // Check for opening parenthesis
        if text_end + 1 >= chars.len() || chars[text_end + 1] != '(' {
            return None;
        }

        // Find closing parenthesis for URL
        let mut url_end = None;
        for i in (text_end + 2)..chars.len() {
            if chars[i] == ')' {
                url_end = Some(i);
                break;
            }
        }

        let url_end = url_end?;

        let link_text = chars[1..text_end].iter().collect::<String>();
        let url = chars[(text_end + 2)..url_end].iter().collect::<String>();
        let raw_content = chars[0..url_end + 1].iter().collect::<String>();

        Some(SyntaxElement::new(
            SyntaxElementType::Link { url, title: None },
            PositionRange::new(offset, offset + url_end + 1),
            raw_content,
            link_text,
        ))
    }

    /// Parse list items
    fn parse_lists(&self, content: &str, offset: usize) -> Vec<SyntaxElement> {
        let mut elements = Vec::new();
        let lines: Vec<&str> = content.lines().collect();
        let mut current_offset = offset;

        for line in lines {
            if let Some(element) = self.parse_list_item(line, current_offset) {
                elements.push(element);
            }
            current_offset += line.len() + 1; // +1 for newline
        }

        elements
    }

    /// Parse a single list item
    fn parse_list_item(&self, line: &str, line_offset: usize) -> Option<SyntaxElement> {
        let trimmed = line.trim_start();
        let indent_level = (line.len() - trimmed.len()) / 2; // Assuming 2 spaces per level

        // Check for unordered list markers
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            let item_text = &trimmed[2..];
            let start_pos = line_offset;
            let end_pos = line_offset + line.len();

            return Some(SyntaxElement::new(
                SyntaxElementType::UnorderedListItem {
                    level: indent_level as u8,
                },
                PositionRange::new(start_pos, end_pos),
                line.to_string(),
                item_text.to_string(),
            ));
        }

        // Check for ordered list markers
        if let Some(dot_pos) = trimmed.find(". ") {
            if let Ok(number) = trimmed[..dot_pos].parse::<u32>() {
                let item_text = &trimmed[dot_pos + 2..];
                let start_pos = line_offset;
                let end_pos = line_offset + line.len();

                return Some(SyntaxElement::new(
                    SyntaxElementType::OrderedListItem {
                        level: indent_level as u8,
                        number,
                    },
                    PositionRange::new(start_pos, end_pos),
                    line.to_string(),
                    item_text.to_string(),
                ));
            }
        }

        None
    }
}

impl SyntaxParser for MarkdownSyntaxParser {
    fn parse_document(&self, content: &str) -> Vec<SyntaxElement> {
        let mut elements = Vec::new();

        // Parse different types of elements
        elements.extend(self.parse_headers(content, 0));
        elements.extend(self.parse_inline_formatting(content, 0));
        elements.extend(self.parse_links(content, 0));
        elements.extend(self.parse_lists(content, 0));

        // Sort elements by position
        elements.sort_by_key(|e| e.range.start);
        elements
    }

    fn parse_line(&self, line: &str, line_start_offset: usize) -> Vec<SyntaxElement> {
        let mut elements = Vec::new();

        // Parse header if present
        if let Some(header) = self.parse_header_line(line, line_start_offset) {
            elements.push(header);
        }

        // Parse list item if present
        if let Some(list_item) = self.parse_list_item(line, line_start_offset) {
            elements.push(list_item);
        }

        // Parse inline formatting
        elements.extend(self.parse_inline_formatting(line, line_start_offset));
        elements.extend(self.parse_links(line, line_start_offset));

        elements.sort_by_key(|e| e.range.start);
        elements
    }

    fn parse_incremental(
        &self,
        content: &str,
        cursor_position: usize,
        change_range: Option<PositionRange>,
    ) -> Vec<SyntaxElement> {
        // For incremental parsing, we'll parse the affected area plus some context
        let start_pos = change_range
            .as_ref()
            .map(|r| r.start.saturating_sub(100))
            .unwrap_or(cursor_position.saturating_sub(100));

        let end_pos = change_range
            .as_ref()
            .map(|r| (r.end + 100).min(content.len()))
            .unwrap_or((cursor_position + 100).min(content.len()));

        let subset = &content[start_pos..end_pos];
        let mut elements = Vec::new();

        elements.extend(self.parse_headers(subset, start_pos));
        elements.extend(self.parse_inline_formatting(subset, start_pos));
        elements.extend(self.parse_links(subset, start_pos));
        elements.extend(self.parse_lists(subset, start_pos));

        elements.sort_by_key(|e| e.range.start);
        elements
    }

    fn find_element_at_position<'a>(
        &self,
        elements: &'a [SyntaxElement],
        position: usize,
    ) -> Option<&'a SyntaxElement> {
        elements.iter().find(|e| e.contains_cursor(position))
    }

    fn update_elements_after_change(
        &self,
        elements: &mut Vec<SyntaxElement>,
        change_range: PositionRange,
        new_content: &str,
    ) {
        let change_delta = new_content.len() as i32 - change_range.len() as i32;

        // Remove elements that overlap with the change range
        elements.retain(|e| !e.range.overlaps(&change_range));

        // Adjust positions of elements after the change
        for element in elements.iter_mut() {
            if element.range.start >= change_range.end {
                element.range.start = (element.range.start as i32 + change_delta) as usize;
                element.range.end = (element.range.end as i32 + change_delta) as usize;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_parsing() {
        let parser = MarkdownSyntaxParser::new();
        let content = "# Header 1\n## Header 2\n### Header 3";
        let elements = parser.parse_document(content);

        assert_eq!(elements.len(), 3);

        if let SyntaxElementType::Header { level } = &elements[0].element_type {
            assert_eq!(*level, 1);
            assert_eq!(elements[0].rendered_content, "Header 1");
        } else {
            panic!("Expected header element");
        }
    }

    #[test]
    fn test_bold_parsing() {
        let parser = MarkdownSyntaxParser::new();
        let content = "This is **bold** text";
        let elements = parser.parse_document(content);

        let bold_elements: Vec<_> = elements
            .iter()
            .filter(|e| matches!(e.element_type, SyntaxElementType::Bold))
            .collect();

        assert_eq!(bold_elements.len(), 1);
        assert_eq!(bold_elements[0].rendered_content, "bold");
        assert_eq!(bold_elements[0].raw_content, "**bold**");
    }

    #[test]
    fn test_italic_parsing() {
        let parser = MarkdownSyntaxParser::new();
        let content = "This is *italic* text";
        let elements = parser.parse_document(content);

        let italic_elements: Vec<_> = elements
            .iter()
            .filter(|e| matches!(e.element_type, SyntaxElementType::Italic))
            .collect();

        assert_eq!(italic_elements.len(), 1);
        assert_eq!(italic_elements[0].rendered_content, "italic");
        assert_eq!(italic_elements[0].raw_content, "*italic*");
    }

    #[test]
    fn test_inline_code_parsing() {
        let parser = MarkdownSyntaxParser::new();
        let content = "This is `code` text";
        let elements = parser.parse_document(content);

        let code_elements: Vec<_> = elements
            .iter()
            .filter(|e| matches!(e.element_type, SyntaxElementType::InlineCode))
            .collect();

        assert_eq!(code_elements.len(), 1);
        assert_eq!(code_elements[0].rendered_content, "code");
        assert_eq!(code_elements[0].raw_content, "`code`");
    }

    #[test]
    fn test_link_parsing() {
        let parser = MarkdownSyntaxParser::new();
        let content = "This is a [link](https://example.com) text";
        let elements = parser.parse_document(content);

        let link_elements: Vec<_> = elements
            .iter()
            .filter(|e| matches!(e.element_type, SyntaxElementType::Link { .. }))
            .collect();

        assert_eq!(link_elements.len(), 1);
        assert_eq!(link_elements[0].rendered_content, "link");

        if let SyntaxElementType::Link { url, .. } = &link_elements[0].element_type {
            assert_eq!(url, "https://example.com");
        }
    }

    #[test]
    fn test_list_parsing() {
        let parser = MarkdownSyntaxParser::new();
        let content = "- Item 1\n- Item 2\n1. Numbered item";
        let elements = parser.parse_document(content);

        let list_elements: Vec<_> = elements
            .iter()
            .filter(|e| {
                matches!(
                    e.element_type,
                    SyntaxElementType::UnorderedListItem { .. }
                        | SyntaxElementType::OrderedListItem { .. }
                )
            })
            .collect();

        assert_eq!(list_elements.len(), 3);
    }

    #[test]
    fn test_position_range() {
        let range = PositionRange::new(5, 10);

        assert!(range.contains(7));
        assert!(!range.contains(10));
        assert!(!range.contains(4));
        assert_eq!(range.len(), 5);

        let other_range = PositionRange::new(8, 15);
        assert!(range.overlaps(&other_range));

        let non_overlapping = PositionRange::new(15, 20);
        assert!(!range.overlaps(&non_overlapping));
    }
}
