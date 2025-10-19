//! AST renderers for converting AST to various output formats
//!
//! This module provides different renderers for converting AST trees to HTML,
//! including standard HTML and WYSIWYG-compatible DOM structures.

use crate::ast::{Node, NodeType, Tree};
use std::collections::HashMap;

/// Render options for controlling output format
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Enable syntax highlighting for code blocks
    pub syntax_highlighting: bool,

    /// Insert soft breaks as hard breaks
    pub soft_break_as_hard_break: bool,

    /// Insert spaces between Chinese and Western characters
    pub auto_space: bool,

    /// Enable WYSIWYG editing mode
    pub wysiwyg_mode: bool,

    /// Custom CSS classes
    pub css_classes: HashMap<NodeType, String>,

    /// Base URL for relative links
    pub base_url: Option<String>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            syntax_highlighting: true,
            soft_break_as_hard_break: true,
            auto_space: true,
            wysiwyg_mode: false,
            css_classes: HashMap::new(),
            base_url: None,
        }
    }
}

/// HTML renderer for converting AST to standard HTML
pub struct HtmlRenderer {
    options: RenderOptions,
    output: String,
}

impl HtmlRenderer {
    /// Create a new HTML renderer
    pub fn new(options: RenderOptions) -> Self {
        Self {
            options,
            output: String::new(),
        }
    }

    /// Render an AST tree to HTML
    pub fn render(&mut self, tree: &Tree) -> String {
        self.output.clear();
        self.render_node(&tree.root);
        self.output.clone()
    }

    /// Render a single node
    fn render_node(&mut self, node: &Node) {
        match node.node_type {
            NodeType::Document => {
                for child in &node.children {
                    self.render_node(child);
                }
            }

            NodeType::Paragraph => {
                self.output.push_str("<p");
                self.render_attributes(node);
                self.output.push('>');

                for child in &node.children {
                    self.render_node(child);
                }

                self.output.push_str("</p>\n");
            }

            NodeType::Heading => {
                let level = node.level.unwrap_or(1);
                self.output.push_str(&format!("<h{}", level));
                self.render_attributes(node);
                self.output.push('>');

                for child in &node.children {
                    self.render_node(child);
                }

                self.output.push_str(&format!("</h{}>\n", level));
            }

            NodeType::CodeBlock => {
                self.output.push_str("<pre><code");
                self.render_attributes(node);
                self.output.push('>');

                // Escape HTML in code content
                let content = node.text_content();
                self.output.push_str(&html_escape(&content));

                self.output.push_str("</code></pre>\n");
            }

            NodeType::Blockquote => {
                self.output.push_str("<blockquote");
                self.render_attributes(node);
                self.output.push_str(">\n");

                for child in &node.children {
                    self.render_node(child);
                }

                self.output.push_str("</blockquote>\n");
            }

            NodeType::List => {
                let ordered_type = "ordered".to_string();
                let tag = if node.get_attribute("type") == Some(&ordered_type) {
                    "ol"
                } else {
                    "ul"
                };

                self.output.push_str(&format!("<{}", tag));
                self.render_attributes(node);
                self.output.push_str(">\n");

                for child in &node.children {
                    self.render_node(child);
                }

                self.output.push_str(&format!("</{}>\n", tag));
            }

            NodeType::ListItem => {
                self.output.push_str("<li");
                self.render_attributes(node);
                self.output.push('>');

                // Handle task list items
                if let Some(checked) = node.get_attribute("checked") {
                    let checked_attr = if checked == "true" { " checked" } else { "" };
                    self.output.push_str(&format!(
                        r#"<input type="checkbox" disabled{} /> "#,
                        checked_attr
                    ));
                }

                for child in &node.children {
                    if child.node_type != NodeType::TaskListItemMarker {
                        self.render_node(child);
                    }
                }

                self.output.push_str("</li>\n");
            }

            NodeType::ThematicBreak => {
                self.output.push_str("<hr");
                self.render_attributes(node);
                self.output.push_str(" />\n");
            }

            NodeType::Text => {
                let text = node.text_content();
                self.output.push_str(&html_escape(&text));
            }

            NodeType::Strong => {
                self.output.push_str("<strong");
                self.render_attributes(node);
                self.output.push('>');

                for child in &node.children {
                    self.render_node(child);
                }

                self.output.push_str("</strong>");
            }

            NodeType::Emph => {
                self.output.push_str("<em");
                self.render_attributes(node);
                self.output.push('>');

                for child in &node.children {
                    self.render_node(child);
                }

                self.output.push_str("</em>");
            }

            NodeType::Code => {
                self.output.push_str("<code");
                self.render_attributes(node);
                self.output.push('>');

                let content = node.text_content();
                self.output.push_str(&html_escape(&content));

                self.output.push_str("</code>");
            }

            NodeType::Link => {
                self.output.push_str("<a");
                self.render_attributes(node);
                self.output.push('>');

                for child in &node.children {
                    self.render_node(child);
                }

                self.output.push_str("</a>");
            }

            NodeType::Image => {
                self.output.push_str("<img");
                self.render_attributes(node);
                self.output.push_str(" />");
            }

            NodeType::Strikethrough => {
                self.output.push_str("<del");
                self.render_attributes(node);
                self.output.push('>');

                for child in &node.children {
                    self.render_node(child);
                }

                self.output.push_str("</del>");
            }

            NodeType::SoftBreak => {
                if self.options.soft_break_as_hard_break {
                    self.output.push_str("<br />");
                } else {
                    self.output.push('\n');
                }
            }

            NodeType::LineBreak => {
                self.output.push_str("<br />\n");
            }

            // Handle other node types as needed
            _ => {
                for child in &node.children {
                    self.render_node(child);
                }
            }
        }
    }

    /// Render HTML attributes for a node
    fn render_attributes(&mut self, node: &Node) {
        // Add ID if present
        if !node.id.is_empty() && self.options.wysiwyg_mode {
            self.output
                .push_str(&format!(r#" data-node-id="{}""#, node.id));
        }

        // Add custom CSS classes
        if let Some(css_class) = self.options.css_classes.get(&node.node_type) {
            self.output.push_str(&format!(r#" class="{}""#, css_class));
        }

        // Add node attributes
        for (key, value) in &node.attributes {
            self.output
                .push_str(&format!(r#" {}="{}""#, key, html_escape(value)));
        }
    }
}

/// WYSIWYG renderer for creating editable DOM structures
#[allow(dead_code)]
pub struct WysiwygRenderer {
    options: RenderOptions,
    output: String,
}

impl WysiwygRenderer {
    /// Create a new WYSIWYG renderer
    pub fn new(options: RenderOptions) -> Self {
        let mut options = options;
        options.wysiwyg_mode = true;

        Self {
            options,
            output: String::new(),
        }
    }

    /// Render an AST tree to WYSIWYG DOM
    pub fn render(&mut self, tree: &Tree) -> String {
        self.output.clear();
        self.render_node(&tree.root);
        self.output.clone()
    }

    /// Render a single node with WYSIWYG enhancements
    fn render_node(&mut self, node: &Node) {
        match node.node_type {
            NodeType::Document => {
                // Wrap in contenteditable container
                self.output
                    .push_str(r#"<div class="vditor-wysiwyg" contenteditable="true">"#);

                for child in &node.children {
                    self.render_node(child);
                }

                self.output.push_str("</div>");
            }

            NodeType::Paragraph => {
                self.output.push_str(r#"<div data-type="p""#);
                self.render_wysiwyg_attributes(node);
                self.output.push('>');

                for child in &node.children {
                    self.render_node(child);
                }

                self.output.push_str("</div>");
            }

            NodeType::Heading => {
                let level = node.level.unwrap_or(1);
                self.output.push_str(&format!(
                    r#"<div data-type="h{}" data-level="{}""#,
                    level, level
                ));
                self.render_wysiwyg_attributes(node);
                self.output.push('>');

                for child in &node.children {
                    self.render_node(child);
                }

                self.output.push_str("</div>");
            }

            NodeType::CodeBlock => {
                let language = node.data.as_str();
                self.output
                    .push_str(r#"<div class="vditor-wysiwyg__block" data-type="code-block""#);
                if !language.is_empty() {
                    self.output
                        .push_str(&format!(r#" data-language="{}""#, language));
                }
                self.render_wysiwyg_attributes(node);
                self.output
                    .push_str(r#"><pre class="vditor-wysiwyg__pre"><code>"#);

                let content = node.text_content();
                self.output.push_str(&html_escape(&content));

                self.output.push_str("</code></pre></div>");
            }

            NodeType::Blockquote => {
                self.output
                    .push_str(r#"<blockquote data-type="blockquote""#);
                self.render_wysiwyg_attributes(node);
                self.output.push('>');

                for child in &node.children {
                    self.render_node(child);
                }

                self.output.push_str("</blockquote>");
            }

            NodeType::List => {
                let default_type = "unordered".to_string();
                let list_type = node.get_attribute("type").unwrap_or(&default_type);
                let tag = if list_type == "ordered" { "ol" } else { "ul" };

                self.output
                    .push_str(&format!(r#"<{} data-type="{}""#, tag, list_type));
                self.render_wysiwyg_attributes(node);
                self.output.push('>');

                for child in &node.children {
                    self.render_node(child);
                }

                self.output.push_str(&format!("</{}>\n", tag));
            }

            NodeType::ListItem => {
                self.output.push_str(r#"<li data-type="li""#);

                // Handle task list items
                if let Some(checked) = node.get_attribute("checked") {
                    self.output.push_str(r#" data-task="true""#);
                    self.output
                        .push_str(&format!(r#" data-checked="{}""#, checked));
                }

                self.render_wysiwyg_attributes(node);
                self.output.push('>');

                // Add task checkbox for task items
                if let Some(checked) = node.get_attribute("checked") {
                    let checked_attr = if checked == "true" { " checked" } else { "" };
                    self.output.push_str(&format!(
                        r#"<input type="checkbox" data-type="task-marker"{} /> "#,
                        checked_attr
                    ));
                }

                for child in &node.children {
                    if child.node_type != NodeType::TaskListItemMarker {
                        self.render_node(child);
                    }
                }

                self.output.push_str("</li>");
            }

            NodeType::Text => {
                let text = node.text_content();
                self.output.push_str(&html_escape(&text));
            }

            NodeType::Strong => {
                self.output.push_str(r#"<strong data-type="strong""#);
                self.render_wysiwyg_attributes(node);
                self.output.push('>');

                for child in &node.children {
                    self.render_node(child);
                }

                self.output.push_str("</strong>");
            }

            NodeType::Emph => {
                self.output.push_str(r#"<em data-type="em""#);
                self.render_wysiwyg_attributes(node);
                self.output.push('>');

                for child in &node.children {
                    self.render_node(child);
                }

                self.output.push_str("</em>");
            }

            NodeType::Code => {
                self.output.push_str(r#"<code data-type="code""#);
                self.render_wysiwyg_attributes(node);
                self.output.push('>');

                let content = node.text_content();
                self.output.push_str(&html_escape(&content));

                self.output.push_str("</code>");
            }

            NodeType::Link => {
                self.output.push_str(r#"<a data-type="link""#);
                self.render_wysiwyg_attributes(node);
                self.output.push('>');

                for child in &node.children {
                    self.render_node(child);
                }

                self.output.push_str("</a>");
            }

            NodeType::Image => {
                self.output.push_str(r#"<img data-type="img""#);
                self.render_wysiwyg_attributes(node);
                self.output.push_str(" />");
            }

            NodeType::Strikethrough => {
                self.output.push_str(r#"<s data-type="s""#);
                self.render_wysiwyg_attributes(node);
                self.output.push('>');

                for child in &node.children {
                    self.render_node(child);
                }

                self.output.push_str("</s>");
            }

            // Handle other node types
            _ => {
                for child in &node.children {
                    self.render_node(child);
                }
            }
        }
    }

    /// Render WYSIWYG-specific attributes
    fn render_wysiwyg_attributes(&mut self, node: &Node) {
        // Always add node ID for WYSIWYG tracking
        self.output
            .push_str(&format!(r#" data-node-id="{}""#, node.id));

        // Add node attributes
        for (key, value) in &node.attributes {
            self.output
                .push_str(&format!(r#" {}="{}""#, key, html_escape(value)));
        }

        // Add Kramdown IAL attributes
        for (key, value) in &node.kramdown_ial {
            self.output
                .push_str(&format!(r#" data-{}="{}""#, key, html_escape(value)));
        }
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

/// Convenience function to render AST to HTML
pub fn render_html(tree: &Tree) -> String {
    let mut renderer = HtmlRenderer::new(RenderOptions::default());
    renderer.render(tree)
}

/// Convenience function to render AST to WYSIWYG DOM
pub fn render_wysiwyg(tree: &Tree) -> String {
    let mut renderer = WysiwygRenderer::new(RenderOptions::default());
    renderer.render(tree)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::MarkdownParser;

    #[test]
    fn test_render_paragraph() {
        let parser = MarkdownParser::new();
        let tree = parser.parse("Hello world!");
        let html = render_html(&tree);

        assert!(html.contains("<p>Hello world!</p>"));
    }

    #[test]
    fn test_render_heading() {
        let parser = MarkdownParser::new();
        let tree = parser.parse("# Heading 1");
        let html = render_html(&tree);

        assert!(html.contains("<h1>Heading 1</h1>"));
    }

    #[test]
    fn test_render_code_block() {
        let parser = MarkdownParser::new();
        let tree = parser.parse("```rust\nfn main() {}\n```");
        let html = render_html(&tree);

        assert!(html.contains("<pre><code"));
        assert!(html.contains("language-rust"));
        assert!(html.contains("fn main() {}"));
    }

    #[test]
    fn test_render_list() {
        let parser = MarkdownParser::new();
        let tree = parser.parse("- Item 1\n- Item 2");
        let html = render_html(&tree);

        assert!(html.contains("<ul"));
        assert!(html.contains("<li>Item 1</li>"));
        assert!(html.contains("<li>Item 2</li>"));
    }

    #[test]
    fn test_render_task_list() {
        let parser = MarkdownParser::new();
        let tree = parser.parse("- [ ] Todo\n- [x] Done");
        let html = render_html(&tree);

        assert!(html.contains(r#"<input type="checkbox" disabled />"#));
        assert!(html.contains(r#"<input type="checkbox" disabled checked />"#));
    }

    #[test]
    fn test_render_inline_elements() {
        let parser = MarkdownParser::new();
        let tree = parser.parse("This is **bold** and *italic* text.");
        let html = render_html(&tree);

        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("<em>italic</em>"));
    }

    #[test]
    fn test_render_wysiwyg() {
        let parser = MarkdownParser::new();
        let tree = parser.parse("# Heading\n\nParagraph text.");
        let wysiwyg = render_wysiwyg(&tree);

        assert!(wysiwyg.contains(r#"<div class="vditor-wysiwyg" contenteditable="true">"#));
        assert!(wysiwyg.contains(r#"data-type="h1""#));
        assert!(wysiwyg.contains(r#"data-type="p""#));
        assert!(wysiwyg.contains(r#"data-node-id"#));
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("A & B"), "A &amp; B");
        assert_eq!(html_escape(r#""quoted""#), "&quot;quoted&quot;");
    }
}
