use wiki2md::ast::BlockKind;
use wiki2md::parse;

#[test]
fn pathological_open_delimiter_runs_are_treated_as_text() {
    // These inputs are intentionally pathological: huge runs of opening delimiters
    // that would otherwise trigger quadratic scanning in the inline parser.
    let cases = [
        ("braces", "{".repeat(20_000)),
        ("brackets", "[".repeat(20_000)),
    ];

    for (name, src) in cases {
        let parse_out = parse::parse_wiki(&src);

        // Ensure the pathological-run guard actually fired (and did not just happen
        // to return quickly for some other reason).
        assert!(
            parse_out
                .diagnostics
                .iter()
                .any(|d| d.code.as_deref() == Some("wikitext.inline.pathological_delim_run")),
            "expected delimiter-run diagnostic for case '{name}'"
        );

        // Ensure we treated the input as ordinary text (no template/link parsing).
        let Some(first_block) = parse_out.document.blocks.first() else {
            panic!("expected at least one block for case '{name}'");
        };

        match &first_block.kind {
            BlockKind::Paragraph { content } => {
                assert_eq!(content.len(), 1, "expected a single text inline for '{name}'");
            }
            other => panic!("expected Paragraph for '{name}', got {other:?}"),
        }
    }
}
