//! Wikitext AST and JSON envelope.
//!
//! This module defines the **contract** between:
//! 1) parsing `.wiki` source -> `Document` (AST), and
//! 2) rendering `Document` -> Markdown.
//!
//! Design goals:
//! - High-fidelity structure for debugging and correctness.
//! - Stable JSON representation for on-disk inspection.
//! - Precise span offsets into the **raw input bytes** (no pre-normalization).
//! - Clear separation between *Wikitext parsing* and *Markdown rendering*.

mod diagnostic;
mod envelope;
mod nodes;
mod span;

pub use diagnostic::*;
pub use envelope::*;
pub use nodes::*;
pub use span::*;

/// JSON schema version for the AST envelope.
///
/// Bump this when making non-backwards-compatible changes to the JSON structure.
pub const SCHEMA_VERSION: u32 = 1;

/// The parser name stored in the JSON envelope.
pub const PARSER_NAME: &str = "wiki2md";

/// The parser version stored in the JSON envelope.
pub const PARSER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn astfile_json_round_trip() {
        let doc = Document {
            span: Span::new(0, 42),
            blocks: vec![BlockNode {
                span: Span::new(0, 12),
                kind: BlockKind::Heading {
                    level: 1,
                    content: vec![InlineNode {
                        span: Span::new(1, 11),
                        kind: InlineKind::Text {
                            value: "Title".to_string(),
                        },
                    }],
                },
            }],
            categories: vec![CategoryTag {
                span: Span::new(30, 41),
                name: "Chess Programmer".to_string(),
                sort_key: Some("Thompson".to_string()),
            }],
            redirect: None,
        };

        let ast = AstFile {
            schema_version: SCHEMA_VERSION,
            parser: ParserInfo {
                name: PARSER_NAME.to_string(),
                version: PARSER_VERSION.to_string(),
            },
            span_encoding: SpanEncoding::default(),
            article_id: "Ken_Thompson".to_string(),
            source: SourceInfo {
                path: Some("docs/wiki/k/Ken_Thompson.wiki".to_string()),
                sha256: None,
                byte_len: 42,
            },
            diagnostics: vec![Diagnostic {
                severity: Severity::Info,
                phase: Some(DiagnosticPhase::Parse),
                code: Some("example".to_string()),
                message: "example diagnostic".to_string(),
                span: Some(Span::new(5, 10)),
                notes: vec!["note".to_string()],
            }],
            document: doc,
        };

        let json = serde_json::to_string_pretty(&ast).expect("serialize");
        let back: AstFile = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(ast, back);
    }
}
