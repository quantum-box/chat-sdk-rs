//! Platform-agnostic Markdown AST and message formatter.
//!
//! Absorbs differences between Slack mrkdwn and Discord Markdown
//! by building a platform-neutral AST, then rendering to each target format.
//!
//! # Example
//! ```
//! use chat_sdk::format::{MessageFormatter, Platform};
//!
//! let msg = MessageFormatter::new()
//!     .bold("Important")
//!     .text(": check out ")
//!     .link("https://example.com", Some("this link"))
//!     .newline()
//!     .italic("— the team")
//!     .build();
//!
//! assert_eq!(msg.render(Platform::Slack), "*Important*: check out <https://example.com|this link>\n_— the team_");
//! assert_eq!(msg.render(Platform::Discord), "**Important**: check out [this link](https://example.com)\n*— the team*");
//! ```

use std::fmt;

/// Target platform for rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Slack,
    Discord,
}

/// A single node in the message AST.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// Plain text.
    Text(String),
    /// Bold text containing child nodes.
    Bold(Vec<Node>),
    /// Italic text containing child nodes.
    Italic(Vec<Node>),
    /// Strikethrough text containing child nodes.
    Strikethrough(Vec<Node>),
    /// Inline code (no nesting).
    Code(String),
    /// Fenced code block with optional language.
    CodeBlock { lang: Option<String>, code: String },
    /// Hyperlink with URL and optional display text.
    Link { url: String, text: Option<String> },
    /// Block quote containing child nodes.
    Blockquote(Vec<Node>),
    /// An unordered list of items, each item is a list of nodes.
    UnorderedList(Vec<Vec<Node>>),
    /// An ordered list of items, each item is a list of nodes.
    OrderedList(Vec<Vec<Node>>),
    /// Line break.
    Newline,
}

/// A rendered message document built from AST nodes.
#[derive(Debug, Clone)]
pub struct Document {
    nodes: Vec<Node>,
}

impl Document {
    /// Render this document for the given platform.
    pub fn render(&self, platform: Platform) -> String {
        render_nodes(&self.nodes, platform)
    }
}

impl fmt::Display for Document {
    /// Displays using Discord markdown by default.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render(Platform::Discord))
    }
}

/// Builder for constructing a [`Document`] from inline calls.
#[derive(Debug, Default)]
pub struct MessageFormatter {
    nodes: Vec<Node>,
}

impl MessageFormatter {
    /// Create a new empty formatter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a plain text span.
    pub fn text(mut self, text: &str) -> Self {
        self.nodes.push(Node::Text(text.to_owned()));
        self
    }

    /// Append bold text (simple string shorthand).
    pub fn bold(mut self, text: &str) -> Self {
        self.nodes
            .push(Node::Bold(vec![Node::Text(text.to_owned())]));
        self
    }

    /// Append bold with nested nodes built via a closure.
    pub fn bold_with<F>(mut self, f: F) -> Self
    where
        F: FnOnce(MessageFormatter) -> MessageFormatter,
    {
        let inner = f(MessageFormatter::new());
        self.nodes.push(Node::Bold(inner.nodes));
        self
    }

    /// Append italic text (simple string shorthand).
    pub fn italic(mut self, text: &str) -> Self {
        self.nodes
            .push(Node::Italic(vec![Node::Text(text.to_owned())]));
        self
    }

    /// Append italic with nested nodes.
    pub fn italic_with<F>(mut self, f: F) -> Self
    where
        F: FnOnce(MessageFormatter) -> MessageFormatter,
    {
        let inner = f(MessageFormatter::new());
        self.nodes.push(Node::Italic(inner.nodes));
        self
    }

    /// Append strikethrough text.
    pub fn strikethrough(mut self, text: &str) -> Self {
        self.nodes
            .push(Node::Strikethrough(vec![Node::Text(text.to_owned())]));
        self
    }

    /// Append strikethrough with nested nodes.
    pub fn strikethrough_with<F>(mut self, f: F) -> Self
    where
        F: FnOnce(MessageFormatter) -> MessageFormatter,
    {
        let inner = f(MessageFormatter::new());
        self.nodes.push(Node::Strikethrough(inner.nodes));
        self
    }

    /// Append inline code.
    pub fn code(mut self, code: &str) -> Self {
        self.nodes.push(Node::Code(code.to_owned()));
        self
    }

    /// Append a fenced code block.
    pub fn code_block(mut self, code: &str, lang: Option<&str>) -> Self {
        self.nodes.push(Node::CodeBlock {
            lang: lang.map(|s| s.to_owned()),
            code: code.to_owned(),
        });
        self
    }

    /// Append a hyperlink.
    pub fn link(mut self, url: &str, text: Option<&str>) -> Self {
        self.nodes.push(Node::Link {
            url: url.to_owned(),
            text: text.map(|s| s.to_owned()),
        });
        self
    }

    /// Append a block quote with plain text.
    pub fn blockquote(mut self, text: &str) -> Self {
        self.nodes
            .push(Node::Blockquote(vec![Node::Text(text.to_owned())]));
        self
    }

    /// Append a block quote with nested nodes.
    pub fn blockquote_with<F>(mut self, f: F) -> Self
    where
        F: FnOnce(MessageFormatter) -> MessageFormatter,
    {
        let inner = f(MessageFormatter::new());
        self.nodes.push(Node::Blockquote(inner.nodes));
        self
    }

    /// Append an unordered list. Each item is built via a closure.
    pub fn unordered_list<F>(mut self, items: Vec<F>) -> Self
    where
        F: FnOnce(MessageFormatter) -> MessageFormatter,
    {
        let list_items: Vec<Vec<Node>> = items
            .into_iter()
            .map(|f| f(MessageFormatter::new()).nodes)
            .collect();
        self.nodes.push(Node::UnorderedList(list_items));
        self
    }

    /// Append an unordered list from plain text items.
    pub fn unordered_list_text(mut self, items: &[&str]) -> Self {
        let list_items: Vec<Vec<Node>> = items
            .iter()
            .map(|s| vec![Node::Text((*s).to_owned())])
            .collect();
        self.nodes.push(Node::UnorderedList(list_items));
        self
    }

    /// Append an ordered list from plain text items.
    pub fn ordered_list_text(mut self, items: &[&str]) -> Self {
        let list_items: Vec<Vec<Node>> = items
            .iter()
            .map(|s| vec![Node::Text((*s).to_owned())])
            .collect();
        self.nodes.push(Node::OrderedList(list_items));
        self
    }

    /// Append a line break.
    pub fn newline(mut self) -> Self {
        self.nodes.push(Node::Newline);
        self
    }

    /// Append a raw AST node.
    pub fn node(mut self, node: Node) -> Self {
        self.nodes.push(node);
        self
    }

    /// Consume the builder and produce a [`Document`].
    pub fn build(self) -> Document {
        Document { nodes: self.nodes }
    }
}

// ── Rendering ──────────────────────────────────────────────

fn render_nodes(nodes: &[Node], platform: Platform) -> String {
    let mut out = String::new();
    for node in nodes {
        render_node(node, platform, &mut out);
    }
    out
}

fn render_node(node: &Node, platform: Platform, out: &mut String) {
    match node {
        Node::Text(t) => out.push_str(t),

        Node::Bold(children) => {
            let inner = render_nodes(children, platform);
            match platform {
                Platform::Slack => {
                    out.push('*');
                    out.push_str(&inner);
                    out.push('*');
                }
                Platform::Discord => {
                    out.push_str("**");
                    out.push_str(&inner);
                    out.push_str("**");
                }
            }
        }

        Node::Italic(children) => {
            let inner = render_nodes(children, platform);
            match platform {
                Platform::Slack => {
                    out.push('_');
                    out.push_str(&inner);
                    out.push('_');
                }
                Platform::Discord => {
                    out.push('*');
                    out.push_str(&inner);
                    out.push('*');
                }
            }
        }

        Node::Strikethrough(children) => {
            let inner = render_nodes(children, platform);
            match platform {
                Platform::Slack => {
                    out.push('~');
                    out.push_str(&inner);
                    out.push('~');
                }
                Platform::Discord => {
                    out.push_str("~~");
                    out.push_str(&inner);
                    out.push_str("~~");
                }
            }
        }

        Node::Code(code) => {
            out.push('`');
            out.push_str(code);
            out.push('`');
        }

        Node::CodeBlock { lang, code } => {
            out.push_str("```");
            if let Some(l) = lang {
                out.push_str(l);
            }
            out.push('\n');
            out.push_str(code);
            if !code.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("```");
        }

        Node::Link { url, text } => match platform {
            Platform::Slack => {
                out.push('<');
                out.push_str(url);
                if let Some(t) = text {
                    out.push('|');
                    out.push_str(t);
                }
                out.push('>');
            }
            Platform::Discord => {
                if let Some(t) = text {
                    out.push('[');
                    out.push_str(t);
                    out.push_str("](");
                    out.push_str(url);
                    out.push(')');
                } else {
                    out.push_str(url);
                }
            }
        },

        Node::Blockquote(children) => {
            let inner = render_nodes(children, platform);
            for line in inner.lines() {
                out.push_str("> ");
                out.push_str(line);
                out.push('\n');
            }
            // Remove trailing newline to keep composable
            if out.ends_with('\n') {
                out.pop();
            }
        }

        Node::UnorderedList(items) => {
            for item_nodes in items {
                let inner = render_nodes(item_nodes, platform);
                // Slack doesn't have native list support; use bullet char
                out.push_str("• ");
                out.push_str(&inner);
                out.push('\n');
            }
            if out.ends_with('\n') {
                out.pop();
            }
        }

        Node::OrderedList(items) => {
            for (i, item_nodes) in items.iter().enumerate() {
                let inner = render_nodes(item_nodes, platform);
                out.push_str(&format!("{}. ", i + 1));
                out.push_str(&inner);
                out.push('\n');
            }
            if out.ends_with('\n') {
                out.pop();
            }
        }

        Node::Newline => {
            out.push('\n');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text() {
        let doc = MessageFormatter::new().text("hello world").build();
        assert_eq!(doc.render(Platform::Slack), "hello world");
        assert_eq!(doc.render(Platform::Discord), "hello world");
    }

    #[test]
    fn bold_rendering() {
        let doc = MessageFormatter::new().bold("important").build();
        assert_eq!(doc.render(Platform::Slack), "*important*");
        assert_eq!(doc.render(Platform::Discord), "**important**");
    }

    #[test]
    fn italic_rendering() {
        let doc = MessageFormatter::new().italic("emphasis").build();
        assert_eq!(doc.render(Platform::Slack), "_emphasis_");
        assert_eq!(doc.render(Platform::Discord), "*emphasis*");
    }

    #[test]
    fn strikethrough_rendering() {
        let doc = MessageFormatter::new().strikethrough("removed").build();
        assert_eq!(doc.render(Platform::Slack), "~removed~");
        assert_eq!(doc.render(Platform::Discord), "~~removed~~");
    }

    #[test]
    fn inline_code() {
        let doc = MessageFormatter::new().code("let x = 1").build();
        assert_eq!(doc.render(Platform::Slack), "`let x = 1`");
        assert_eq!(doc.render(Platform::Discord), "`let x = 1`");
    }

    #[test]
    fn code_block_with_lang() {
        let doc = MessageFormatter::new()
            .code_block("fn main() {}", Some("rust"))
            .build();
        let expected = "```rust\nfn main() {}\n```";
        assert_eq!(doc.render(Platform::Slack), expected);
        assert_eq!(doc.render(Platform::Discord), expected);
    }

    #[test]
    fn code_block_without_lang() {
        let doc = MessageFormatter::new().code_block("hello", None).build();
        let expected = "```\nhello\n```";
        assert_eq!(doc.render(Platform::Slack), expected);
    }

    #[test]
    fn link_with_text() {
        let doc = MessageFormatter::new()
            .link("https://example.com", Some("click here"))
            .build();
        assert_eq!(
            doc.render(Platform::Slack),
            "<https://example.com|click here>"
        );
        assert_eq!(
            doc.render(Platform::Discord),
            "[click here](https://example.com)"
        );
    }

    #[test]
    fn link_without_text() {
        let doc = MessageFormatter::new()
            .link("https://example.com", None)
            .build();
        assert_eq!(doc.render(Platform::Slack), "<https://example.com>");
        assert_eq!(doc.render(Platform::Discord), "https://example.com");
    }

    #[test]
    fn blockquote() {
        let doc = MessageFormatter::new().blockquote("quoted text").build();
        assert_eq!(doc.render(Platform::Slack), "> quoted text");
        assert_eq!(doc.render(Platform::Discord), "> quoted text");
    }

    #[test]
    fn blockquote_multiline() {
        let doc = MessageFormatter::new().blockquote("line1\nline2").build();
        assert_eq!(doc.render(Platform::Slack), "> line1\n> line2");
    }

    #[test]
    fn unordered_list() {
        let doc = MessageFormatter::new()
            .unordered_list_text(&["alpha", "beta", "gamma"])
            .build();
        assert_eq!(doc.render(Platform::Slack), "• alpha\n• beta\n• gamma");
        assert_eq!(doc.render(Platform::Discord), "• alpha\n• beta\n• gamma");
    }

    #[test]
    fn ordered_list() {
        let doc = MessageFormatter::new()
            .ordered_list_text(&["first", "second"])
            .build();
        assert_eq!(doc.render(Platform::Slack), "1. first\n2. second");
    }

    #[test]
    fn nested_formatting() {
        let doc = MessageFormatter::new()
            .bold_with(|f| f.text("bold and ").italic("italic"))
            .build();
        assert_eq!(doc.render(Platform::Slack), "*bold and _italic_*");
        assert_eq!(doc.render(Platform::Discord), "**bold and *italic***");
    }

    #[test]
    fn complex_message() {
        let doc = MessageFormatter::new()
            .bold("Important")
            .text(": check out ")
            .link("https://example.com", Some("this link"))
            .newline()
            .italic("— the team")
            .build();

        assert_eq!(
            doc.render(Platform::Slack),
            "*Important*: check out <https://example.com|this link>\n_— the team_"
        );
        assert_eq!(
            doc.render(Platform::Discord),
            "**Important**: check out [this link](https://example.com)\n*— the team*"
        );
    }

    #[test]
    fn display_trait_uses_discord() {
        let doc = MessageFormatter::new().bold("test").build();
        assert_eq!(format!("{doc}"), "**test**");
    }

    #[test]
    fn empty_document() {
        let doc = MessageFormatter::new().build();
        assert_eq!(doc.render(Platform::Slack), "");
        assert_eq!(doc.render(Platform::Discord), "");
    }

    #[test]
    fn rich_list_items() {
        let doc = MessageFormatter::new()
            .unordered_list(vec![
                |f: MessageFormatter| f.bold("item1").text(" desc"),
                |f: MessageFormatter| f.code("item2"),
            ])
            .build();
        assert_eq!(doc.render(Platform::Slack), "• *item1* desc\n• `item2`");
    }

    #[test]
    fn newline_between_sections() {
        let doc = MessageFormatter::new()
            .text("header")
            .newline()
            .newline()
            .text("body")
            .build();
        assert_eq!(doc.render(Platform::Slack), "header\n\nbody");
    }

    #[test]
    fn strikethrough_nested() {
        let doc = MessageFormatter::new()
            .strikethrough_with(|f| f.text("old ").bold("value"))
            .build();
        assert_eq!(doc.render(Platform::Slack), "~old *value*~");
        assert_eq!(doc.render(Platform::Discord), "~~old **value**~~");
    }

    #[test]
    fn blockquote_with_formatting() {
        let doc = MessageFormatter::new()
            .blockquote_with(|f| f.bold("note").text(": important"))
            .build();
        assert_eq!(doc.render(Platform::Slack), "> *note*: important");
        assert_eq!(doc.render(Platform::Discord), "> **note**: important");
    }

    #[test]
    fn raw_node_api() {
        let doc = MessageFormatter::new()
            .node(Node::Text("raw".to_owned()))
            .build();
        assert_eq!(doc.render(Platform::Slack), "raw");
    }
}
