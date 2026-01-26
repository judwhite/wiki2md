//! AST -> Markdown renderer.
//!
//! This module intentionally operates **only** on the parsed AST (typically loaded
//! from JSON) and does not inspect raw `.wiki` text.

use crate::ast::*;

/// Rendering options that control formatting decisions.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// If true, render `CodeBlockKind::LeadingSpace` as a Markdown blockquote rather than
    /// a fenced code block.
    pub leading_space_as_blockquote: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            leading_space_as_blockquote: true,
        }
    }
}

#[derive(Debug, Default)]
struct RenderContext {
    refs: Vec<String>,
}

pub fn render_doc(doc: &Document) -> String {
    render_doc_with_options(doc, &RenderOptions::default())
}

pub fn render_doc_with_options(doc: &Document, opts: &RenderOptions) -> String {
    let mut ctx = RenderContext::default();
    let mut out = String::new();
    for (bi, block) in doc.blocks.iter().enumerate() {
        if bi > 0 {
            // separate blocks with a single blank line.
            out.push_str("\n\n");
        }
        out.push_str(&render_block(block, &mut ctx, opts));
    }

    // trim trailing whitespace/newlines for stable output.
    while matches!(out.as_bytes().last(), Some(b'\n' | b' ' | b'\t' | b'\r')) {
        out.pop();
    }
    out
}

fn render_block(block: &BlockNode, ctx: &mut RenderContext, opts: &RenderOptions) -> String {
    match &block.kind {
        BlockKind::Heading { level, content } => render_heading(*level, content, ctx, opts),
        BlockKind::Paragraph { content } => render_inlines(content, ctx, opts),
        BlockKind::List { items } => render_list(items, ctx, opts, 0),
        BlockKind::CodeBlock { block } => {
            render_code_block(block.kind, block.lang.as_deref(), &block.text, ctx, opts)
        }
        BlockKind::Table { table } => render_table(table, ctx, opts),
        BlockKind::BlockQuote { blocks } => {
            let mut inner = String::new();
            for (i, b) in blocks.iter().enumerate() {
                if i > 0 {
                    inner.push_str("\n\n");
                }
                inner.push_str(&render_block(b, ctx, opts));
            }
            prefix_lines(&inner, "> ")
        }
        BlockKind::HorizontalRule => "---".to_string(),
        BlockKind::References { .. } => render_references(ctx),
        BlockKind::HtmlBlock { node } => render_html_block(node, ctx, opts),
        BlockKind::MagicWord { name } => format!("<!-- {} -->", name),
        BlockKind::Raw { text } => {
            // keep raw blocks visible but non-destructive.
            format!("```text\n{}\n```", text.trim_end_matches('\n'))
        }
    }
}

fn render_heading(level: u8, content: &[InlineNode], ctx: &mut RenderContext, opts: &RenderOptions) -> String {
    // special-case: leading <span id="..."></span> anchors are better emitted on their own line.
    let mut content_slice = content;
    let mut prefix = String::new();
    if let Some(first) = content.first()
        && let InlineKind::HtmlTag { node } = &first.kind
            && node.name.eq_ignore_ascii_case("span")
                && let Some(id_attr) = node
                    .attrs
                    .iter()
                    .find(|a| a.name.eq_ignore_ascii_case("id"))
                    .and_then(|a| a.value.as_ref())
                {
                    // emit a stable HTML anchor.
                    prefix.push_str(&format!("<a name=\"{}\"></a>\n", id_attr));
                    content_slice = &content[1..];
                }

    let hashes = "#".repeat(level.max(1) as usize);
    let title = render_inlines(content_slice, ctx, opts).trim().to_string();
    if prefix.is_empty() {
        format!("{} {}", hashes, title)
    } else {
        format!("{}{} {}", prefix, hashes, title)
    }
}

fn render_list(items: &[ListItem], ctx: &mut RenderContext, opts: &RenderOptions, indent: usize) -> String {
    let mut out = String::new();
    for (idx, item) in items.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        let prefix = match item.marker {
            ListMarker::Unordered => "- ".to_string(),
            ListMarker::Ordered => "1. ".to_string(),
            ListMarker::Term => "- ".to_string(),
            ListMarker::Definition => "- ".to_string(),
        };
        out.push_str(&" ".repeat(indent));

        // render item blocks. if the first block is a paragraph, inline it on the list line.
        if let Some(first) = item.blocks.first() {
            match &first.kind {
                BlockKind::Paragraph { content: inlines } => {
                    out.push_str(&prefix);
                    out.push_str(render_inlines(inlines, ctx, opts).trim());

                    // render remaining blocks (including nested lists) indented.
                    for b in item.blocks.iter().skip(1) {
                        out.push('\n');
                        let rendered = render_block(b, ctx, opts);
                        out.push_str(&prefix_lines(&rendered, &format!("{}  ", " ".repeat(indent))));
                    }
                }
                _ => {
                    out.push_str(&prefix);
                    // no paragraph: render blocks on subsequent lines.
                    let rendered = render_block(first, ctx, opts);
                    out.push_str(&prefix_lines(&rendered, &format!("{}  ", " ".repeat(indent))));
                    for b in item.blocks.iter().skip(1) {
                        out.push('\n');
                        let rendered = render_block(b, ctx, opts);
                        out.push_str(&prefix_lines(&rendered, &format!("{}  ", " ".repeat(indent))));
                    }
                }
            }
        } else {
            out.push_str(&prefix);
        }
    }
    out
}

fn render_code_block(
    kind: CodeBlockKind,
    lang: Option<&str>,
    text: &str,
    _ctx: &mut RenderContext,
    opts: &RenderOptions,
) -> String {
    match kind {
        CodeBlockKind::LeadingSpace if opts.leading_space_as_blockquote => {
            // treat as quoted text (matches the legacy behavior for chessprogramming pages).
            prefix_lines(text.trim_end_matches('\n'), "> ")
        }
        _ => {
            let mut out = String::new();
            out.push_str("```");
            if let Some(l) = lang
                && !l.trim().is_empty() {
                    out.push_str(l.trim());
                }
            out.push('\n');
            out.push_str(text.trim_end_matches('\n'));
            out.push_str("\n```");
            out
        }
    }
}

fn render_html_block(node: &HtmlBlock, ctx: &mut RenderContext, opts: &RenderOptions) -> String {
    let mut out = String::new();
    out.push('<');
    out.push_str(&node.name);
    for a in &node.attrs {
        out.push(' ');
        out.push_str(&a.name);
        if let Some(v) = &a.value {
            out.push_str("=\"");
            out.push_str(v);
            out.push('"');
        }
    }

    if node.self_closing {
        out.push_str(" />");
        return out;
    }

    out.push('>');

    if !node.children.is_empty() {
        out.push('\n');
        for (i, b) in node.children.iter().enumerate() {
            if i > 0 {
                out.push_str("\n\n");
            }
            out.push_str(&render_block(b, ctx, opts));
        }
        out.push('\n');
    }

    out.push_str(&format!("</{}>", node.name));
    out
}

fn render_table(table: &Table, ctx: &mut RenderContext, opts: &RenderOptions) -> String {
    // basic Markdown table rendering.
    // we flatten cell blocks into a single line of text.
    let mut rows: Vec<Vec<String>> = Vec::new();
    for row in &table.rows {
        let mut cols: Vec<String> = Vec::new();
        for cell in &row.cells {
            cols.push(render_table_cell(cell, ctx, opts));
        }
        rows.push(cols);
    }

    if rows.is_empty() {
        return String::new();
    }

    let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    for r in &mut rows {
        while r.len() < col_count {
            r.push(String::new());
        }
    }

    let mut out = String::new();
    let header_row_idx = rows
        .iter()
        .position(|_| true)
        .unwrap_or(0);
    let header = &rows[header_row_idx];

    out.push('|');
    for cell in header {
        out.push(' ');
        out.push_str(&escape_table_cell(cell));
        out.push(' ');
        out.push('|');
    }
    out.push('\n');

    out.push('|');
    for _ in 0..col_count {
        out.push_str(" --- |");
    }
    out.push('\n');

    for (ri, row) in rows.iter().enumerate() {
        if ri == header_row_idx {
            continue;
        }
        out.push('|');
        for cell in row {
            out.push(' ');
            out.push_str(&escape_table_cell(cell));
            out.push(' ');
            out.push('|');
        }
        out.push('\n');
    }

    out.trim_end_matches('\n').to_string()
}

fn render_table_cell(cell: &TableCell, ctx: &mut RenderContext, opts: &RenderOptions) -> String {
    let mut parts: Vec<String> = Vec::new();
    for b in &cell.blocks {
        let s = render_block(b, ctx, opts);
        let s = s.replace('\n', " ");
        let s = s.trim().to_string();
        if !s.is_empty() {
            parts.push(s);
        }
    }
    parts.join(" ")
}

fn render_references(ctx: &mut RenderContext) -> String {
    if ctx.refs.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    for (i, r) in ctx.refs.iter().enumerate() {
        let n = i + 1;
        let body = r.trim();
        if body.is_empty() {
            out.push_str(&format!("[^{}]:\n", n));
        } else {
            out.push_str(&format!("[^{}]: {}\n", n, body));
        }
    }
    out.trim_end_matches('\n').to_string()
}

fn render_inlines(inlines: &[InlineNode], ctx: &mut RenderContext, opts: &RenderOptions) -> String {
    let mut out = String::new();
    for node in inlines {
        out.push_str(&render_inline(node, ctx, opts));
    }
    out
}

fn render_inline(node: &InlineNode, ctx: &mut RenderContext, opts: &RenderOptions) -> String {
    match &node.kind {
        InlineKind::Text { value } => {
            // normalize raw newlines into spaces for Markdown paragraphs.
            value.replace(['\r', '\n'], " ")
        }
        InlineKind::Bold { content } => format!("**{}**", render_inlines(content, ctx, opts)),
        InlineKind::Italic { content } => format!("*{}*", render_inlines(content, ctx, opts)),
        InlineKind::BoldItalic { content } => format!("***{}***", render_inlines(content, ctx, opts)),
        InlineKind::LineBreak => "<br/>".to_string(),
        InlineKind::InternalLink { link } => render_internal_link(link, ctx, opts),
        InlineKind::ExternalLink { link } => render_external_link(link, ctx, opts),
        InlineKind::FileLink { link } => render_file_link(link, ctx, opts),
        InlineKind::Template { node } => render_template(node, ctx, opts),
        InlineKind::Ref { node } => {
            let content = node
                .content
                .as_ref()
                .map(|c| render_inlines(c, ctx, opts))
                .unwrap_or_default();
            ctx.refs.push(content);
            format!("[^{}]", ctx.refs.len())
        }
        InlineKind::HtmlTag { node } => render_html_tag(node, ctx, opts),
        InlineKind::Raw { text } => text.clone(),
    }
}

fn render_internal_link(link: &InternalLink, ctx: &mut RenderContext, opts: &RenderOptions) -> String {
    let label = match &link.text {
        Some(nodes) => render_inlines(nodes, ctx, opts),
        None => link.target.replace('_', " "),
    };

    // in-page link
    if link.target.is_empty() {
        if let Some(anchor) = &link.anchor {
            return format!("[{}](#{})", label.trim(), normalize_anchor(anchor));
        }
        return label;
    }

    let article_id = crate::sanitize_article_id(&link.target);
    let bucket = crate::lower_first_letter_bucket(&article_id);
    let mut href = format!("../{}/{}.md", bucket, article_id);
    if let Some(anchor) = &link.anchor {
        href.push('#');
        href.push_str(&normalize_anchor(anchor));
    }
    format!("[{}]({})", label.trim(), href)
}

fn render_external_link(link: &ExternalLink, ctx: &mut RenderContext, opts: &RenderOptions) -> String {
    match &link.text {
        Some(nodes) => {
            let label = render_inlines(nodes, ctx, opts);
            format!("[{}]({})", label.trim(), link.url)
        }
        None => format!("<{}>", link.url),
    }
}

fn render_file_link(link: &FileLink, ctx: &mut RenderContext, opts: &RenderOptions) -> String {
    // best-effort: link to the File: page on chessprogramming.org.
    let file_target = link.target.replace(' ', "_");
    let file_page = format!("https://www.chessprogramming.org/File:{}", file_target);

    // caption: pick the last param that isn't an option-like token.
    let caption = link
        .params
        .last()
        .map(|p| render_inlines(&p.content, ctx, opts))
        .unwrap_or_else(|| link.target.clone());

    format!("[{}]({})", caption.trim(), file_page)
}

fn render_template(inv: &TemplateInvocation, ctx: &mut RenderContext, opts: &RenderOptions) -> String {
    match inv.name.kind {
        TemplateNameKind::ParserFunction if inv.name.raw.eq_ignore_ascii_case("#evu") => {
            // {{#evu:URL|...}} => just emit the URL as a link.
            let url = inv
                .params
                .first()
                .map(|p| render_inlines(&p.value, ctx, opts))
                .unwrap_or_default();
            if url.trim().is_empty() {
                "".to_string()
            } else {
                format!("[Video]({})", url.trim())
            }
        }
        _ => {
            // preserve unknown templates in a non-destructive way.
            let mut s = String::new();
            s.push_str("{{");
            s.push_str(&inv.name.raw);
            for p in &inv.params {
                s.push('|');
                if let Some(n) = &p.name {
                    s.push_str(n);
                    s.push('=');
                }
                s.push_str(&render_inlines(&p.value, ctx, opts));
            }
            s.push_str("}}");
            s
        }
    }
}

fn render_html_tag(tag: &HtmlTag, ctx: &mut RenderContext, opts: &RenderOptions) -> String {
    // conservative pass-through for most tags.
    // special-case <span id="...">...</span> => <a name="...">...</a> for stable anchors.
    if tag.name.eq_ignore_ascii_case("span")
        && let Some(id) = tag
            .attrs
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case("id"))
            .and_then(|a| a.value.as_ref())
        {
            let inner = render_inlines(&tag.children, ctx, opts);
            if inner.trim().is_empty() {
                return format!("<a name=\"{}\"></a>", id);
            }
            return format!("<a name=\"{}\">{}</a>", id, inner);
        }
    let mut out = String::new();
    out.push('<');
    out.push_str(&tag.name);
    for a in &tag.attrs {
        out.push(' ');
        out.push_str(&a.name);
        if let Some(v) = &a.value {
            out.push_str("=\"");
            out.push_str(v);
            out.push('"');
        }
    }
    if tag.self_closing {
        out.push_str(" />");
        return out;
    }

    out.push('>');
    out.push_str(&render_inlines(&tag.children, ctx, opts));
    out.push_str(&format!("</{}>", tag.name));
    out
}

fn normalize_anchor(anchor: &str) -> String {
    anchor.trim().replace(' ', "_")
}

fn escape_table_cell(s: &str) -> String {
    s.replace('|', "\\|")
}

fn prefix_lines(text: &str, prefix: &str) -> String {
    let mut out = String::new();
    for (i, line) in text.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(prefix);
        out.push_str(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_refs_as_footnotes_at_references_block() {
        let ast_file = AstFile {
            schema_version: SCHEMA_VERSION,
            parser: ParserInfo {
                name: PARSER_NAME.to_string(),
                version: PARSER_VERSION.to_string(),
            },
            span_encoding: SpanEncoding::default(),
            article_id: "Test".to_string(),
            source: SourceInfo {
                path: None,
                sha256: None,
                byte_len: 0,
            },
            diagnostics: vec![],
            document: Document {
                span: Span::new(0, 0),
                blocks: vec![
                    BlockNode {
                        span: Span::new(0, 0),
                        kind: BlockKind::Paragraph {
                            content: vec![
                                InlineNode {
                                    span: Span::new(0, 4),
                                    kind: InlineKind::Text {
                                        value: "Text".to_string(),
                                    },
                                },
                                InlineNode {
                                    span: Span::new(4, 4),
                                    kind: InlineKind::Ref {
                                        node: RefNode {
                                            attrs: vec![],
                                            content: Some(vec![InlineNode {
                                                span: Span::new(0, 8),
                                                kind: InlineKind::Text {
                                                    value: "Ref body".to_string(),
                                                },
                                            }]),
                                            self_closing: false,
                                        },
                                    },
                                },
                            ],
                        },
                    },
                    BlockNode {
                        span: Span::new(0, 0),
                        kind: BlockKind::References {
                            node: ReferencesNode { attrs: vec![] },
                        },
                    },
                ],
                categories: vec![],
                redirect: None,
            },
        };

        let md = render_doc(&ast_file.document);
        assert!(md.contains("Text[^1]"));
        assert!(md.contains("[^1]: Ref body"));
    }
}
