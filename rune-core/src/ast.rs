//! AST (Abstract Syntax Tree) implementation for Rust Lute engine
//!
//! This module provides the core AST node structure and operations for parsing and rendering
//! Markdown documents, inspired by the Lute engine's architecture.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Node types for the AST, mirroring Lute's NodeType
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeType {
    // Document structure
    Document,

    // Block elements
    Paragraph,
    Heading,
    ThematicBreak,
    CodeBlock,
    HTMLBlock,
    LinkReferenceDefinition,
    Blockquote,
    List,
    ListItem,
    Table,
    TableHead,
    TableRow,
    TableCell,

    // Inline elements
    Text,
    SoftBreak,
    LineBreak,
    Code,
    HTMLInline,
    Emph,
    Strong,
    Link,
    Image,

    // Extended elements
    Strikethrough,
    TaskListItemMarker,

    // Math elements
    InlineMath,
    MathBlock,

    // Custom block elements
    SuperBlock,
    KramdownBlockIAL,

    // Editor specific
    Caret,
}

/// Position within a document
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: usize,
    pub column: usize,
    pub offset: usize,
}

impl Position {
    pub fn new(line: usize, column: usize, offset: usize) -> Self {
        Self {
            line,
            column,
            offset,
        }
    }

    pub fn zero() -> Self {
        Self::new(0, 0, 0)
    }
}

/// AST Node structure, inspired by Lute's Node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    /// Unique identifier for the node
    pub id: String,

    /// Node type
    pub node_type: NodeType,

    /// Parent node reference (Option to handle root)
    #[serde(skip)]
    pub parent: Option<Box<Node>>,

    /// Child nodes
    pub children: Vec<Node>,

    /// Raw token data
    pub tokens: Vec<u8>,

    /// Additional data for the node
    pub data: String,

    /// Node attributes (for HTML attributes, etc.)
    pub attributes: HashMap<String, String>,

    /// Position in source text
    pub position: Option<Position>,

    /// Level (for headings, list items, etc.)
    pub level: Option<usize>,

    /// Whether this node is open (for parsing)
    pub open: bool,

    /// Kramdown IAL (Inline Attribute List)
    pub kramdown_ial: Vec<(String, String)>,
}

impl Node {
    /// Create a new node with given type
    pub fn new(node_type: NodeType) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            node_type,
            parent: None,
            children: Vec::new(),
            tokens: Vec::new(),
            data: String::new(),
            attributes: HashMap::new(),
            position: None,
            level: None,
            open: true,
            kramdown_ial: Vec::new(),
        }
    }

    /// Create a text node with content
    pub fn text(content: &str) -> Self {
        let mut node = Self::new(NodeType::Text);
        node.tokens = content.as_bytes().to_vec();
        node.data = content.to_string();
        node
    }

    /// Add a child node
    pub fn append_child(&mut self, child: Node) {
        self.children.push(child);
    }

    /// Insert a child at specific index
    pub fn insert_child(&mut self, index: usize, child: Node) {
        if index <= self.children.len() {
            self.children.insert(index, child);
        }
    }

    /// Remove a child at specific index
    pub fn remove_child(&mut self, index: usize) -> Option<Node> {
        if index < self.children.len() {
            Some(self.children.remove(index))
        } else {
            None
        }
    }

    /// Get first child
    pub fn first_child(&self) -> Option<&Node> {
        self.children.first()
    }

    /// Get last child
    pub fn last_child(&self) -> Option<&Node> {
        self.children.last()
    }

    /// Check if this node is a block element
    pub fn is_block(&self) -> bool {
        matches!(
            self.node_type,
            NodeType::Document
                | NodeType::Paragraph
                | NodeType::Heading
                | NodeType::ThematicBreak
                | NodeType::CodeBlock
                | NodeType::HTMLBlock
                | NodeType::LinkReferenceDefinition
                | NodeType::Blockquote
                | NodeType::List
                | NodeType::ListItem
                | NodeType::Table
                | NodeType::TableHead
                | NodeType::TableRow
                | NodeType::TableCell
                | NodeType::MathBlock
                | NodeType::SuperBlock
                | NodeType::KramdownBlockIAL
        )
    }

    /// Check if this node is an inline element
    pub fn is_inline(&self) -> bool {
        !self.is_block()
    }

    /// Get text content of this node and its children
    pub fn text_content(&self) -> String {
        match self.node_type {
            NodeType::Text => self.data.clone(),
            _ => self
                .children
                .iter()
                .map(|child| child.text_content())
                .collect::<Vec<_>>()
                .join(""),
        }
    }

    /// Set an attribute
    pub fn set_attribute(&mut self, key: &str, value: &str) {
        self.attributes.insert(key.to_string(), value.to_string());
    }

    /// Get an attribute
    pub fn get_attribute(&self, key: &str) -> Option<&String> {
        self.attributes.get(key)
    }

    /// Add Kramdown IAL
    pub fn add_ial(&mut self, key: &str, value: &str) {
        self.kramdown_ial.push((key.to_string(), value.to_string()));
    }
}

/// AST tree structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tree {
    /// Root node of the tree
    pub root: Node,

    /// Parse options used
    pub parse_options: ParseOptions,
}

impl Tree {
    /// Create a new tree with document root
    pub fn new() -> Self {
        Self {
            root: Node::new(NodeType::Document),
            parse_options: ParseOptions::default(),
        }
    }

    /// Create tree with specific parse options
    pub fn with_options(options: ParseOptions) -> Self {
        Self {
            root: Node::new(NodeType::Document),
            parse_options: options,
        }
    }

    /// Walk the tree with a visitor function
    pub fn walk<F>(&self, mut visitor: F)
    where
        F: FnMut(&Node, bool) -> WalkStatus,
    {
        self.walk_node(&self.root, &mut visitor);
    }

    fn walk_node<F>(&self, node: &Node, visitor: &mut F)
    where
        F: FnMut(&Node, bool) -> WalkStatus,
    {
        // Visit node entering
        match visitor(node, true) {
            WalkStatus::Continue => {}
            WalkStatus::SkipChildren => return,
            WalkStatus::Terminate => return,
        }

        // Visit children
        for child in &node.children {
            self.walk_node(child, visitor);
        }

        // Visit node exiting
        visitor(node, false);
    }
}

impl Default for Tree {
    fn default() -> Self {
        Self::new()
    }
}

/// Walk status for tree traversal
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkStatus {
    Continue,
    SkipChildren,
    Terminate,
}

/// Parse options for the AST parser
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseOptions {
    /// Enable GFM (GitHub Flavored Markdown)
    pub gfm: bool,

    /// Enable footnotes
    pub footnotes: bool,

    /// Enable custom heading IDs
    pub heading_id: bool,

    /// Enable emoji aliases
    pub emoji: bool,

    /// Enable YAML front matter
    pub yaml_front_matter: bool,

    /// Enable task lists
    pub task_lists: bool,

    /// Enable strikethrough
    pub strikethrough: bool,

    /// Enable tables
    pub tables: bool,

    /// Enable math blocks and inline math
    pub math: bool,

    /// Enable custom blocks
    pub custom_blocks: bool,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            gfm: true,
            footnotes: true,
            heading_id: true,
            emoji: true,
            yaml_front_matter: true,
            task_lists: true,
            strikethrough: true,
            tables: true,
            math: true,
            custom_blocks: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_creation() {
        let node = Node::new(NodeType::Paragraph);
        assert_eq!(node.node_type, NodeType::Paragraph);
        assert!(node.open);
        assert!(node.children.is_empty());
    }

    #[test]
    fn test_text_node() {
        let node = Node::text("Hello, world!");
        assert_eq!(node.node_type, NodeType::Text);
        assert_eq!(node.text_content(), "Hello, world!");
    }

    #[test]
    fn test_node_hierarchy() {
        let mut paragraph = Node::new(NodeType::Paragraph);
        let text = Node::text("Hello");

        paragraph.append_child(text);
        assert_eq!(paragraph.children.len(), 1);
        assert_eq!(paragraph.text_content(), "Hello");
    }

    #[test]
    fn test_block_vs_inline() {
        let paragraph = Node::new(NodeType::Paragraph);
        let text = Node::new(NodeType::Text);

        assert!(paragraph.is_block());
        assert!(!paragraph.is_inline());
        assert!(!text.is_block());
        assert!(text.is_inline());
    }

    #[test]
    fn test_tree_creation() {
        let tree = Tree::new();
        assert_eq!(tree.root.node_type, NodeType::Document);
    }

    #[test]
    fn test_tree_walk() {
        let mut tree = Tree::new();
        let mut paragraph = Node::new(NodeType::Paragraph);
        paragraph.append_child(Node::text("Hello"));
        tree.root.append_child(paragraph);

        let mut node_count = 0;
        tree.walk(|_node, entering| {
            if entering {
                node_count += 1;
            }
            WalkStatus::Continue
        });

        assert_eq!(node_count, 3); // Document, Paragraph, Text
    }
}
