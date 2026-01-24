use crate::ast::Span;
use serde::{Deserialize, Serialize};

/// Severity level of a diagnostic emitted by the parser or validator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// The phase that produced the diagnostic.
///
/// We keep this optional so callers can log diagnostics even if they do not
/// distinguish phases yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticPhase {
    Lex,
    Parse,
    Validate,
    Normalize,
}

/// A structured diagnostic for debugging parsing/validation issues.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: Severity,

    /// Which phase produced this diagnostic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<DiagnosticPhase>,

    /// A stable identifier like `wikitext.unclosed_tag`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,

    /// Human readable message.
    pub message: String,

    /// The source span this diagnostic refers to, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,

    /// Optional notes that can help explain recovery decisions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}
