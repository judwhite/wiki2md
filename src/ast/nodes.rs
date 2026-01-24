use crate::ast::Span;
use serde::{Deserialize, Serialize};

/// Root AST node for a parsed wikitext document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    /// Span covering the entire document.
    pub span: Span,

    #[serde(default)]
    pub blocks: Vec<BlockNode>,

    /// Categories (i.e., `[[Category:Name|sort]]`) captured as metadata.
    ///
    /// Categories are not typically rendered inline; they are stored separately
    /// so the renderer can decide whether to emit them and how.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<CategoryTag>,

    /// Redirect target if the page is a redirect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redirect: Option<Redirect>,
}

/// A category membership tag, e.g. `[[Category:Chess Programmer|Thompson]]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CategoryTag {
    pub span: Span,
    pub name: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_key: Option<String>,
}

/// Redirect marker, e.g. `#REDIRECT [[Target]]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Redirect {
    pub span: Span,
    pub target: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
}

/// A block node with a source span and a tagged kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockNode {
    pub span: Span,

    #[serde(flatten)]
    pub kind: BlockKind,
}

/// Block-level node kinds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BlockKind {
    Heading {
        /// Heading level (1..=6). Level 1 is allowed.
        level: u8,
        content: Vec<InlineNode>,
    },

    Paragraph {
        content: Vec<InlineNode>,
    },

    /// A hierarchical list block.
    List {
        items: Vec<ListItem>,
    },

    /// A MediaWiki table.
    Table {
        table: Table,
    },

    /// A fenced or otherwise verbatim code/pre block.
    CodeBlock {
        block: CodeBlock,
    },

    /// A placeholder for `<references />`.
    References {
        node: ReferencesNode,
    },

    /// A generic HTML-ish block tag (e.g. `<div>...</div>`).
    HtmlBlock {
        node: HtmlBlock,
    },

    /// A magic word like `__TOC__`.
    MagicWord {
        name: String,
    },

    /// A horizontal rule.
    HorizontalRule,

    /// A blockquote, typically from wikitext indentation or explicit HTML.
    BlockQuote {
        blocks: Vec<BlockNode>,
    },

    /// Unparsed or unsupported block text preserved for round-tripping/debug.
    Raw {
        text: String,
    },
}

/// A list item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListItem {
    pub span: Span,
    pub marker: ListMarker,

    /// Blocks that make up this list item's content.
    #[serde(default)]
    pub blocks: Vec<BlockNode>,
}

/// List marker types in wikitext.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListMarker {
    /// `*` bullet list.
    Unordered,
    /// `#` numbered list.
    Ordered,
    /// `;` definition term.
    Term,
    /// `:` definition / indentation.
    Definition,
}

/// Code-like blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeBlock {
    pub kind: CodeBlockKind,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,

    /// Raw code text as it appeared in the source.
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeBlockKind {
    /// `<syntaxhighlight ...>...</syntaxhighlight>`
    SyntaxHighlight,
    /// `<pre>...</pre>`
    PreTag,
    /// Lines beginning with a single leading space.
    LeadingSpace,
}

/// Represents the `<references />` tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferencesNode {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attrs: Vec<HtmlAttr>,
}

/// A generic HTML-ish block tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HtmlBlock {
    pub name: String,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attrs: Vec<HtmlAttr>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<BlockNode>,

    pub self_closing: bool,
}

/// A generic HTML attribute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HtmlAttr {
    pub name: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,

    /// Optional attribute span (not required for all parsers).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
}

/// Inline node with a span and tagged kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InlineNode {
    pub span: Span,

    #[serde(flatten)]
    pub kind: InlineKind,
}

/// Inline-level node kinds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InlineKind {
    Text { value: String },

    Bold { content: Vec<InlineNode> },
    Italic { content: Vec<InlineNode> },
    BoldItalic { content: Vec<InlineNode> },

    InternalLink { link: InternalLink },
    ExternalLink { link: ExternalLink },

    /// `[[File:...|...]]` / `[[Image:...|...]]` / `[[Media:...|...]]`.
    FileLink { link: FileLink },

    /// `<br>` / `<br/>`.
    LineBreak,

    /// `<ref ...>...</ref>` or `<ref ... />`.
    Ref { node: RefNode },

    /// Generic HTML-ish inline tag, e.g. `<span id="..."></span>`.
    HtmlTag { node: HtmlTag },

    /// `{{...}}` templates and parser functions.
    Template { node: TemplateInvocation },

    /// Unparsed or unsupported inline content preserved for debug.
    Raw { text: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InternalLink {
    /// Raw target text inside `[[...]]`, excluding the optional label.
    pub target: String,

    /// Optional section anchor after `#` in the target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,

    /// Optional label (after `|`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<Vec<InlineNode>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalLink {
    pub url: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<Vec<InlineNode>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileLink {
    pub namespace: FileNamespace,
    pub target: String,

    /// Pipe-separated parameters after the filename.
    ///
    /// We intentionally keep these as a list of parsed inline fragments rather
    /// than prematurely classifying them into "options" vs "caption".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<FileParam>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileNamespace {
    File,
    Image,
    Media,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileParam {
    pub span: Span,
    pub content: Vec<InlineNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefNode {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attrs: Vec<HtmlAttr>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<InlineNode>>,

    pub self_closing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HtmlTag {
    pub name: String,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attrs: Vec<HtmlAttr>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<InlineNode>,

    pub self_closing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateInvocation {
    pub name: TemplateName,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<TemplateParam>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateName {
    /// Raw name as it appeared (minus surrounding braces).
    pub raw: String,

    pub kind: TemplateNameKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateNameKind {
    Template,
    ParserFunction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateParam {
    pub span: Span,

    /// Named parameter key (left of `=`), if present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Parameter value as parsed inline content.
    pub value: Vec<InlineNode>,
}

/* -----------------------------
 * Tables
 * ----------------------------- */

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Table {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attrs: Vec<HtmlAttr>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<TableCaption>,

    #[serde(default)]
    pub rows: Vec<TableRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableCaption {
    pub span: Span,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attrs: Vec<HtmlAttr>,

    pub content: Vec<InlineNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableRow {
    pub span: Span,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attrs: Vec<HtmlAttr>,

    #[serde(default)]
    pub cells: Vec<TableCell>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableCell {
    pub span: Span,
    pub kind: TableCellKind,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attrs: Vec<HtmlAttr>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colspan: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rowspan: Option<u32>,

    /// Cell content as nested blocks (tables and lists can nest).
    #[serde(default)]
    pub blocks: Vec<BlockNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableCellKind {
    Header,
    Data,
}
