//! Inline renderer for converting markdown syntax to HTML with cursor-aware editing

use crate::editor_state::CursorPosition;
use crate::syntax_parser::{SyntaxElement, SyntaxElementType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Rendered HTML element with metadata for editing
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderedElement {
    /// HTML content of the element
    pub html: String,
    /// CSS classes applied to the element
    pub css_classes: Vec<String>,
    /// Data attributes for the element
    pub data_attributes: HashMap<String, String>,
    /// Whether this element is currently being edited
    pub is_editing: bool,
    /// Raw markdown content for editing mode
    pub raw_content: String,
    /// Position range in the original document
    pub position_range: (usize, usize),
}

impl RenderedElement {
    /// Create a new rendered element
    pub fn new(
        html: String,
        css_classes: Vec<String>,
        raw_content: String,
        position_range: (usize, usize),
    ) -> Self {
        Self {
            html,
            css_classes,
            data_attributes: HashMap::new(),
            is_editing: false,
            raw_content,
            position_range,
        }
    }

    /// Add a CSS class to the element
    pub fn add_class(&mut self, class: &str) {
        if !self.css_classes.contains(&class.to_string()) {
            self.css_classes.push(class.to_string());
        }
    }

    /// Add a data attribute to the element
    pub fn add_data_attribute(&mut self, key: &str, value: &str) {
        self.data_attributes.insert(key.to_string(), value.to_string());
    }

    /// Set editing state
    pub fn set_editing(&mut self, editing: bool) {
        self.is_editing = editing;
        if editing {
            self.add_class("editing");
        } else {
            self.css_classes.retain(|c| c != "editing");
        }
    }

    /// Generate complete HTML with attributes
    pub fn to_html(&self) -> String {
        let tag = self.extract_tag_name();
        let classes = if self.css_classes.is_empty() {
            String::new()
        } else {
            format!(" class=\"{}\"", self.css_classes.join(" "))
        };

        let data_attrs = if self.data_attributes.is_empty() {
            String::new()
        } else {
            self.data_attributes
                .iter()
                .map(|(k, v)| format!(" data-{}=\"{}\"", k, v))
                .collect::<Vec<_>>()
                .join("")
        };

        if self.is_editing {
            // Return raw content for editing
            format!(
                "<{tag}{classes}{data_attrs} contenteditable=\"true\">{}</{tag}>",
                html_escape(&self.raw_content)
            )
        } else {
            // Return rendered HTML
            format!("<{tag}{classes}{data_attrs}>{}</{tag}>", self.html)
        }
    }

    /// Extract tag name from HTML content
    fn extract_tag_name(&self) -> &str {
        if self.html.starts_with("<h") {
            "h1" // Default to h1, will be overridden by specific header levels
        } else if self.html.starts_with("<strong>") {
            "strong"
        } else if self.html.starts_with("<em>") {
            "em"
        } else if self.html.starts_with("<code>") {
            "code"
        } else if self.html.starts_with("<a ") {
            "a"
        } else if self.html.starts_with("<li>") {
            "li"
        } else {
            "span"
        }
    }
}

/// Trait for rendering markdown syntax elements to HTML with cursor awareness
pub trait InlineRenderer {
    /// Render a syntax element to HTML
    fn render_element(&self, element: &SyntaxElement) -> RenderedElement;

    /// Render multiple elements with cursor awareness
    fn render_elements_with_cursor(
        &self,
        elements: &[SyntaxElement],
        cursor_position: &CursorPosition,
    ) -> Vec<RenderedElement>;

    /// Extract raw content from a rendered element for editing
    fn extract_raw_content(&self, rendered: &RenderedElement) -> String;

    /// Check if cursor is within an element for editing activation
    fn is_cursor_in_element(&self, element: &SyntaxElement, cursor_position: &CursorPosition) -> bool;

    /// Render document content with inline elements
    fn render_document(
        &self,
        content: &str,
        elements: &[SyntaxElement],
        cursor_position: &CursorPosition,
    ) -> String;
}

/// Default implementation of the inline renderer
#[derive(Debug, Default)]
pub struct MarkdownInlineRenderer {
    /// CSS class prefix for generated elements
    pub class_prefix: String,
}

impl MarkdownInlineRenderer {
    /// Create a new markdown inline renderer
    pub fn new() -> Self {
        Self {
            class_prefix: "md".to_string(),
        }
    }

    /// Create renderer with custom class prefix
    pub fn with_class_prefix(class_prefix: String) -> Self {
        Self { class_prefix }
    }

    /// Generate CSS class name with prefix
    fn css_class(&self, base: &str) -> String {
        format!("{}-{}", self.class_prefix, base)
    }

    /// Render header element
    fn render_header(&self, level: u8, content: &str, raw_content: &str, range: (usize, usize)) -> RenderedElement {
        let tag = format!("h{}", level);
        let html = format!("<{}>{}</{}>", tag, html_escape(content), tag);
        let css_classes = vec![
            self.css_class("header"),
            self.css_class(&format!("header-{}", level)),
        ];

        let mut element = RenderedElement::new(html, css_classes, raw_content.to_string(), range);
        element.add_data_attribute("level", &level.to_string());
        element
    }

    /// Render bold element
    fn render_bold(&self, content: &str, raw_content: &str, range: (usize, usize)) -> RenderedElement {
        let html = format!("<strong>{}</strong>", html_escape(content));
        let css_classes = vec![self.css_class("bold")];

        RenderedElement::new(html, css_classes, raw_content.to_string(), range)
    }

    /// Render italic element
    fn render_italic(&self, content: &str, raw_content: &str, range: (usize, usize)) -> RenderedElement {
        let html = format!("<em>{}</em>", html_escape(content));
        let css_classes = vec![self.css_class("italic")];

        RenderedElement::new(html, css_classes, raw_content.to_string(), range)
    }

    /// Render inline code element
    fn render_inline_code(&self, content: &str, raw_content: &str, range: (usize, usize)) -> RenderedElement {
        let html = format!("<code>{}</code>", html_escape(content));
        let css_classes = vec![self.css_class("code"), self.css_class("inline-code")];

        RenderedElement::new(html, css_classes, raw_content.to_string(), range)
    }

    /// Render code block element
    fn render_code_block(&self, content: &str, language: &Option<String>, raw_content: &str, range: (usize, usize)) -> RenderedElement {
        let lang_class = language
            .as_ref()
            .map(|l| format!(" language-{}", l))
            .unwrap_or_default();
        
        let html = format!(
            "<pre><code class=\"{}{}\">{}</code></pre>",
            self.css_class("code-block"),
            lang_class,
            html_escape(content)
        );
        
        let mut css_classes = vec![self.css_class("code-block")];
        if let Some(lang) = language {
            css_classes.push(format!("language-{}", lang));
        }

        let mut element = RenderedElement::new(html, css_classes, raw_content.to_string(), range);
        if let Some(lang) = language {
            element.add_data_attribute("language", lang);
        }
        element
    }

    /// Render link element
    fn render_link(&self, text: &str, url: &str, title: &Option<String>, raw_content: &str, range: (usize, usize)) -> RenderedElement {
        let title_attr = title
            .as_ref()
            .map(|t| format!(" title=\"{}\"", html_escape(t)))
            .unwrap_or_default();
        
        let html = format!(
            "<a href=\"{}\"{}>{}</a>",
            html_escape(url),
            title_attr,
            html_escape(text)
        );
        
        let css_classes = vec![self.css_class("link")];

        let mut element = RenderedElement::new(html, css_classes, raw_content.to_string(), range);
        element.add_data_attribute("url", url);
        if let Some(title) = title {
            element.add_data_attribute("title", title);
        }
        element
    }

    /// Render list item element
    fn render_list_item(&self, content: &str, level: u8, item_type: &str, number: Option<u32>, raw_content: &str, range: (usize, usize)) -> RenderedElement {
        let html = format!("<li>{}</li>", html_escape(content));
        let mut css_classes = vec![
            self.css_class("list-item"),
            self.css_class(&format!("list-item-{}", item_type)),
        ];
        
        if level > 0 {
            css_classes.push(self.css_class(&format!("level-{}", level)));
        }

        let mut element = RenderedElement::new(html, css_classes, raw_content.to_string(), range);
        element.add_data_attribute("level", &level.to_string());
        element.add_data_attribute("type", item_type);
        
        if let Some(num) = number {
            element.add_data_attribute("number", &num.to_string());
        }
        
        element
    }
}

impl InlineRenderer for MarkdownInlineRenderer {
    fn render_element(&self, element: &SyntaxElement) -> RenderedElement {
        let range = (element.range.start, element.range.end);
        
        match &element.element_type {
            SyntaxElementType::Header { level } => {
                self.render_header(*level, &element.rendered_content, &element.raw_content, range)
            }
            SyntaxElementType::Bold => {
                self.render_bold(&element.rendered_content, &element.raw_content, range)
            }
            SyntaxElementType::Italic => {
                self.render_italic(&element.rendered_content, &element.raw_content, range)
            }
            SyntaxElementType::InlineCode => {
                self.render_inline_code(&element.rendered_content, &element.raw_content, range)
            }
            SyntaxElementType::CodeBlock { language } => {
                self.render_code_block(&element.rendered_content, language, &element.raw_content, range)
            }
            SyntaxElementType::Link { url, title } => {
                self.render_link(&element.rendered_content, url, title, &element.raw_content, range)
            }
            SyntaxElementType::UnorderedListItem { level } => {
                self.render_list_item(&element.rendered_content, *level, "unordered", None, &element.raw_content, range)
            }
            SyntaxElementType::OrderedListItem { level, number } => {
                self.render_list_item(&element.rendered_content, *level, "ordered", Some(*number), &element.raw_content, range)
            }
        }
    }

    fn render_elements_with_cursor(
        &self,
        elements: &[SyntaxElement],
        cursor_position: &CursorPosition,
    ) -> Vec<RenderedElement> {
        elements
            .iter()
            .map(|element| {
                let mut rendered = self.render_element(element);
                
                // Check if cursor is within this element
                if self.is_cursor_in_element(element, cursor_position) {
                    rendered.set_editing(true);
                    rendered.add_class("cursor-active");
                }
                
                rendered
            })
            .collect()
    }

    fn extract_raw_content(&self, rendered: &RenderedElement) -> String {
        rendered.raw_content.clone()
    }

    fn is_cursor_in_element(&self, element: &SyntaxElement, cursor_position: &CursorPosition) -> bool {
        element.contains_cursor(cursor_position.absolute)
    }

    fn render_document(
        &self,
        content: &str,
        elements: &[SyntaxElement],
        cursor_position: &CursorPosition,
    ) -> String {
        if elements.is_empty() {
            return html_escape(content);
        }

        let mut result = String::new();
        let mut last_pos = 0;
        
        // Sort elements by position to ensure proper ordering
        let mut sorted_elements = elements.to_vec();
        sorted_elements.sort_by_key(|e| e.range.start);

        for element in &sorted_elements {
            // Add any content before this element
            if element.range.start > last_pos {
                let before_content = &content[last_pos..element.range.start];
                result.push_str(&html_escape(before_content));
            }

            // Render the element
            let mut rendered = self.render_element(element);
            
            // Check if cursor is within this element for editing
            if self.is_cursor_in_element(element, cursor_position) {
                rendered.set_editing(true);
            }

            result.push_str(&rendered.to_html());
            last_pos = element.range.end;
        }

        // Add any remaining content after the last element
        if last_pos < content.len() {
            let remaining_content = &content[last_pos..];
            result.push_str(&html_escape(remaining_content));
        }

        result
    }
}

/// Escape HTML special characters
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
    use crate::syntax_parser::{PositionRange, SyntaxElement, SyntaxElementType};

    #[test]
    fn test_render_header() {
        let renderer = MarkdownInlineRenderer::new();
        let element = SyntaxElement::new(
            SyntaxElementType::Header { level: 1 },
            PositionRange::new(0, 10),
            "# Header".to_string(),
            "Header".to_string(),
        );

        let rendered = renderer.render_element(&element);
        assert!(rendered.html.contains("<h1>Header</h1>"));
        assert!(rendered.css_classes.contains(&"md-header".to_string()));
        assert!(rendered.css_classes.contains(&"md-header-1".to_string()));
    }

    #[test]
    fn test_render_bold() {
        let renderer = MarkdownInlineRenderer::new();
        let element = SyntaxElement::new(
            SyntaxElementType::Bold,
            PositionRange::new(0, 8),
            "**bold**".to_string(),
            "bold".to_string(),
        );

        let rendered = renderer.render_element(&element);
        assert!(rendered.html.contains("<strong>bold</strong>"));
        assert!(rendered.css_classes.contains(&"md-bold".to_string()));
    }

    #[test]
    fn test_render_italic() {
        let renderer = MarkdownInlineRenderer::new();
        let element = SyntaxElement::new(
            SyntaxElementType::Italic,
            PositionRange::new(0, 8),
            "*italic*".to_string(),
            "italic".to_string(),
        );

        let rendered = renderer.render_element(&element);
        assert!(rendered.html.contains("<em>italic</em>"));
        assert!(rendered.css_classes.contains(&"md-italic".to_string()));
    }

    #[test]
    fn test_render_inline_code() {
        let renderer = MarkdownInlineRenderer::new();
        let element = SyntaxElement::new(
            SyntaxElementType::InlineCode,
            PositionRange::new(0, 6),
            "`code`".to_string(),
            "code".to_string(),
        );

        let rendered = renderer.render_element(&element);
        assert!(rendered.html.contains("<code>code</code>"));
        assert!(rendered.css_classes.contains(&"md-code".to_string()));
        assert!(rendered.css_classes.contains(&"md-inline-code".to_string()));
    }

    #[test]
    fn test_render_link() {
        let renderer = MarkdownInlineRenderer::new();
        let element = SyntaxElement::new(
            SyntaxElementType::Link {
                url: "https://example.com".to_string(),
                title: None,
            },
            PositionRange::new(0, 25),
            "[link](https://example.com)".to_string(),
            "link".to_string(),
        );

        let rendered = renderer.render_element(&element);
        assert!(rendered.html.contains("<a href=\"https://example.com\">link</a>"));
        assert!(rendered.css_classes.contains(&"md-link".to_string()));
    }

    #[test]
    fn test_cursor_awareness() {
        let renderer = MarkdownInlineRenderer::new();
        let element = SyntaxElement::new(
            SyntaxElementType::Bold,
            PositionRange::new(5, 13),
            "**bold**".to_string(),
            "bold".to_string(),
        );

        let cursor_inside = CursorPosition::new(0, 7, 7);
        let cursor_outside = CursorPosition::new(0, 2, 2);

        assert!(renderer.is_cursor_in_element(&element, &cursor_inside));
        assert!(!renderer.is_cursor_in_element(&element, &cursor_outside));
    }

    #[test]
    fn test_editing_mode() {
        let renderer = MarkdownInlineRenderer::new();
        let element = SyntaxElement::new(
            SyntaxElementType::Bold,
            PositionRange::new(0, 8),
            "**bold**".to_string(),
            "bold".to_string(),
        );

        let cursor_position = CursorPosition::new(0, 4, 4);
        let rendered_elements = renderer.render_elements_with_cursor(&[element], &cursor_position);

        assert_eq!(rendered_elements.len(), 1);
        assert!(rendered_elements[0].is_editing);
        assert!(rendered_elements[0].css_classes.contains(&"editing".to_string()));
        assert!(rendered_elements[0].css_classes.contains(&"cursor-active".to_string()));
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("&<>\"'"), "&amp;&lt;&gt;&quot;&#x27;");
    }

    #[test]
    fn test_extract_raw_content() {
        let renderer = MarkdownInlineRenderer::new();
        let rendered = RenderedElement::new(
            "<strong>bold</strong>".to_string(),
            vec!["md-bold".to_string()],
            "**bold**".to_string(),
            (0, 8),
        );

        assert_eq!(renderer.extract_raw_content(&rendered), "**bold**");
    }
}