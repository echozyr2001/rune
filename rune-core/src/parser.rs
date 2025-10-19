//! Markdown parser for converting Markdown text to AST
//!
//! This module provides a simplified Markdown parser that converts Markdown text
//! into an AST structure, supporting basic GFM features.

use crate::ast::{Node, NodeType, ParseOptions, Tree};
use regex::Regex;

/// Markdown parser for converting text to AST
pub struct MarkdownParser {
    options: ParseOptions,
}

impl MarkdownParser {
    /// Create a new parser with default options
    pub fn new() -> Self {
        Self {
            options: ParseOptions::default(),
        }
    }

    /// Create a new parser with custom options
    pub fn with_options(options: ParseOptions) -> Self {
        Self { options }
    }

    /// Parse markdown text into an AST tree
    pub fn parse(&self, markdown: &str) -> Tree {
        let mut tree = Tree::with_options(self.options.clone());
        let lines = markdown.lines().collect::<Vec<_>>();

        let mut line_index = 0;
        while line_index < lines.len() {
            line_index += self.parse_block(&mut tree.root, &lines, line_index);
        }

        tree
    }

    /// Parse a block element starting at the given line index
    /// Returns the number of lines consumed
    fn parse_block(&self, parent: &mut Node, lines: &[&str], start_index: usize) -> usize {
        if start_index >= lines.len() {
            return 1;
        }

        let line = lines[start_index].trim_end();

        // Empty line
        if line.is_empty() {
            return 1;
        }

        // Heading
        if let Some(heading) = self.parse_heading(line) {
            parent.append_child(heading);
            return 1;
        }

        // Code block
        if line.starts_with("```") {
            return self.parse_code_block(parent, lines, start_index);
        }

        // Blockquote
        if line.starts_with("> ") {
            return self.parse_blockquote(parent, lines, start_index);
        }

        // List
        if self.is_list_item(line) {
            return self.parse_list(parent, lines, start_index);
        }

        // Thematic break
        if self.is_thematic_break(line) {
            let mut hr = Node::new(NodeType::ThematicBreak);
            hr.tokens = line.as_bytes().to_vec();
            parent.append_child(hr);
            return 1;
        }

        // Default: paragraph
        self.parse_paragraph(parent, lines, start_index)
    }

    /// Parse heading (ATX style)
    fn parse_heading(&self, line: &str) -> Option<Node> {
        let re = Regex::new(r"^(#{1,6})\s+(.+)$").unwrap();
        if let Some(captures) = re.captures(line) {
            let level = captures.get(1).unwrap().as_str().len();
            let text = captures.get(2).unwrap().as_str();

            let mut heading = Node::new(NodeType::Heading);
            heading.level = Some(level);
            heading.tokens = line.as_bytes().to_vec();

            // Parse inline content
            self.parse_inline_content(&mut heading, text);

            Some(heading)
        } else {
            None
        }
    }

    /// Parse code block
    fn parse_code_block(&self, parent: &mut Node, lines: &[&str], start_index: usize) -> usize {
        let first_line = lines[start_index];
        let language = first_line.strip_prefix("```").unwrap_or("").trim();

        let mut code_block = Node::new(NodeType::CodeBlock);
        if !language.is_empty() {
            code_block.set_attribute("class", &format!("language-{}", language));
            code_block.data = language.to_string();
        }

        let mut content_lines = Vec::new();
        let mut line_index = start_index + 1;

        while line_index < lines.len() {
            let line = lines[line_index];
            if line.trim() == "```" {
                break;
            }
            content_lines.push(line);
            line_index += 1;
        }

        let content = content_lines.join("\n");
        code_block.tokens = content.as_bytes().to_vec();
        code_block.append_child(Node::text(&content));

        parent.append_child(code_block);
        line_index - start_index + 1
    }

    /// Parse blockquote
    fn parse_blockquote(&self, parent: &mut Node, lines: &[&str], start_index: usize) -> usize {
        let mut blockquote = Node::new(NodeType::Blockquote);
        let mut quote_lines = Vec::new();
        let mut line_index = start_index;

        while line_index < lines.len() {
            let line = lines[line_index];
            if line.starts_with("> ") {
                quote_lines.push(line.strip_prefix("> ").unwrap());
                line_index += 1;
            } else if line.trim().is_empty()
                && line_index + 1 < lines.len()
                && lines[line_index + 1].starts_with("> ")
            {
                quote_lines.push("");
                line_index += 1;
            } else {
                break;
            }
        }

        // Parse the blockquote content as markdown
        let quote_content = quote_lines.join("\n");
        let sub_parser = MarkdownParser::with_options(self.options.clone());
        let quote_tree = sub_parser.parse(&quote_content);

        for child in quote_tree.root.children {
            blockquote.append_child(child);
        }

        parent.append_child(blockquote);
        line_index - start_index
    }

    /// Check if line is a list item
    fn is_list_item(&self, line: &str) -> bool {
        let trimmed = line.trim_start();
        // Unordered list
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
            return true;
        }
        // Ordered list
        let re = Regex::new(r"^\d+\.\s").unwrap();
        re.is_match(trimmed)
    }

    /// Parse list
    fn parse_list(&self, parent: &mut Node, lines: &[&str], start_index: usize) -> usize {
        let mut list = Node::new(NodeType::List);
        let first_line = lines[start_index].trim_start();

        // Determine list type
        if first_line.starts_with("- ")
            || first_line.starts_with("* ")
            || first_line.starts_with("+ ")
        {
            list.set_attribute("type", "unordered");
        } else {
            list.set_attribute("type", "ordered");
        }

        // Compile ordered list regex once per parse_list call to avoid creating it inside the loop
        let ordered_re = Regex::new(r"^\s*\d+\.\s").unwrap();

        let mut line_index = start_index;

        while line_index < lines.len() {
            let line = lines[line_index];
            if !self.is_list_item(line) {
                if line.trim().is_empty() {
                    line_index += 1;
                    continue;
                } else {
                    break;
                }
            }

            let mut list_item = Node::new(NodeType::ListItem);
            let content = if line.trim_start().starts_with("- ")
                || line.trim_start().starts_with("* ")
                || line.trim_start().starts_with("+ ")
            {
                line.trim_start()
                    .strip_prefix(&line.trim_start().chars().next().unwrap().to_string())
                    .unwrap()
                    .strip_prefix(" ")
                    .unwrap()
                    .to_string()
            } else {
                // Ordered list
                ordered_re.replace(line, "").to_string()
            };

            // Check for task list item
            if self.options.task_lists && content.starts_with("[ ] ") {
                list_item.set_attribute("type", "task");
                list_item.set_attribute("checked", "false");
                let task_marker = Node::new(NodeType::TaskListItemMarker);
                list_item.append_child(task_marker);
                self.parse_inline_content(&mut list_item, content.strip_prefix("[ ] ").unwrap());
            } else if self.options.task_lists && content.starts_with("[x] ") {
                list_item.set_attribute("type", "task");
                list_item.set_attribute("checked", "true");
                let task_marker = Node::new(NodeType::TaskListItemMarker);
                list_item.append_child(task_marker);
                self.parse_inline_content(&mut list_item, content.strip_prefix("[x] ").unwrap());
            } else {
                self.parse_inline_content(&mut list_item, &content);
            }

            list.append_child(list_item);
            line_index += 1;
        }

        parent.append_child(list);
        line_index - start_index
    }

    /// Check if line is a thematic break
    fn is_thematic_break(&self, line: &str) -> bool {
        let trimmed = line.trim();
        if trimmed.len() < 3 {
            return false;
        }

        // Check for --- or *** or ___
        trimmed.chars().all(|c| c == '-') && trimmed.len() >= 3
            || trimmed.chars().all(|c| c == '*') && trimmed.len() >= 3
            || trimmed.chars().all(|c| c == '_') && trimmed.len() >= 3
    }

    /// Parse paragraph
    fn parse_paragraph(&self, parent: &mut Node, lines: &[&str], start_index: usize) -> usize {
        let mut paragraph = Node::new(NodeType::Paragraph);
        let mut paragraph_lines = Vec::new();
        let mut line_index = start_index;

        while line_index < lines.len() {
            let line = lines[line_index];
            if line.trim().is_empty() {
                break;
            }
            if line_index > start_index && self.starts_block(line) {
                break;
            }

            paragraph_lines.push(line);
            line_index += 1;
        }

        let content = paragraph_lines.join(" ");
        paragraph.tokens = content.as_bytes().to_vec();
        self.parse_inline_content(&mut paragraph, &content);

        parent.append_child(paragraph);
        line_index - start_index
    }

    /// Check if line starts a new block element
    fn starts_block(&self, line: &str) -> bool {
        let trimmed = line.trim();

        // Heading
        if trimmed.starts_with('#') {
            return true;
        }

        // Code block
        if trimmed.starts_with("```") {
            return true;
        }

        // Blockquote
        if trimmed.starts_with("> ") {
            return true;
        }

        // List
        if self.is_list_item(line) {
            return true;
        }

        // Thematic break
        if self.is_thematic_break(line) {
            return true;
        }

        false
    }

    /// Parse inline content (bold, italic, links, etc.)
    fn parse_inline_content(&self, parent: &mut Node, text: &str) {
        let mut remaining = text;

        while !remaining.is_empty() {
            // Try to find inline elements
            if let Some((element, consumed)) = self.parse_next_inline(remaining) {
                parent.append_child(element);
                remaining = &remaining[consumed..];
            } else {
                // No inline element found, take one character as text
                let ch = remaining.chars().next().unwrap();
                let text_node = Node::text(&ch.to_string());
                parent.append_child(text_node);
                remaining = &remaining[ch.len_utf8()..];
            }
        }
    }

    /// Parse the next inline element from the text
    fn parse_next_inline(&self, text: &str) -> Option<(Node, usize)> {
        // Bold (**text** or __text__)
        if let Some((content, end)) = self.find_delimiter_pair(text, "**") {
            let mut bold = Node::new(NodeType::Strong);
            self.parse_inline_content(&mut bold, &content);
            return Some((bold, end));
        }

        if let Some((content, end)) = self.find_delimiter_pair(text, "__") {
            let mut bold = Node::new(NodeType::Strong);
            self.parse_inline_content(&mut bold, &content);
            return Some((bold, end));
        }

        // Italic (*text* or _text_)
        if let Some((content, end)) = self.find_delimiter_pair(text, "*") {
            let mut italic = Node::new(NodeType::Emph);
            self.parse_inline_content(&mut italic, &content);
            return Some((italic, end));
        }

        if let Some((content, end)) = self.find_delimiter_pair(text, "_") {
            let mut italic = Node::new(NodeType::Emph);
            self.parse_inline_content(&mut italic, &content);
            return Some((italic, end));
        }

        // Strikethrough (~~text~~)
        if self.options.strikethrough {
            if let Some((content, end)) = self.find_delimiter_pair(text, "~~") {
                let mut strike = Node::new(NodeType::Strikethrough);
                self.parse_inline_content(&mut strike, &content);
                return Some((strike, end));
            }
        }

        // Inline code (`code`)
        if let Some((content, end)) = self.find_delimiter_pair(text, "`") {
            let mut code = Node::new(NodeType::Code);
            code.append_child(Node::text(&content));
            return Some((code, end));
        }

        // Links [text](url)
        if let Some((link_node, consumed)) = self.parse_link(text) {
            return Some((link_node, consumed));
        }

        // Images ![alt](url)
        if let Some((img_node, consumed)) = self.parse_image(text) {
            return Some((img_node, consumed));
        }

        None
    }

    /// Find delimiter pair (e.g., **bold**, *italic*)
    fn find_delimiter_pair(&self, text: &str, delimiter: &str) -> Option<(String, usize)> {
        if !text.starts_with(delimiter) {
            return None;
        }

        let content_start = delimiter.len();
        if let Some(end_pos) = text[content_start..].find(delimiter) {
            let content = text[content_start..content_start + end_pos].to_string();
            let total_consumed = content_start + end_pos + delimiter.len();
            Some((content, total_consumed))
        } else {
            None
        }
    }

    /// Parse link [text](url)
    fn parse_link(&self, text: &str) -> Option<(Node, usize)> {
        if !text.starts_with('[') {
            return None;
        }

        let re = Regex::new(r"^\[([^\]]*)\]\(([^)]*)\)").unwrap();
        if let Some(captures) = re.captures(text) {
            let full_match = captures.get(0).unwrap().as_str();
            let link_text = captures.get(1).unwrap().as_str();
            let url = captures.get(2).unwrap().as_str();

            let mut link = Node::new(NodeType::Link);
            link.set_attribute("href", url);
            self.parse_inline_content(&mut link, link_text);

            Some((link, full_match.len()))
        } else {
            None
        }
    }

    /// Parse image ![alt](url)
    fn parse_image(&self, text: &str) -> Option<(Node, usize)> {
        if !text.starts_with("![") {
            return None;
        }

        let re = Regex::new(r"^!\[([^\]]*)\]\(([^)]*)\)").unwrap();
        if let Some(captures) = re.captures(text) {
            let full_match = captures.get(0).unwrap().as_str();
            let alt_text = captures.get(1).unwrap().as_str();
            let url = captures.get(2).unwrap().as_str();

            let mut image = Node::new(NodeType::Image);
            image.set_attribute("src", url);
            image.set_attribute("alt", alt_text);

            Some((image, full_match.len()))
        } else {
            None
        }
    }
}

impl Default for MarkdownParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_heading() {
        let parser = MarkdownParser::new();
        let tree = parser.parse("# Heading 1\n## Heading 2");

        assert_eq!(tree.root.children.len(), 2);
        assert_eq!(tree.root.children[0].node_type, NodeType::Heading);
        assert_eq!(tree.root.children[0].level, Some(1));
        assert_eq!(tree.root.children[1].level, Some(2));
    }

    #[test]
    fn test_parse_paragraph() {
        let parser = MarkdownParser::new();
        let tree = parser.parse("This is a paragraph.");

        assert_eq!(tree.root.children.len(), 1);
        assert_eq!(tree.root.children[0].node_type, NodeType::Paragraph);
    }

    #[test]
    fn test_parse_code_block() {
        let parser = MarkdownParser::new();
        let tree = parser.parse("```rust\nfn main() {}\n```");

        assert_eq!(tree.root.children.len(), 1);
        assert_eq!(tree.root.children[0].node_type, NodeType::CodeBlock);
        assert_eq!(
            tree.root.children[0].get_attribute("class"),
            Some(&"language-rust".to_string())
        );
    }

    #[test]
    fn test_parse_list() {
        let parser = MarkdownParser::new();
        let tree = parser.parse("- Item 1\n- Item 2");

        assert_eq!(tree.root.children.len(), 1);
        assert_eq!(tree.root.children[0].node_type, NodeType::List);
        assert_eq!(tree.root.children[0].children.len(), 2);
    }

    #[test]
    fn test_parse_task_list() {
        let parser = MarkdownParser::new();
        let tree = parser.parse("- [ ] Todo item\n- [x] Done item");

        assert_eq!(tree.root.children.len(), 1);
        assert_eq!(tree.root.children[0].node_type, NodeType::List);
        assert_eq!(
            tree.root.children[0].children[0].get_attribute("checked"),
            Some(&"false".to_string())
        );
        assert_eq!(
            tree.root.children[0].children[1].get_attribute("checked"),
            Some(&"true".to_string())
        );
    }

    #[test]
    fn test_parse_inline_bold() {
        let parser = MarkdownParser::new();
        let tree = parser.parse("This is **bold** text.");

        let paragraph = &tree.root.children[0];
        assert_eq!(paragraph.node_type, NodeType::Paragraph);

        // Should have multiple inline nodes
        assert!(paragraph.children.len() > 1);

        // Find the bold node
        let bold_node = paragraph
            .children
            .iter()
            .find(|n| n.node_type == NodeType::Strong);
        assert!(bold_node.is_some());
    }

    #[test]
    fn test_parse_link() {
        let parser = MarkdownParser::new();
        let tree = parser.parse("Visit [Google](https://google.com) now.");

        let paragraph = &tree.root.children[0];
        let link_node = paragraph
            .children
            .iter()
            .find(|n| n.node_type == NodeType::Link);
        assert!(link_node.is_some());
        assert_eq!(
            link_node.unwrap().get_attribute("href"),
            Some(&"https://google.com".to_string())
        );
    }
}
