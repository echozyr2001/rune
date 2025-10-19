//! Rune Quill - Markdown text processing engine
//!
//! This module provides the Rune Quill for converting markdown
//! to various output formats, including HTML and WYSIWYG DOM.

use crate::parser::MarkdownParser;
use crate::render::{render_html, render_wysiwyg, RenderOptions};
use regex::Regex;

/// The Rune Quill - A text processing engine for markdown
///
/// The Quill converts markdown text into various output formats
/// with support for both standard HTML and WYSIWYG editing.
#[allow(dead_code)]
pub struct Quill {
    parse_options: crate::ast::ParseOptions,
    render_options: RenderOptions,
}

impl Quill {
    /// Create a new Quill with default options
    pub fn new() -> Self {
        Self {
            parse_options: crate::ast::ParseOptions::default(),
            render_options: RenderOptions::default(),
        }
    }

    /// Create a new Quill with custom options
    pub fn with_options(
        parse_options: crate::ast::ParseOptions,
        render_options: RenderOptions,
    ) -> Self {
        Self {
            parse_options,
            render_options,
        }
    }

    /// Convert markdown to HTML
    pub fn markdown_to_html(&self, markdown: &str) -> String {
        let parser = MarkdownParser::with_options(self.parse_options.clone());
        let tree = parser.parse(markdown);
        render_html(&tree)
    }

    /// Convert markdown to WYSIWYG DOM
    pub fn markdown_to_wysiwyg(&self, markdown: &str) -> String {
        let parser = MarkdownParser::with_options(self.parse_options.clone());
        let tree = parser.parse(markdown);
        render_wysiwyg(&tree)
    }

    /// SpinDOM functionality: HTML -> Markdown -> AST -> WYSIWYG DOM
    /// This is the core function that transforms DOM structures through markdown
    pub fn spin_wysiwyg_dom(&self, html: &str) -> String {
        // Step 1: Convert HTML to Markdown
        let markdown = self.html_to_markdown(html);

        // Step 2: Parse Markdown to AST
        let parser = MarkdownParser::with_options(self.parse_options.clone());
        let tree = parser.parse(&markdown);

        // Step 3: Render AST to WYSIWYG DOM
        render_wysiwyg(&tree)
    }

    /// Convert HTML to Markdown
    pub fn html_to_markdown(&self, html: &str) -> String {
        let mut markdown = html.to_string();

        // Convert HTML tags to Markdown FIRST (before removing data attributes)
        markdown = self.convert_html_to_markdown(&markdown);

        // Then remove WYSIWYG-specific data attributes that might remain
        markdown = self.clean_wysiwyg_attributes(&markdown);

        // Clean up extra whitespace
        markdown = self.normalize_whitespace(&markdown);

        markdown
    }

    /// Clean WYSIWYG-specific attributes from HTML
    fn clean_wysiwyg_attributes(&self, html: &str) -> String {
        let mut cleaned = html.to_string();

        // Remove data-node-id attributes
        let re = Regex::new(r#"\s*data-node-id="[^"]*""#).unwrap();
        cleaned = re.replace_all(&cleaned, "").to_string();

        // Remove data-type attributes
        let re = Regex::new(r#"\s*data-type="[^"]*""#).unwrap();
        cleaned = re.replace_all(&cleaned, "").to_string();

        // Remove data-level attributes
        let re = Regex::new(r#"\s*data-level="[^"]*""#).unwrap();
        cleaned = re.replace_all(&cleaned, "").to_string();

        // Remove contenteditable attributes
        let re = Regex::new(r#"\s*contenteditable="[^"]*""#).unwrap();
        cleaned = re.replace_all(&cleaned, "").to_string();

        // Remove vditor-wysiwyg classes
        let re = Regex::new(r#"\s*class="[^"]*vditor-wysiwyg[^"]*""#).unwrap();
        cleaned = re.replace_all(&cleaned, "").to_string();

        cleaned
    }

    /// Convert HTML tags to Markdown syntax
    fn convert_html_to_markdown(&self, html: &str) -> String {
        let mut markdown = html.to_string();

        // Convert headings
        for level in 1..=6 {
            let pattern = format!(r#"<h{0}[^>]*>(.*?)</h{0}>"#, level);
            let re = Regex::new(&pattern).unwrap();
            let replacement = format!("{} $1\n\n", "#".repeat(level));
            markdown = re.replace_all(&markdown, &replacement).to_string();

            // Handle div-based headings (WYSIWYG format)
            let pattern = format!(r#"<div[^>]*data-type="h{0}"[^>]*>(.*?)</div>"#, level);
            let re = Regex::new(&pattern).unwrap();
            let replacement = format!("{} $1\n\n", "#".repeat(level));
            markdown = re.replace_all(&markdown, &replacement).to_string();
        }

        // Convert paragraphs
        let re = Regex::new(r#"<p[^>]*>(.*?)</p>"#).unwrap();
        markdown = re.replace_all(&markdown, "$1\n\n").to_string();

        // Handle div-based paragraphs (WYSIWYG format)
        let re = Regex::new(r#"<div[^>]*data-type="p"[^>]*>(.*?)</div>"#).unwrap();
        markdown = re.replace_all(&markdown, "$1\n\n").to_string();

        // Convert strong/bold
        let re = Regex::new(r#"<strong[^>]*>(.*?)</strong>"#).unwrap();
        markdown = re.replace_all(&markdown, "**$1**").to_string();

        // Convert em/italic
        let re = Regex::new(r#"<em[^>]*>(.*?)</em>"#).unwrap();
        markdown = re.replace_all(&markdown, "*$1*").to_string();

        // Convert code blocks FIRST (before inline code)
        let re =
            Regex::new(r#"<pre[^>]*><code[^>]*class="language-([^"]*)"[^>]*>(.*?)</code></pre>"#)
                .unwrap();
        markdown = re.replace_all(&markdown, "```$1\n$2\n```").to_string();

        let re = Regex::new(r#"<pre[^>]*><code[^>]*>(.*?)</code></pre>"#).unwrap();
        markdown = re.replace_all(&markdown, "```\n$1\n```").to_string();

        // Handle WYSIWYG code blocks
        let re = Regex::new(r#"<div[^>]*data-type="code-block"[^>]*data-language="([^"]*)"[^>]*>.*?<code[^>]*>(.*?)</code>.*?</div>"#).unwrap();
        markdown = re.replace_all(&markdown, "```$1\n$2\n```").to_string();

        // Convert inline code (after code blocks)
        let re = Regex::new(r#"<code[^>]*>(.*?)</code>"#).unwrap();
        markdown = re.replace_all(&markdown, "`$1`").to_string();

        // Convert strikethrough
        let re = Regex::new(r#"<s[^>]*>(.*?)</s>"#).unwrap();
        markdown = re.replace_all(&markdown, "~~$1~~").to_string();
        let re = Regex::new(r#"<del[^>]*>(.*?)</del>"#).unwrap();
        markdown = re.replace_all(&markdown, "~~$1~~").to_string();

        // Convert links
        let re = Regex::new(r#"<a[^>]*href="([^"]*)"[^>]*>(.*?)</a>"#).unwrap();
        markdown = re.replace_all(&markdown, "[$2]($1)").to_string();

        // Convert images
        let re = Regex::new(r#"<img[^>]*src="([^"]*)"[^>]*alt="([^"]*)"[^>]*/?>"#).unwrap();
        markdown = re.replace_all(&markdown, "![$2]($1)").to_string();
        let re = Regex::new(r#"<img[^>]*alt="([^"]*)"[^>]*src="([^"]*)"[^>]*/?>"#).unwrap();
        markdown = re.replace_all(&markdown, "![$1]($2)").to_string();

        // Convert blockquotes
        let re = Regex::new(r#"<blockquote[^>]*>(.*?)</blockquote>"#).unwrap();
        markdown = re
            .replace_all(&markdown, |caps: &regex::Captures| {
                let content = &caps[1];
                let lines: Vec<&str> = content.lines().collect();
                let quoted_lines: Vec<String> = lines
                    .iter()
                    .map(|line| format!("> {}", line.trim()))
                    .collect();
                quoted_lines.join("\n")
            })
            .to_string();

        // Convert unordered lists
        let re = Regex::new(r#"<ul[^>]*>(.*?)</ul>"#).unwrap();
        markdown = re
            .replace_all(&markdown, |caps: &regex::Captures| {
                let content = &caps[1];
                self.convert_list_items(content, "- ")
            })
            .to_string();

        // Convert ordered lists
        let re = Regex::new(r#"<ol[^>]*>(.*?)</ol>"#).unwrap();
        markdown = re
            .replace_all(&markdown, |caps: &regex::Captures| {
                let content = &caps[1];
                self.convert_ordered_list_items(content)
            })
            .to_string();

        // Convert horizontal rules
        let re = Regex::new(r#"<hr[^>]*/?>"#).unwrap();
        markdown = re.replace_all(&markdown, "\n---\n").to_string();

        // Remove remaining HTML tags
        let re = Regex::new(r#"<[^>]*>"#).unwrap();
        markdown = re.replace_all(&markdown, "").to_string();

        markdown
    }

    /// Convert list items from HTML to Markdown
    fn convert_list_items(&self, html: &str, prefix: &str) -> String {
        let re = Regex::new(r#"<li[^>]*>(.*?)</li>"#).unwrap();
        let checkbox_re =
            Regex::new(r#"<input[^>]*type="checkbox"[^>]*(?:checked[^>]*)?/?>\s*"#).unwrap();
        let mut result = String::new();

        for caps in re.captures_iter(html) {
            let content = &caps[1];

            // Check for task list items
            if content.contains(r#"type="checkbox""#) {
                let is_checked = content.contains("checked");

                let clean_content = checkbox_re.replace(content, "").to_string();
                let task_status = if is_checked { "[x]" } else { "[ ]" };
                result.push_str(&format!("- {} {}\n", task_status, clean_content.trim()));
            } else {
                result.push_str(&format!("{}{}\n", prefix, content.trim()));
            }
        }

        result
    }

    /// Convert ordered list items from HTML to Markdown
    fn convert_ordered_list_items(&self, html: &str) -> String {
        let re = Regex::new(r#"<li[^>]*>(.*?)</li>"#).unwrap();
        let mut result = String::new();
        let mut index = 1;

        for caps in re.captures_iter(html) {
            let content = &caps[1];
            result.push_str(&format!("{}. {}\n", index, content.trim()));
            index += 1;
        }

        result
    }

    /// Normalize whitespace in markdown
    fn normalize_whitespace(&self, markdown: &str) -> String {
        let mut normalized = markdown.to_string();

        // Remove excessive newlines
        let re = Regex::new(r"\n\s*\n\s*\n").unwrap();
        normalized = re.replace_all(&normalized, "\n\n").to_string();

        // Trim leading and trailing whitespace
        normalized = normalized.trim().to_string();

        // Ensure proper line endings
        if !normalized.ends_with('\n') && !normalized.is_empty() {
            normalized.push('\n');
        }

        normalized
    }

    /// Format Markdown text (similar to Lute's Format function)
    pub fn format_markdown(&self, markdown: &str) -> String {
        let parser = MarkdownParser::with_options(self.parse_options.clone());
        let tree = parser.parse(markdown);

        // For now, we'll just convert back to markdown through HTML conversion
        // This ensures consistent formatting
        let html = render_html(&tree);
        self.html_to_markdown(&html)
    }
}

impl Default for Quill {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_markdown_to_html() {
        let quill = Quill::new();
        let markdown = "# Hello\n\nThis is **bold** text.";
        let html = quill.markdown_to_html(markdown);

        assert!(html.contains("<h1>Hello</h1>"));
        assert!(html.contains("<strong>bold</strong>"));
    }

    #[test]
    fn test_markdown_to_wysiwyg() {
        let quill = Quill::new();
        let markdown = "# Hello\n\nParagraph.";
        let wysiwyg = quill.markdown_to_wysiwyg(markdown);

        assert!(wysiwyg.contains(r#"class="vditor-wysiwyg""#));
        assert!(wysiwyg.contains(r#"data-type="h1""#));
        assert!(wysiwyg.contains(r#"data-type="p""#));
    }

    #[test]
    fn test_html_to_markdown() {
        let quill = Quill::new();
        let html = "<h1>Hello</h1><p>This is <strong>bold</strong> text.</p>";
        let markdown = quill.html_to_markdown(html);

        assert!(markdown.contains("# Hello"));
        assert!(markdown.contains("**bold**"));
    }

    #[test]
    fn test_spin_wysiwyg_dom() {
        let quill = Quill::new();
        let input_html = r#"<div data-type="h1">Hello</div><div data-type="p">World</div>"#;
        let output = quill.spin_wysiwyg_dom(input_html);

        assert!(output.contains(r#"class="vditor-wysiwyg""#));
        assert!(output.contains(r#"data-type="h1""#));
        assert!(output.contains(r#"data-type="p""#));
    }

    #[test]
    fn test_clean_wysiwyg_attributes() {
        let quill = Quill::new();
        let html = r#"<div data-node-id="123" data-type="p" class="vditor-wysiwyg">Content</div>"#;
        let cleaned = quill.clean_wysiwyg_attributes(html);

        assert!(!cleaned.contains("data-node-id"));
        assert!(!cleaned.contains("data-type"));
        assert!(!cleaned.contains("vditor-wysiwyg"));
    }

    #[test]
    fn test_convert_task_list() {
        let quill = Quill::new();
        let html = r#"<ul><li><input type="checkbox" checked /> Task 1</li><li><input type="checkbox" /> Task 2</li></ul>"#;
        let markdown = quill.html_to_markdown(html);

        assert!(markdown.contains("- [x] Task 1"));
        assert!(markdown.contains("- [ ] Task 2"));
    }

    #[test]
    fn test_convert_code_block() {
        let quill = Quill::new();
        let html = r#"<pre><code class="language-rust">fn main() {}</code></pre>"#;
        let markdown = quill.html_to_markdown(html);

        assert!(markdown.contains("```rust"));
        assert!(markdown.contains("fn main() {}"));
        assert!(markdown.contains("```"));
    }
}
