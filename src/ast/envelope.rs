use crate::ast::{Diagnostic, Document};
use serde::{Deserialize, Serialize};

/// Top-level JSON file written to `./docs/json/{bucket}/{article_id}.json`.
///
/// This wraps a parsed `Document` with metadata that makes debugging easier
/// (schema versioning, span encoding, source info, diagnostics).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AstFile {
    /// Schema version for this JSON payload.
    pub schema_version: u32,

    pub parser: ParserInfo,

    /// How to interpret all `Span` values contained in this file.
    pub span_encoding: SpanEncoding,

    /// Stable identifier used for caching on disk.
    pub article_id: String,

    pub source: SourceInfo,

    /// Parser/validator diagnostics.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,

    pub document: Document,
}

/// Identifies the program that produced the AST.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParserInfo {
    pub name: String,
    pub version: String,
}

/// Captures how `Span` offsets should be interpreted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpanEncoding {
    pub unit: SpanUnit,
    pub base: SpanBase,
}

impl Default for SpanEncoding {
    fn default() -> Self {
        Self {
            unit: SpanUnit::Byte,
            base: SpanBase::RawInput,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanUnit {
    /// Byte offsets (UTF-8).
    Byte,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanBase {
    /// Offsets are measured against the raw input bytes as read from disk
    /// (no normalization pass was applied before spanning).
    RawInput,
}

/// Optional information about the input source used to produce the AST.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceInfo {
    /// If available, a path to the `.wiki` file used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    /// If available, a SHA-256 of the `.wiki` content used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,

    /// Length of the input in bytes.
    pub byte_len: u64,
}
