//! Pure-Rust parsers, analyzers, catalogs, and refactoring transforms shared by
//! the `openidea.dart` and `openidea.flutter` plugins.

pub mod cli;
pub mod dart_syntax;
pub mod flutter_catalog;
pub mod flutter_widgets;
pub mod pubspec;
pub mod test_json;

/// 0-based line and UTF-16 code-unit column position.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextPos {
    pub line: u32,
    pub character: u32,
}

impl TextPos {
    pub const fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

/// 0-based UTF-16 text span.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextSpan {
    pub start: TextPos,
    pub end: TextPos,
}

impl TextSpan {
    pub const fn new(start: TextPos, end: TextPos) -> Self {
        Self { start, end }
    }
}

/// Precomputed line start byte offsets for fast byte-offset <-> UTF-16 `TextPos` mapping.
#[derive(Clone, Debug)]
pub struct LineIndex {
    line_starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![0];
        for (idx, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(idx + 1);
            }
        }
        Self { line_starts }
    }

    pub fn line_count(&self) -> u32 {
        self.line_starts.len() as u32
    }

    pub fn line_start(&self, line: u32) -> Option<usize> {
        self.line_starts.get(line as usize).copied()
    }

    /// Converts a byte offset into a 0-based `(line, utf16_col)` position.
    pub fn pos_at(&self, text: &str, byte_offset: usize) -> TextPos {
        let clamped = byte_offset.min(text.len());
        let line_idx = match self.line_starts.binary_search(&clamped) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let line_start = self.line_starts[line_idx];
        let slice = &text[line_start..clamped];
        let utf16_col: usize = slice.chars().map(char::len_utf16).sum();
        TextPos::new(line_idx as u32, utf16_col as u32)
    }

    /// Converts a 0-based `(line, utf16_col)` position to a byte offset.
    pub fn offset_at(&self, text: &str, pos: TextPos) -> usize {
        let line_idx = (pos.line as usize).min(self.line_starts.len().saturating_sub(1));
        let start = self.line_starts[line_idx];
        let end = self
            .line_starts
            .get(line_idx + 1)
            .map(|&n| n.saturating_sub(1))
            .unwrap_or(text.len());
        let line_text = text[start..end].strip_suffix('\r').unwrap_or(&text[start..end]);
        let mut utf16 = 0u32;
        for (rel_byte, ch) in line_text.char_indices() {
            if utf16 >= pos.character {
                return start + rel_byte;
            }
            utf16 += ch.len_utf16() as u32;
        }
        start + line_text.len()
    }

    pub fn span(&self, text: &str, byte_start: usize, byte_end: usize) -> TextSpan {
        TextSpan::new(self.pos_at(text, byte_start), self.pos_at(text, byte_end))
    }
}

/// Counts UTF-16 code units in `s`.
pub fn utf16_len(s: &str) -> u32 {
    s.chars().map(char::len_utf16).sum::<usize>() as u32
}

/// Byte offset of `utf16_col` within a single line (without trailing newline).
pub fn utf16_col_to_byte(line: &str, utf16_col: u32) -> usize {
    let mut seen = 0u32;
    for (idx, ch) in line.char_indices() {
        if seen >= utf16_col {
            return idx;
        }
        seen += ch.len_utf16() as u32;
    }
    line.len()
}
