use wiki2md::{ast::*, parse};
use wiki2md::parse::ParseOutput;

fn get_ast_file(src: String, parse_out: ParseOutput) -> AstFile {
    AstFile {
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
            byte_len: src.as_bytes().len() as u64,
        },
        diagnostics: parse_out.diagnostics,
        document: parse_out.document,
    }
}

/// Regression test for pathological list marker prefixes like `:::::::::::::::::`.
///
/// Without a depth cap, deeply nested lists can produce an AST that exceeds
/// `serde_json`'s recursion limit when the tool round-trips the AST through
/// pretty-printed JSON.
#[test]
fn json_round_trip_survives_pathological_list_depth() {
    // 200 levels is well beyond anything we'd want to support structurally.
    // the parser should clamp this down to a safe depth.
    let src = format!("{}item\n", ":".repeat(200));
    let parse_out = parse::parse_document(&src);
    let ast = get_ast_file(src, parse_out);

    // the key part of this test: pretty JSON should serialize and deserialize
    // without triggering recursion-limit errors.
    let json = serde_json::to_string_pretty(&ast).expect("serialize");
    let back: AstFile = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(ast, back);
}

#[test]
fn regression_deeply_nested_colons() {
    let src = include_str!("crashes/minimized000.txt").to_string();
    let parse_out = parse::parse_document(&src);
    let ast = get_ast_file(src, parse_out);

    let json = serde_json::to_string_pretty(&ast).expect("serialize");
    let back: AstFile = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(ast, back);
}
