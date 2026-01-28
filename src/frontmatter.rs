//! YAML frontmatter handling for generated Markdown.
//!
//! Goals:
//! - Preserve existing YAML frontmatter verbatim by default.
//! - Generate frontmatter when missing.
//! - Optionally regenerate frontmatter, best-effort merge of preserved fields.

use crate::ast::*;
use deunicode::deunicode;
use serde_yaml::Value;
use std::path::Path;
use std::{fs, io};
use time::{OffsetDateTime, macros::format_description};

/// Top-level frontmatter we generate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frontmatter {
    pub wiki2md: Wiki2mdMeta,
    pub aliases: Vec<String>,
    pub tags: Vec<String>,

    /// Reserved for future use. If empty/None, it is omitted from generated YAML.
    pub summary: Option<String>,

    /// Extra unrecognized YAML keys preserved during regeneration.
    pub extras_yaml: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wiki2mdMeta {
    pub article_id: String,
    pub source_url: String,
    pub generated_by: String,
    pub last_fetched_date: String,
    pub schema_version: u32,
}

impl Frontmatter {
    pub fn to_yaml_string(&self) -> String {
        let mut out = String::new();
        out.push_str("---\n");
        out.push_str("wiki2md:\n");
        out.push_str(&format!("  article_id: {}\n", self.wiki2md.article_id));
        out.push_str(&format!("  source_url: {}\n", self.wiki2md.source_url));
        out.push_str(&format!("  generated_by: {}\n", self.wiki2md.generated_by));
        out.push_str(&format!(
            "  last_fetched_date: {}\n",
            self.wiki2md.last_fetched_date
        ));
        out.push_str(&format!(
            "  schema_version: {}\n",
            self.wiki2md.schema_version
        ));

        out.push_str("aliases:\n");
        for a in &self.aliases {
            out.push_str(&format!("  - {}\n", yaml_quote(a)));
        }

        if let Some(summary) = self.summary.as_ref().filter(|s| !s.trim().is_empty()) {
            out.push_str(&format!("summary: {}\n", yaml_quote(summary)));
        }

        if self.tags.is_empty() {
            out.push_str("tags: []\n");
        } else {
            out.push_str("tags:\n");
            for t in &self.tags {
                out.push_str(&format!("  - {}\n", t));
            }
        }

        if let Some(extra) = self.extras_yaml.as_ref().filter(|s| !s.trim().is_empty()) {
            // ensure we end with a newline before appending.
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(extra);
            if !out.ends_with('\n') {
                out.push('\n');
            }
        }

        out.push_str("---\n");
        out
    }
}

fn yaml_quote(s: &str) -> String {
    // escape backslashes and double quotes.
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// If `text` starts with YAML frontmatter (`---` ... `---`), return the frontmatter
/// block verbatim (including both `---` lines and their original newlines) and
/// the remainder of the document.
pub fn split_yaml_frontmatter(text: &str) -> Option<(String, &str)> {
    // accept both \n and \r\n.
    if !text.starts_with("---") {
        return None;
    }
    // "---" must be exactly on the first line.
    if !(text.starts_with("---\n") || text.starts_with("---\r\n")) {
        return None;
    }

    // scan line-by-line to preserve verbatim content until closing delimiter found.
    let mut pos = 0usize;
    let mut lines = text.split_inclusive(['\n']);
    let first = lines.next()?;
    pos += first.len();

    for line in lines {
        pos += line.len();
        // `line` includes trailing `\n`.
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed == "---" {
            let fm = text[..pos].to_string();
            let rest = &text[pos..];
            return Some((fm, rest));
        }
    }
    None
}

/// Build frontmatter from a parsed document.
pub fn build_frontmatter(
    article_id: &str,
    wiki_path: &Path,
    doc: &Document,
    mediawiki_base_url: &str,
) -> io::Result<Frontmatter> {
    let source_url = format!(
        "{}/{}",
        mediawiki_base_url.trim_end_matches('/'),
        article_id
    );

    let last_fetched_date = wiki_file_mod_date(wiki_path)?;

    let aliases = vec![article_id.replace('_', " ")];

    let tags = extract_tags(doc, article_id);

    Ok(Frontmatter {
        wiki2md: Wiki2mdMeta {
            article_id: article_id.to_string(),
            source_url,
            generated_by: "wiki2md".to_string(),
            last_fetched_date,
            schema_version: 1,
        },
        aliases,
        tags,
        summary: None,
        extras_yaml: None,
    })
}

/// When frontmatter regeneration is requested, we still want to preserve user-authored
/// fields where possible (e.g., an LLM summary) and any extra top-level keys.
///
/// This function:
/// - Extracts a top-level `summary` string (if present).
/// - Preserves any other unknown top-level keys by serializing them back to YAML.
pub fn merge_existing_frontmatter_for_regeneration(
    generated: &mut Frontmatter,
    existing_yaml: &str,
) {
    let Some((yaml_body, _rest)) = split_yaml_frontmatter(existing_yaml) else {
        return;
    };

    let Some(inner) = extract_yaml_inner(&yaml_body) else {
        return;
    };

    let Ok(val) = serde_yaml::from_str::<Value>(&inner) else {
        return;
    };
    let Value::Mapping(mut map) = val else {
        return;
    };

    // preserve `summary` if present and non-empty.
    if let Some(Value::String(s)) = map.get(Value::String("summary".to_string()))
        && !s.trim().is_empty()
    {
        generated.summary = Some(s.clone());
    }

    // remove keys we manage.
    for k in ["wiki2md", "aliases", "tags", "summary"] {
        map.remove(Value::String(k.to_string()));
    }

    if map.is_empty() {
        generated.extras_yaml = None;
        return;
    }

    // serialize the remaining keys.
    let serialized = serde_yaml::to_string(&Value::Mapping(map)).unwrap_or_default();
    let extras = strip_yaml_document_markers(&serialized);
    if !extras.trim().is_empty() {
        generated.extras_yaml = Some(extras);
    }
}

fn extract_yaml_inner(frontmatter_block: &str) -> Option<String> {
    // preserve content between delimiter lines.
    let mut lines = frontmatter_block.lines();
    let first = lines.next()?;
    if first.trim_end() != "---" {
        return None;
    }

    let mut out = String::new();
    for line in lines {
        if line.trim_end() == "---" {
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    Some(out)
}

fn strip_yaml_document_markers(s: &str) -> String {
    // serde_yaml typically omits these markers; added checks for defensive code.
    let mut out = s.to_string();
    if out.starts_with("---\n") {
        out = out.trim_start_matches("---\n").to_string();
    }
    if out.ends_with("...\n") {
        out = out.trim_end_matches("...\n").to_string();
    }
    out
}

fn wiki_file_mod_date(wiki_path: &Path) -> io::Result<String> {
    let meta = fs::metadata(wiki_path)?;
    let mtime = meta.modified()?;
    let dt = OffsetDateTime::from(mtime);
    let fmt = format_description!("[year]-[month]-[day]");
    Ok(dt.format(&fmt).unwrap_or_else(|_| "1970-01-01".to_string()))
}

/// Extract tags from:
/// - The first "breadcrumb" paragraph (detected by a link to `Main Page`).
/// - bottom-of-article `[[Category:...]]` metadata stored in `doc.categories`.
pub fn extract_tags(doc: &Document, article_id: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();

    let article_title = article_id.replace('_', " ").to_ascii_lowercase();

    // top-of-page nav tags.
    if let Some(nav) = find_top_nav_links(doc) {
        for target in nav {
            if target.eq_ignore_ascii_case("Main Page") {
                continue;
            }
            // ignore self
            if target.replace('_', " ").to_ascii_lowercase() == article_title {
                continue;
            }
            if let Some(t) = normalize_tag(&target) {
                out.push(t);
            }
        }
    }

    // bottom categories near the end of the article.
    let doc_end = doc.span.end as usize;
    let bottom_window = doc_end.saturating_sub(8 * 1024);
    for cat in &doc.categories {
        if (doc_end <= 8 * 1024 || (cat.span.start as usize) >= bottom_window)
            && let Some(t) = normalize_tag(&cat.name)
        {
            out.push(t);
        }
    }

    // deduplicate and sort
    out.sort();
    out.dedup();
    out
}

fn find_top_nav_links(doc: &Document) -> Option<Vec<String>> {
    for block in &doc.blocks {
        let BlockKind::Paragraph { content } = &block.kind else {
            continue;
        };
        let mut targets: Vec<String> = Vec::new();
        let mut saw_main = false;
        collect_internal_link_targets(content, &mut targets, &mut saw_main);
        if saw_main {
            return Some(targets);
        }
    }
    None
}

fn collect_internal_link_targets(nodes: &[InlineNode], out: &mut Vec<String>, saw_main: &mut bool) {
    for n in nodes {
        match &n.kind {
            InlineKind::InternalLink { link } => {
                if link.target.eq_ignore_ascii_case("Main Page") {
                    *saw_main = true;
                }
                out.push(link.target.clone());
                if let Some(t) = &link.text {
                    collect_internal_link_targets(t, out, saw_main);
                }
            }
            InlineKind::Bold { content }
            | InlineKind::Italic { content }
            | InlineKind::BoldItalic { content } => {
                collect_internal_link_targets(content, out, saw_main)
            }
            InlineKind::Ref { node } => {
                if let Some(c) = &node.content {
                    collect_internal_link_targets(c, out, saw_main);
                }
            }
            InlineKind::HtmlTag { node } => {
                collect_internal_link_targets(&node.children, out, saw_main)
            }
            InlineKind::Template { node } => {
                for p in &node.params {
                    collect_internal_link_targets(&p.value, out, saw_main);
                }
            }
            InlineKind::FileLink { link } => {
                for p in &link.params {
                    collect_internal_link_targets(&p.content, out, saw_main);
                }
            }
            InlineKind::ExternalLink { link } => {
                if let Some(t) = &link.text {
                    collect_internal_link_targets(t, out, saw_main);
                }
            }
            InlineKind::Text { .. } | InlineKind::LineBreak | InlineKind::Raw { .. } => {}
        }
    }
}

/// Normalize a tag according to Obsidian constraints (for now: no nesting).
///
/// Rules enforced:
/// - Only lowercase ASCII letters, digits, `_`, `-`
/// - Must start with a letter
/// - Must not be fully numeric
/// - Must not be empty
/// - <= 50 chars
/// - No `/` characters
pub fn normalize_tag(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    // transliterate into the 26-letter English alphabet using `deunicode`.
    let s = deunicode(raw).to_ascii_lowercase();
    if s.is_empty() {
        return None;
    }

    // replace common separators/punctuation with `_`, drop others.
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            'a'..='z' | '0'..='9' | '_' | '-' => out.push(ch),
            '/' => out.push('_'),
            ' ' | '\t' | '\n' | '\r' | ':' | '.' | ',' | ';' | '(' | ')' | '[' | ']' | '{'
            | '}' | '!' | '?' | '\'' | '"' | '&' | '+' | '=' | '#' => out.push('_'),
            _ => out.push('_'),
        }
    }

    // collapse underscores.
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    out = out.trim_matches('_').to_string();

    if out.is_empty() {
        return None;
    }

    // convert if fully numeric.
    if out.chars().all(|c| c.is_ascii_digit()) {
        let mut tag = if out.len() == 4 {
            if let Ok(y) = out.parse::<i32>() {
                if (1400..=2100).contains(&y) {
                    format!("y{}", out)
                } else {
                    format!("n{}", out)
                }
            } else {
                format!("n{}", out)
            }
        } else {
            format!("n{}", out)
        };
        if tag.len() > 50 {
            tag.truncate(50);
        }
        return Some(tag);
    }

    // ensure starts with a letter.
    if !out.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
        out = format!("t{}", out);
    }

    // enforce allowed charset and max length.
    out.retain(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    if out.is_empty() {
        return None;
    }
    if out.len() > 50 {
        out.truncate(50);
    }
    // must not contain `/`.
    if out.contains('/') {
        return None;
    }
    Some(out)
}
