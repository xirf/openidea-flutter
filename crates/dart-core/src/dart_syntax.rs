//! Dart & `pubspec.yaml` lexer, semantic token generator, runnable/directive
//! scanner, and folding range detector.

use crate::{LineIndex, TextPos, TextSpan, utf16_len};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticKind {
    Namespace,
    Type,
    Class,
    Enum,
    Interface,
    Struct,
    TypeParameter,
    Parameter,
    Variable,
    Property,
    EnumMember,
    Event,
    Function,
    Method,
    Macro,
    Keyword,
    Modifier,
    Comment,
    String,
    Number,
    Regexp,
    Operator,
    Decorator,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticMod {
    Declaration,
    Definition,
    Readonly,
    Static,
    Deprecated,
    Abstract,
    Async,
    Documentation,
    DefaultLibrary,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DartToken {
    pub span: TextSpan,
    pub kind: SemanticKind,
    pub modifiers: Vec<SemanticMod>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunnableKind {
    Main,
    Test,
    TestWidget,
    Group,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunnableEntry {
    pub line: u32,
    pub kind: RunnableKind,
    pub label: String,
    pub test_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectiveLink {
    pub kind: &'static str,
    pub uri: String,
    pub span: TextSpan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DartFoldKind {
    Imports,
    Comment,
    Region,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DartFold {
    pub start_line: u32,
    pub end_line: u32,
    pub kind: DartFoldKind,
}

const DART_KEYWORDS: &[&str] = &[
    "abstract", "as", "assert", "async", "await", "base", "break", "case", "catch", "class",
    "const", "continue", "covariant", "default", "deferred", "do", "dynamic", "else", "enum",
    "export", "extends", "extension", "external", "factory", "false", "final", "finally", "for",
    "Function", "get", "hide", "if", "implements", "import", "in", "interface", "is", "late",
    "library", "mixin", "new", "null", "on", "operator", "part", "required", "rethrow", "return",
    "sealed", "set", "show", "static", "super", "switch", "sync", "this", "throw", "true", "try",
    "typedef", "var", "void", "when", "while", "with", "yield",
];

const DART_CORE_TYPES: &[&str] = &[
    "bool", "double", "int", "num", "String", "dynamic", "Object", "Never", "Null", "Future",
    "FutureOr", "Stream", "Iterable", "Iterator", "List", "Set", "Map", "Symbol", "Type",
    "Record", "Enum", "Duration", "DateTime", "Uri", "RegExp", "Pattern", "Match", "Error",
    "Exception", "StackTrace", "Comparable", "Sink", "Completer", "BigInt", "Runes", "Uint8List",
    "ByteData",
];

const FLUTTER_TYPES: &[&str] = &[
    "Widget", "StatelessWidget", "StatefulWidget", "State", "InheritedWidget", "InheritedNotifier",
    "BuildContext", "Key", "GlobalKey", "ValueKey", "ObjectKey", "UniqueKey", "Color", "Colors",
    "CupertinoColors", "Icons", "CupertinoIcons", "IconData", "TextStyle", "EdgeInsets",
    "EdgeInsetsDirectional", "Alignment", "BoxDecoration", "BorderRadius", "Border", "BoxShadow",
    "Text", "RichText", "Icon", "Image", "Container", "SizedBox", "Padding", "Center", "Align",
    "Column", "Row", "Flex", "Expanded", "Flexible", "Spacer", "Stack", "Positioned", "Wrap",
    "ListView", "GridView", "SingleChildScrollView", "CustomScrollView", "SliverList", "Scaffold",
    "AppBar", "BottomNavigationBar", "NavigationBar", "FloatingActionButton", "Drawer",
    "MaterialApp", "CupertinoApp", "Theme", "ThemeData", "ColorScheme", "MediaQuery", "Navigator",
    "Builder", "LayoutBuilder", "StatefulBuilder", "FutureBuilder", "StreamBuilder", "ValueListenableBuilder",
    "AnimatedBuilder", "GestureDetector", "InkWell", "ElevatedButton", "TextButton", "OutlinedButton",
    "IconButton", "TextField", "TextFormField", "Form", "Card", "Divider", "CircularProgressIndicator",
    "TickerProviderStateMixin", "SingleTickerProviderStateMixin", "AnimationController", "Animation",
    "ChangeNotifier", "ValueNotifier", "FocusNode", "ScrollController", "TextEditingController",
    "WidgetTester",
];

#[derive(Clone, Copy)]
enum OverlayMode {
    Dart { highlight_calls: bool },
    Flutter,
}

/// Emits non-clobbering Dart overlay tokens alongside `dart language-server`:
/// - `[SymbolRef]` links inside `///` doc comments
/// - When `highlight_calls` is true: function/method declarations and calls
///   (including generic calls like `readList<String>(...)` and static/dot calls
///   like `IcaModelOption.fromJson(...)`)
pub fn dart_overlay_semantic_tokens(text: &str, highlight_calls: bool) -> Vec<DartToken> {
    scan_overlay_tokens(text, OverlayMode::Dart { highlight_calls })
}

/// Emits non-clobbering Flutter overlay tokens alongside `dart language-server`:
/// - `Colors.*`, `CupertinoColors.*`, `Icons.*`, `CupertinoIcons.*` enum members
/// - Core Flutter framework/widget types (`StatelessWidget`, `BuildContext`, etc.)
pub fn flutter_overlay_semantic_tokens(text: &str) -> Vec<DartToken> {
    scan_overlay_tokens(text, OverlayMode::Flutter)
}

fn scan_overlay_tokens(text: &str, mode: OverlayMode) -> Vec<DartToken> {
    let idx = LineIndex::new(text);
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;
    let mut tokens = Vec::new();
    let mut prev_word: Option<String> = None;
    let mut prev_was_dot = false;
    let mut delims: Vec<u8> = Vec::new();

    while i < len {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // Line comment or doc comment
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            let start = i;
            let is_doc = bytes.get(i + 2) == Some(&b'/') && bytes.get(i + 3) != Some(&b'/');
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            if matches!(mode, OverlayMode::Dart { .. }) && is_doc {
                emit_doc_comment_refs(&idx, text, start, i, &mut tokens);
            }
            prev_was_dot = false;
            continue;
        }

        // Block comment (Dart supports nested /* ... */)
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i += 2;
            let mut depth = 1usize;
            while i < len && depth > 0 {
                if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                    depth += 1;
                    i += 2;
                } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            prev_was_dot = false;
            continue;
        }

        // Raw string: r'...' / r"..." / r'''...''' / r"""..."""
        if b == b'r' && matches!(bytes.get(i + 1), Some(b'\'' | b'"')) {
            i = scan_string(bytes, i + 1, bytes[i + 1], true);
            prev_was_dot = false;
            prev_word = None;
            continue;
        }

        // Standard string
        if b == b'\'' || b == b'"' {
            i = scan_string(bytes, i, b, false);
            prev_was_dot = false;
            prev_word = None;
            continue;
        }

        // @annotation
        if b == b'@' {
            let start = i;
            i += 1;
            while i < len && is_ident_continue(bytes[i]) {
                i += 1;
            }
            if matches!(mode, OverlayMode::Dart { .. }) && i > start + 1 {
                tokens.push(DartToken {
                    span: idx.span(text, start, i),
                    kind: SemanticKind::Decorator,
                    modifiers: Vec::new(),
                });
            }
            prev_was_dot = false;
            prev_word = None;
            continue;
        }

        // Numbers
        if b.is_ascii_digit() {
            i = skip_number(bytes, i);
            prev_was_dot = false;
            prev_word = None;
            continue;
        }

        // Identifiers
        if is_ident_start(b) {
            let start = i;
            i += 1;
            while i < len && is_ident_continue(bytes[i]) {
                i += 1;
            }
            let word = &text[start..i];
            match mode {
                OverlayMode::Dart { highlight_calls } => {
                    let starts_lower_or_underscore = word
                        .bytes()
                        .next()
                        .is_some_and(|c| c.is_ascii_lowercase() || c == b'_');
                    if !prev_was_dot
                        && delims.last() == Some(&b'(')
                        && bytes.get(i) == Some(&b':')
                        && starts_lower_or_underscore
                        && !DART_KEYWORDS.contains(&word)
                    {
                        tokens.push(DartToken {
                            span: idx.span(text, start, i),
                            kind: SemanticKind::Parameter,
                            modifiers: vec![SemanticMod::DefaultLibrary],
                        });
                    } else if !prev_was_dot
                        && DART_CORE_TYPES.contains(&word)
                        && next_non_ws_byte(bytes, i) != Some(b':')
                    {
                        tokens.push(DartToken {
                            span: idx.span(text, start, i),
                            kind: SemanticKind::Type,
                            modifiers: vec![SemanticMod::DefaultLibrary],
                        });
                    } else if highlight_calls
                        && starts_lower_or_underscore
                        && !DART_KEYWORDS.contains(&word)
                        && is_followed_by_call_parens(bytes, i)
                    {
                        if prev_was_dot {
                            let mut mods = vec![SemanticMod::Declaration];
                            if prev_word
                                .as_deref()
                                .and_then(|w| w.bytes().next())
                                .is_some_and(|c| c.is_ascii_uppercase())
                            {
                                mods.push(SemanticMod::Static);
                            }
                            tokens.push(DartToken {
                                span: idx.span(text, start, i),
                                kind: SemanticKind::Method,
                                modifiers: mods,
                            });
                        } else {
                            tokens.push(DartToken {
                                span: idx.span(text, start, i),
                                kind: SemanticKind::Function,
                                modifiers: vec![SemanticMod::Declaration],
                            });
                        }
                    }
                }
                OverlayMode::Flutter => {
                    if prev_was_dot {
                        if matches!(
                            prev_word.as_deref(),
                            Some("Colors" | "CupertinoColors" | "Icons" | "CupertinoIcons")
                        ) {
                            tokens.push(DartToken {
                                span: idx.span(text, start, i),
                                kind: SemanticKind::EnumMember,
                                modifiers: vec![SemanticMod::Static, SemanticMod::Readonly],
                            });
                        }
                    } else if FLUTTER_TYPES.contains(&word) {
                        tokens.push(DartToken {
                            span: idx.span(text, start, i),
                            kind: SemanticKind::Class,
                            modifiers: vec![SemanticMod::DefaultLibrary],
                        });
                    }
                }
            }
            prev_word = Some(word.to_owned());
            prev_was_dot = false;
            continue;
        }

        match b {
            b'(' | b'{' | b'[' => delims.push(b),
            b')' | b'}' | b']' => {
                delims.pop();
            }
            _ => {}
        }
        prev_was_dot = b == b'.';
        if !prev_was_dot {
            prev_word = None;
        }
        i += 1;
    }

    tokens
}

fn skip_number(bytes: &[u8], mut i: usize) -> usize {
    let len = bytes.len();
    if bytes[i] == b'0' && matches!(bytes.get(i + 1), Some(b'x' | b'X')) {
        i += 2;
        while i < len && (bytes[i].is_ascii_hexdigit() || bytes[i] == b'_') {
            i += 1;
        }
        return i;
    }
    while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
        i += 1;
    }
    if bytes.get(i) == Some(&b'.') && bytes.get(i + 1).is_some_and(u8::is_ascii_digit) {
        i += 1;
        while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
            i += 1;
        }
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(bytes.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
            i += 1;
        }
    }
    i
}

/// Returns `true` if `bytes[i..]` begins with optional whitespace followed by `(`
/// or balanced Dart type arguments `<...>` followed by `(`.
pub(crate) fn is_followed_by_call_parens(bytes: &[u8], mut i: usize) -> bool {
    let len = bytes.len();
    while i < len && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    match bytes.get(i) {
        Some(b'(') => true,
        Some(b'<') => {
            let mut depth = 1usize;
            i += 1;
            while i < len && depth > 0 {
                let b = bytes[i];
                match b {
                    b'<' => depth += 1,
                    b'>' => depth -= 1,
                    c if c.is_ascii_alphanumeric()
                        || matches!(c, b'_' | b'.' | b',' | b'?' | b' ' | b'\t' | b'\r' | b'\n') => {}
                    _ => return false,
                }
                i += 1;
            }
            if depth != 0 {
                return false;
            }
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            bytes.get(i) == Some(&b'(')
        }
        _ => false,
    }
}

#[derive(Clone, Copy)]
pub struct HighlightProfile {
    pub flutter_aware: bool,
}

impl HighlightProfile {
    pub const DART: Self = Self { flutter_aware: false };
    pub const FLUTTER: Self = Self { flutter_aware: true };
}

/// Produces full-file lexical semantic tokens for Dart source code (used in offline/standalone contexts).
pub fn dart_semantic_tokens(text: &str, profile: HighlightProfile) -> Vec<DartToken> {
    let idx = LineIndex::new(text);
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;
    let mut tokens = Vec::new();
    let mut prev_word: Option<String> = None;
    let mut prev_was_dot = false;

    while i < len {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // Line comment or doc comment
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            let start = i;
            let is_doc = bytes.get(i + 2) == Some(&b'/') && bytes.get(i + 3) != Some(&b'/');
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            if is_doc {
                emit_doc_comment(&idx, text, start, i, &mut tokens);
            } else {
                tokens.push(DartToken {
                    span: idx.span(text, start, i),
                    kind: SemanticKind::Comment,
                    modifiers: Vec::new(),
                });
            }
            prev_was_dot = false;
            continue;
        }

        // Block comment (Dart supports nested /* ... */)
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let start = i;
            let is_doc = bytes.get(i + 2) == Some(&b'*') && bytes.get(i + 3) != Some(&b'/');
            i += 2;
            let mut depth = 1usize;
            while i < len && depth > 0 {
                if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                    depth += 1;
                    i += 2;
                } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            let modifiers = if is_doc { vec![SemanticMod::Documentation] } else { Vec::new() };
            push_multiline_token(&idx, text, start, i, SemanticKind::Comment, &modifiers, &mut tokens);
            prev_was_dot = false;
            continue;
        }

        // Raw string: r'...' / r"..." / r'''...''' / r"""..."""
        if b == b'r' && matches!(bytes.get(i + 1), Some(b'\'' | b'"')) {
            let start = i;
            let quote = bytes[i + 1];
            let end = scan_string(bytes, i + 1, quote, true);
            push_multiline_token(&idx, text, start, end, SemanticKind::String, &[], &mut tokens);
            i = end;
            prev_was_dot = false;
            prev_word = None;
            continue;
        }

        // Standard string with interpolation
        if b == b'\'' || b == b'"' {
            let start = i;
            let end = scan_string(bytes, i, b, false);
            push_multiline_token(&idx, text, start, end, SemanticKind::String, &[], &mut tokens);
            i = end;
            prev_was_dot = false;
            prev_word = None;
            continue;
        }

        // @annotation
        if b == b'@' {
            let start = i;
            i += 1;
            while i < len && is_ident_continue(bytes[i]) {
                i += 1;
            }
            if i > start + 1 {
                tokens.push(DartToken {
                    span: idx.span(text, start, i),
                    kind: SemanticKind::Decorator,
                    modifiers: Vec::new(),
                });
            }
            prev_was_dot = false;
            prev_word = None;
            continue;
        }

        // Numbers
        if b.is_ascii_digit() {
            let start = i;
            if b == b'0' && matches!(bytes.get(i + 1), Some(b'x' | b'X')) {
                i += 2;
                while i < len && (bytes[i].is_ascii_hexdigit() || bytes[i] == b'_') {
                    i += 1;
                }
            } else {
                while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                    i += 1;
                }
                if bytes.get(i) == Some(&b'.') && bytes.get(i + 1).is_some_and(u8::is_ascii_digit) {
                    i += 1;
                    while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                        i += 1;
                    }
                }
                if matches!(bytes.get(i), Some(b'e' | b'E')) {
                    i += 1;
                    if matches!(bytes.get(i), Some(b'+' | b'-')) {
                        i += 1;
                    }
                    while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                        i += 1;
                    }
                }
            }
            tokens.push(DartToken {
                span: idx.span(text, start, i),
                kind: SemanticKind::Number,
                modifiers: Vec::new(),
            });
            prev_was_dot = false;
            prev_word = None;
            continue;
        }

        // Identifiers & keywords
        if is_ident_start(b) {
            let start = i;
            i += 1;
            while i < len && is_ident_continue(bytes[i]) {
                i += 1;
            }
            let word = &text[start..i];
            let next_non_ws = next_non_ws_byte(bytes, i);
            let (kind, modifiers) = classify_word(
                word,
                prev_word.as_deref(),
                prev_was_dot,
                next_non_ws,
                profile,
            );
            if let Some(kind) = kind {
                tokens.push(DartToken {
                    span: idx.span(text, start, i),
                    kind,
                    modifiers,
                });
            }
            prev_word = Some(word.to_owned());
            prev_was_dot = false;
            continue;
        }

        prev_was_dot = b == b'.';
        if !prev_was_dot {
            prev_word = None;
        }
        i += 1;
    }

    tokens
}

fn classify_word(
    word: &str,
    prev_word: Option<&str>,
    prev_was_dot: bool,
    next_non_ws: Option<u8>,
    profile: HighlightProfile,
) -> (Option<SemanticKind>, Vec<SemanticMod>) {
    if DART_KEYWORDS.contains(&word) {
        let is_modifier = matches!(
            word,
            "abstract" | "async" | "base" | "const" | "covariant" | "external" | "final"
                | "interface" | "late" | "override" | "required" | "sealed" | "static" | "sync"
        );
        let mut mods = Vec::new();
        if word == "async" || word == "await" || word == "sync" {
            mods.push(SemanticMod::Async);
        }
        return (
            Some(if is_modifier { SemanticKind::Modifier } else { SemanticKind::Keyword }),
            mods,
        );
    }

    if prev_was_dot {
        if next_non_ws == Some(b'(') {
            return (Some(SemanticKind::Method), Vec::new());
        }
        if matches!(prev_word, Some("Colors" | "CupertinoColors" | "Icons" | "CupertinoIcons")) {
            return (Some(SemanticKind::EnumMember), vec![SemanticMod::Static, SemanticMod::Readonly]);
        }
        return (Some(SemanticKind::Property), Vec::new());
    }

    if next_non_ws == Some(b':') && word.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c == '_') {
        return (Some(SemanticKind::Parameter), Vec::new());
    }

    if profile.flutter_aware && FLUTTER_TYPES.contains(&word) {
        return (Some(SemanticKind::Class), vec![SemanticMod::DefaultLibrary]);
    }

    if DART_CORE_TYPES.contains(&word) {
        return (Some(SemanticKind::Type), vec![SemanticMod::DefaultLibrary]);
    }

    if word.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        if word.len() > 1 && word.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_') {
            return (Some(SemanticKind::EnumMember), vec![SemanticMod::Readonly, SemanticMod::Static]);
        }
        return (Some(SemanticKind::Class), Vec::new());
    }

    if matches!(
        prev_word,
        Some("class" | "mixin" | "enum" | "typedef" | "extension" | "extends" | "implements" | "with")
    ) {
        return (Some(SemanticKind::Class), vec![SemanticMod::Declaration]);
    }

    if next_non_ws == Some(b'(') {
        return (Some(SemanticKind::Function), Vec::new());
    }

    if word.starts_with('k') && word.as_bytes().get(1).is_some_and(u8::is_ascii_uppercase) {
        return (Some(SemanticKind::Variable), vec![SemanticMod::Readonly]);
    }

    (None, Vec::new())
}

/// Semantic highlighting for `pubspec.yaml` and `analysis_options.yaml`.
pub fn pubspec_semantic_tokens(text: &str) -> Vec<DartToken> {
    let mut tokens = Vec::new();
    for (line_num, raw_line) in text.split('\n').enumerate() {
        let line_u32 = line_num as u32;
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('#') {
            let col = utf16_len(&line[..indent]);
            tokens.push(DartToken {
                span: TextSpan::new(TextPos::new(line_u32, col), TextPos::new(line_u32, col + utf16_len(trimmed))),
                kind: SemanticKind::Comment,
                modifiers: Vec::new(),
            });
            continue;
        }
        if let Some(colon) = trimmed.find(':')
            && !trimmed.starts_with('-')
        {
            let key = trimmed[..colon].trim_end();
            let key_col = utf16_len(&line[..indent]);
            let kind = if indent == 0 {
                SemanticKind::Namespace
            } else if matches!(key, "sdk" | "path" | "git" | "url" | "ref" | "version" | "asset" | "family") {
                SemanticKind::Keyword
            } else {
                SemanticKind::Property
            };
            tokens.push(DartToken {
                span: TextSpan::new(TextPos::new(line_u32, key_col), TextPos::new(line_u32, key_col + utf16_len(key))),
                kind,
                modifiers: Vec::new(),
            });
            let after = &trimmed[colon + 1..];
            let val_trimmed = after.trim();
            if !val_trimmed.is_empty() && !val_trimmed.starts_with('#') {
                let ws = after.len() - after.trim_start().len();
                let val_byte = indent + colon + 1 + ws;
                let val_no_comment = val_trimmed.split(" #").next().unwrap_or(val_trimmed).trim_end();
                let val_col = utf16_len(&line[..val_byte]);
                let vkind = if val_no_comment == "true" || val_no_comment == "false" {
                    SemanticKind::Keyword
                } else {
                    SemanticKind::String
                };
                tokens.push(DartToken {
                    span: TextSpan::new(
                        TextPos::new(line_u32, val_col),
                        TextPos::new(line_u32, val_col + utf16_len(val_no_comment)),
                    ),
                    kind: vkind,
                    modifiers: Vec::new(),
                });
            }
        }
    }
    tokens
}

/// Finds `main(...)`, `test(...)`, `testWidgets(...)`, and `group(...)` runnable declarations.
pub fn find_runnables(text: &str) -> Vec<RunnableEntry> {
    let mut entries = Vec::new();
    for (line_num, raw_line) in text.split('\n').enumerate() {
        let line_u32 = line_num as u32;
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with("/*") {
            continue;
        }
        if is_main_declaration(trimmed) {
            entries.push(RunnableEntry {
                line: line_u32,
                kind: RunnableKind::Main,
                label: "Run main()".into(),
                test_name: None,
            });
            continue;
        }
        for (prefix, kind) in [
            ("testWidgets(", RunnableKind::TestWidget),
            ("test(", RunnableKind::Test),
            ("group(", RunnableKind::Group),
        ] {
            if let Some(rest) = trimmed.strip_prefix(prefix)
                && let Some(name) = extract_first_string_arg(rest)
            {
                let label = match kind {
                    RunnableKind::TestWidget => format!("Run widget test '{name}'"),
                    RunnableKind::Test => format!("Run test '{name}'"),
                    RunnableKind::Group => format!("Run group '{name}'"),
                    RunnableKind::Main => "Run main()".into(),
                };
                entries.push(RunnableEntry {
                    line: line_u32,
                    kind,
                    label,
                    test_name: Some(name),
                });
                break;
            }
        }
    }
    entries
}

/// Finds `import`, `export`, and `part` directive string spans.
pub fn find_directives(text: &str) -> Vec<DirectiveLink> {
    let mut links = Vec::new();
    for (line_num, raw_line) in text.split('\n').enumerate() {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let trimmed = line.trim_start();
        for kw in ["import ", "export ", "part "] {
            if let Some(after) = trimmed.strip_prefix(kw) {
                let rest = after.trim_start();
                if rest.starts_with("of ") {
                    continue;
                }
                if let Some((uri, q_start, q_end)) = quoted_literal_in_line(line) {
                    let start_col = utf16_len(&line[..q_start]);
                    let end_col = utf16_len(&line[..q_end]);
                    links.push(DirectiveLink {
                        kind: kw.trim(),
                        uri: uri.to_owned(),
                        span: TextSpan::new(
                            TextPos::new(line_num as u32, start_col),
                            TextPos::new(line_num as u32, end_col),
                        ),
                    });
                }
                break;
            }
        }
    }
    links
}

/// Finds import blocks, `// region ... // endregion` blocks, and multi-line comment blocks.
pub fn find_fold_ranges(text: &str) -> Vec<DartFold> {
    let mut folds = Vec::new();
    let lines: Vec<&str> = text.split('\n').collect();
    let mut import_start: Option<u32> = None;
    let mut import_end: u32 = 0;
    let mut region_stack: Vec<u32> = Vec::new();
    let mut doc_start: Option<u32> = None;
    let mut doc_end: u32 = 0;

    for (idx, raw) in lines.iter().enumerate() {
        let line_u32 = idx as u32;
        let trimmed = raw.trim();
        let is_directive = trimmed.starts_with("import ") || trimmed.starts_with("export ") || trimmed.starts_with("part ");
        if is_directive {
            if import_start.is_none() {
                import_start = Some(line_u32);
            }
            import_end = line_u32;
        } else if !trimmed.is_empty()
            && let Some(start) = import_start.take()
            && import_end > start
        {
            folds.push(DartFold {
                start_line: start,
                end_line: import_end,
                kind: DartFoldKind::Imports,
            });
        }

        if trimmed.starts_with("// region") || trimmed.starts_with("//#region") {
            region_stack.push(line_u32);
        } else if (trimmed.starts_with("// endregion") || trimmed.starts_with("//#endregion"))
            && let Some(start) = region_stack.pop()
            && line_u32 > start
        {
            folds.push(DartFold {
                start_line: start,
                end_line: line_u32,
                kind: DartFoldKind::Region,
            });
        }

        if trimmed.starts_with("///") {
            if doc_start.is_none() {
                doc_start = Some(line_u32);
            }
            doc_end = line_u32;
        } else if let Some(start) = doc_start.take()
            && doc_end > start
        {
            folds.push(DartFold {
                start_line: start,
                end_line: doc_end,
                kind: DartFoldKind::Comment,
            });
        }
    }

    if let Some(start) = import_start
        && import_end > start
    {
        folds.push(DartFold {
            start_line: start,
            end_line: import_end,
            kind: DartFoldKind::Imports,
        });
    }
    if let Some(start) = doc_start
        && doc_end > start
    {
        folds.push(DartFold {
            start_line: start,
            end_line: doc_end,
            kind: DartFoldKind::Comment,
        });
    }
    folds
}

fn emit_doc_comment(idx: &LineIndex, text: &str, start: usize, end: usize, out: &mut Vec<DartToken>) {
    emit_doc_comment_impl(idx, text, start, end, true, out);
}

fn emit_doc_comment_refs(idx: &LineIndex, text: &str, start: usize, end: usize, out: &mut Vec<DartToken>) {
    emit_doc_comment_impl(idx, text, start, end, false, out);
}

fn emit_doc_comment_impl(
    idx: &LineIndex,
    text: &str,
    start: usize,
    end: usize,
    include_text: bool,
    out: &mut Vec<DartToken>,
) {
    let slice = &text[start..end];
    let mut cursor = 0usize;
    while let Some(rel_open) = slice[cursor..].find('[') {
        let open = cursor + rel_open;
        if let Some(rel_close) = slice[open + 1..].find(']') {
            let close = open + 1 + rel_close;
            let inside = &slice[open + 1..close];
            let valid_ref = !inside.is_empty()
                && inside.len() <= 64
                && inside.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.'));
            if valid_ref {
                if include_text && open > cursor {
                    out.push(DartToken {
                        span: idx.span(text, start + cursor, start + open),
                        kind: SemanticKind::Comment,
                        modifiers: vec![SemanticMod::Documentation],
                    });
                }
                out.push(DartToken {
                    span: idx.span(text, start + open + 1, start + close),
                    kind: SemanticKind::Type,
                    modifiers: vec![SemanticMod::Documentation],
                });
                cursor = close + 1;
                continue;
            }
        }
        break;
    }
    if include_text && cursor < slice.len() {
        out.push(DartToken {
            span: idx.span(text, start + cursor, end),
            kind: SemanticKind::Comment,
            modifiers: vec![SemanticMod::Documentation],
        });
    }
}

fn push_multiline_token(
    idx: &LineIndex,
    text: &str,
    start: usize,
    end: usize,
    kind: SemanticKind,
    modifiers: &[SemanticMod],
    out: &mut Vec<DartToken>,
) {
    let mut seg_start = start;
    for (rel, &b) in text.as_bytes()[start..end].iter().enumerate() {
        if b == b'\n' {
            let abs = start + rel;
            if abs > seg_start {
                out.push(DartToken {
                    span: idx.span(text, seg_start, abs),
                    kind,
                    modifiers: modifiers.to_vec(),
                });
            }
            seg_start = abs + 1;
        }
    }
    if end > seg_start {
        out.push(DartToken {
            span: idx.span(text, seg_start, end),
            kind,
            modifiers: modifiers.to_vec(),
        });
    }
}

fn scan_string(bytes: &[u8], start: usize, quote: u8, raw: bool) -> usize {
    let len = bytes.len();
    let triple = bytes.get(start + 1) == Some(&quote) && bytes.get(start + 2) == Some(&quote);
    let mut i = start + if triple { 3 } else { 1 };
    while i < len {
        let b = bytes[i];
        if !raw && b == b'\\' {
            i = (i + 2).min(len);
            continue;
        }
        if !triple && b == b'\n' {
            return i;
        }
        if b == quote {
            if triple {
                if bytes.get(i + 1) == Some(&quote) && bytes.get(i + 2) == Some(&quote) {
                    return i + 3;
                }
            } else {
                return i + 1;
            }
        }
        i += 1;
    }
    len
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'$'
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

fn next_non_ws_byte(bytes: &[u8], mut i: usize) -> Option<u8> {
    while i < bytes.len() {
        if !bytes[i].is_ascii_whitespace() {
            return Some(bytes[i]);
        }
        i += 1;
    }
    None
}

fn is_main_declaration(trimmed: &str) -> bool {
    trimmed.starts_with("void main(")
        || trimmed.starts_with("Future<void> main(")
        || trimmed.starts_with("Future<void> main (")
        || trimmed.starts_with("main(")
}

fn extract_first_string_arg(rest: &str) -> Option<String> {
    let trimmed = rest.trim_start();
    let quote = trimmed.as_bytes().first().copied().filter(|&b| b == b'\'' || b == b'"')?;
    let mut out = String::new();
    let mut escaped = false;
    for ch in trimmed[1..].chars() {
        if escaped {
            out.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch as u32 == quote as u32 {
            return Some(out);
        } else {
            out.push(ch);
        }
    }
    None
}

fn quoted_literal_in_line(line: &str) -> Option<(&str, usize, usize)> {
    for (idx, b) in line.bytes().enumerate() {
        if (b == b'\'' || b == b'"')
            && let Some(rel_end) = line[idx + 1..].find(b as char)
        {
            let end = idx + 1 + rel_end;
            return Some((&line[idx + 1..end], idx + 1, end));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_dart_keywords_doc_refs_and_flutter_widgets() {
        let src = "\
/// Renders [MyApp].
@immutable
class MyApp extends StatelessWidget {
  final Color color = Colors.blue;
  Widget build(BuildContext context) => Text('hi');
}
";
        let tokens = dart_semantic_tokens(src, HighlightProfile::FLUTTER);
        assert!(tokens.iter().any(|t| t.kind == SemanticKind::Decorator));
        assert!(tokens.iter().any(|t| t.kind == SemanticKind::Type && t.modifiers.contains(&SemanticMod::Documentation)));
        assert!(tokens.iter().any(|t| t.kind == SemanticKind::Class && t.modifiers.contains(&SemanticMod::DefaultLibrary)));
        assert!(tokens.iter().any(|t| t.kind == SemanticKind::EnumMember));
        assert!(tokens.iter().any(|t| t.kind == SemanticKind::String));
        // Tokens must be non-overlapping and sorted.
        for pair in tokens.windows(2) {
            assert!(pair[0].span.end <= pair[1].span.start, "overlapping tokens: {pair:?}");
        }
    }

    #[test]
    fn finds_runnables_directives_and_folds() {
        let src = "\
import 'dart:async';
import 'src/helper.dart';

/// Doc line 1
/// Doc line 2
void main() {
  testWidgets('renders title', (tester) async {});
}
";
        let runnables = find_runnables(src);
        assert_eq!(runnables.len(), 2);
        assert_eq!(runnables[0].kind, RunnableKind::Main);
        assert_eq!(runnables[1].kind, RunnableKind::TestWidget);
        assert_eq!(runnables[1].test_name.as_deref(), Some("renders title"));

        let directives = find_directives(src);
        assert_eq!(directives.len(), 2);
        assert_eq!(directives[1].uri, "src/helper.dart");

        let folds = find_fold_ranges(src);
        assert!(folds.iter().any(|f| f.kind == DartFoldKind::Imports && f.start_line == 0 && f.end_line == 1));
        assert!(folds.iter().any(|f| f.kind == DartFoldKind::Comment && f.start_line == 3 && f.end_line == 4));
    }

    #[test]
    fn dart_overlay_highlights_generic_and_static_calls_without_clobbering() {
        let src = "\
/// Reads a [AppSourceAGIChat] from its plain JSON.
@Freezed(makeCollectionsUnmodifiable: false)
AppSourceAGIChat appSourceAGIChatFromMap(Map<String, dynamic> json) {
  if (a < b && c > (d)) {
    for (var x in items) { return (x); }
  }
  final m = {'key': 1, mapKey: 2};
  return AppSourceAGIChat(
    enabled: readBool(data: json['enabled'], defVal: true),
    allowedAccesses: readList<String>(data: json['allowedAccesses']),
    onboarding: readMap<String, AppSourceAGIChatSuggestion>(data: json['onboarding']),
    conv: (dynamic v, int i) => IcaModelOption.fromJson(json: v),
    loaded: obj.load<int>(1),
  );
}
";
        let tokens = dart_overlay_semantic_tokens(src, true);
        let text_of = |t: &DartToken| -> &str {
            let line = src.lines().nth(t.span.start.line as usize).unwrap();
            &line[t.span.start.character as usize..t.span.end.character as usize]
        };
        let names: Vec<_> = tokens.iter().map(|t| (text_of(t), t.kind, t.modifiers.clone())).collect();

        assert!(names.contains(&("@Freezed", SemanticKind::Decorator, Vec::new())));
        assert!(names.contains(&("makeCollectionsUnmodifiable", SemanticKind::Parameter, vec![SemanticMod::DefaultLibrary])));
        assert!(names.contains(&("enabled", SemanticKind::Parameter, vec![SemanticMod::DefaultLibrary])));
        assert!(names.contains(&("data", SemanticKind::Parameter, vec![SemanticMod::DefaultLibrary])));
        assert!(names.contains(&("AppSourceAGIChat", SemanticKind::Type, vec![SemanticMod::Documentation])));
        assert!(names.contains(&("Map", SemanticKind::Type, vec![SemanticMod::DefaultLibrary])));
        assert!(names.contains(&("String", SemanticKind::Type, vec![SemanticMod::DefaultLibrary])));
        assert!(names.contains(&("dynamic", SemanticKind::Type, vec![SemanticMod::DefaultLibrary])));
        assert!(names.contains(&("int", SemanticKind::Type, vec![SemanticMod::DefaultLibrary])));
        assert!(names.contains(&("appSourceAGIChatFromMap", SemanticKind::Function, vec![SemanticMod::Declaration])));
        assert!(names.contains(&("readBool", SemanticKind::Function, vec![SemanticMod::Declaration])));
        assert!(names.contains(&("readList", SemanticKind::Function, vec![SemanticMod::Declaration])));
        assert!(names.contains(&("readMap", SemanticKind::Function, vec![SemanticMod::Declaration])));
        assert!(names.contains(&(
            "fromJson",
            SemanticKind::Method,
            vec![SemanticMod::Declaration, SemanticMod::Static]
        )));
        assert!(names.contains(&("load", SemanticKind::Method, vec![SemanticMod::Declaration])));

        assert!(names.contains(&("json", SemanticKind::Parameter, vec![SemanticMod::DefaultLibrary])));

        // Ensure keywords, map keys, non-core types, constructors, and comparison `a < b && c > (d)` are NOT emitted.
        for forbidden in ["if", "for", "return", "a", "b", "c", "AppSourceAGIChat", "json", "mapKey"] {
            if forbidden == "AppSourceAGIChat" {
                assert!(!names.iter().any(|(n, k, m)| *n == forbidden && (*k != SemanticKind::Type || !m.contains(&SemanticMod::Documentation))));
            } else if forbidden == "json" {
                assert!(!names.iter().any(|(n, k, _)| *n == forbidden && *k != SemanticKind::Parameter));
            } else {
                assert!(!names.iter().any(|(n, _, _)| *n == forbidden), "unexpected token for {forbidden}");
            }
        }
        for pair in tokens.windows(2) {
            assert!(pair[0].span.end <= pair[1].span.start, "overlapping tokens: {pair:?}");
        }
    }

    #[test]
    fn flutter_overlay_emits_only_widgets_colors_and_icons() {
        let src = "Widget build(BuildContext context) => Icon(Icons.favorite, color: Colors.blue);";
        let tokens = flutter_overlay_semantic_tokens(src);
        let words: Vec<_> = tokens
            .iter()
            .map(|t| (&src[t.span.start.character as usize..t.span.end.character as usize], t.kind))
            .collect();
        assert!(words.contains(&("Widget", SemanticKind::Class)));
        assert!(words.contains(&("BuildContext", SemanticKind::Class)));
        assert!(words.contains(&("Icon", SemanticKind::Class)));
        assert!(words.contains(&("favorite", SemanticKind::EnumMember)));
        assert!(words.contains(&("blue", SemanticKind::EnumMember)));
        assert!(!words.iter().any(|(w, _)| *w == "build"));
    }
}
