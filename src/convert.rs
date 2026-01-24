use regex::{Captures, Regex};

/// Converts MediaWiki markup to Markdown.
///
/// Key design choices:
/// - Code blocks are lifted out first and replaced with `{{ CODE BLOCK N }}` placeholders.
///   This prevents any other conversions from mutating code.
/// - Wiki tables are converted to Markdown tables and lifted out as `{{ TABLE N }}` placeholders.
///   Table cell contents still get inline wiki->markdown conversion before the table is emitted.
///
/// The MediaWiki table syntax this parser targets is described in the MediaWiki tables specification provided with this project.
pub fn wiki_to_markdown(input: &str) -> String {
    // 1) Lift code blocks so no downstream regex touches code.
    let (mut text, code_blocks) = lift_code_blocks(input);

    // 2) Convert <span id="..."></span> anchors early (outside code/tables).
    text = convert_span_ids(&text);

    // 3) Convert & lift wiki tables.
    let (t2, tables) = lift_and_convert_tables(&text);
    text = t2;

    // 4) Headings (process deepest first).
    text = convert_headings(&text);

    // 5) Extract anchor from header (`### <a name="..."></a> Title` -> `<a name="..."></a>\n### Title`)
    text = extract_anchor_from_header(&text);

    // 6) Inline conversions over the remaining (non-code, non-table) text.
    text = convert_wiki_links(&text);
    text = convert_external_links(&text);
    text = convert_bold_italic(&text);
    text = convert_perft_calls(&text);

    // 7) Misc markup cleanups.
    text = convert_br_to_newlines(&text);
    text = fix_space_before_punctuation(&text);

    // 8) Blockquotes (lines starting with exactly one leading space).
    text = convert_blockquotes(&text);
    text = process_quote_spacing(&text);

    // 9) Newline cleanups (safe now because code/tables are still lifted).
    text = cleanup_newlines(&text);

    // 10) Change breadcrumbs to top-level heading.
    text = convert_breadcrumbs_to_heading(&text);

    // 11) Restore tables, then code blocks.
    text = restore_placeholders(text, "TABLE", &tables);
    text = restore_placeholders(text, "CODE BLOCK", &code_blocks);

    text.trim().to_string()
}

fn convert_span_ids(input: &str) -> String {
    let re_span = Regex::new(r#"<span\s+id="([^"]+)"></span>"#).unwrap();
    re_span
        .replace_all(input, |caps: &Captures| {
            format!(r#"<a name="{}"></a>"#, &caps[1])
        })
        .to_string()
}

fn convert_headings(input: &str) -> String {
    let mut text = input.to_string();

    let re_h4 = Regex::new(r"(?m)^\s*====(.+?)====\s*$").unwrap();
    text = re_h4.replace_all(&text, "\n##### $1\n").to_string();

    let re_h3 = Regex::new(r"(?m)^\s*===(.+?)===\s*$").unwrap();
    text = re_h3.replace_all(&text, "\n#### $1\n").to_string();

    let re_h2 = Regex::new(r"(?m)^\s*==(.+?)==\s*$").unwrap();
    text = re_h2.replace_all(&text, "\n### $1\n").to_string();

    let re_h1 = Regex::new(r"(?m)^\s*=(.+?)=\s*$").unwrap();
    text = re_h1.replace_all(&text, "\n## $1\n").to_string();

    text
}

fn extract_anchor_from_header(input: &str) -> String {
    let re_anchor = Regex::new(r#"(?m)^(#{2,6})\s*(<a name="[^"]+"></a>)\s*(.*)$"#).unwrap();
    re_anchor.replace_all(input, "$2\n$1 $3").to_string()
}

/// Converts wiki links: [[target|label]] or [[target]] to Markdown links.
///
/// With the new on-disk layout, Markdown files live in `docs/md/<first_letter>/<Page>.md`.
/// Any Markdown file is one directory deep, so links are rendered as `../<first_letter>/<Page>.md`.
fn convert_wiki_links(input: &str) -> String {
    let re_wiki_link = Regex::new(r"\[\[(?P<inner>.*?)]]").unwrap();

    re_wiki_link
        .replace_all(input, |caps: &Captures| {
            let inner = &caps["inner"];

            let (target_raw, label_raw) = match inner.split_once('|') {
                Some((t, l)) => (t, l),
                None => (inner, inner),
            };

            let target_raw = target_raw.trim();
            let label = label_raw.trim();

            // Pure in-page anchors (e.g. [[#Section|label]]) remain anchors.
            if target_raw.starts_with('#') {
                let anchor = normalize_anchor(target_raw);
                return format!("[{}]({})", label, anchor);
            }

            let (page_raw, anchor_raw) = match target_raw.split_once('#') {
                Some((p, a)) => (p.trim(), Some(a.trim())),
                None => (target_raw, None),
            };

            let page = page_raw.replace(' ', "_");
            let dir = lower_first_letter_dir(&page);

            let mut href = format!("../{}/{}.md", dir, page);
            if let Some(a) = anchor_raw {
                href.push('#');
                href.push_str(&normalize_anchor(a));
            }

            format!("[{}]({})", label, href)
        })
        .to_string()
}

fn normalize_anchor(anchor: &str) -> String {
    // Keep leading '#' if present, but normalize spaces to underscores in the id portion.
    if let Some(rest) = anchor.strip_prefix('#') {
        format!("#{}", rest.replace(' ', "_"))
    } else {
        anchor.replace(' ', "_")
    }
}

fn lower_first_letter_dir(page: &str) -> String {
    let first = page.chars().next().unwrap_or('x');
    first.to_lowercase().collect::<String>()
}

fn convert_external_links(input: &str) -> String {
    let re_ext_link = Regex::new(r"\[(?P<url>https?://\S+)\s+(?P<label>[^]]+)]").unwrap();
    re_ext_link.replace_all(input, "[$label]($url)").to_string()
}

/// MediaWiki bold/italic:
/// - ''italic''
/// - '''bold'''
/// - '''''bold+italic'''''
fn convert_bold_italic(input: &str) -> String {
    let mut text = input.to_string();

    let re_bold_italic = Regex::new(r"'''''(.*?)'''''").unwrap();
    text = re_bold_italic.replace_all(&text, "***$1***").to_string();

    let re_bold = Regex::new(r"'''(.*?)'''").unwrap();
    text = re_bold.replace_all(&text, "**$1**").to_string();

    let re_italic = Regex::new(r"''(.*?)''").unwrap();
    text = re_italic.replace_all(&text, "*$1*").to_string();

    text
}

fn convert_perft_calls(input: &str) -> String {
    let mut text = input.to_string();

    let re_perft = Regex::new(r"(?i)(perft\(\d+\))").unwrap();
    text = re_perft.replace_all(&text, "`$1`").to_string();

    // existing cleanup behavior
    text = text.replace("``perft", "`perft");
    text = text.replace(")``", ")`");

    text
}

fn convert_br_to_newlines(input: &str) -> String {
    let re_br = Regex::new(r"(?i)<\s*br\s*/?\s*>").unwrap();
    re_br.replace_all(input, "\n").to_string()
}

fn fix_space_before_punctuation(input: &str) -> String {
    let re_space_after_close_tag = Regex::new(r">\s+([.:,;])").unwrap();
    re_space_after_close_tag.replace_all(input, ">$1").to_string()
}

fn convert_blockquotes(input: &str) -> String {
    let re_quote = Regex::new(r"(?m)^ (?P<content>[^ ].*)$").unwrap();
    re_quote.replace_all(input, "> $content").to_string()
}

fn cleanup_newlines(input: &str) -> String {
    let mut text = input.to_string();

    // remove superfluous newlines
    text = text.replace("\n\n\n", "\n\n");
    text = text.replace("\n\n\n", "\n\n");
    text = text.replace("\n\n\n", "\n\n");

    text.trim().to_string()
}

fn convert_breadcrumbs_to_heading(input: &str) -> String {
    // change breadcrumbs to top-level heading
    // Be flexible about the Home link target, since link paths can vary.
    let re_nav = Regex::new(r"\*\*\[Home]\([^)]*Main_Page[^)]*\).* \* (.+)\*\*").unwrap();
    re_nav.replace_all(input, "# $1").to_string()
}

fn restore_placeholders(mut text: String, kind: &str, blocks: &[String]) -> String {
    for (i, block) in blocks.iter().enumerate() {
        let placeholder = format!("{{{{ {} {} }}}}", kind, i + 1);
        text = text.replace(&placeholder, block);
    }
    text
}

fn lift_code_blocks(input: &str) -> (String, Vec<String>) {
    // Order matters: syntaxhighlight is sometimes used instead of <pre>.
    let mut text = input.to_string();
    let mut blocks: Vec<String> = Vec::new();

    // <syntaxhighlight lang="cpp"> ... </syntaxhighlight>
    let re_syntax = Regex::new(
        r#"(?is)<syntaxhighlight(?P<attrs>[^>]*)>(?P<code>.*?)</syntaxhighlight>"#,
    )
    .unwrap();
    text = re_syntax
        .replace_all(&text, |caps: &Captures| {
            let attrs = caps.name("attrs").map(|m| m.as_str()).unwrap_or("");
            let code = caps.name("code").map(|m| m.as_str()).unwrap_or("");

            let lang = extract_lang_from_attrs(attrs).unwrap_or_else(|| "text".to_string());
            let code = trim_code_block(code);

            let fenced = format!("```{}\n{}\n```", lang, code);
            blocks.push(fenced);
            format!("\n{{{{ CODE BLOCK {} }}}}\n", blocks.len())
        })
        .to_string();

    // <pre> ... </pre>
    let re_pre = Regex::new(r"(?is)<pre(?P<attrs>[^>]*)>(?P<code>.*?)</pre>").unwrap();
    text = re_pre
        .replace_all(&text, |caps: &Captures| {
            let _attrs = caps.name("attrs").map(|m| m.as_str()).unwrap_or("");
            let code = caps.name("code").map(|m| m.as_str()).unwrap_or("");

            // Keep historical behavior: default to ```c fences.
            let code = trim_code_block(code);
            let fenced = format!("```c\n{}\n```", code);

            blocks.push(fenced);
            format!("\n{{{{ CODE BLOCK {} }}}}\n", blocks.len())
        })
        .to_string();

    (text, blocks)
}

fn extract_lang_from_attrs(attrs: &str) -> Option<String> {
    // lang="cpp" or lang=cpp
    let re = Regex::new(r#"(?i)\blang\s*=\s*"?([a-z0-9_+-]+)"?"#).unwrap();
    re.captures(attrs).map(|c| c[1].to_string())
}

fn trim_code_block(code: &str) -> String {
    // Keep internal newlines, but trim leading/trailing blank lines for nicer fences.
    code.trim_matches('\n').trim_end().to_string()
}

/* -----------------------------
 * Table conversion
 * ----------------------------- */

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Align {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone)]
struct Cell {
    is_header: bool,
    text: String,
    align: Option<Align>,
    colspan: usize,
    rowspan: usize,
}

fn lift_and_convert_tables(input: &str) -> (String, Vec<String>) {
    let mut out = String::with_capacity(input.len());
    let mut tables: Vec<String> = Vec::new();

    let mut in_table_depth: usize = 0;
    let mut table_buf = String::new();

    // Handle optional <div class="noresize"> wrapper as described in the table docs.
    // If we see a noresize div immediately wrapping a table, drop the div wrapper so the Markdown
    // table still renders (many Markdown engines treat HTML blocks as "raw" and won't parse tables inside).
    let mut pending_noresize_div: Option<String> = None;
    let mut drop_next_div_close = false;

    for chunk in input.split_inclusive('\n') {
        let trimmed = chunk.trim_start();

        if in_table_depth == 0 {
            // Detect a noresize wrapper line.
            if looks_like_noresize_div_open(trimmed) {
                // Don't emit yet; only drop it if it directly wraps a table.
                pending_noresize_div = Some(chunk.to_string());
                continue;
            }

            if trimmed.starts_with("{|") {
                // If a noresize div was pending, drop it and remember to drop the matching </div>.
                if pending_noresize_div.take().is_some() {
                    drop_next_div_close = true;
                }

                in_table_depth = 1;
                table_buf.clear();
                table_buf.push_str(chunk);
                continue;
            }

            // Not in table and no table start: flush any pending div line.
            if let Some(div_line) = pending_noresize_div.take() {
                out.push_str(&div_line);
            }

            // Optionally drop a closing </div> right after a table conversion.
            if drop_next_div_close && looks_like_div_close(trimmed) {
                drop_next_div_close = false;
                continue;
            }
            drop_next_div_close = false;

            out.push_str(chunk);
            continue;
        }

        // Inside table buffer
        if trimmed.starts_with("{|") {
            in_table_depth += 1;
        }
        if trimmed.starts_with("|}") {
            in_table_depth = in_table_depth.saturating_sub(1);
        }

        table_buf.push_str(chunk);

        if in_table_depth == 0 {
            // Completed a full table block.
            let md_table = match wikitable_to_markdown(&table_buf) {
                Ok(t) => t,
                Err(_) => format!("```text\n{}\n```", table_buf.trim_end()),
            };

            tables.push(md_table);
            out.push_str(&format!("\n{{{{ TABLE {} }}}}\n", tables.len()));
        }
    }

    // Flush pending div if file ended without a table.
    if let Some(div_line) = pending_noresize_div.take() {
        out.push_str(&div_line);
    }

    // If we ended still inside a table (malformed), just emit the buffered text.
    if in_table_depth != 0 && !table_buf.is_empty() {
        out.push_str(&table_buf);
    }

    (out, tables)
}

fn looks_like_noresize_div_open(trimmed_line: &str) -> bool {
    // Accept a few common forms:
    // <div class="noresize">
    // <div class='noresize'>
    // <div class=noresize>
    let l = trimmed_line.trim();
    if !l.to_ascii_lowercase().starts_with("<div") {
        return false;
    }
    let lower = l.to_ascii_lowercase();
    lower.contains("class=\"noresize\"") || lower.contains("class='noresize'") || lower.contains("class=noresize")
}

fn looks_like_div_close(trimmed_line: &str) -> bool {
    trimmed_line.trim().eq_ignore_ascii_case("</div>")
}

fn wikitable_to_markdown(table_wiki: &str) -> Result<String, ()> {
    let mut lines = table_wiki.lines();

    // Table start
    let first = lines.next().ok_or(())?;
    let first_trim = first.trim_start();
    if !first_trim.starts_with("{|") {
        return Err(());
    }

    let table_attrs = first_trim.trim_start_matches("{|").trim();
    let default_align = align_from_attrs(table_attrs);

    let mut rows: Vec<Vec<Cell>> = Vec::new();
    let mut current_row: Vec<Cell> = Vec::new();
    let mut current_cell: Option<Cell> = None;

    // Optional caption (we keep it, but it will be rendered above the markdown table)
    let mut caption: Option<String> = None;

    for raw_line in lines {
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim_start();

        // End of table
        if trimmed.starts_with("|}") {
            flush_cell_and_row(&mut current_cell, &mut current_row, &mut rows);
            break;
        }

        // Caption (only legal between {| and first row)
        if trimmed.starts_with("|+") {
            // Syntax: |+ [attrs] | caption text
            let after = trimmed.trim_start_matches("|+").trim();
            let (_attrs, cap_text) = split_attrs_and_content(after);
            let cap_text = wiki_inline_to_markdown(cap_text);
            if !cap_text.trim().is_empty() {
                caption = Some(cap_text.trim().to_string());
            }
            continue;
        }

        // New row marker
        if trimmed.starts_with("|-") {
            flush_cell_and_row(&mut current_cell, &mut current_row, &mut rows);
            continue;
        }

        // Header cell(s)
        if trimmed.starts_with('!') {
            // Starting a new cell line finalizes any previous cell.
            if let Some(cell) = current_cell.take() {
                current_row.push(cell);
            }

            let cell_line = trimmed.trim_start_matches('!');
            let parts = split_multi_cells(cell_line, "!!");
            for (idx, part) in parts.iter().enumerate() {
                let (attrs, content) = split_attrs_and_content(part);
                let (colspan, rowspan) = parse_spans(attrs);
                let align = align_from_attrs(attrs).or(default_align);
                let text = wiki_inline_to_markdown(content);

                let cell = Cell {
                    is_header: true,
                    text,
                    align,
                    colspan,
                    rowspan,
                };

                if idx + 1 == parts.len() {
                    current_cell = Some(cell);
                } else {
                    current_row.push(cell);
                }
            }
            continue;
        }

        // Data cell(s)
        if trimmed.starts_with('|') {
            // Starting a new cell line finalizes any previous cell.
            if let Some(cell) = current_cell.take() {
                current_row.push(cell);
            }

            let cell_line = trimmed.trim_start_matches('|');
            let parts = split_multi_cells(cell_line, "||");
            for (idx, part) in parts.iter().enumerate() {
                let (attrs, content) = split_attrs_and_content(part);
                let (colspan, rowspan) = parse_spans(attrs);
                let align = align_from_attrs(attrs).or(default_align);
                let text = wiki_inline_to_markdown(content);

                let cell = Cell {
                    is_header: false,
                    text,
                    align,
                    colspan,
                    rowspan,
                };

                if idx + 1 == parts.len() {
                    current_cell = Some(cell);
                } else {
                    current_row.push(cell);
                }
            }
            continue;
        }

        // Continuation line for the current cell content.
        if let Some(ref mut cell) = current_cell {
            if !cell.text.is_empty() {
                cell.text.push('\n');
            }
            cell.text.push_str(&wiki_inline_to_markdown(trimmed));
        }
    }

    if rows.is_empty() && current_row.is_empty() && current_cell.is_none() {
        return Err(());
    }

    // Convert to a rectangular cell matrix, expanding colspan/rowspan.
    let expanded = expand_spans(&rows);

    if expanded.is_empty() {
        return Err(());
    }

    // Column alignments: prefer Center/Right if seen in any cell; default Left.
    let col_aligns = derive_column_alignments(&expanded);

    // Emit markdown.
    let mut md = String::new();
    if let Some(c) = caption {
        md.push_str(&format!("*{}*\n\n", c));
    }

    md.push_str(&render_markdown_table(&expanded, &col_aligns));

    Ok(md.trim_end().to_string())
}

fn flush_cell_and_row(current_cell: &mut Option<Cell>, current_row: &mut Vec<Cell>, rows: &mut Vec<Vec<Cell>>) {
    if let Some(cell) = current_cell.take() {
        current_row.push(cell);
    }
    if !current_row.is_empty() {
        rows.push(std::mem::take(current_row));
    }
}

fn split_multi_cells(cell_line: &str, sep: &str) -> Vec<String> {
    // Split on `||` or `!!`. These separators are not allowed inside attribute lists in practice;
    // if users need literal pipes, they should escape them (<nowiki>|</nowiki>), per MediaWiki docs.
    cell_line
        .split(sep)
        .map(|s| s.trim().to_string())
        .collect()
}

fn split_attrs_and_content(segment: &str) -> (&str, &str) {
    // MediaWiki cell syntax:
    //   | attr1 attr2 | content
    // If there are no attributes, content follows directly:
    //   | content
    //
    // We only treat the first '|' as an attribute/content separator when the left side looks like attributes
    // (contains '='), which avoids breaking `<nowiki>|</nowiki>` or other content starting with tags.
    if let Some((left, right)) = split_once_unquoted(segment, '|') {
        if left.contains('=') {
            return (left.trim(), right.trim());
        }
    }
    ("", segment.trim())
}

fn split_once_unquoted(s: &str, needle: char) -> Option<(&str, &str)> {
    let mut in_quotes = false;
    for (i, ch) in s.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            c if c == needle && !in_quotes => {
                let (a, b) = s.split_at(i);
                let b = &b[needle.len_utf8()..];
                return Some((a, b));
            }
            _ => {}
        }
    }
    None
}

fn parse_spans(attrs: &str) -> (usize, usize) {
    let colspan = parse_span_attr(attrs, "colspan").unwrap_or(1);
    let rowspan = parse_span_attr(attrs, "rowspan").unwrap_or(1);
    (colspan.max(1), rowspan.max(1))
}

fn parse_span_attr(attrs: &str, name: &str) -> Option<usize> {
    let re = Regex::new(&format!(r#"(?i)\b{}\s*=\s*"?(\d+)"?"#, regex::escape(name))).unwrap();
    re.captures(attrs)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<usize>().ok())
}

fn align_from_attrs(attrs: &str) -> Option<Align> {
    if attrs.is_empty() {
        return None;
    }

    // style="text-align:center;"
    let re_style = Regex::new(r#"(?i)text-align\s*:\s*(left|center|right)"#).unwrap();
    if let Some(c) = re_style.captures(attrs) {
        return match &c[1].to_ascii_lowercase()[..] {
            "left" => Some(Align::Left),
            "center" => Some(Align::Center),
            "right" => Some(Align::Right),
            _ => None,
        };
    }

    // align="center" (allow missing trailing space by appending one)
    let re_align = Regex::new(r#"(?i)\balign\s*=\s*"?\s*(left|center|right)\s*"?\s"#).unwrap();
    if let Some(c) = re_align.captures(&format!("{} ", attrs)) {
        return match &c[1].to_ascii_lowercase()[..] {
            "left" => Some(Align::Left),
            "center" => Some(Align::Center),
            "right" => Some(Align::Right),
            _ => None,
        };
    }

    None
}

/// Inline wiki->markdown conversion for table cells.
/// This is intentionally narrower than `wiki_to_markdown` and avoids heading/blockquote logic.
fn wiki_inline_to_markdown(input: &str) -> String {
    let mut text = input.to_string();

    // Convert wiki links.
    text = convert_wiki_links(&text);

    // External links.
    text = convert_external_links(&text);

    // Bold/italic.
    text = convert_bold_italic(&text);

    // Perft.
    text = convert_perft_calls(&text);

    // Normalize <br/> to <br/> for use inside Markdown table cells (tables can't contain raw newlines reliably).
    let re_br = Regex::new(r"(?i)<\s*br\s*/?\s*>").unwrap();
    text = re_br.replace_all(&text, "<br/>").to_string();

    // Trim whitespace but preserve intentional internal spacing.
    text.trim().to_string()
}

fn expand_spans(rows: &[Vec<Cell>]) -> Vec<Vec<Cell>> {
    // Expand colspan/rowspan into a rectangular grid.
    let mut expanded: Vec<Vec<Cell>> = Vec::new();

    // For each column index, how many more rows should we insert a blank cell due to rowspan?
    let mut pending_rowspans: Vec<usize> = Vec::new();

    for row in rows {
        let mut out_row: Vec<Cell> = Vec::new();
        let mut cell_iter = row.iter().cloned();
        let mut next_cell = cell_iter.next();
        let mut col_idx: usize = 0;

        loop {
            // Fill rowspan blanks first.
            if col_idx < pending_rowspans.len() && pending_rowspans[col_idx] > 0 {
                pending_rowspans[col_idx] -= 1;
                out_row.push(Cell {
                    is_header: false,
                    text: String::new(),
                    align: None,
                    colspan: 1,
                    rowspan: 1,
                });
                col_idx += 1;
                continue;
            }

            let Some(mut cell) = next_cell.take() else {
                break;
            };

            let colspan = cell.colspan.max(1);
            let rowspan = cell.rowspan.max(1);
            cell.colspan = 1;
            cell.rowspan = 1;

            // Ensure pending_rowspans is long enough.
            if pending_rowspans.len() < col_idx + colspan {
                pending_rowspans.resize(col_idx + colspan, 0);
            }

            // Register rowspan blanks for subsequent rows (as empty cells).
            if rowspan > 1 {
                for i in 0..colspan {
                    pending_rowspans[col_idx + i] = pending_rowspans[col_idx + i].max(rowspan - 1);
                }
            }

            // Push the real cell, then pad colspan-1 blanks.
            out_row.push(cell);

            for _ in 1..colspan {
                out_row.push(Cell {
                    is_header: false,
                    text: String::new(),
                    align: None,
                    colspan: 1,
                    rowspan: 1,
                });
            }

            col_idx += colspan;
            next_cell = cell_iter.next();
        }

        // After consuming row cells, there may still be trailing rowspan blanks.
        while col_idx < pending_rowspans.len() {
            if pending_rowspans[col_idx] > 0 {
                pending_rowspans[col_idx] -= 1;
                out_row.push(Cell {
                    is_header: false,
                    text: String::new(),
                    align: None,
                    colspan: 1,
                    rowspan: 1,
                });
            }
            col_idx += 1;
        }

        expanded.push(out_row);
    }

    // Normalize to max width
    let max_cols = expanded.iter().map(|r| r.len()).max().unwrap_or(0);
    for r in expanded.iter_mut() {
        while r.len() < max_cols {
            r.push(Cell {
                is_header: false,
                text: String::new(),
                align: None,
                colspan: 1,
                rowspan: 1,
            });
        }
    }

    // Normalize cell text for markdown table safety.
    for row in expanded.iter_mut() {
        for cell in row.iter_mut() {
            cell.text = normalize_table_cell_text(&cell.text);
        }
    }

    expanded
}

fn normalize_table_cell_text(s: &str) -> String {
    let mut out = s.trim().to_string();

    // Markdown tables can't safely contain newlines; convert to <br/>.
    out = out.replace('\n', "<br/>");

    // Escape pipes so they don't break the table.
    out = out.replace('|', r"\|");

    out
}

fn derive_column_alignments(rows: &[Vec<Cell>]) -> Vec<Align> {
    let cols = rows.get(0).map(|r| r.len()).unwrap_or(0);
    let mut aligns = vec![Align::Left; cols];

    for c in 0..cols {
        let mut saw_center = false;
        let mut saw_right = false;

        for r in rows {
            if let Some(a) = r.get(c).and_then(|cell| cell.align) {
                match a {
                    Align::Center => saw_center = true,
                    Align::Right => saw_right = true,
                    Align::Left => {}
                }
            }
        }

        aligns[c] = if saw_center {
            Align::Center
        } else if saw_right {
            Align::Right
        } else {
            Align::Left
        };
    }

    aligns
}

fn render_markdown_table(rows: &[Vec<Cell>], col_aligns: &[Align]) -> String {
    let mut out = String::new();

    if rows.is_empty() {
        return out;
    }

    // Use first row as the header row.
    let header = &rows[0];
    out.push_str(&render_markdown_row(header));
    out.push('\n');
    out.push_str(&render_alignment_row(col_aligns));
    out.push('\n');

    for row in rows.iter().skip(1) {
        out.push_str(&render_markdown_row(row));
        out.push('\n');
    }

    out.trim_end().to_string()
}

fn render_markdown_row(row: &[Cell]) -> String {
    let mut s = String::new();
    s.push('|');
    for cell in row {
        s.push(' ');
        s.push_str(cell.text.trim());
        s.push(' ');
        s.push('|');
    }
    s
}

fn render_alignment_row(col_aligns: &[Align]) -> String {
    let mut s = String::new();
    s.push('|');
    for a in col_aligns {
        let spec = match a {
            Align::Left => "---",
            Align::Center => ":---:",
            Align::Right => "---:",
        };
        s.push(' ');
        s.push_str(spec);
        s.push(' ');
        s.push('|');
    }
    s
}

fn process_quote_spacing(input: &str) -> String {
    let mut new_text = String::with_capacity(input.len());
    let mut was_in_quote = false;

    for line in input.lines() {
        let is_quote = line.starts_with("> ");

        if is_quote {
            if !was_in_quote {
                if !new_text.is_empty() && !new_text.ends_with("\n\n") {
                    if new_text.ends_with('\n') {
                        new_text.push('\n');
                    } else {
                        new_text.push_str("\n\n");
                    }
                }
            } else {
                new_text.push_str(">\n");
            }
        } else if was_in_quote && !line.trim().is_empty() && !new_text.ends_with("\n\n") {
            if new_text.ends_with('\n') {
                new_text.push('\n');
            } else {
                new_text.push_str("\n\n");
            }
        }

        new_text.push_str(line);
        new_text.push('\n');
        was_in_quote = is_quote;
    }

    new_text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_anchor_extraction() {
        let input = r#"==<span id="Bulk"></span>Bulk-counting=="#;
        let output = wiki_to_markdown(input);
        let expected = r#"<a name="Bulk"></a>
### Bulk-counting"#;

        assert_eq!(output, expected);
    }

    #[test]
    fn test_wiki_links_include_directory_layout() {
        let input = "See [[Initial Position|initial position]] and [[C]].";
        let output = wiki_to_markdown(input);
        let expected = "See [initial position](../i/Initial_Position.md) and [C](../c/C.md).";

        assert_eq!(output, expected);
    }

    #[test]
    fn test_headers() {
        let input = "=Perft Function=\n==Bulk Counting==";
        let output = wiki_to_markdown(input);
        let expected = "## Perft Function\n\n### Bulk Counting";

        assert_eq!(output, expected);
    }

    #[test]
    fn test_span_to_anchor() {
        let input = r#"Header <span id="Bulk"></span>"#;
        let output = wiki_to_markdown(input);
        let expected = r#"Header <a name="Bulk"></a>"#;

        assert_eq!(output, expected);
    }

    #[test]
    fn test_code_block_formatting() {
        let input = "<pre>int main() { return 0; }</pre>";
        let output = wiki_to_markdown(input);
        let expected = "```c\nint main() { return 0; }\n```";

        assert_eq!(output, expected);
    }

    #[test]
    fn test_code_block_is_protected_from_other_replacements() {
        let input = r#"
<pre>
See [[Not A Link|nope]] and '''bold'''.
</pre>
"#;

        let output = wiki_to_markdown(input);
        let expected = "```c\nSee [[Not A Link|nope]] and '''bold'''.\n```";
        assert_eq!(output, expected);
    }

    #[test]
    fn test_code_block_with_attributes_and_case() {
        let input = r#"<PRE class="code">int x = 1;</PRE>"#;
        let output = wiki_to_markdown(input);
        let expected = "```c\nint x = 1;\n```";

        assert_eq!(output, expected);
    }

    #[test]
    fn test_deeper_headings() {
        let input = "===Level 3===\n====Level 4====";
        let output = wiki_to_markdown(input);
        let expected = "#### Level 3\n\n##### Level 4";

        assert_eq!(output, expected);
    }

    #[test]
    fn test_wiki_link_with_anchor() {
        let input = "See [[Perft Results#Initial Position|init]].";
        let output = wiki_to_markdown(input);
        let expected = "See [init](../p/Perft_Results.md#Initial_Position).";

        assert_eq!(output, expected);
    }

    #[test]
    fn test_quote_spacing_logic() {
        let input = r#"
Preceding Text
 I believe...
 I carried...
Following Text
"#;

        let output = wiki_to_markdown(input);

        let expected = r#"Preceding Text

> I believe...
>
> I carried...

Following Text"#;

        assert_eq!(output, expected);
    }

    #[test]
    fn test_blockquotes_vs_code_indentation() {
        let input = r#"
 I am a quote.
```c
  if (depth == 0)
    return 1;

```

"#;
        let output = wiki_to_markdown(input);
        let expected = r#"> I am a quote.

```c
  if (depth == 0)
    return 1;

```"#;

        assert_eq!(output, expected);
    }

    #[test]
    fn test_formatting_and_perft() {
        let input = "The '''perft(5)''' result is `perft(6)`";
        let output = wiki_to_markdown(input);
        let expected = "The **`perft(5)`** result is `perft(6)`";

        assert_eq!(output, expected);
    }

    #[test]
    fn test_punctuation_cleanup() {
        let input = "</ref> . <ref> , </ref> ;";
        let output = wiki_to_markdown(input);
        let expected = "</ref>. <ref>, </ref>;";

        assert_eq!(output, expected);
    }

    #[test]
    fn test_table_basic_conversion() {
        let input = r#"
=Table=
{| class="wikitable"
|-
!   
! 10 
! 50 
! 100 
|-
! 0 
| style="text-align:center;" | 50.00% 
| style="text-align:center;" | 51.00% 
| style="text-align:center;" | 52.00% 
|-
! 1 
| style="text-align:center;" | 60.00% 
| style="text-align:center;" |   
| style="text-align:center;" | 62.00% 
|}
"#;

        let output = wiki_to_markdown(input);
        let expected = r#"## Table

|  | 10 | 50 | 100 |
| --- | :---: | :---: | :---: |
| 0 | 50.00% | 51.00% | 52.00% |
| 1 | 60.00% |  | 62.00% |"#;

        assert_eq!(output, expected);
    }

    #[test]
    fn test_table_inline_cell_syntax() {
        let input = r#"
{| class="wikitable"
|-
! A !! B !! C
|-
| 1 || 2 || style="text-align:right;" | 3
|}
"#;

        let output = wiki_to_markdown(input);
        let expected = r#"| A | B | C |
| --- | --- | ---: |
| 1 | 2 | 3 |"#;

        assert_eq!(output, expected);
    }

    #[test]
    fn test_table_is_not_converted_inside_code_blocks() {
        let input = r#"
<pre>
{| class="wikitable"
|-
! A !! B
|-
| 1 || 2
|}
</pre>
"#;
        let output = wiki_to_markdown(input);
        let expected = "```c\n{| class=\"wikitable\"\n|-\n! A !! B\n|-\n| 1 || 2\n|}\n```";
        assert_eq!(output, expected);
    }
}
