//! Wikitext -> AST parser.
//!
//! This parser is intentionally **incremental** and **error-tolerant**.
//! It aims to provide:
//! - Stable byte `Span`s into the raw input.
//! - A debuggable AST suitable for JSON serialization.
//! - Reasonable recovery for malformed markup.
//!
//! The current implementation targets the subset of Wikitext used by
//! https://chessprogramming.org pages (headings, paragraphs, lists,
//! links, refs, basic HTML tags, templates, and MediaWiki tables).

mod table;
mod util;

use crate::ast::*;

use util::{collect_lines, line_trimmed_start, parse_html_attrs, strip_cr};

/// Result of parsing a document.
#[derive(Debug, Clone)]
pub struct ParseOutput {
    pub document: Document,
    pub diagnostics: Vec<Diagnostic>,
}

/// Parse a `.wiki` file (Wikitext) into an AST `Document`.
///
/// Spans are byte offsets into the raw `src` input.
pub fn parse_document(src: &str) -> ParseOutput {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut blocks: Vec<BlockNode> = Vec::new();
    let mut categories: Vec<CategoryTag> = Vec::new();
    let mut redirect: Option<Redirect> = None;

    let lines = collect_lines(src);
    let mut i: usize = 0;

    // redirect is typically the first non-empty line.
    while i < lines.len() {
        let line = lines[i];
        let text = strip_cr(&src[line.start..line.end]);
        if text.trim().is_empty() {
            i += 1;
            continue;
        }
        if let Some(r) = try_parse_redirect(src, line, text) {
            redirect = Some(r);
            i += 1;
        }
        break;
    }

    while i < lines.len() {
        let line = lines[i];
        let raw = &src[line.start..line.end];
        let text = strip_cr(raw);
        let trimmed = text.trim();

        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        // categories as metadata (often at the bottom of the page)
        if let Some(cat) = try_parse_category(line, text) {
            categories.push(cat);
            i += 1;
            continue;
        }

        // block-level <references />
        if let Some(node) = try_parse_references(line, text) {
            blocks.push(BlockNode {
                span: Span::new(line.start as u64, line.end as u64),
                kind: BlockKind::References { node },
            });
            i += 1;
            continue;
        }

        // magic words like __TOC__
        if let Some(name) = try_parse_magic_word(trimmed) {
            blocks.push(BlockNode {
                span: Span::new(line.start as u64, line.end as u64),
                kind: BlockKind::MagicWord { name },
            });
            i += 1;
            continue;
        }

        // horizontal rule
        if trimmed == "----" {
            blocks.push(BlockNode {
                span: Span::new(line.start as u64, line.end as u64),
                kind: BlockKind::HorizontalRule,
            });
            i += 1;
            continue;
        }

        // headings
        if let Some((level, inner_start, inner_end)) = try_parse_heading(src, line, text) {
            let content_slice = &src[inner_start..inner_end];
            let inlines = util::parse_inlines(src, inner_start, content_slice, &mut diagnostics);
            blocks.push(BlockNode {
                span: Span::new(line.start as u64, line.end as u64),
                kind: BlockKind::Heading {
                    level,
                    content: inlines,
                },
            });
            i += 1;
            continue;
        }

        // tables
        if line_trimmed_start(src, line).starts_with("{|") {
            match table::parse_table(src, &lines, i, &mut diagnostics) {
                Ok((node, next_i)) => {
                    blocks.push(node);
                    if next_i <= i {
                        diagnostics.push(Diagnostic {
                            severity: Severity::Error,
                            phase: Some(DiagnosticPhase::Parse),
                            code: Some("wikitext.table.parse_failed".to_string()),
                            message: format!("Table parsing error: next index ({next_i}) is not greater than current index ({i})"),
                            span: Some(Span::new(line.start as u64, line.end as u64)),
                            notes: vec![],
                        });
                        i += 1;
                        continue;
                    }
                    i = next_i;
                    continue;
                }
                Err(e) => {
                    diagnostics.push(Diagnostic {
                        severity: Severity::Warning,
                        phase: Some(DiagnosticPhase::Parse),
                        code: Some("wikitext.table.parse_failed".to_string()),
                        message: format!("Failed to parse table: {e}"),
                        span: Some(Span::new(line.start as u64, line.end as u64)),
                        notes: vec![],
                    });
                    // fall back to raw block.
                    blocks.push(BlockNode {
                        span: Span::new(line.start as u64, line.end as u64),
                        kind: BlockKind::Raw {
                            text: raw.to_string(),
                        },
                    });
                    i += 1;
                    continue;
                }
            }
        }

        // <pre> and <syntaxhighlight> code blocks.
        if let Some((node, next_i)) = try_parse_code_block(src, &lines, i, &mut diagnostics) {
            blocks.push(node);
            if next_i <= i {
                diagnostics.push(Diagnostic {
                    severity: Severity::Error,
                    phase: Some(DiagnosticPhase::Parse),
                    code: Some("wikitext.html_codeblock.parse_failed".to_string()),
                    message: format!("Code block parsing error: next index ({next_i}) is not greater than current index ({i})"),
                    span: Some(Span::new(line.start as u64, line.end as u64)),
                    notes: vec![],
                });
                i += 1;
                continue;
            }
            i = next_i;
            continue;
        }

        // leading-space preformatted blocks.
        if text.starts_with(' ') {
            let (node, next_i) = parse_leading_space_block(src, &lines, i);
            blocks.push(node);
            if next_i <= i {
                diagnostics.push(Diagnostic {
                    severity: Severity::Error,
                    phase: Some(DiagnosticPhase::Parse),
                    code: Some("wikitext.indented_codeblock.parse_failed".to_string()),
                    message: format!("Code block parsing error: next index ({next_i}) is not greater than current index ({i})"),
                    span: Some(Span::new(line.start as u64, line.end as u64)),
                    notes: vec![],
                });
                i += 1;
                continue;
            }
            i = next_i;
            continue;
        }

        // lists
        if is_list_line(text) {
            let (node, next_i) = parse_list_block(src, &lines, i, &mut diagnostics);
            blocks.push(node);
            if next_i <= i {
                diagnostics.push(Diagnostic {
                    severity: Severity::Error,
                    phase: Some(DiagnosticPhase::Parse),
                    code: Some("wikitext.list.parse_failed".to_string()),
                    message: format!("List parsing error: next index ({next_i}) is not greater than current index ({i})"),
                    span: Some(Span::new(line.start as u64, line.end as u64)),
                    notes: vec![],
                });
                i += 1;
                continue;
            }
            i = next_i;
            continue;
        }

        // paragraph: gather until blank or a block-start.
        let start_i = i;
        let para_start = lines[start_i].start;
        let mut end_i = i;
        while end_i < lines.len() {
            let ln = lines[end_i];
            let t = strip_cr(&src[ln.start..ln.end]);
            if t.trim().is_empty() {
                break;
            }
            if is_block_start(src, ln, t) {
                break;
            }
            end_i += 1;
        }

        if end_i == start_i {
            // should not happen due to the block-start handling above.
            i += 1;
            continue;
        }

        let para_end = lines[end_i - 1].end;
        let slice = &src[para_start..para_end];
        let inlines = util::parse_inlines(src, para_start, slice, &mut diagnostics);
        blocks.push(BlockNode {
            span: Span::new(para_start as u64, para_end as u64),
            kind: BlockKind::Paragraph { content: inlines },
        });
        i = end_i;
    }

    let doc = Document {
        span: Span::new(0, src.len() as u64),
        blocks,
        categories,
        redirect,
    };

    ParseOutput {
        document: doc,
        diagnostics,
    }
}

fn try_parse_redirect(_src: &str, line: util::LineRange, text: &str) -> Option<Redirect> {
    let trimmed = text.trim_start();
    let upper = trimmed.to_ascii_uppercase();
    if !upper.starts_with("#REDIRECT") {
        return None;
    }
    let leading_ws = text.len().saturating_sub(trimmed.len());
    // find the first internal link after #REDIRECT.
    if let Some(open) = trimmed.find("[[")
        && let Some(close_rel) = trimmed[open + 2..].find("]]")
    {
        let close = open + 2 + close_rel;
        let inner = &trimmed[open + 2..close];
        let (target, anchor) = split_target_anchor(inner.trim());
        let start_abs = line.start + leading_ws + open;
        let end_abs = line.start + leading_ws + close + 2;
        return Some(Redirect {
            span: Span::new(start_abs as u64, end_abs as u64),
            target: target.to_string(),
            anchor: anchor.map(|s| s.to_string()),
        });
    }
    // fallback span: whole line.
    Some(Redirect {
        span: Span::new(line.start as u64, line.end as u64),
        target: String::new(),
        anchor: None,
    })
}

fn try_parse_category(line: util::LineRange, text: &str) -> Option<CategoryTag> {
    let trimmed = text.trim();
    // common form: [[Category:Name|Sort]]
    if !trimmed.starts_with("[[") || !trimmed.ends_with("]]") {
        return None;
    }
    let inner = &trimmed[2..trimmed.len() - 2];
    let inner_trim = inner.trim_start();
    if !inner_trim.to_ascii_lowercase().starts_with("category:") {
        return None;
    }
    let after = &inner_trim["category:".len()..];
    let (name_raw, sort_raw) = match after.split_once('|') {
        Some((a, b)) => (a.trim(), Some(b.trim())),
        None => (after.trim(), None),
    };
    let sort_key = sort_raw.filter(|s| !s.is_empty()).map(|s| s.to_string());
    Some(CategoryTag {
        span: Span::new(line.start as u64, line.end as u64),
        name: name_raw.to_string(),
        sort_key,
    })
}

fn try_parse_references(_line: util::LineRange, text: &str) -> Option<ReferencesNode> {
    let trimmed = text.trim();
    let lower = trimmed.to_ascii_lowercase();
    if !lower.starts_with("<references") {
        return None;
    }
    if !lower.ends_with("/>") && !lower.ends_with("></references>") {
        return None;
    }
    // parse attributes in the opening tag.
    // <references ... />
    if let Some(end) = trimmed.find('>') {
        let open = &trimmed[..=end];
        let attrs_str = open
            .trim_start_matches('<')
            .trim_start_matches(|c: char| c.is_ascii_alphabetic())
            .trim();
        let attrs_str = attrs_str.trim_end_matches("/>").trim_end_matches('>');
        let attrs = parse_html_attrs(attrs_str);
        return Some(ReferencesNode { attrs });
    }
    Some(ReferencesNode { attrs: vec![] })
}

fn try_parse_magic_word(trimmed: &str) -> Option<String> {
    // very small subset for now.
    if trimmed.starts_with("__") && trimmed.ends_with("__") && trimmed.len() > 4 {
        return Some(trimmed.to_string());
    }
    None
}

fn try_parse_heading(src: &str, line: util::LineRange, _text: &str) -> Option<(u8, usize, usize)> {
    // allow leading whitespace, but compute spans against raw `src`.
    let raw_line = &src[line.start..line.end];
    let mut offset_in_line = 0usize;
    for (idx, ch) in raw_line.char_indices() {
        if ch == ' ' || ch == '\t' {
            offset_in_line = idx + ch.len_utf8();
            continue;
        }
        break;
    }

    let trimmed_start = &raw_line[offset_in_line..];
    let bytes = trimmed_start.as_bytes();
    let mut n = 0usize;
    while n < bytes.len() && bytes[n] == b'=' {
        n += 1;
    }
    if n == 0 || n > 6 {
        return None;
    }
    // must also end with the same number of '=' (ignoring trailing whitespace).
    let mut end_idx = trimmed_start.len();
    while end_idx > 0 {
        let ch = trimmed_start[..end_idx].chars().last().unwrap();
        if ch.is_whitespace() {
            end_idx -= ch.len_utf8();
            continue;
        }
        break;
    }
    if end_idx < n * 2 {
        return None;
    }
    let tail = &trimmed_start[..end_idx];
    if !tail.ends_with(&"=".repeat(n)) {
        return None;
    }
    // inner range (exclude the '=' runs).
    let mut inner_start_rel = n;
    let mut inner_end_rel = tail.len() - n;
    // trim whitespace inside.
    while inner_start_rel < inner_end_rel {
        let ch = tail[inner_start_rel..].chars().next().unwrap();
        if ch.is_whitespace() {
            inner_start_rel += ch.len_utf8();
        } else {
            break;
        }
    }
    while inner_end_rel > inner_start_rel {
        let ch = tail[..inner_end_rel].chars().last().unwrap();
        if ch.is_whitespace() {
            inner_end_rel -= ch.len_utf8();
        } else {
            break;
        }
    }
    let inner_start = line.start + offset_in_line + inner_start_rel;
    let inner_end = line.start + offset_in_line + inner_end_rel;
    // level: n '=' => level n (allow level1).
    let level = n as u8;
    // validate that the slice is UTF-8 aligned (it should be).
    let _ = &src[inner_start..inner_end];
    Some((level, inner_start, inner_end))
}

fn try_parse_code_block(
    src: &str,
    lines: &[util::LineRange],
    start_i: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(BlockNode, usize)> {
    let line = lines[start_i];
    let trimmed = line_trimmed_start(src, line);
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("<pre") {
        return parse_tagged_code_block(
            src,
            lines,
            start_i,
            "pre",
            CodeBlockKind::PreTag,
            diagnostics,
        );
    }
    if lower.starts_with("<syntaxhighlight") {
        return parse_tagged_code_block(
            src,
            lines,
            start_i,
            "syntaxhighlight",
            CodeBlockKind::SyntaxHighlight,
            diagnostics,
        );
    }
    None
}

fn parse_tagged_code_block(
    src: &str,
    lines: &[util::LineRange],
    start_i: usize,
    tag: &str,
    kind: CodeBlockKind,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(BlockNode, usize)> {
    let start_line = lines[start_i];
    let start_abs = start_line.start
        + (src[start_line.start..start_line.end].len() - line_trimmed_start(src, start_line).len());
    let remaining = &src[start_abs..];
    let open_pat = format!("<{}", tag);
    if remaining.len() < open_pat.len()
        || !remaining[..open_pat.len()].eq_ignore_ascii_case(&open_pat)
    {
        return None;
    }
    // find end of opening tag.
    let open_end_rel = remaining.find('>')?;
    let open_end_abs = start_abs + open_end_rel + 1;
    let open_tag = &src[start_abs..open_end_abs];
    let attrs_str = open_tag
        .trim_start_matches('<')
        .trim_start_matches(|c: char| c.is_ascii_alphabetic())
        .trim();
    let attrs_str = attrs_str.trim_end_matches('>').trim_end_matches('/').trim();
    let attrs = parse_html_attrs(attrs_str);
    let lang = attrs
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case("lang"))
        .and_then(|a| a.value.clone());

    let close_pat = format!("</{}>", tag);
    let search_haystack = &remaining[open_end_rel + 1..];

    // search using byte windows
    // this replaces .to_ascii_lowercase().find() without the allocation
    let close_rel = search_haystack
        .as_bytes()
        .windows(close_pat.len())
        .position(|window| window.eq_ignore_ascii_case(close_pat.as_bytes()));

    let Some(close_rel) = close_rel else {
        diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            phase: Some(DiagnosticPhase::Parse),
            code: Some("wikitext.codeblock.unclosed".to_string()),
            message: format!("Unclosed <{}> tag", tag),
            span: Some(Span::new(
                start_abs as u64,
                (start_abs + open_end_rel + 1) as u64,
            )),
            notes: vec![],
        });
        // consume only this line.
        let node = BlockNode {
            span: Span::new(start_abs as u64, start_line.end as u64),
            kind: BlockKind::CodeBlock {
                block: CodeBlock {
                    kind,
                    lang,
                    text: String::new(),
                },
            },
        };
        return Some((node, start_i + 1));
    };
    let close_start_abs = start_abs + open_end_rel + 1 + close_rel;
    let close_end_abs = close_start_abs + close_pat.len();
    let code_text = &src[open_end_abs..close_start_abs];

    // determine how many lines we consumed.
    let mut next_i = start_i;
    while next_i < lines.len() && lines[next_i].end_with_newline <= close_end_abs {
        next_i += 1;
    }

    let node = BlockNode {
        span: Span::new(start_abs as u64, close_end_abs as u64),
        kind: BlockKind::CodeBlock {
            block: CodeBlock {
                kind,
                lang,
                text: code_text.to_string(),
            },
        },
    };
    Some((node, next_i))
}

fn parse_leading_space_block(
    src: &str,
    lines: &[util::LineRange],
    start_i: usize,
) -> (BlockNode, usize) {
    let mut i = start_i;
    let start = lines[start_i].start;
    let mut end = lines[start_i].end;
    let mut buf = String::new();
    while i < lines.len() {
        let line = lines[i];
        let text = strip_cr(&src[line.start..line.end]);
        if !text.starts_with(' ') {
            break;
        }
        end = line.end;
        let mut content = text;
        if let Some(rest) = content.strip_prefix(' ') {
            content = rest;
        }
        buf.push_str(content);
        buf.push('\n');
        i += 1;
    }
    if buf.ends_with('\n') {
        buf.pop();
    }
    (
        BlockNode {
            span: Span::new(start as u64, end as u64),
            kind: BlockKind::CodeBlock {
                block: CodeBlock {
                    kind: CodeBlockKind::LeadingSpace,
                    lang: None,
                    text: buf,
                },
            },
        },
        i,
    )
}

fn is_list_line(text: &str) -> bool {
    let trimmed = text.trim_start();
    matches!(trimmed.chars().next(), Some('*' | '#' | ';' | ':'))
}

fn parse_list_block(
    src: &str,
    lines: &[util::LineRange],
    start_i: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> (BlockNode, usize) {
    // collect contiguous list lines.
    let mut i = start_i;
    let mut list_lines: Vec<(util::LineRange, String, usize, String)> = Vec::new();
    // (line_range, prefix, content_start_abs, content_slice)

    while i < lines.len() {
        let lr = lines[i];
        let raw = strip_cr(&src[lr.start..lr.end]);
        if raw.trim().is_empty() {
            break;
        }
        if !is_list_line(raw) {
            break;
        }
        let trimmed = raw.trim_start();
        let leading_ws = raw.len() - trimmed.len();
        let mut prefix = String::new();
        for ch in trimmed.chars() {
            match ch {
                '*' | '#' | ';' | ':' => prefix.push(ch),
                _ => break,
            }
        }
        if prefix.is_empty() {
            break;
        }
        let content_start_rel = leading_ws + prefix.len();
        let mut content_start_abs = lr.start + content_start_rel;
        // skip one optional space after markers.
        if src[content_start_abs..lr.end].starts_with(' ') {
            content_start_abs += 1;
        }
        let content_slice = src[content_start_abs..lr.end].to_string();
        list_lines.push((lr, prefix, content_start_abs, content_slice));
        i += 1;
    }

    // build nested lists with a stack of contexts.
    #[derive(Debug)]
    struct ListCtx {
        items: Vec<ListItem>,
    }

    fn attach_child_list(parent: &mut ListItem, child: ListCtx) {
        if child.items.is_empty() {
            return;
        }
        let span = child
            .items
            .first()
            .unwrap()
            .span
            .cover(child.items.last().unwrap().span);
        parent.blocks.push(BlockNode {
            span,
            kind: BlockKind::List { items: child.items },
        });
    }

    let mut stack: Vec<ListCtx> = vec![ListCtx { items: Vec::new() }];

    for (lr, prefix, content_start_abs, _content_owned) in list_lines {
        let depth = prefix.chars().count().max(1);
        let last_marker_ch = prefix.chars().last().unwrap();
        let marker = match last_marker_ch {
            '*' => ListMarker::Unordered,
            '#' => ListMarker::Ordered,
            ';' => ListMarker::Term,
            ':' => ListMarker::Definition,
            _ => ListMarker::Unordered,
        };

        // pop contexts until we are at the desired depth.
        while stack.len() > depth {
            let child = stack.pop().unwrap();
            if let Some(parent_ctx) = stack.last_mut() {
                if let Some(parent_item) = parent_ctx.items.last_mut() {
                    attach_child_list(parent_item, child);
                } else {
                    // no parent item: flatten.
                    parent_ctx.items.extend(child.items);
                }
            }
        }

        // push contexts if the list is getting deeper.
        while stack.len() < depth {
            if let Some(parent_ctx) = stack.last_mut()
                && parent_ctx.items.is_empty()
            {
                diagnostics.push(Diagnostic {
                    severity: Severity::Warning,
                    phase: Some(DiagnosticPhase::Parse),
                    code: Some("wikitext.list.missing_parent".to_string()),
                    message: "Nested list item without a parent; inserting dummy item".to_string(),
                    span: Some(Span::new(lr.start as u64, lr.end as u64)),
                    notes: vec![],
                });
                parent_ctx.items.push(ListItem {
                    span: Span::new(lr.start as u64, lr.start as u64),
                    marker: ListMarker::Unordered,
                    blocks: vec![],
                });
            }
            stack.push(ListCtx { items: Vec::new() });
        }

        // build list item blocks (single paragraph for now).
        let content_slice = &src[content_start_abs..lr.end];
        let mut item_blocks: Vec<BlockNode> = Vec::new();
        if !content_slice.trim().is_empty() {
            let inlines = util::parse_inlines(src, content_start_abs, content_slice, diagnostics);
            if !inlines.is_empty() {
                item_blocks.push(BlockNode {
                    span: Span::new(content_start_abs as u64, lr.end as u64),
                    kind: BlockKind::Paragraph { content: inlines },
                });
            }
        }

        let item = ListItem {
            span: Span::new(lr.start as u64, lr.end as u64),
            marker,
            blocks: item_blocks,
        };

        stack.last_mut().unwrap().items.push(item);
    }

    // attach any remaining nested lists.
    while stack.len() > 1 {
        let child = stack.pop().unwrap();
        let parent_ctx = stack.last_mut().unwrap();
        if let Some(parent_item) = parent_ctx.items.last_mut() {
            attach_child_list(parent_item, child);
        } else {
            parent_ctx.items.extend(child.items);
        }
    }

    let items = stack.pop().unwrap().items;
    let span = if items.is_empty() {
        Span::new(lines[start_i].start as u64, lines[start_i].end as u64)
    } else {
        items
            .first()
            .unwrap()
            .span
            .cover(items.last().unwrap().span)
    };

    (
        BlockNode {
            span,
            kind: BlockKind::List { items },
        },
        i,
    )
}

fn is_block_start(src: &str, line: util::LineRange, text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    // standalone categories are treated as document metadata, not paragraph content.
    if try_parse_category(line, text).is_some() {
        return true;
    }

    if try_parse_heading(src, line, text).is_some() {
        return true;
    }
    if line_trimmed_start(src, line).starts_with("{|") {
        return true;
    }
    if is_list_line(text) {
        return true;
    }
    let t = trimmed.to_ascii_lowercase();
    if t.starts_with("<pre") || t.starts_with("<syntaxhighlight") {
        return true;
    }
    if t.starts_with("<references") {
        return true;
    }
    if try_parse_magic_word(trimmed).is_some() {
        return true;
    }
    if trimmed == "----" {
        return true;
    }
    false
}

fn split_target_anchor(s: &str) -> (&str, Option<&str>) {
    match s.split_once('#') {
        Some((a, b)) => (a, Some(b)),
        None => (s, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_heading_and_link() {
        let src = "=Title=\nSee [[Other Page|link]].\n";
        let out = parse_document(src);
        assert!(out.diagnostics.is_empty());
        assert_eq!(out.document.blocks.len(), 2);
        match &out.document.blocks[0].kind {
            BlockKind::Heading { level, content } => {
                assert_eq!(*level, 1);
                assert!(matches!(content[0].kind, InlineKind::Text { .. }));
            }
            _ => panic!("expected heading"),
        }
        match &out.document.blocks[1].kind {
            BlockKind::Paragraph { content } => {
                assert!(
                    content
                        .iter()
                        .any(|n| matches!(n.kind, InlineKind::InternalLink { .. }))
                );
            }
            _ => panic!("expected paragraph"),
        }
    }

    #[test]
    fn parses_ref_and_references_block() {
        let src = "Text<ref name=\"a\">Ref body</ref>\n<references />\n";
        let out = parse_document(src);
        assert_eq!(out.document.blocks.len(), 2);

        match &out.document.blocks[0].kind {
            BlockKind::Paragraph { content } => {
                assert!(
                    content
                        .iter()
                        .any(|n| matches!(n.kind, InlineKind::Ref { .. }))
                );
            }
            _ => panic!("expected paragraph"),
        }

        assert!(matches!(
            out.document.blocks[1].kind,
            BlockKind::References { .. }
        ));
    }

    #[test]
    fn parses_file_link() {
        let src = "[[FILE:Example.jpg|thumb|An example]]";
        let mut diagnostics = Vec::new();
        let inlines = util::parse_inlines(src, 0, src, &mut diagnostics);
        assert!(
            inlines
                .iter()
                .any(|n| matches!(n.kind, InlineKind::FileLink { .. }))
        );
    }

    #[test]
    fn parses_basic_table() {
        let src = "{| class=\"wikitable\"\n|-\n! H1 !! H2\n|-\n| A || B\n|}\n";
        let out = parse_document(src);
        assert_eq!(out.document.blocks.len(), 1);
        let BlockKind::Table { table } = &out.document.blocks[0].kind else {
            panic!("expected table block");
        };
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0].cells.len(), 2);
        assert_eq!(table.rows[1].cells.len(), 2);
        assert_eq!(table.rows[0].cells[0].kind, TableCellKind::Header);
        assert_eq!(table.rows[1].cells[0].kind, TableCellKind::Data);
    }
}
