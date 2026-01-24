use crate::ast::{
    Diagnostic, DiagnosticPhase, ExternalLink, FileLink, FileNamespace, FileParam, HtmlAttr, HtmlTag,
    InlineKind, InlineNode, InternalLink, RefNode, Severity, Span, TemplateInvocation, TemplateName,
    TemplateNameKind, TemplateParam,
};

/// A byte range for a single line in the source.
///
/// - `start..end` is the line content excluding the trailing `\n`.
/// - `end_with_newline` is `end` or `end+1` if the line ended with `\n`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    pub start: usize,
    pub end: usize,
    pub end_with_newline: usize,
}

pub fn collect_lines(src: &str) -> Vec<LineRange> {
    let bytes = src.as_bytes();
    let mut out: Vec<LineRange> = Vec::new();
    let mut start = 0usize;
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\n' {
            out.push(LineRange {
                start,
                end: i,
                end_with_newline: i + 1,
            });
            start = i + 1;
        }
    }
    if start <= src.len() {
        out.push(LineRange {
            start,
            end: src.len(),
            end_with_newline: src.len(),
        });
    }
    out
}

pub fn strip_cr(s: &str) -> &str {
    s.strip_suffix('\r').unwrap_or(s)
}

pub fn line_trimmed_start(src: &str, line: LineRange) -> &str {
    strip_cr(&src[line.start..line.end]).trim_start()
}

/// Parse a sequence of HTML-like attributes (small subset).
///
/// Example: `style="text-align:center;" rowspan="2"`.
pub fn parse_html_attrs(mut s: &str) -> Vec<HtmlAttr> {
    let mut attrs = Vec::new();
    while !s.is_empty() {
        // skip whitespace.
        let trimmed = s.trim_start();
        let ws = s.len() - trimmed.len();
        s = &s[ws..];
        if s.is_empty() {
            break;
        }

        // parse name.
        let mut name_end = 0usize;
        for (i, ch) in s.char_indices() {
            if ch.is_whitespace() || ch == '=' {
                break;
            }
            name_end = i + ch.len_utf8();
        }
        if name_end == 0 {
            break;
        }
        let name = &s[..name_end];
        s = &s[name_end..];

        // optional value.
        let mut value: Option<String> = None;
        let trimmed = s.trim_start();
        let ws = s.len() - trimmed.len();
        s = &s[ws..];
        if s.starts_with('=') {
            s = &s[1..];
            let trimmed = s.trim_start();
            let ws = s.len() - trimmed.len();
            s = &s[ws..];

            if let Some(q) = s.chars().next() {
                if q == '"' || q == '\'' {
                    s = &s[q.len_utf8()..];
                    if let Some(end_q) = s.find(q) {
                        value = Some(s[..end_q].to_string());
                        s = &s[end_q + q.len_utf8()..];
                    } else {
                        // unterminated quote; take rest.
                        value = Some(s.to_string());
                        s = "";
                    }
                } else {
                    // unquoted token.
                    let mut end = s.len();
                    for (i, ch) in s.char_indices() {
                        if ch.is_whitespace() {
                            end = i;
                            break;
                        }
                    }
                    value = Some(s[..end].to_string());
                    s = &s[end..];
                }
            }
        }

        attrs.push(HtmlAttr {
            name: name.to_string(),
            value,
            span: None,
        });
    }
    attrs
}

/// Parse inline content for paragraphs, headings, etc.
///
/// `base_abs` is the absolute byte offset of `slice` within the original source.
pub fn parse_inlines(
    full_src: &str,
    base_abs: usize,
    slice: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<InlineNode> {
    let mut out: Vec<InlineNode> = Vec::new();
    let mut i: usize = 0;
    let mut text_start: usize = 0;

    let flush_text = |out: &mut Vec<InlineNode>, text_start: &mut usize, i: usize| {
        if *text_start < i {
            let txt = &slice[*text_start..i];
            out.push(InlineNode {
                span: Span::new((base_abs + *text_start) as u64, (base_abs + i) as u64),
                kind: InlineKind::Text {
                    value: txt.to_string(),
                },
            });
        }
        *text_start = i;
    };

    while i < slice.len() {
        let rem = &slice[i..];

        // <br>, <br/>, <br />
        if rem.starts_with('<')
            && let Some((node, consumed)) = try_parse_line_break(base_abs + i, rem) {
                flush_text(&mut out, &mut text_start, i);
                out.push(node);
                i += consumed;
                text_start = i;
                continue;
            }

        // <ref ...> ... </ref>
        if rem.starts_with('<')
            && let Some((node, consumed)) = try_parse_ref_tag(full_src, base_abs + i, rem, diagnostics) {
                flush_text(&mut out, &mut text_start, i);
                out.push(node);
                i += consumed;
                text_start = i;
                continue;
            }

        // <span ...></span>
        if rem.starts_with('<')
            && let Some((node, consumed)) = try_parse_simple_html_tag(full_src, base_abs + i, rem, "span", diagnostics)
            {
                flush_text(&mut out, &mut text_start, i);
                out.push(node);
                i += consumed;
                text_start = i;
                continue;
            }

        // internal links [[...]]
        if rem.starts_with("[[")
            && let Some(close_rel) = rem[2..].find("]]" ) {
                let inner_end = 2 + close_rel;
                let inner = &rem[2..inner_end];
                let consumed = inner_end + 2;

                flush_text(&mut out, &mut text_start, i);
                out.push(parse_bracket_link(full_src, base_abs + i, base_abs + i + 2, inner, diagnostics));
                i += consumed;
                text_start = i;
                continue;
            }

        // external links [https://... label]
        if rem.starts_with('[') && !rem.starts_with("[[")
            && let Some(end_rel) = rem[1..].find(']') {
                let inner_end = 1 + end_rel;
                let inner = &rem[1..inner_end];
                let inner_trim = inner.trim_start();
                if inner_trim.starts_with("http://") || inner_trim.starts_with("https://") {
                    flush_text(&mut out, &mut text_start, i);

                    let (url, label) = split_first_ws(inner_trim);
                    let url_abs_start = base_abs + i + 1 + (inner.len() - inner_trim.len());

                    let text_nodes = label.map(|lbl| {
                        // label start is after url + whitespace.
                        let label_pos = inner_trim.find(lbl).unwrap_or(inner_trim.len());
                        let abs = url_abs_start + label_pos;
                        parse_inlines(full_src, abs, lbl, diagnostics)
                    });

                    out.push(InlineNode {
                        span: Span::new((base_abs + i) as u64, (base_abs + i + inner_end + 1) as u64),
                        kind: InlineKind::ExternalLink {
                            link: ExternalLink {
                                url: url.to_string(),
                                text: text_nodes,
                            },
                        },
                    });

                    i += inner_end + 1;
                    text_start = i;
                    continue;
                }
            }

        // templates {{...}}
        if rem.starts_with("{{")
            && let Some(consumed) = find_matching_braces(rem) {
                let inner = &rem[2..consumed - 2];
                flush_text(&mut out, &mut text_start, i);
                out.push(parse_template(full_src, base_abs + i, base_abs + i + 2, inner, diagnostics));
                i += consumed;
                text_start = i;
                continue;
            }

        // emphasis: `''italic''`, `'''bold'''`, `'''''bold italic'''''`.
        if rem.starts_with("''")
            && let Some((node, consumed)) = try_parse_emphasis(full_src, base_abs + i, slice, i, diagnostics) {
                flush_text(&mut out, &mut text_start, i);
                out.push(node);
                i += consumed;
                text_start = i;
                continue;
            }

        // default: advance by one char.
        let ch_len = rem.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        i += ch_len;
    }

    flush_text(&mut out, &mut text_start, i);
    out
}

fn try_parse_line_break(abs_start: usize, rem: &str) -> Option<(InlineNode, usize)> {
    let lower = rem.to_ascii_lowercase();
    if !lower.starts_with("<br") {
        return None;
    }
    let end = rem.find('>')?;
    // accept <br>, <br/>, <br /> ...
    let tag = &lower[..=end];
    if !tag.starts_with("<br") {
        return None;
    }
    let consumed = end + 1;
    Some((
        InlineNode {
            span: Span::new(abs_start as u64, (abs_start + consumed) as u64),
            kind: InlineKind::LineBreak,
        },
        consumed,
    ))
}

fn try_parse_emphasis(
    full_src: &str,
    abs_start: usize,
    full_slice: &str,
    rel_i: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(InlineNode, usize)> {
    let rem = &full_slice[rel_i..];
    // prefer longer delimiters.
    for (delim, kind) in [
        ("'''''", "bi"),
        ("'''", "b"),
        ("''", "i"),
    ] {
        if rem.starts_with(delim) {
            let delim_len = delim.len();
            let after = &rem[delim_len..];
            if let Some(close_rel) = after.find(delim) {
                let inner_rel_start = rel_i + delim_len;
                let inner_rel_end = inner_rel_start + close_rel;
                let inner = &full_slice[inner_rel_start..inner_rel_end];
                let children = parse_inlines(full_src, abs_start + delim_len, inner, diagnostics);
                let consumed = delim_len + close_rel + delim_len;
                let span = Span::new(abs_start as u64, (abs_start + consumed) as u64);
                let inline_kind = match kind {
                    "bi" => InlineKind::BoldItalic { content: children },
                    "b" => InlineKind::Bold { content: children },
                    "i" => InlineKind::Italic { content: children },
                    _ => InlineKind::Text {
                        value: rem[..consumed].to_string(),
                    },
                };
                return Some((InlineNode { span, kind: inline_kind }, consumed));
            }
        }
    }
    None
}

fn parse_bracket_link(
    full_src: &str,
    abs_start: usize,
    abs_inner_start: usize,
    inner: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> InlineNode {
    let inner_trim = inner.trim_start();
    let lower = inner_trim.to_ascii_lowercase();
    if lower.starts_with("file:") || lower.starts_with("image:") || lower.starts_with("media:") {
        return parse_file_link(full_src, abs_start, abs_inner_start, inner, diagnostics);
    }
    parse_internal_link(full_src, abs_start, abs_inner_start, inner, diagnostics)
}

fn split_target_anchor(s: &str) -> (&str, Option<&str>) {
    if let Some((a, b)) = s.split_once('#') {
        (a, Some(b))
    } else {
        (s, None)
    }
}

fn parse_internal_link(
    full_src: &str,
    abs_start: usize,
    abs_inner_start: usize,
    inner: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> InlineNode {
    let span = Span::new(abs_start as u64, (abs_start + 2 + inner.len() + 2) as u64);
    let (target_part, label_part) = match inner.split_once('|') {
        Some((a, b)) => (a, Some(b)),
        None => (inner, None),
    };
    let target_trim = target_part.trim();
    let (target, anchor) = split_target_anchor(target_trim);

    let text_nodes = if let Some(lbl) = label_part {
        let lbl_trim = lbl.trim();
        if lbl_trim.is_empty() {
            None
        } else {
            let rel = inner.find(lbl).unwrap_or(0);
            let abs = abs_inner_start + rel;
            Some(parse_inlines(full_src, abs, lbl_trim, diagnostics))
        }
    } else {
        None
    };

    InlineNode {
        span,
        kind: InlineKind::InternalLink {
            link: InternalLink {
                target: target.to_string(),
                anchor: anchor.filter(|a| !a.is_empty()).map(|a| a.to_string()),
                text: text_nodes,
            },
        },
    }
}

fn parse_file_link(
    full_src: &str,
    abs_start: usize,
    abs_inner_start: usize,
    inner: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> InlineNode {
    let span = Span::new(abs_start as u64, (abs_start + 2 + inner.len() + 2) as u64);
    let parts = split_top_level(inner, '|');
    if parts.is_empty() {
        return InlineNode {
            span,
            kind: InlineKind::Text {
                value: format!("[[{}]]", inner),
            },
        };
    }

    let first = &inner[parts[0].0..parts[0].1];
    let first_trim = first.trim();
    let (ns_raw, target_raw) = match first_trim.split_once(':') {
        Some((a, b)) => (a.trim(), b.trim()),
        None => ("file", first_trim),
    };
    let namespace = match ns_raw.to_ascii_lowercase().as_str() {
        "file" => FileNamespace::File,
        "image" => FileNamespace::Image,
        "media" => FileNamespace::Media,
        _ => FileNamespace::File,
    };

    let mut params: Vec<FileParam> = Vec::new();
    for seg in parts.iter().skip(1) {
        let raw = &inner[seg.0..seg.1];
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let leading = raw.len() - raw.trim_start().len();
        let abs = abs_inner_start + seg.0 + leading;
        let nodes = parse_inlines(full_src, abs, trimmed, diagnostics);
        params.push(FileParam {
            span: Span::new((abs_inner_start + seg.0) as u64, (abs_inner_start + seg.1) as u64),
            content: nodes,
        });
    }

    InlineNode {
        span,
        kind: InlineKind::FileLink {
            link: FileLink {
                namespace,
                target: target_raw.to_string(),
                params,
            },
        },
    }
}

fn try_parse_ref_tag(
    full_src: &str,
    abs_start: usize,
    rem: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(InlineNode, usize)> {
    let lower = rem.to_ascii_lowercase();
    if !lower.starts_with("<ref") {
        return None;
    }
    let open_end = rem.find('>')?;
    let open_tag = &rem[..=open_end];
    let self_closing = open_tag.trim_end().ends_with("/>");
    let attrs_str = open_tag
        .trim_start_matches('<')
        .trim_start_matches(|c: char| c.is_ascii_alphabetic())
        .trim();
    let attrs_str = attrs_str.trim_end_matches('>').trim_end_matches("/>").trim();
    let attrs = parse_html_attrs(attrs_str);

    if self_closing {
        let consumed = open_end + 1;
        return Some((
            InlineNode {
                span: Span::new(abs_start as u64, (abs_start + consumed) as u64),
                kind: InlineKind::Ref {
                    node: RefNode {
                        attrs,
                        content: None,
                        self_closing: true,
                    },
                },
            },
            consumed,
        ));
    }

    let close_pat = "</ref>";
    let Some(close_rel) = lower[open_end + 1..].find(close_pat) else {
        diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            phase: Some(DiagnosticPhase::Parse),
            code: Some("wikitext.ref.unclosed".to_string()),
            message: "Unclosed <ref> tag".to_string(),
            span: Some(Span::new(abs_start as u64, (abs_start + open_end + 1) as u64)),
            notes: vec![],
        });
        return None;
    };

    let content_start_rel = open_end + 1;
    let close_start_rel = open_end + 1 + close_rel;
    let content = &rem[content_start_rel..close_start_rel];
    let content_nodes = if content.trim().is_empty() {
        None
    } else {
        Some(parse_inlines(full_src, abs_start + content_start_rel, content, diagnostics))
    };
    let consumed = close_start_rel + close_pat.len();

    Some((
        InlineNode {
            span: Span::new(abs_start as u64, (abs_start + consumed) as u64),
            kind: InlineKind::Ref {
                node: RefNode {
                    attrs,
                    content: content_nodes,
                    self_closing: false,
                },
            },
        },
        consumed,
    ))
}

fn try_parse_simple_html_tag(
    full_src: &str,
    abs_start: usize,
    rem: &str,
    tag_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(InlineNode, usize)> {
    let lower = rem.to_ascii_lowercase();
    let open_pat = format!("<{}", tag_name);
    if !lower.starts_with(&open_pat) {
        return None;
    }
    let open_end = rem.find('>')?;
    let open_tag = &rem[..=open_end];

    let attrs_str = open_tag
        .trim_start_matches('<')
        .trim_start_matches(|c: char| c.is_ascii_alphabetic())
        .trim();
    let attrs_str = attrs_str.trim_end_matches('>').trim_end_matches('/').trim();
    let attrs = parse_html_attrs(attrs_str);

    // self-closing?
    if open_tag.trim_end().ends_with("/>") {
        let consumed = open_end + 1;
        return Some((
            InlineNode {
                span: Span::new(abs_start as u64, (abs_start + consumed) as u64),
                kind: InlineKind::HtmlTag {
                    node: HtmlTag {
                        name: tag_name.to_string(),
                        attrs,
                        children: vec![],
                        self_closing: true,
                    },
                },
            },
            consumed,
        ));
    }

    let close_pat = format!("</{}>", tag_name);
    let Some(close_rel) = lower[open_end + 1..].find(&close_pat) else {
        diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            phase: Some(DiagnosticPhase::Parse),
            code: Some("wikitext.html.unclosed".to_string()),
            message: format!("Unclosed <{}> tag", tag_name),
            span: Some(Span::new(abs_start as u64, (abs_start + open_end + 1) as u64)),
            notes: vec![],
        });
        return None;
    };

    let content_start_rel = open_end + 1;
    let close_start_rel = open_end + 1 + close_rel;
    let content = &rem[content_start_rel..close_start_rel];
    let children = if content.is_empty() {
        vec![]
    } else {
        parse_inlines(full_src, abs_start + content_start_rel, content, diagnostics)
    };
    let consumed = close_start_rel + close_pat.len();

    Some((
        InlineNode {
            span: Span::new(abs_start as u64, (abs_start + consumed) as u64),
            kind: InlineKind::HtmlTag {
                node: HtmlTag {
                    name: tag_name.to_string(),
                    attrs,
                    children,
                    self_closing: false,
                },
            },
        },
        consumed,
    ))
}

fn parse_template(
    full_src: &str,
    abs_start: usize,
    abs_inner_start: usize,
    inner: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> InlineNode {
    let span = Span::new(abs_start as u64, (abs_start + 2 + inner.len() + 2) as u64);
    let parts = split_top_level(inner, '|');
    if parts.is_empty() {
        return InlineNode {
            span,
            kind: InlineKind::Text {
                value: format!("{{{{{}}}}}", inner),
            },
        };
    }

    let first_raw = inner[parts[0].0..parts[0].1].trim();
    let mut name_raw = first_raw.to_string();
    let mut name_kind = TemplateNameKind::Template;
    let mut params: Vec<TemplateParam> = Vec::new();

    // parser function: {{#name:arg|k=v}}
    if first_raw.starts_with('#') {
        name_kind = TemplateNameKind::ParserFunction;
        if let Some((n, rest)) = first_raw.split_once(':') {
            name_raw = n.trim().to_string();
            let rest_trim = rest.trim();
            if !rest_trim.is_empty() {
                // abs offset of `rest_trim` within the original `inner`.
                let part0 = &inner[parts[0].0..parts[0].1];
                let rel = part0.find(rest_trim).unwrap_or(part0.len() - rest_trim.len());
                let abs = abs_inner_start + parts[0].0 + rel;
                let nodes = parse_inlines(full_src, abs, rest_trim, diagnostics);
                params.push(TemplateParam {
                    span: Span::new(abs as u64, (abs + rest_trim.len()) as u64),
                    name: None,
                    value: nodes,
                });
            }
        }
    }

    for seg in parts.iter().skip(1) {
        let raw = &inner[seg.0..seg.1];
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(eq_pos) = find_top_level_eq(trimmed) {
            let (n, v_with_eq) = trimmed.split_at(eq_pos);
            let v = &v_with_eq[1..];
            let n_trim = n.trim();
            let v_trim = v.trim();

            let raw_abs = abs_inner_start + seg.0;
            let v_rel = raw.find(v_trim).unwrap_or(raw.len() - v_trim.len());
            let v_abs = raw_abs + v_rel;
            let value_nodes = parse_inlines(full_src, v_abs, v_trim, diagnostics);
            params.push(TemplateParam {
                span: Span::new(raw_abs as u64, (abs_inner_start + seg.1) as u64),
                name: Some(n_trim.to_string()),
                value: value_nodes,
            });
        } else {
            let raw_abs = abs_inner_start + seg.0;
            let lead = raw.len() - raw.trim_start().len();
            let v_abs = raw_abs + lead;
            let value_nodes = parse_inlines(full_src, v_abs, trimmed, diagnostics);
            params.push(TemplateParam {
                span: Span::new(raw_abs as u64, (abs_inner_start + seg.1) as u64),
                name: None,
                value: value_nodes,
            });
        }
    }

    InlineNode {
        span,
        kind: InlineKind::Template {
            node: TemplateInvocation {
                name: TemplateName {
                    raw: name_raw,
                    kind: name_kind,
                },
                params,
            },
        },
    }
}

fn find_matching_braces(s: &str) -> Option<usize> {
    // `s` starts with "{{".
    let mut depth = 0usize;
    let mut i = 0usize;
    while i < s.len() {
        let rem = &s[i..];
        if rem.starts_with("{{") {
            depth += 1;
            i += 2;
            continue;
        }
        if rem.starts_with("}}") {
            if depth == 0 {
                return None;
            }
            depth -= 1;
            i += 2;
            if depth == 0 {
                return Some(i);
            }
            continue;
        }
        let ch_len = rem.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        i += ch_len;
    }
    None
}

fn split_first_ws(s: &str) -> (&str, Option<&str>) {
    let mut seen_non_ws = false;
    for (i, ch) in s.char_indices() {
        if !seen_non_ws {
            if ch.is_whitespace() {
                continue;
            }
            seen_non_ws = true;
        }
        if seen_non_ws && ch.is_whitespace() {
            let url = &s[..i];
            let rest = s[i..].trim_start();
            if rest.is_empty() {
                return (url, None);
            }
            return (url, Some(rest));
        }
    }
    (s, None)
}

/// Split by `delim` at top-level (ignoring nested `{{...}}` and `[[...]]`).
/// Returns byte ranges into `s`.
pub fn split_top_level(s: &str, delim: char) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut seg_start = 0usize;
    let mut tpl_depth = 0usize;
    let mut link_depth = 0usize;
    while i < s.len() {
        let rem = &s[i..];
        if rem.starts_with("{{") {
            tpl_depth += 1;
            i += 2;
            continue;
        }
        if rem.starts_with("}}") {
            tpl_depth = tpl_depth.saturating_sub(1);
            i += 2;
            continue;
        }
        if rem.starts_with("[[") {
            link_depth += 1;
            i += 2;
            continue;
        }
        if rem.starts_with("]]" ) {
            link_depth = link_depth.saturating_sub(1);
            i += 2;
            continue;
        }
        if tpl_depth == 0 && link_depth == 0
            && let Some(ch) = rem.chars().next()
                && ch == delim {
                    out.push((seg_start, i));
                    i += ch.len_utf8();
                    seg_start = i;
                    continue;
                }
        let ch_len = rem.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        i += ch_len;
    }
    out.push((seg_start, s.len()));
    out
}

fn find_top_level_eq(s: &str) -> Option<usize> {
    // for now: first '=' not inside nested templates or links.
    let mut i = 0usize;
    let mut tpl_depth = 0usize;
    let mut link_depth = 0usize;
    while i < s.len() {
        let rem = &s[i..];
        if rem.starts_with("{{") {
            tpl_depth += 1;
            i += 2;
            continue;
        }
        if rem.starts_with("}}") {
            tpl_depth = tpl_depth.saturating_sub(1);
            i += 2;
            continue;
        }
        if rem.starts_with("[[") {
            link_depth += 1;
            i += 2;
            continue;
        }
        if rem.starts_with("]]" ) {
            link_depth = link_depth.saturating_sub(1);
            i += 2;
            continue;
        }
        if tpl_depth == 0 && link_depth == 0
            && let Some(ch) = rem.chars().next()
                && ch == '=' {
                    return Some(i);
                }
        let ch_len = rem.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        i += ch_len;
    }
    None
}
