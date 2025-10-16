//! Basic syntax highlighting for raw markdown mode

use serde::{Deserialize, Serialize};

/// Syntax highlighting token types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenType {
    /// Plain text
    Text,
    /// Header markers (# ## ###)
    Header,
    /// Bold markers (**)
    Bold,
    /// Italic markers (*)
    Italic,
    /// Code markers (`)
    Code,
    /// Link syntax
    Link,
    /// List markers (- * + 1.)
    ListMarker,
    /// Blockquote markers (>)
    Blockquote,
    /// Horizontal rule (---)
    HorizontalRule,
}

/// A highlighted token with position information
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HighlightToken {
    /// Type of token
    pub token_type: TokenType,
    /// Start position in the content
    pub start: usize,
    /// End position in the content
    pub end: usize,
    /// The actual text content
    pub text: String,
}

impl HighlightToken {
    /// Create a new highlight token
    pub fn new(token_type: TokenType, start: usize, end: usize, text: String) -> Self {
        Self {
            token_type,
            start,
            end,
            text,
        }
    }

    /// Get the length of the token
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// Check if the token is empty
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

/// Syntax highlighter for markdown in raw mode
pub struct SyntaxHighlighter {
    /// Whether to highlight inline code
    pub highlight_code: bool,
    /// Whether to highlight links
    pub highlight_links: bool,
    /// Whether to highlight headers
    pub highlight_headers: bool,
    /// Whether to highlight lists
    pub highlight_lists: bool,
}

impl SyntaxHighlighter {
    /// Create a new syntax highlighter with default settings
    pub fn new() -> Self {
        Self {
            highlight_code: true,
            highlight_links: true,
            highlight_headers: true,
            highlight_lists: true,
        }
    }

    /// Highlight markdown content
    pub fn highlight(&self, content: &str) -> Vec<HighlightToken> {
        let mut tokens = Vec::new();
        let mut offset = 0;

        for line in content.lines() {
            tokens.extend(self.highlight_line(line, offset));
            offset += line.len() + 1; // +1 for newline
        }

        tokens
    }

    /// Highlight a line of markdown text
    pub fn highlight_line(&self, line: &str, line_offset: usize) -> Vec<HighlightToken> {
        let mut tokens = Vec::new();
        let trimmed = line.trim_start();
        let indent_len = line.len() - trimmed.len();

        // Check for headers
        if self.highlight_headers && trimmed.starts_with('#') {
            let header_end = trimmed.find(' ').unwrap_or(trimmed.len());
            tokens.push(HighlightToken::new(
                TokenType::Header,
                line_offset + indent_len,
                line_offset + indent_len + header_end,
                trimmed[..header_end].to_string(),
            ));

            // Rest is text
            if header_end < trimmed.len() {
                tokens.push(HighlightToken::new(
                    TokenType::Text,
                    line_offset + indent_len + header_end,
                    line_offset + line.len(),
                    trimmed[header_end..].to_string(),
                ));
            }
            return tokens;
        }

        // Check for list markers
        if self.highlight_lists {
            if let Some(list_token) = self.detect_list_marker(trimmed, line_offset + indent_len) {
                tokens.push(list_token.clone());

                // Rest is text
                let marker_len = list_token.len();
                if marker_len < trimmed.len() {
                    tokens.push(HighlightToken::new(
                        TokenType::Text,
                        line_offset + indent_len + marker_len,
                        line_offset + line.len(),
                        trimmed[marker_len..].to_string(),
                    ));
                }
                return tokens;
            }
        }

        // Check for blockquote
        if trimmed.starts_with('>') {
            let quote_end = if trimmed.len() > 1 && trimmed.chars().nth(1) == Some(' ') {
                2
            } else {
                1
            };
            tokens.push(HighlightToken::new(
                TokenType::Blockquote,
                line_offset + indent_len,
                line_offset + indent_len + quote_end,
                trimmed[..quote_end].to_string(),
            ));

            // Rest is text
            if quote_end < trimmed.len() {
                tokens.push(HighlightToken::new(
                    TokenType::Text,
                    line_offset + indent_len + quote_end,
                    line_offset + line.len(),
                    trimmed[quote_end..].to_string(),
                ));
            }
            return tokens;
        }

        // Check for horizontal rule
        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            tokens.push(HighlightToken::new(
                TokenType::HorizontalRule,
                line_offset + indent_len,
                line_offset + line.len(),
                trimmed.to_string(),
            ));
            return tokens;
        }

        // Highlight inline elements (bold, italic, code)
        tokens.extend(self.highlight_inline_elements(line, line_offset));

        // If no tokens were created, treat entire line as text
        if tokens.is_empty() {
            tokens.push(HighlightToken::new(
                TokenType::Text,
                line_offset,
                line_offset + line.len(),
                line.to_string(),
            ));
        }

        tokens
    }

    /// Detect list marker at the start of a line
    fn detect_list_marker(&self, line: &str, offset: usize) -> Option<HighlightToken> {
        // Unordered list markers
        if line.starts_with("- ") {
            return Some(HighlightToken::new(
                TokenType::ListMarker,
                offset,
                offset + 2,
                "- ".to_string(),
            ));
        } else if line.starts_with("* ") {
            return Some(HighlightToken::new(
                TokenType::ListMarker,
                offset,
                offset + 2,
                "* ".to_string(),
            ));
        } else if line.starts_with("+ ") {
            return Some(HighlightToken::new(
                TokenType::ListMarker,
                offset,
                offset + 2,
                "+ ".to_string(),
            ));
        }

        // Ordered list markers (e.g., "1. ", "2. ")
        if let Some(dot_pos) = line.find(". ") {
            let number_str = &line[..dot_pos];
            if number_str.chars().all(|c| c.is_ascii_digit()) {
                let marker_len = dot_pos + 2;
                return Some(HighlightToken::new(
                    TokenType::ListMarker,
                    offset,
                    offset + marker_len,
                    line[..marker_len].to_string(),
                ));
            }
        }

        None
    }

    /// Highlight inline elements like bold, italic, code
    fn highlight_inline_elements(&self, line: &str, line_offset: usize) -> Vec<HighlightToken> {
        let mut tokens = Vec::new();
        let mut pos = 0;
        let chars: Vec<char> = line.chars().collect();

        while pos < chars.len() {
            // Check for inline code (`)
            if self.highlight_code && chars[pos] == '`' {
                if let Some(end) = self.find_closing_char(&chars, pos + 1, '`') {
                    tokens.push(HighlightToken::new(
                        TokenType::Code,
                        line_offset + pos,
                        line_offset + end + 1,
                        chars[pos..=end].iter().collect(),
                    ));
                    pos = end + 1;
                    continue;
                }
            }

            // Check for bold (**)
            if pos + 1 < chars.len() && chars[pos] == '*' && chars[pos + 1] == '*' {
                if let Some(end) = self.find_closing_sequence(&chars, pos + 2, "**") {
                    tokens.push(HighlightToken::new(
                        TokenType::Bold,
                        line_offset + pos,
                        line_offset + end + 2,
                        chars[pos..=end + 1].iter().collect(),
                    ));
                    pos = end + 2;
                    continue;
                }
            }

            // Check for italic (*)
            if chars[pos] == '*' {
                if let Some(end) = self.find_closing_char(&chars, pos + 1, '*') {
                    tokens.push(HighlightToken::new(
                        TokenType::Italic,
                        line_offset + pos,
                        line_offset + end + 1,
                        chars[pos..=end].iter().collect(),
                    ));
                    pos = end + 1;
                    continue;
                }
            }

            // Check for links [text](url)
            if self.highlight_links && chars[pos] == '[' {
                if let Some(link_token) = self.parse_link(&chars, pos, line_offset) {
                    tokens.push(link_token.clone());
                    pos = link_token.end - line_offset;
                    continue;
                }
            }

            pos += 1;
        }

        tokens
    }

    /// Find closing character for inline elements
    fn find_closing_char(&self, chars: &[char], start: usize, closing: char) -> Option<usize> {
        chars[start..].iter().position(|&c| c == closing).map(|pos| start + pos)
    }

    /// Find closing sequence for multi-character markers
    fn find_closing_sequence(&self, chars: &[char], start: usize, sequence: &str) -> Option<usize> {
        let seq_chars: Vec<char> = sequence.chars().collect();
        let seq_len = seq_chars.len();

        for i in start..chars.len() {
            if i + seq_len <= chars.len() {
                let matches = (0..seq_len).all(|j| chars[i + j] == seq_chars[j]);
                if matches {
                    return Some(i);
                }
            }
        }
        None
    }

    /// Parse markdown link syntax [text](url)
    fn parse_link(&self, chars: &[char], start: usize, offset: usize) -> Option<HighlightToken> {
        // Find closing ]
        let bracket_end = chars[(start + 1)..].iter().position(|&c| c == ']').map(|pos| start + 1 + pos);

        if let Some(bracket_end) = bracket_end {
            // Check for (url) after ]
            if bracket_end + 1 < chars.len() && chars[bracket_end + 1] == '(' {
                // Find closing )
                for i in (bracket_end + 2)..chars.len() {
                    if chars[i] == ')' {
                        return Some(HighlightToken::new(
                            TokenType::Link,
                            offset + start,
                            offset + i + 1,
                            chars[start..=i].iter().collect(),
                        ));
                    }
                }
            }
        }

        None
    }
}

impl Default for SyntaxHighlighter {
    fn default() -> Self {
        Self::new()
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_highlight_header() {
        let highlighter = SyntaxHighlighter::new();
        let tokens = highlighter.highlight_line("# Header", 0);

        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].token_type, TokenType::Header);
        assert_eq!(tokens[0].text, "#");
        assert_eq!(tokens[1].token_type, TokenType::Text);
    }

    #[test]
    fn test_highlight_list_unordered() {
        let highlighter = SyntaxHighlighter::new();
        let tokens = highlighter.highlight_line("- List item", 0);

        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].token_type, TokenType::ListMarker);
        assert_eq!(tokens[0].text, "- ");
        assert_eq!(tokens[1].token_type, TokenType::Text);
        assert_eq!(tokens[1].text, "List item");
    }

    #[test]
    fn test_highlight_list_ordered() {
        let highlighter = SyntaxHighlighter::new();
        let tokens = highlighter.highlight_line("1. First item", 0);

        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].token_type, TokenType::ListMarker);
        assert_eq!(tokens[0].text, "1. ");
    }

    #[test]
    fn test_highlight_blockquote() {
        let highlighter = SyntaxHighlighter::new();
        let tokens = highlighter.highlight_line("> Quote", 0);

        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].token_type, TokenType::Blockquote);
        assert_eq!(tokens[0].text, "> ");
    }

    #[test]
    fn test_highlight_horizontal_rule() {
        let highlighter = SyntaxHighlighter::new();
        let tokens = highlighter.highlight_line("---", 0);

        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token_type, TokenType::HorizontalRule);
    }

    #[test]
    fn test_highlight_inline_code() {
        let highlighter = SyntaxHighlighter::new();
        let tokens = highlighter.highlight_inline_elements("`code`", 0);

        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token_type, TokenType::Code);
        assert_eq!(tokens[0].text, "`code`");
    }

    #[test]
    fn test_highlight_bold() {
        let highlighter = SyntaxHighlighter::new();
        let tokens = highlighter.highlight_inline_elements("**bold**", 0);

        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token_type, TokenType::Bold);
        assert_eq!(tokens[0].text, "**bold**");
    }

    #[test]
    fn test_highlight_italic() {
        let highlighter = SyntaxHighlighter::new();
        let tokens = highlighter.highlight_inline_elements("*italic*", 0);

        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token_type, TokenType::Italic);
        assert_eq!(tokens[0].text, "*italic*");
    }

    #[test]
    fn test_highlight_link() {
        let highlighter = SyntaxHighlighter::new();
        let tokens = highlighter.highlight_inline_elements("[text](url)", 0);

        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token_type, TokenType::Link);
        assert_eq!(tokens[0].text, "[text](url)");
    }

    #[test]
    fn test_highlight_full_content() {
        let highlighter = SyntaxHighlighter::new();
        let content = "# Header\n- List item\n`code`";
        let tokens = highlighter.highlight(content);

        assert!(!tokens.is_empty());
        assert!(tokens.iter().any(|t| t.token_type == TokenType::Header));
        assert!(tokens.iter().any(|t| t.token_type == TokenType::ListMarker));
    }

    #[test]
    fn test_detect_list_marker() {
        let highlighter = SyntaxHighlighter::new();

        assert!(highlighter.detect_list_marker("- item", 0).is_some());
        assert!(highlighter.detect_list_marker("* item", 0).is_some());
        assert!(highlighter.detect_list_marker("+ item", 0).is_some());
        assert!(highlighter.detect_list_marker("1. item", 0).is_some());
        assert!(highlighter.detect_list_marker("regular text", 0).is_none());
    }
}
