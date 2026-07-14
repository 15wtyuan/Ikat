#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticCode {
    FenceUnknownTag,
    FenceUnknownAttr,
    FenceUnknownCssProp,
    FenceBadCssValue,
    FenceBadAttrValue,
    DuplicateId,
    UnclosedTag,
    InvalidContentModel,
    InvalidIdRef,
    InvalidTemplateRoot,
    UnregisteredCustomElement,
    InvalidAriaRelation,
    TokenizerError,
}

#[derive(Debug, Clone)]
pub struct SourceLocation {
    pub file: String,
    pub offset: usize,
    pub line: u32,
    pub column: u32,
    pub source_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteKind {
    Help,
    Note,
    Related,
}

#[derive(Debug, Clone)]
pub struct DiagnosticNote {
    pub kind: NoteKind,
    pub text: String,
    pub location: Option<SourceLocation>,
}

/// A structured diagnostic produced by the fence pipeline.
///
/// The pipeline collects ALL diagnostics in a single pass and reports them
/// once, rather than failing on the first error -- this is critical for
/// AI-assisted authoring where fixing all errors in one round minimises
/// dialogue turns.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: DiagnosticCode,
    pub message: String,
    pub location: SourceLocation,
    pub notes: Vec<DiagnosticNote>,
}

impl Diagnostic {
    pub fn error(
        code: DiagnosticCode,
        message: impl Into<String>,
        location: SourceLocation,
    ) -> Self {
        Self {
            severity: Severity::Error,
            code,
            message: message.into(),
            location,
            notes: Vec::new(),
        }
    }

    pub fn with_help(mut self, text: impl Into<String>) -> Self {
        self.notes.push(DiagnosticNote {
            kind: NoteKind::Help,
            text: text.into(),
            location: None,
        });
        self
    }
}

/// Pre-computed line-offset table for O(log n) offset-to-line/column lookup.
///
/// Built once per source file. `locate(offset)` performs a binary search over
/// `line_starts` to find the 1-based (line, column) pair.
#[derive(Debug, Clone)]
pub struct LineMap {
    line_starts: Vec<usize>,
    source: String,
}

impl LineMap {
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Self {
            line_starts,
            source: source.to_string(),
        }
    }

    /// Convert a byte offset to a 1-based (line, column) pair.
    pub fn locate(&self, offset: usize) -> (u32, u32) {
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        };
        let col = offset.saturating_sub(self.line_starts[line_idx]);
        ((line_idx + 1) as u32, (col + 1) as u32)
    }

    /// Build a full `SourceLocation` for a byte offset, including the source
    /// text of the offending line (trimmed of trailing newlines).
    pub fn source_location(&self, offset: usize, file: String) -> SourceLocation {
        let (line, column) = self.locate(offset);
        let source_text = self.source_line(line);
        SourceLocation {
            file,
            offset,
            line,
            column,
            source_text,
        }
    }

    fn source_line(&self, line: u32) -> String {
        let idx = (line as usize).saturating_sub(1);
        let start = *self.line_starts.get(idx).unwrap_or(&0);
        let end = self
            .line_starts
            .get(idx + 1)
            .copied()
            .unwrap_or(self.source.len());
        self.source[start..end]
            .trim_end_matches('\n')
            .trim_end_matches('\r')
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_map_single_line() {
        let map = LineMap::new("hello world");
        assert_eq!(map.locate(0), (1, 1));
        assert_eq!(map.locate(5), (1, 6));
    }

    #[test]
    fn line_map_multi_line() {
        let map = LineMap::new("ab\ncd\nef");
        assert_eq!(map.locate(0), (1, 1));
        assert_eq!(map.locate(3), (2, 1));
        assert_eq!(map.locate(6), (3, 1));
    }

    #[test]
    fn source_location_has_line_text() {
        let map = LineMap::new("ab\ncd");
        let loc = map.source_location(3, "test.html".into());
        assert_eq!(loc.line, 2);
        assert_eq!(loc.column, 1);
        assert_eq!(loc.source_text, "cd");
    }
}
