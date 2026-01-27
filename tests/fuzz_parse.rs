//! Lightweight fuzz-style tests; no external fuzz harness required.

use wiki2md::{ast::*, parse};

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

#[derive(Clone)]
struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self { state: seed.max(1) }
    }

    fn next_u64(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    fn gen_range(&mut self, hi: usize) -> usize {
        (self.next_u64() as usize) % hi
    }
}

fn gen_wikitext_like(rng: &mut XorShift64, len: usize) -> String {
    // restrict to a "wikitext-relevant" alphabet, so we hit interesting parsing paths
    // while keeping the string valid UTF-8.
    const DICT: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 \n\t[]{}|!<>='\"/:#*;-_";
    let mut s = String::with_capacity(len);
    for _ in 0..len {
        let ch = DICT[rng.gen_range(DICT.len())] as char;
        s.push(ch);
    }
    s
}

#[test]
fn fuzz_parse_random_inputs_total_and_in_bounds() {
    // keep cases bounded so this doesn't slow down normal `cargo test` too much.
    let mut rng = XorShift64::new(0xC0FFEE);
    for _case in 0..2_000 {
        let len = rng.gen_range(4_000);
        let input = gen_wikitext_like(&mut rng, len);
        let out = parse::parse_wiki(&input);
        validate_document(&out.document, input.len());
    }
}

#[test]
fn fuzz_parse_codeblock_same_line_with_tail_does_not_hang() {
    // this targets the historical hang: `<pre>...</pre>` on a single line, followed by `\n\n`.
    // the parser must advance and must preserve any trailing text after `</pre>`.
    let input = "<pre>code</pre> tail\n\n";
    let out = parse::parse_wiki(input);
    validate_document(&out.document, input.len());

    // basic structural assertion: we should have a code block, and we should not drop the tail.
    // the exact AST shape can evolve; this just ensures the tail isn't silently eaten.
    let mut saw_code = false;
    let mut saw_tail = false;
    for b in &out.document.blocks {
        match &b.kind {
            BlockKind::CodeBlock { .. } => saw_code = true,
            BlockKind::Paragraph { content } => {
                let txt: String = content
                    .iter()
                    .filter_map(|n| match &n.kind {
                        InlineKind::Text { value } => Some(value.as_str()),
                        _ => None,
                    })
                    .collect();
                if txt.contains("tail") {
                    saw_tail = true;
                }
            }
            _ => {}
        }
    }
    assert!(saw_code, "expected a code block");
    assert!(saw_tail, "expected trailing text after </pre> to be preserved");
}
