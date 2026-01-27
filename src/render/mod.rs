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

    /// Obsidian's Markdown renderer can misinterpret literal `*` characters
    /// in normal text as emphasis markers, even when surrounded by spaces.
    ///
    /// When enabled, any literal `*` that would otherwise be rendered as text
    /// (i.e., from plain text/Raw nodes, not the emphasis markers we emit for
    /// Bold/Italic) is replaced with `obsidian_text_asterisk_replacement`.
    pub obsidian_text_asterisk_workaround: bool,

    /// Text to replace `*` with when `obsidian_text_asterisk_workaround` is true.
    /// The default value is `&middot;`.
    pub obsidian_text_asterisk_replacement: String,

    /// If true, render standalone `[[File:...]]` links as Markdown images.
    pub render_file_links_as_images: bool,

    /// Base URL used for MediaWiki file resolution.
    ///
    /// For chessprogramming.org, this should be `https://www.chessprogramming.org`.
    pub mediawiki_base_url: String,

    /// Default width (in pixels) to request for embedded images.
    pub default_image_width_px: u32,

    /// If true, prefer a `NNNpx` option from the wikitext file params.
    ///
    /// We default this to `false` because Markdown/Obsidian layouts differ from
    /// the wiki and a stable default size is usually more readable.
    pub respect_wikitext_image_width: bool,

    /// If true, insert a horizontal rule (`---`) after the first top-of-document
    /// rendered figure/image block.
    pub insert_hr_after_top_image: bool,

    /// If true, include a `# References` heading when rendering references.
    pub emit_references_heading: bool,

    /// If true, emit a `<br/>` line before the references heading to visually
    /// separate it from preceding content.
    pub emit_br_before_references: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            leading_space_as_blockquote: true,
            obsidian_text_asterisk_workaround: true,
            obsidian_text_asterisk_replacement: "&middot;".to_string(),
            render_file_links_as_images: true,
            mediawiki_base_url: "https://www.chessprogramming.org".to_string(),
            default_image_width_px: 300,
            respect_wikitext_image_width: false,
            insert_hr_after_top_image: true,
            emit_references_heading: true,
            emit_br_before_references: true,
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
    let mut inserted_top_image_hr = false;
    let mut seen_heading = false;

    for (bi, block) in doc.blocks.iter().enumerate() {
        if !out.is_empty() {
            // separate blocks with a single blank line.
            out.push_str("\n\n");
        }

        let is_top_image = !seen_heading
            && opts.insert_hr_after_top_image
            && !inserted_top_image_hr
            && block_is_standalone_image_paragraph(block, opts);

        let rendered = match &block.kind {
            BlockKind::References { .. } => {
                let prev_is_refs_heading = bi
                    .checked_sub(1)
                    .and_then(|pi| doc.blocks.get(pi))
                    .map(|b| heading_is_named_references(b, opts))
                    .unwrap_or(false);

                render_references(&mut ctx, opts, /*emit_heading*/ !prev_is_refs_heading)
            }
            _ => render_block(block, &mut ctx, opts),
        };

        out.push_str(&rendered);

        if is_top_image {
            out.push_str("\n\n---");
            inserted_top_image_hr = true;
        }

        if matches!(&block.kind, BlockKind::Heading { .. }) {
            seen_heading = true;
        }
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
        BlockKind::Paragraph { content } => render_paragraph(content, ctx, opts),
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
        // most documents render references via `render_doc_with_options` so that
        // we can decide whether to emit a heading based on the surrounding context.
        BlockKind::References { .. } => render_references(ctx, opts, /*emit_heading*/ true),
        BlockKind::HtmlBlock { node } => render_html_block(node, ctx, opts),
        BlockKind::MagicWord { name } => format!("<!-- {} -->", name),
        BlockKind::Raw { text } => {
            // keep raw blocks visible but non-destructive.
            format!("```text\n{}\n```", text.trim_end_matches('\n'))
        }
    }
}

fn heading_is_named_references(block: &BlockNode, opts: &RenderOptions) -> bool {
    match &block.kind {
        BlockKind::Heading { content, .. } => {
            let mut dummy = RenderContext::default();
            render_inlines(content, &mut dummy, opts)
                .trim()
                .eq_ignore_ascii_case("references")
        }
        _ => false,
    }
}

fn block_is_standalone_image_paragraph(block: &BlockNode, opts: &RenderOptions) -> bool {
    if !opts.render_file_links_as_images {
        return false;
    }
    match &block.kind {
        BlockKind::Paragraph { content } => extract_standalone_file_link(content)
            .is_some_and(|l| matches!(l.namespace, FileNamespace::File | FileNamespace::Image)),
        _ => false,
    }
}

fn render_paragraph(content: &[InlineNode], ctx: &mut RenderContext, opts: &RenderOptions) -> String {
    if opts.render_file_links_as_images
        && let Some(link) = extract_standalone_file_link(content)
        && matches!(link.namespace, FileNamespace::File | FileNamespace::Image)
    {
        return render_file_figure(link, ctx, opts);
    }
    render_inlines(content, ctx, opts)
}

fn extract_standalone_file_link(content: &[InlineNode]) -> Option<&FileLink> {
    let mut file: Option<&FileLink> = None;
    for node in content {
        match &node.kind {
            InlineKind::FileLink { link } => {
                if file.is_some() {
                    return None;
                }
                file = Some(link);
            }
            InlineKind::Text { value } => {
                if !value.trim().is_empty() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    file
}

fn render_file_figure(link: &FileLink, ctx: &mut RenderContext, opts: &RenderOptions) -> String {
    let caption_param = link
        .params
        .iter()
        .rev()
        .find(|p| !file_param_is_option_like(p));

    let caption_inlines: Vec<InlineNode> = match caption_param {
        Some(p) => p.content.clone(),
        None => {
            // FileLink has no span; this node is synthetic and only used for rendering.
            // use a best-effort span from existing params (if any), otherwise default.
            let span = link.params.first().map(|p| p.span).unwrap_or_default();

            vec![InlineNode {
                span,
                kind: InlineKind::Text {
                    value: link.target.clone(),
                },
            }]
        }
    };

    // split the caption into the visible portion and any `<ref>` markers.
    let mut display: Vec<InlineNode> = Vec::new();
    let mut ref_nodes: Vec<&InlineNode> = Vec::new();
    for n in &caption_inlines {
        if matches!(n.kind, InlineKind::Ref { .. }) {
            ref_nodes.push(n);
        } else {
            display.push(n.clone());
        }
    }

    let caption_text = render_inlines(&display, ctx, opts).trim().to_string();
    let alt = if caption_text.is_empty() {
        link.target.trim().to_string()
    } else {
        caption_text.clone()
    };

    let width = if opts.respect_wikitext_image_width {
        file_link_width_px(link).unwrap_or(opts.default_image_width_px)
    } else {
        opts.default_image_width_px
    };
    let url = mediawiki_file_thumb_url(&opts.mediawiki_base_url, &link.target, width);

    let mut refs = String::new();
    for rn in ref_nodes {
        refs.push_str(&render_inline(rn, ctx, opts));
    }

    // keep the caption on the same line as the image using HTML.
    format!(
        "![{}]({})<br />*{}*{}",
        alt.trim(),
        url,
        alt.trim(),
        refs
    )
}

fn mediawiki_file_thumb_url(base: &str, filename: &str, width_px: u32) -> String {
    let base = base.trim_end_matches('/');
    let name = canonicalize_mediawiki_filename(filename);

    // MediaWiki stores files under /images/<h1>/<h2>/<name>, where <h1> and <h2>
    // are derived from the MD5 of the canonical filename.
    let digest = md5::compute(name.as_bytes());
    let hex = format!("{:x}", digest);
    let h1 = &hex[0..1];
    let h2 = &hex[0..2];

    // match the common MediaWiki thumbnail URL format:
    // /images/thumb/<h1>/<h2>/<name>/<width>px-<name>
    if width_px > 0 {
        format!(
            "{}/images/thumb/{}/{}/{}/{}px-{}",
            base, h1, h2, name, width_px, name
        )
    } else {
        // fallback to the original file URL.
        format!("{}/images/{}/{}/{}", base, h1, h2, name)
    }
}

fn canonicalize_mediawiki_filename(filename: &str) -> String {
    let trimmed = filename.trim().replace(' ', "_");
    let mut chars = trimmed.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut out = String::new();
    for c in first.to_uppercase() {
        out.push(c);
    }
    out.push_str(chars.as_str());
    out
}

fn file_link_width_px(link: &FileLink) -> Option<u32> {
    for p in &link.params {
        let Some(token) = file_param_plain_text(p) else {
            continue;
        };
        if let Some(px) = parse_px(token.trim()) {
            return Some(px);
        }
    }
    None
}

fn file_param_plain_text(p: &FileParam) -> Option<String> {
    let mut s = String::new();
    for n in &p.content {
        match &n.kind {
            InlineKind::Text { value } => s.push_str(value),
            InlineKind::Raw { text } => s.push_str(text),
            _ => return None,
        }
    }
    Some(s)
}

fn file_param_is_option_like(p: &FileParam) -> bool {
    let Some(raw) = file_param_plain_text(p) else {
        return false;
    };
    let t = raw.trim().to_ascii_lowercase();
    if t.is_empty() {
        return true;
    }
    matches!(
        t.as_str(),
        "thumb" | "thumbnail" | "frame" | "frameless" | "border" | "right" | "left" | "center" | "none" | "upright"
    ) || parse_px(&t).is_some()
}

fn parse_px(s: &str) -> Option<u32> {
    let s = s.trim();
    let s = s.strip_suffix("px")?;
    if s.is_empty() {
        return None;
    }
    if !s.as_bytes().iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<u32>().ok().filter(|n| *n > 0 && *n <= 4096)
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

fn render_references(ctx: &mut RenderContext, opts: &RenderOptions, emit_heading: bool) -> String {
    if ctx.refs.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    if emit_heading && opts.emit_br_before_references {
        out.push_str("<br/>\n\n");
    }
    if emit_heading && opts.emit_references_heading {
        out.push_str("# References\n\n");
    }
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
    // Obsidian misinterprets multiple literal asterisks in normal text as emphasis
    // markers, even when surrounded by spaces.
    //
    // when enabled, the code replaces `*` in plain text/Raw nodes with a safer token
    // (default: `&middot;`). the code does not touch the `*` characters if they're
    // emphasis or part of a list.
    let apply_star_workaround = opts.obsidian_text_asterisk_workaround;

    let mut out = String::new();
    for node in inlines {
        // footnote markers should attach to the preceding token (no extra space).
        if matches!(node.kind, InlineKind::Ref { .. }) {
            while matches!(out.as_bytes().last(), Some(b' ' | b'\t')) {
                out.pop();
            }
        }

        let mut rendered = render_inline(node, ctx, opts);

        if apply_star_workaround {
            match node.kind {
                InlineKind::Text { .. } | InlineKind::Raw { .. } => {
                    rendered = rendered.replace('*', &opts.obsidian_text_asterisk_replacement);
                }
                _ => {}
            }
        }

        // if the previous inline emitted an explicit newline (e.g. <br/>\n),
        // strip leading spaces on the next fragment for cleaner output.
        if out.ends_with('\n') {
            let trimmed = rendered.trim_start_matches([' ', '\t']);
            if trimmed.len() != rendered.len() {
                rendered = trimmed.to_string();
            }
        }

        out.push_str(&rendered);
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
        // emit a real newline after the HTML break so that Markdown renderers (e.g., Obsidian)
        // don't treat the following text as part of the same visual line.
        InlineKind::LineBreak => "<br/>\n".to_string(),
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
    // best-effort: link to the "File:" page on the configured MediaWiki base.
    let base = opts.mediawiki_base_url.trim_end_matches('/');
    let file_target = link.target.replace(' ', "_");
    let file_page = format!("{}/File:{}", base, file_target);

    // caption: pick the last param that isn't an option-like token;
    // fall back to the file name.
    let caption_param = link
        .params
        .iter()
        .rev()
        .find(|p| !file_param_is_option_like(p));
    let caption = caption_param
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
    use crate::parse::*;
    use super::*;

    #[test]
    fn obsidian_replaces_single_literal_asterisk_in_text() {
        // defensively rewrite literal asterisks in normal text to a safer token.
        let src = "A * B\n";
        let parsed = parse_document(src);
        let md = render_doc(&parsed.document);

        assert!(
            md.contains("A &middot; B"),
            "expected literal '*' in text to be replaced with '&middot;': {md}"
        );

        // But we should still be able to emit Markdown emphasis markers elsewhere.
        let src2 = "''Italic''\n";
        let parsed2 = parse_document(src2);
        let md2 = render_doc(&parsed2.document);
        assert!(
            md2.contains("*Italic*"),
            "expected italic markers to remain '*' (not replaced): {md2}"
        );
    }

    #[test]
    fn barend_swets_markdown_formatting_features() {
        // tests:
        // - literal-asterisk substitution workaround
        // - file links with nested links in captions
        // - `<ref>` extraction (including refs in file captions)
        // - leading-space block quotes (including blank-line continuation)
        // - reference placement and formatting
        let src = r#"'''[[Main Page|Home]] * [[People]] * Barend Swets'''

[[FILE:BarendSwets.jpg|border|right|thumb|200px| Barend Swets <ref>Image from [[Barend Swets]] ('''1977'''). ''Computers in de opmars''. Schakend Nederland 09-1977 (Dutch), [http://example.com pdf] hosted by [[Hein Veldhuis]]</ref> ]] 

'''Barend Swets''',<br/>
a Dutch engineer <ref>Bio ref</ref>.

=Quotes=
==1997==
By [[Robert Hyatt]], 1997 <ref>Quote ref</ref>:
 Problem is, no one else has stepped forward in [[WCCC 1977|1977]].


 Problem continues after a blank line.

<references />
"#;

        let parsed = parse_document(src);
        let md = render_doc(&parsed.document);

        // asterisks in plain text become middots, but bold markers remain.
        assert!(md.contains("&middot;"), "expected Obsidian middot workaround in output: {md}");

        // file links become a figure-like Markdown image block.
        assert!(
            md.contains(
                "![Barend Swets](https://www.chessprogramming.org/images/thumb/a/a9/BarendSwets.jpg/300px-BarendSwets.jpg)<br />*Barend Swets*[^1]"
            ),
            "expected file link to render as an image figure: {md}"
        );

        // the top-of-document image gets a horizontal rule separator.
        assert!(md.contains("\n\n---\n\n"), "expected horizontal rule after top image: {md}");

        // `<br/>` should force a newline and not leave a leading space.
        assert!(
            md.contains("**Barend Swets**,<br/>\na Dutch engineer"),
            "expected `<br/>` to be followed by a newline in Markdown: {md}"
        );

        // the quote should render as a Markdown blockquote, and the internal link inside should render.
        assert!(md.contains("\n> Problem is, no one else"), "expected blockquote rendering: {md}");
        assert!(md.contains("[1977](../w/WCCC_1977.md)"), "expected internal link rendering inside blockquote: {md}");

        // blank lines inside leading-space quotes should not terminate the quote.
        assert!(
            md.contains("> \n> Problem continues"),
            "expected blank-line continuation inside blockquote: {md}"
        );

        // refs should attach without a preceding space.
        assert!(md.contains("1997[^"), "expected ref marker to attach to preceding token: {md}");

        // refs should not leak raw `<ref>` tags.
        assert!(!md.contains("<ref>"), "did not expect literal `<ref>` tags in Markdown: {md}");

        // the references section should be emitted and include the first ref from the image caption.
        // we also emit a `<br/>` spacer before the heading for readability in Obsidian.
        assert!(
            md.contains("\n\n<br/>\n\n# References"),
            "expected a `<br/>` spacer before the references heading: {md}"
        );
        assert!(md.contains("[^1]: Image from [Barend Swets](../b/Barend_Swets.md)"), "expected first reference to be the image caption ref: {md}");
        assert!(md.contains("hosted by [Hein Veldhuis](../h/Hein_Veldhuis.md)"), "expected nested internal link inside the image ref to render: {md}");
        assert!(md.contains("[pdf](http://example.com)"), "expected external link inside the image ref to render: {md}");
    }

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
