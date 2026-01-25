use std::fs;
use std::path::PathBuf;
use wiki2md::ast::*;
use wiki2md::parse;
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
fn crashers_do_not_panic() {
    let crash_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("crashes");

    let cases = ["minimized000.txt", "minimized001.txt", "minimized002.txt"];

    let mut failures: Vec<String> = Vec::new();

    for file in cases {
        let path = crash_dir.join(file);

        let bytes = fs::read(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

        // accept arbitrary bytes (AFL inputs are frequently non-UTF8).
        let src = String::from_utf8_lossy(&bytes).into_owned();

        // catch panics so the test can report which file blew up.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || -> Result<(), String> {
            let parse_out = parse::parse_document(&src);
            let ast = get_ast_file(src, parse_out);

            let json = serde_json::to_string_pretty(&ast)
                .map_err(|e| format!("serialize failed: {e}"))?;

            let back: AstFile = serde_json::from_str(&json)
                .map_err(|e| format!("deserialize failed: {e}"))?;

            if ast != back {
                return Err("AST mismatch after JSON round-trip".to_string());
            }

            Ok(())
        }));

        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(msg)) => failures.push(format!("{file}: {msg}")),
            Err(panic_payload) => {
                let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                    (*s).to_string()
                } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "<non-string panic payload>".to_string()
                };

                failures.push(format!("{file}: panicked: {msg}"));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "one or more regression inputs failed:\n{}",
        failures.join("\n")
    );
}
