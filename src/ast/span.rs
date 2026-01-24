use serde::{Deserialize, Serialize};

/// A half-open byte-span `[start, end)` into the **raw** `.wiki` source.
///
/// Offsets are measured in bytes (UTF-8). This is deliberate:
/// - It matches Rust string indexing constraints.
/// - It stays stable even when the text contains multi-byte Unicode.
///
/// IMPORTANT: Spans are defined over the *pre-normalization* input bytes.
/// Do not rewrite line endings or otherwise transform the input before
/// computing spans unless you also maintain an explicit mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Hash)]
pub struct Span {
    pub start: u64,
    pub end: u64,
}

impl Span {
    #[inline]
    pub fn new(start: u64, end: u64) -> Self {
        debug_assert!(start <= end, "Span start must be <= end");
        Self { start, end }
    }

    #[inline]
    pub fn len(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    #[inline]
    pub fn contains(&self, pos: u64) -> bool {
        self.start <= pos && pos < self.end
    }

    /// Returns a span that covers both `self` and `other`.
    #[inline]
    pub fn cover(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}
