//! AFL++ fuzz target for `wiki2md`.
//!
//! This binary is intentionally stdin-driven, so it can be used with AFL++.
//! Build and run it via `cargo-afl`:
//!
//! ```bash
//! cargo install cargo-afl
//!
//! cargo afl build --release --features afl_fuzz --bin wiki2md_afl_parse
//!
//! mkdir -p fuzz/afl/out
//!
//! cargo afl fuzz \
//!   -i fuzz/afl/in \
//!   -o fuzz/afl/out \
//!   -x fuzz/afl/dict/wikitext.dict \
//!   target/release/wiki2md_afl_parse
//! ```
//!
//! Rust panics normally unwind and exit with a non-crashing status code.
//! AFL++ only treats crashes as signals/aborts. We therefore catch any unwind
//! and turn it into `abort()`.

use std::io::Read;

use wiki2md::{ast::*, parse, render};

const MAX_INPUT_LEN: usize = 1_000_000; // 1MB guardrail; AFL++ will typically cap this anyway.

fn check_span(span: &Span, len: usize) {
    let s = span.start as usize;
    let e = span.end as usize;
    assert!(s <= e, "invalid span: start > end: {span:?}");
    assert!(e <= len, "span out of bounds (len={len}): {span:?}");
}

fn check_inlines(nodes: &[InlineNode], len: usize) {
    for n in nodes {
        check_span(&n.span, len);
        match &n.kind {
            InlineKind::Text { .. } => {}
            InlineKind::Bold { content }
            | InlineKind::Italic { content }
            | InlineKind::BoldItalic { content } => check_inlines(content, len),
            InlineKind::InternalLink { link } => {
                if let Some(t) = &link.text {
                    check_inlines(t, len);
                }
            }
            InlineKind::ExternalLink { link } => {
                if let Some(t) = &link.text {
                    check_inlines(t, len);
                }
            }
            InlineKind::FileLink { link } => {
                for p in &link.params {
                    check_span(&p.span, len);
                    check_inlines(&p.content, len);
                }
            }
            InlineKind::LineBreak => {}
            InlineKind::Ref { node } => {
                for a in &node.attrs {
                    if let Some(s) = &a.span {
                        check_span(s, len);
                    }
                }
                if let Some(c) = &node.content {
                    check_inlines(c, len);
                }
            }
            InlineKind::HtmlTag { node } => {
                for a in &node.attrs {
                    if let Some(s) = &a.span {
                        check_span(s, len);
                    }
                }
                check_inlines(&node.children, len);
            }
            InlineKind::Template { node } => {
                for p in &node.params {
                    check_span(&p.span, len);
                    check_inlines(&p.value, len);
                }
            }
            InlineKind::Raw { .. } => {}
        }
    }
}

fn check_blocks(nodes: &[BlockNode], len: usize) {
    for n in nodes {
        check_span(&n.span, len);
        match &n.kind {
            BlockKind::Heading { content, .. } => check_inlines(content, len),
            BlockKind::Paragraph { content } => check_inlines(content, len),
            BlockKind::List { items } => {
                for it in items {
                    check_span(&it.span, len);
                    check_blocks(&it.blocks, len);
                }
            }
            BlockKind::Table { table } => {
                for a in &table.attrs {
                    if let Some(s) = &a.span {
                        check_span(s, len);
                    }
                }
                if let Some(cap) = &table.caption {
                    check_span(&cap.span, len);
                    for a in &cap.attrs {
                        if let Some(s) = &a.span {
                            check_span(s, len);
                        }
                    }
                    check_inlines(&cap.content, len);
                }
                for row in &table.rows {
                    check_span(&row.span, len);
                    for a in &row.attrs {
                        if let Some(s) = &a.span {
                            check_span(s, len);
                        }
                    }
                    for cell in &row.cells {
                        check_span(&cell.span, len);
                        for a in &cell.attrs {
                            if let Some(s) = &a.span {
                                check_span(s, len);
                            }
                        }
                        check_blocks(&cell.blocks, len);
                    }
                }
            }
            BlockKind::CodeBlock { .. } => {}
            BlockKind::References { node } => {
                for a in &node.attrs {
                    if let Some(s) = &a.span {
                        check_span(s, len);
                    }
                }
            }
            BlockKind::HtmlBlock { node } => {
                for a in &node.attrs {
                    if let Some(s) = &a.span {
                        check_span(s, len);
                    }
                }
                check_blocks(&node.children, len);
            }
            BlockKind::MagicWord { .. } => {}
            BlockKind::HorizontalRule => {}
            BlockKind::BlockQuote { blocks } => check_blocks(blocks, len),
            BlockKind::Raw { .. } => {}
        }
    }
}

fn validate_document(doc: &Document, src_len: usize) {
    check_span(&doc.span, src_len);
    for c in &doc.categories {
        check_span(&c.span, src_len);
    }
    if let Some(r) = &doc.redirect {
        check_span(&r.span, src_len);
    }
    check_blocks(&doc.blocks, src_len);
}

fn run_one_input(data: &[u8]) {
    if data.len() > MAX_INPUT_LEN {
        // guardrail: avoid pathological OOM / quadratic behavior on enormous inputs.
        return;
    }

    // wikitext exports should be UTF-8, but AFL++ will happily hand us arbitrary bytes.
    // lossy conversion keeps the harness total (no early returns that reduce coverage).
    let src = String::from_utf8_lossy(data).to_string();

    let out = parse::parse_document(&src);

    // build a full envelope to exercise JSON serialization.
    let ast_file = AstFile {
        schema_version: SCHEMA_VERSION,
        parser: ParserInfo {
            name: PARSER_NAME.to_string(),
            version: PARSER_VERSION.to_string(),
        },
        span_encoding: SpanEncoding::default(),
        article_id: "fuzz".to_string(),
        source: SourceInfo {
            path: None,
            sha256: None,
            byte_len: src.len() as u64,
        },
        diagnostics: out.diagnostics,
        document: out.document,
    };

    // invariants that must hold for any input (valid or invalid):
    // - spans never go out of bounds
    // - renderer never panics
    validate_document(&ast_file.document, src.len());

    // JSON round-trip must never panic.
    let json = serde_json::to_vec(&ast_file).unwrap();
    let back: AstFile = serde_json::from_slice(&json).unwrap();

    // rendering should never panic.
    let _md = render::render_ast(&back);
}

fn main() {
    let mut data = Vec::new();
    std::io::stdin().read_to_end(&mut data).unwrap();

    // convert any panic into an abort().
    if std::panic::catch_unwind(|| run_one_input(&data)).is_err() {
        std::process::abort();
    }
}
