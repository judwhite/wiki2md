use crate::ast::{
    BlockKind, BlockNode, Diagnostic, DiagnosticPhase, HtmlAttr, Severity, Span, Table,
    TableCaption, TableCell, TableCellKind, TableRow,
};

use super::util::{parse_html_attrs, strip_cr, LineRange};
use super::util;

// NOTE: This is a deliberately conservative implementation of the MediaWiki
// table grammar. It focuses on the common "wikitable" patterns found in the
// https://chessprogramming.org wiki export and aims to preserve spans for 
// debugging.

#[derive(Debug)]
struct CellBuilder {
    kind: TableCellKind,
    span_start: usize,
    span_end: usize,
    attrs: Vec<HtmlAttr>,
    rowspan: Option<u32>,
    colspan: Option<u32>,
    content_abs_start: usize,
    content: String,
}

#[derive(Debug)]
struct RowBuilder {
    span_start: usize,
    span_end: usize,
    attrs: Vec<HtmlAttr>,
    cells: Vec<TableCell>,
}

fn finish_cell(
    src: &str,
    cell: &mut Option<CellBuilder>,
    row: &mut Option<RowBuilder>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(b) = cell.take() else { return; };
    let blocks = cell_content_to_blocks(src, b.content_abs_start, &b.content, diagnostics);
    let span = Span::new(b.span_start as u64, b.span_end as u64);
    let cell = TableCell {
        kind: b.kind,
        span,
        attrs: b.attrs,
        rowspan: b.rowspan,
        colspan: b.colspan,
        blocks,
    };
    if row.is_none() {
        *row = Some(RowBuilder {
            span_start: b.span_start,
            span_end: b.span_end,
            attrs: vec![],
            cells: vec![cell],
        });
    } else {
        let r = row.as_mut().unwrap();
        r.span_end = r.span_end.max(b.span_end);
        r.cells.push(cell);
    }
}

fn finish_row(row: &mut Option<RowBuilder>, table: &mut Table) {
    let Some(rb) = row.take() else { return; };
    let span = Span::new(rb.span_start as u64, rb.span_end as u64);
    table.rows.push(TableRow {
        span,
        attrs: rb.attrs,
        cells: rb.cells,
    });
}

pub fn parse_table(
    src: &str,
    lines: &[LineRange],
    start_i: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(BlockNode, usize), String> {
    let start_line = lines[start_i];
    let start_text = strip_cr(&src[start_line.start..start_line.end]);
    let trimmed = start_text.trim_start();
    if !trimmed.starts_with("{|") {
        return Err("not a table".to_string());
    }

    // table attributes come after "{|" on the same line.
    let attrs_str = trimmed.strip_prefix("{|").unwrap_or("").trim();
    let table_attrs = parse_html_attrs(attrs_str);

    let mut table = Table {
        attrs: table_attrs,
        caption: None,
        rows: Vec::new(),
    };

    let mut i = start_i + 1;
    let mut depth: i32 = 1; // table nesting depth based on { | and | }

    let mut current_row: Option<RowBuilder> = None;
    let mut current_cell: Option<CellBuilder> = None;

    let mut table_end_abs = start_line.end;

    while i < lines.len() {
        let lr = lines[i];
        let line_raw = strip_cr(&src[lr.start..lr.end]);
        let trimmed_start = line_raw.trim_start();

        // track nesting depth (nested tables within cells).
        if trimmed_start.starts_with("{|") {
            depth += 1;
            if depth > 1 {
                if let Some(cell) = current_cell.as_mut() {
                    append_line(&mut cell.content, line_raw);
                    cell.span_end = lr.end;
                }
                i += 1;
                continue;
            }
        }
        if trimmed_start.starts_with("|}") {
            if depth == 1 {
                // end of this table.
                finish_cell(src, &mut current_cell, &mut current_row, diagnostics);
                finish_row(&mut current_row, &mut table);
                table_end_abs = lr.end;
                i += 1;
                break;
            } else {
                depth -= 1;
                if let Some(cell) = current_cell.as_mut() {
                    append_line(&mut cell.content, line_raw);
                    cell.span_end = lr.end;
                }
                i += 1;
                continue;
            }
        }

        if depth > 1 {
            // inside a nested table; treat as raw content.
            if let Some(cell) = current_cell.as_mut() {
                append_line(&mut cell.content, line_raw);
                cell.span_end = lr.end;
            }
            i += 1;
            continue;
        }

        // caption
        if trimmed_start.starts_with("|+") {
            // finish any pending cell/row (caption should precede rows).
            finish_cell(src, &mut current_cell, &mut current_row, diagnostics);
            finish_row(&mut current_row, &mut table);

            let after = trimmed_start.strip_prefix("|+").unwrap_or("");
            let (attrs, content, content_abs) = split_attrs_content(after, lr.start + (line_raw.len() - trimmed_start.len()) + 2);
            let cap_nodes = util::parse_inlines(src, content_abs, content, diagnostics);
            let span = Span::new(lr.start as u64, lr.end as u64);
            table.caption = Some(TableCaption {
                span,
                attrs,
                content: cap_nodes,
            });
            i += 1;
            continue;
        }

        // row separator
        if trimmed_start.starts_with("|-") {
            finish_cell(src, &mut current_cell, &mut current_row, diagnostics);
            finish_row(&mut current_row, &mut table);

            let after = trimmed_start.strip_prefix("|-").unwrap_or("");
            let attrs = parse_html_attrs(after.trim());
            let row_start_abs = lr.start + (line_raw.len() - trimmed_start.len());
            current_row = Some(RowBuilder {
                span_start: row_start_abs,
                span_end: lr.end,
                attrs,
                cells: Vec::new(),
            });
            i += 1;
            continue;
        }

        // cell line (header or data)
        if trimmed_start.starts_with('!') || trimmed_start.starts_with('|') {
            // finish any pending cell (a new cell line implies a previous cell ended).
            finish_cell(src, &mut current_cell, &mut current_row, diagnostics);

            let is_header = trimmed_start.starts_with('!');
            let marker = if is_header { '!' } else { '|' };
            let kind = if is_header { TableCellKind::Header } else { TableCellKind::Data };

            let line_abs_start = lr.start + (line_raw.len() - trimmed_start.len());
            let rest = trimmed_start.strip_prefix(marker).unwrap_or("");

            let sep = if is_header { "!!" } else { "||" };
            let segments = split_cell_segments(rest, sep);

            for (seg_idx, seg) in segments.iter().enumerate() {
                let seg_abs_start = line_abs_start + 1 + seg.start;
                let seg_abs_end = line_abs_start + 1 + seg.end;
                let seg_str = &rest[seg.start..seg.end];

                let (attrs, content, content_abs) = split_attrs_content(seg_str, seg_abs_start);
                let (rowspan, colspan) = extract_spans(&attrs);

                if seg_idx + 1 == segments.len() {
                    // last segment: allow multiline continuation.
                    current_cell = Some(CellBuilder {
                        kind,
                        span_start: seg_abs_start,
                        span_end: seg_abs_end,
                        attrs,
                        rowspan,
                        colspan,
                        content_abs_start: content_abs,
                        content: content.to_string(),
                    });
                } else {
                    // immediate cell.
                    let blocks = cell_content_to_blocks(src, content_abs, content, diagnostics);
                    let cell = TableCell {
                        kind,
                        span: Span::new(seg_abs_start as u64, seg_abs_end as u64),
                        attrs,
                        rowspan,
                        colspan,
                        blocks,
                    };
                    if let Some(r) = current_row.as_mut() {
                        r.span_end = r.span_end.max(seg_abs_end);
                        r.cells.push(cell);
                    } else {
                        current_row = Some(RowBuilder {
                            span_start: seg_abs_start,
                            span_end: seg_abs_end,
                            attrs: vec![],
                            cells: vec![cell],
                        });
                    }
                }
            }

            i += 1;
            continue;
        }

        // continuation line for current cell content.
        if let Some(cell) = current_cell.as_mut() {
            append_line(&mut cell.content, line_raw);
            cell.span_end = lr.end;
            i += 1;
            continue;
        }

        // otherwise: ignore stray lines inside the table and record a diagnostic.
        if !trimmed_start.is_empty() {
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                phase: Some(DiagnosticPhase::Parse),
                code: Some("wikitext.table.unexpected_line".to_string()),
                message: "Unexpected line inside table".to_string(),
                span: Some(Span::new(lr.start as u64, lr.end as u64)),
                notes: vec![line_raw.to_string()],
            });
        }
        i += 1;
    }

    let node = BlockNode {
        span: Span::new(start_line.start as u64, table_end_abs as u64),
        kind: BlockKind::Table { table },
    };
    Ok((node, i))
}

fn append_line(buf: &mut String, line: &str) {
    if !buf.is_empty() {
        buf.push('\n');
    }
    buf.push_str(line);
}

struct Segment {
    start: usize,
    end: usize,
}

fn split_cell_segments(rest: &str, sep: &str) -> Vec<Segment> {
    // split on top-level separators ("||" or "!!") not inside nested templates/links.
    let mut out = Vec::new();
    let mut tpl_depth = 0i32;
    let mut link_depth = 0i32;
    let mut i = 0usize;
    let mut last = 0usize;
    while i < rest.len() {
        let rem = &rest[i..];
        if rem.starts_with("{{") {
            tpl_depth += 1;
            i += 2;
            continue;
        }
        if rem.starts_with("}}") {
            tpl_depth = (tpl_depth - 1).max(0);
            i += 2;
            continue;
        }
        if rem.starts_with("[[") {
            link_depth += 1;
            i += 2;
            continue;
        }
        if rem.starts_with("]]" ) {
            link_depth = (link_depth - 1).max(0);
            i += 2;
            continue;
        }
        if tpl_depth == 0 && link_depth == 0 && rem.starts_with(sep) {
            out.push(Segment { start: last, end: i });
            i += sep.len();
            last = i;
            continue;
        }
        let ch_len = rem.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        i += ch_len;
    }
    out.push(Segment {
        start: last,
        end: rest.len(),
    });
    out
}

/// Split a cell/caption segment into (attrs, content, content_abs_start).
///
/// `seg_abs_start` is the absolute byte offset of `seg_str` within the source.
fn split_attrs_content(seg_str: &str, seg_abs_start: usize) -> (Vec<HtmlAttr>, &str, usize) {
    // MediaWiki tables use a single pipe `|` to separate attributes from content.
    // if no separator, treat the whole segment as content.
    if let Some(pipe_pos) = find_attr_separator(seg_str) {
        let (left, right) = seg_str.split_at(pipe_pos);
        let right = &right[1..];
        let attrs = parse_html_attrs(left.trim());
        let content = right.trim_start();
        let lead = right.len() - content.len();
        let content_abs = seg_abs_start + pipe_pos + 1 + lead;
        (attrs, content, content_abs)
    } else {
        let content = seg_str.trim_start();
        let lead = seg_str.len() - content.len();
        (vec![], content, seg_abs_start + lead)
    }
}

fn find_attr_separator(seg_str: &str) -> Option<usize> {
    // find the first `|` not inside quotes or nested templates/links.
    let mut tpl_depth = 0i32;
    let mut link_depth = 0i32;
    let mut in_quote: Option<char> = None;
    let mut i = 0usize;
    while i < seg_str.len() {
        let rem = &seg_str[i..];
        if rem.starts_with("{{") {
            tpl_depth += 1;
            i += 2;
            continue;
        }
        if rem.starts_with("}}") {
            tpl_depth = (tpl_depth - 1).max(0);
            i += 2;
            continue;
        }
        if rem.starts_with("[[") {
            link_depth += 1;
            i += 2;
            continue;
        }
        if rem.starts_with("]]" ) {
            link_depth = (link_depth - 1).max(0);
            i += 2;
            continue;
        }

        let ch = rem.chars().next().unwrap();
        if ch == '"' || ch == '\'' {
            if let Some(q) = in_quote {
                if q == ch {
                    in_quote = None;
                }
            } else {
                in_quote = Some(ch);
            }
            i += ch.len_utf8();
            continue;
        }

        if in_quote.is_none() && tpl_depth == 0 && link_depth == 0 && ch == '|' {
            return Some(i);
        }
        i += ch.len_utf8();
    }
    None
}

fn extract_spans(attrs: &[HtmlAttr]) -> (Option<u32>, Option<u32>) {
    let mut rowspan = None;
    let mut colspan = None;
    for a in attrs {
        if a.name.eq_ignore_ascii_case("rowspan")
            && let Some(v) = a.value.as_deref().and_then(|s| s.parse::<u32>().ok()) {
                rowspan = Some(v);
            }
        if a.name.eq_ignore_ascii_case("colspan")
            && let Some(v) = a.value.as_deref().and_then(|s| s.parse::<u32>().ok()) {
                colspan = Some(v);
            }
    }
    (rowspan, colspan)
}

fn cell_content_to_blocks(
    src: &str,
    abs_start: usize,
    content: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<BlockNode> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return vec![];
    }
    let lead = content.len() - content.trim_start().len();
    let abs = abs_start + lead;
    let inlines = util::parse_inlines(src, abs, trimmed, diagnostics);
    vec![BlockNode {
        span: Span::new(abs as u64, (abs + trimmed.len()) as u64),
        kind: BlockKind::Paragraph { content: inlines },
    }]
}
