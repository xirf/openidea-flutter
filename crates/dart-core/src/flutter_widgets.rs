//! Flutter widget tree scanner, closing labels, wrap/unwrap/extract, and
//! StatelessWidget <-> StatefulWidget conversions.

use std::collections::{HashMap, HashSet};

use crate::{LineIndex, TextPos, TextSpan, utf16_len};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClosingLabel {
    pub pos: TextPos,
    pub label: String,
    pub tooltip: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WidgetSpan {
    pub name: String,
    pub start_byte: usize,
    pub open_paren_byte: usize,
    pub close_paren_byte: usize,
    pub span: TextSpan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WrapKind {
    Container,
    Padding,
    Center,
    SizedBox,
    Column,
    Row,
    Expanded,
    Flexible,
    Builder,
    StreamBuilder,
    FutureBuilder,
}

impl WrapKind {
    pub fn title(self) -> &'static str {
        match self {
            Self::Container => "Wrap with Container",
            Self::Padding => "Wrap with Padding",
            Self::Center => "Wrap with Center",
            Self::SizedBox => "Wrap with SizedBox",
            Self::Column => "Wrap with Column",
            Self::Row => "Wrap with Row",
            Self::Expanded => "Wrap with Expanded",
            Self::Flexible => "Wrap with Flexible",
            Self::Builder => "Wrap with Builder",
            Self::StreamBuilder => "Wrap with StreamBuilder",
            Self::FutureBuilder => "Wrap with FutureBuilder",
        }
    }

    pub const ALL: [Self; 11] = [
        Self::Container,
        Self::Padding,
        Self::Center,
        Self::SizedBox,
        Self::Column,
        Self::Row,
        Self::Expanded,
        Self::Flexible,
        Self::Builder,
        Self::StreamBuilder,
        Self::FutureBuilder,
    ];
}

/// Scans all multi-line PascalCase constructor calls and returns closing labels (`// Widget`)
/// for calls spanning at least `min_lines` lines when the closing line has no trailing comment.
pub fn closing_labels(text: &str, min_lines: u32) -> Vec<ClosingLabel> {
    let idx = LineIndex::new(text);
    let min_lines = min_lines.max(2);
    let mut labels = Vec::new();
    let lines: Vec<&str> = text.split('\n').collect();

    for call in scan_widget_calls(text) {
        let start_pos = idx.pos_at(text, call.start_byte);
        let close_pos = idx.pos_at(text, call.close_paren_byte);
        if close_pos.line.saturating_sub(start_pos.line) + 1 < min_lines {
            continue;
        }
        let line_str = lines
            .get(close_pos.line as usize)
            .map(|l| l.strip_suffix('\r').unwrap_or(l))
            .unwrap_or("");
        let rest_of_line = line_str
            .get(
                close_pos
                    .character
                    .min(utf16_len(line_str))
                    .try_into()
                    .unwrap_or(0)..,
            )
            .unwrap_or("");
        // Place at end of closing line, skip if line already has a trailing comment.
        if line_str.contains("//") {
            continue;
        }
        let _ = rest_of_line;
        let end_col = utf16_len(line_str);
        labels.push(ClosingLabel {
            pos: TextPos::new(close_pos.line, end_col),
            label: format!(" // {}", call.name),
            tooltip: format!("End of {}", call.name),
        });
    }
    labels.sort_by_key(|l| (l.pos.line, l.pos.character));
    labels.dedup_by_key(|l| l.pos.line);
    labels
}

/// Finds the innermost widget constructor expression containing `pos`.
pub fn widget_at_cursor(text: &str, pos: TextPos) -> Option<WidgetSpan> {
    let idx = LineIndex::new(text);
    let offset = idx.offset_at(text, pos);
    scan_widget_calls(text)
        .into_iter()
        .filter(|w| w.start_byte <= offset && offset <= w.close_paren_byte + 1)
        .min_by_key(|w| w.close_paren_byte - w.start_byte)
}

/// Wraps `span` with the requested Flutter widget, preserving indentation.
pub fn wrap_with_widget(text: &str, span: &WidgetSpan, kind: WrapKind) -> String {
    let expr = &text[span.start_byte..=span.close_paren_byte];
    let indent = line_indent_at(text, span.start_byte);
    let child_indent = format!("{indent}  ");
    let indented_expr = reindent_block(expr, &indent, &child_indent);
    match kind {
        WrapKind::Container => format!("Container(\n{child_indent}child: {indented_expr},\n{indent})"),
        WrapKind::Padding => format!(
            "Padding(\n{child_indent}padding: const EdgeInsets.all(8.0),\n{child_indent}child: {indented_expr},\n{indent})"
        ),
        WrapKind::Center => format!("Center(\n{child_indent}child: {indented_expr},\n{indent})"),
        WrapKind::SizedBox => format!("SizedBox(\n{child_indent}child: {indented_expr},\n{indent})"),
        WrapKind::Expanded => format!("Expanded(\n{child_indent}child: {indented_expr},\n{indent})"),
        WrapKind::Flexible => format!("Flexible(\n{child_indent}child: {indented_expr},\n{indent})"),
        WrapKind::Column => format!(
            "Column(\n{child_indent}children: [\n{child_indent}  {},\n{child_indent}],\n{indent})",
            reindent_block(expr, &indent, &format!("{child_indent}  "))
        ),
        WrapKind::Row => format!(
            "Row(\n{child_indent}children: [\n{child_indent}  {},\n{child_indent}],\n{indent})",
            reindent_block(expr, &indent, &format!("{child_indent}  "))
        ),
        WrapKind::Builder => format!(
            "Builder(\n{child_indent}builder: (context) {{\n{child_indent}  return {};\n{child_indent}}},\n{indent})",
            reindent_block(expr, &indent, &format!("{child_indent}  "))
        ),
        WrapKind::StreamBuilder => format!(
            "StreamBuilder(\n{child_indent}stream: null,\n{child_indent}builder: (context, snapshot) {{\n{child_indent}  return {};\n{child_indent}}},\n{indent})",
            reindent_block(expr, &indent, &format!("{child_indent}  "))
        ),
        WrapKind::FutureBuilder => format!(
            "FutureBuilder(\n{child_indent}future: null,\n{child_indent}builder: (context, snapshot) {{\n{child_indent}  return {};\n{child_indent}}},\n{indent})",
            reindent_block(expr, &indent, &format!("{child_indent}  "))
        ),
    }
}

/// Unwraps a widget with a `child:` argument, returning the replacement for `span`.
pub fn remove_widget_wrapper(text: &str, span: &WidgetSpan) -> Option<String> {
    let args = &text[span.open_paren_byte + 1..span.close_paren_byte];
    let child_expr = find_top_level_named_arg(args, "child")?;
    let indent = line_indent_at(text, span.start_byte);
    let child_indent = format!("{indent}  ");
    Some(reindent_out(child_expr.trim().trim_end_matches(','), &child_indent, &indent))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtractError {
    InvalidName,
    Unsafe(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CapturedVar {
    name: String,
    ty: String,
}

/// Checks whether extracting `span` into a top-level `StatelessWidget` is safe.
pub fn can_extract_flutter_widget(text: &str, span: &WidgetSpan) -> Result<(), ExtractError> {
    analyze_extraction(text, span).map(|_| ())
}

/// Extracts `span` into a new `StatelessWidget` class appended to the file.
/// Captures free local variables, enclosing parameters, and class fields as
/// constructor arguments and `final` fields, or returns `Err` when safe
/// extraction cannot be established.
/// Returns `(call_replacement, class_definition_to_append)`.
pub fn extract_flutter_widget(
    text: &str,
    span: &WidgetSpan,
    widget_name: &str,
) -> Result<(String, String), ExtractError> {
    let clean_name = widget_name.trim();
    if clean_name.is_empty()
        || !clean_name
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_uppercase())
        || !clean_name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err(ExtractError::InvalidName);
    }

    let captured = analyze_extraction(text, span)?;
    let expr = &text[span.start_byte..=span.close_paren_byte];
    let expr_indent = line_indent_at(text, span.start_byte);
    let body_expr = reindent_to(expr, &expr_indent, "    ");

    if captured.is_empty() {
        let call = format!("const {clean_name}()");
        let class_def = format!(
            "\n\nclass {clean_name} extends StatelessWidget {{\n  const {clean_name}({{super.key}});\n\n  @override\n  Widget build(BuildContext context) {{\n    return {body_expr};\n  }}\n}}\n"
        );
        return Ok((call, class_def));
    }

    let private_vars: Vec<&CapturedVar> = captured
        .iter()
        .filter(|v| v.name.starts_with('_'))
        .collect();
    let public_vars: Vec<&CapturedVar> = captured
        .iter()
        .filter(|v| !v.name.starts_with('_'))
        .collect();

    let mut call_args = Vec::with_capacity(captured.len());
    for v in &private_vars {
        call_args.push(v.name.clone());
    }
    for v in &public_vars {
        call_args.push(format!("{}: {}", v.name, v.name));
    }
    let call = format!("{clean_name}({})", call_args.join(", "));

    let mut ctor_parts = Vec::with_capacity(private_vars.len() + 1);
    for v in &private_vars {
        ctor_parts.push(format!("this.{}", v.name));
    }
    let mut named_params = Vec::with_capacity(public_vars.len() + 1);
    named_params.push("super.key".to_owned());
    for v in &public_vars {
        named_params.push(format!("required this.{}", v.name));
    }
    let ctor_sig = if named_params.len() <= 2 {
        let mut all = ctor_parts;
        all.push(format!("{{{}}}", named_params.join(", ")));
        format!("  const {clean_name}({});", all.join(", "))
    } else {
        let mut lines = Vec::new();
        for p in ctor_parts {
            lines.push(format!("    {p},"));
        }
        lines.push("    {".to_owned());
        for p in named_params {
            lines.push(format!("      {p},"));
        }
        lines.push("    }".to_owned());
        format!("  const {clean_name}(\n{}\n  );", lines.join("\n"))
    };

    let mut fields = String::new();
    for v in &captured {
        fields.push_str(&format!("  final {} {};\n", v.ty, v.name));
    }

    let class_def = format!(
        "\n\nclass {clean_name} extends StatelessWidget {{\n{ctor_sig}\n\n{fields}\n  @override\n  Widget build(BuildContext context) {{\n    return {body_expr};\n  }}\n}}\n"
    );
    Ok((call, class_def))
}

/// Converts a `StatelessWidget` class around `line` to a `StatefulWidget` + `State`.
pub fn convert_stateless_to_stateful(text: &str, line: u32) -> Option<(TextSpan, String)> {
    let idx = LineIndex::new(text);
    let cursor = idx.offset_at(text, TextPos::new(line, 0));
    let (class_start, class_end, class_name, body) = find_enclosing_class(text, cursor, "StatelessWidget")?;
    let state_name = if class_name.starts_with('_') {
        format!("{class_name}State")
    } else {
        format!("_{class_name}State")
    };
    let clean_body = remove_super_key_constructors(&body, &class_name);
    let replacement = format!(
        "class {class_name} extends StatefulWidget {{\n  const {class_name}({{super.key}});\n\n  @override\n  State<{class_name}> createState() => {state_name}();\n}}\n\nclass {state_name} extends State<{class_name}> {{{clean_body}}}"
    );
    Some((idx.span(text, class_start, class_end), replacement))
}

/// Converts a `StatefulWidget` + its `State<Name>` class around `line` to a `StatelessWidget`.
pub fn convert_stateful_to_stateless(text: &str, line: u32) -> Option<(TextSpan, String)> {
    let idx = LineIndex::new(text);
    let cursor = idx.offset_at(text, TextPos::new(line, 0));
    let (widget_start, _, class_name, _) = find_enclosing_class(text, cursor, "StatefulWidget")?;
    let state_marker = format!("extends State<{class_name}>");
    let state_rel = text[widget_start..].find(&state_marker)?;
    let state_class_kw = text[widget_start..widget_start + state_rel].rfind("class ")?;
    let state_start = widget_start + state_class_kw;
    let state_brace = text[state_start..].find('{')? + state_start;
    let state_end = matching_brace(text, state_brace)? + 1;
    let state_body = &text[state_brace + 1..state_end - 1];
    let replacement = format!(
        "class {class_name} extends StatelessWidget {{\n  const {class_name}({{super.key}});\n{state_body}}}"
    );
    Some((idx.span(text, widget_start, state_end), replacement))
}

fn scan_widget_calls(text: &str) -> Vec<WidgetSpan> {
    let idx = LineIndex::new(text);
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;
    let mut out = Vec::new();
    let mut paren_stack: Vec<Option<(usize, usize, String)>> = Vec::new();

    while i < len {
        let b = bytes[i];
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
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
            continue;
        }
        if b == b'\'' || b == b'"' {
            i = skip_string(bytes, i, b);
            continue;
        }
        if b.is_ascii_uppercase() && (i == 0 || !is_ident_char(bytes[i - 1])) {
            let start = i;
            while i < len && is_ident_char(bytes[i]) {
                i += 1;
            }
            let name = text[start..i].to_owned();
            let mut j = i;
            // Allow optional named constructor `.foo` or generic `<...>` before `(`
            if bytes.get(j) == Some(&b'.') {
                j += 1;
                while j < len && is_ident_char(bytes[j]) {
                    j += 1;
                }
            }
            while j < len && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if bytes.get(j) == Some(&b'(') {
                // Exclude class/method header lookalikes (`class Foo`, `extends Foo`)
                let prefix = text[..start].trim_end();
                let is_decl = prefix.ends_with("class")
                    || prefix.ends_with("mixin")
                    || prefix.ends_with("enum")
                    || prefix.ends_with("extends")
                    || prefix.ends_with("factory");
                if !is_decl {
                    // Include leading `const ` if present right before the widget name
                    let full_start = if let Some(stripped) = text[..start].strip_suffix("const ") {
                        if stripped.is_empty() || !is_ident_char(stripped.as_bytes()[stripped.len() - 1]) {
                            start - 6
                        } else {
                            start
                        }
                    } else {
                        start
                    };
                    paren_stack.push(Some((full_start, j, name)));
                    i = j + 1;
                    continue;
                }
            }
            continue;
        }
        if b == b'(' {
            paren_stack.push(None);
        } else if b == b')'
            && let Some(entry) = paren_stack.pop()
            && let Some((start_byte, open_paren_byte, name)) = entry
        {
            out.push(WidgetSpan {
                name,
                start_byte,
                open_paren_byte,
                close_paren_byte: i,
                span: idx.span(text, start_byte, i + 1),
            });
        }
        i += 1;
    }
    out
}

fn find_top_level_named_arg<'a>(args: &'a str, name: &str) -> Option<&'a str> {
    let bytes = args.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;
    let mut depth_paren = 0usize;
    let mut depth_brace = 0usize;
    let mut depth_bracket = 0usize;

    while i < len {
        let b = bytes[i];
        if b == b'\'' || b == b'"' {
            i = skip_string(bytes, i, b);
            continue;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if depth_paren == 0
            && depth_brace == 0
            && depth_bracket == 0
            && let Some(rest) = args[i..].strip_prefix(name)
            && (i == 0 || !is_ident_char(bytes[i - 1]))
            && rest.trim_start().starts_with(':')
        {
            let colon = args[i..].find(':')? + i;
            let val_start = colon + 1;
            let mut k = val_start;
            let mut dp = 0usize;
            let mut db = 0usize;
            let mut dk = 0usize;
            while k < len {
                let c = bytes[k];
                if c == b'\'' || c == b'"' {
                    k = skip_string(bytes, k, c);
                    continue;
                }
                match c {
                    b'(' => dp += 1,
                    b')' => {
                        if dp == 0 {
                            break;
                        }
                        dp -= 1;
                    }
                    b'{' => db += 1,
                    b'}' => db = db.saturating_sub(1),
                    b'[' => dk += 1,
                    b']' => dk = dk.saturating_sub(1),
                    b',' if dp == 0 && db == 0 && dk == 0 => return Some(&args[val_start..k]),
                    _ => {}
                }
                k += 1;
            }
            return Some(&args[val_start..k]);
        }
        match b {
            b'(' => depth_paren += 1,
            b')' => depth_paren = depth_paren.saturating_sub(1),
            b'{' => depth_brace += 1,
            b'}' => depth_brace = depth_brace.saturating_sub(1),
            b'[' => depth_bracket += 1,
            b']' => depth_bracket = depth_bracket.saturating_sub(1),
            _ => {}
        }
        i += 1;
    }
    None
}

fn find_enclosing_class(text: &str, cursor: usize, base: &str) -> Option<(usize, usize, String, String)> {
    let needle = format!("extends {base}");
    let mut search_from = 0usize;
    while let Some(rel) = text[search_from..].find(&needle) {
        let ext_pos = search_from + rel;
        if let Some(cls_rel) = text[..ext_pos].rfind("class ") {
            let header = text[cls_rel + 6..ext_pos].trim();
            let class_name = header.split_whitespace().next()?.to_owned();
            if let Some(brace_rel) = text[ext_pos..].find('{') {
                let open_brace = ext_pos + brace_rel;
                if let Some(close_brace) = matching_brace(text, open_brace)
                    && cursor >= cls_rel
                    && cursor <= close_brace + 1
                {
                    return Some((
                        cls_rel,
                        close_brace + 1,
                        class_name,
                        text[open_brace + 1..close_brace].to_owned(),
                    ));
                }
            }
        }
        search_from = ext_pos + needle.len();
    }
    None
}

fn remove_super_key_constructors(body: &str, class_name: &str) -> String {
    let sig = format!("const {class_name}({{super.key}});");
    let mut out = String::new();
    for line in body.split_inclusive('\n') {
        if line.trim() == sig {
            continue;
        }
        out.push_str(line);
    }
    out
}

fn matching_brace(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\'' || b == b'"' {
            i = skip_string(bytes, i, b);
            continue;
        }
        if b == b'{' {
            depth += 1;
        } else if b == b'}' {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

fn skip_string(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i = (i + 2).min(bytes.len());
            continue;
        }
        if bytes[i] == quote {
            return i + 1;
        }
        i += 1;
    }
    bytes.len()
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn line_indent_at(text: &str, byte_offset: usize) -> String {
    let line_start = text[..byte_offset].rfind('\n').map(|p| p + 1).unwrap_or(0);
    text[line_start..byte_offset]
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect()
}

fn reindent_block(expr: &str, old_indent: &str, new_indent: &str) -> String {
    let mut out = String::new();
    for (idx, line) in expr.split('\n').enumerate() {
        if idx > 0 {
            out.push('\n');
            let rest = line.strip_prefix(old_indent).unwrap_or(line);
            out.push_str(new_indent);
            out.push_str(rest);
        } else {
            out.push_str(line);
        }
    }
    out
}

fn reindent_out(expr: &str, old_indent: &str, new_indent: &str) -> String {
    reindent_block(expr, old_indent, new_indent)
}

fn reindent_to(expr: &str, old_indent: &str, new_indent: &str) -> String {
    reindent_block(expr, old_indent, new_indent)
}

const DART_EXTRACT_KEYWORDS: &[&str] = &[
    "abstract", "as", "assert", "async", "await", "base", "break", "case", "catch", "class",
    "const", "continue", "covariant", "default", "deferred", "do", "dynamic", "else", "enum",
    "export", "extends", "extension", "external", "factory", "false", "final", "finally", "for",
    "Function", "get", "hide", "if", "implements", "import", "in", "interface", "is", "late",
    "library", "mixin", "new", "null", "on", "operator", "part", "required", "rethrow", "return",
    "sealed", "set", "show", "static", "super", "switch", "sync", "this", "throw", "true", "try",
    "typedef", "var", "void", "when", "while", "with", "yield",
];

#[derive(Clone, Debug)]
enum ScopeKind {
    Class {
        type_params: HashSet<String>,
        open_brace: usize,
    },
    Function {
        params: Vec<(String, Option<String>)>,
        open_brace: usize,
    },
    ForBlock {
        bindings: Vec<(String, Option<String>)>,
        open_brace: usize,
    },
    CatchBlock {
        bindings: Vec<String>,
        open_brace: usize,
    },
    Block {
        open_brace: usize,
    },
    Literal,
}

#[derive(Clone, Debug)]
struct ArrowFrame {
    paren_depth: usize,
    bracket_depth: usize,
    brace_depth: usize,
    params: Vec<(String, Option<String>)>,
}

struct ExtractContext {
    top_level_names: HashSet<String>,
    class_type_params: HashSet<String>,
    class_methods: HashSet<String>,
    scopes: Vec<HashMap<String, Option<String>>>,
}

impl ExtractContext {
    fn lookup(&self, name: &str) -> Option<&Option<String>> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return Some(ty);
            }
        }
        None
    }
}

fn analyze_extraction(text: &str, span: &WidgetSpan) -> Result<Vec<CapturedVar>, ExtractError> {
    if span.start_byte > span.close_paren_byte || span.close_paren_byte >= text.len() {
        return Err(ExtractError::Unsafe("invalid widget span".into()));
    }
    let bytes = text.as_bytes();
    let top_level_names = collect_top_level_names(text);

    // Walk text[..span.start_byte] tracking braces and enclosing arrow closures.
    let mut brace_stack: Vec<ScopeKind> = Vec::new();
    let mut arrow_stack: Vec<ArrowFrame> = Vec::new();
    let mut paren_stack: Vec<usize> = Vec::new();
    let mut bracket_depth = 0usize;

    let mut i = 0usize;
    while i < span.start_byte {
        let b = bytes[i];
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            i = skip_line_comment_bytes(bytes, i);
            continue;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i = skip_block_comment_bytes(bytes, i);
            continue;
        }
        if is_string_start(bytes, i) {
            i = skip_any_dart_string(bytes, i);
            continue;
        }

        match b {
            b'(' => paren_stack.push(i),
            b')' => {
                if let Some(open_p) = paren_stack.pop() {
                    let cur_p_depth = paren_stack.len();
                    arrow_stack.retain(|f| {
                        f.brace_depth < brace_stack.len()
                            || (f.brace_depth == brace_stack.len()
                                && (f.bracket_depth < bracket_depth
                                    || (f.bracket_depth == bracket_depth && f.paren_depth <= cur_p_depth)))
                    });
                    // Check if `(params) =>` immediately follows (with optional `async`).
                    let mut after = skip_ws_and_comments(bytes, i + 1);
                    if after + 5 <= span.start_byte
                        && &bytes[after..after + 5] == b"async"
                        && !bytes.get(after + 5).is_some_and(|c| is_ident_char(*c))
                    {
                        after = skip_ws_and_comments(bytes, after + 5);
                    }
                    if after + 2 <= span.start_byte
                        && bytes[after] == b'='
                        && bytes.get(after + 1) == Some(&b'>')
                        && !is_control_header_before(text, open_p)
                    {
                        let params = parse_param_list(&text[open_p + 1..i]);
                        arrow_stack.push(ArrowFrame {
                            paren_depth: cur_p_depth,
                            bracket_depth,
                            brace_depth: brace_stack.len(),
                            params,
                        });
                    }
                }
            }
            b'[' => bracket_depth += 1,
            b']' => {
                bracket_depth = bracket_depth.saturating_sub(1);
                arrow_stack.retain(|f| {
                    !(f.brace_depth == brace_stack.len() && bracket_depth < f.bracket_depth)
                });
            }
            b',' | b';' => {
                let cur_p_depth = paren_stack.len();
                arrow_stack.retain(|f| {
                    !(f.brace_depth == brace_stack.len()
                        && f.bracket_depth == bracket_depth
                        && f.paren_depth == cur_p_depth)
                });
            }
            b'{' => {
                let scope = classify_open_brace(text, i, brace_stack.is_empty());
                brace_stack.push(scope);
            }
            b'}' => {
                brace_stack.pop();
                arrow_stack.retain(|f| f.brace_depth <= brace_stack.len());
            }
            _ => {}
        }
        i += 1;
    }

    let mut class_type_params = HashSet::new();
    let mut class_fields: HashMap<String, Option<String>> = HashMap::new();
    let mut class_methods: HashSet<String> = HashSet::new();

    if let Some((class_idx, open_brace)) = brace_stack.iter().enumerate().rev().find_map(|(idx, s)| {
        if let ScopeKind::Class { open_brace, type_params } = s {
            Some(((idx, type_params.clone()), *open_brace))
        } else {
            None
        }
    }) {
        class_type_params = class_idx.1;
        let close_brace = matching_brace(text, open_brace).unwrap_or(text.len());
        scan_class_members(&text[open_brace + 1..close_brace], &mut class_fields, &mut class_methods);
    }

    let mut scopes: Vec<HashMap<String, Option<String>>> = Vec::new();
    if !class_fields.is_empty() {
        scopes.push(class_fields);
    }

    for idx in 0..brace_stack.len() {
        let next_open = brace_stack
            .get(idx + 1)
            .and_then(|s| match s {
                ScopeKind::Class { open_brace, .. }
                | ScopeKind::Function { open_brace, .. }
                | ScopeKind::ForBlock { open_brace, .. }
                | ScopeKind::CatchBlock { open_brace, .. }
                | ScopeKind::Block { open_brace } => Some(*open_brace),
                ScopeKind::Literal => None,
            })
            .unwrap_or(span.start_byte);

        match &brace_stack[idx] {
            ScopeKind::Class { .. } | ScopeKind::Literal => {}
            ScopeKind::Function { params, open_brace } => {
                let mut map = HashMap::new();
                for (name, ty) in params {
                    map.insert(name.clone(), ty.clone());
                }
                if *open_brace < next_open {
                    scan_block_declarations(&text[*open_brace + 1..next_open], &mut map);
                }
                scopes.push(map);
            }
            ScopeKind::ForBlock { bindings, open_brace } => {
                let mut map = HashMap::new();
                for (name, ty) in bindings {
                    map.insert(name.clone(), ty.clone());
                }
                if *open_brace < next_open {
                    scan_block_declarations(&text[*open_brace + 1..next_open], &mut map);
                }
                scopes.push(map);
            }
            ScopeKind::CatchBlock { bindings, open_brace } => {
                let mut map = HashMap::new();
                for name in bindings {
                    map.insert(name.clone(), None);
                }
                if *open_brace < next_open {
                    scan_block_declarations(&text[*open_brace + 1..next_open], &mut map);
                }
                scopes.push(map);
            }
            ScopeKind::Block { open_brace } => {
                let mut map = HashMap::new();
                if *open_brace < next_open {
                    scan_block_declarations(&text[*open_brace + 1..next_open], &mut map);
                }
                scopes.push(map);
            }
        }
    }

    for arrow in arrow_stack {
        let mut map = HashMap::new();
        for (name, ty) in arrow.params {
            map.insert(name, ty);
        }
        if !map.is_empty() {
            scopes.push(map);
        }
    }

    let ctx = ExtractContext {
        top_level_names,
        class_type_params,
        class_methods,
        scopes,
    };

    let mut expr_locals: Vec<HashSet<String>> = Vec::new();
    let mut captured: Vec<CapturedVar> = Vec::new();
    let mut seen_captured: HashSet<String> = HashSet::new();

    scan_expr_range(
        text,
        span.start_byte,
        span.close_paren_byte + 1,
        &ctx,
        &mut expr_locals,
        &mut captured,
        &mut seen_captured,
    )?;

    Ok(captured)
}

fn scan_expr_range(
    text: &str,
    start: usize,
    end: usize,
    ctx: &ExtractContext,
    expr_locals: &mut Vec<HashSet<String>>,
    captured: &mut Vec<CapturedVar>,
    seen_captured: &mut HashSet<String>,
) -> Result<(), ExtractError> {
    let bytes = text.as_bytes();
    let mut i = start;
    let mut after_dot = false;
    let mut prev_punct: u8 = b'(';

    while i < end {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            i = skip_line_comment_bytes(bytes, i).min(end);
            continue;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i = skip_block_comment_bytes(bytes, i).min(end);
            continue;
        }
        if is_raw_string_start(bytes, i) {
            i = skip_any_dart_string(bytes, i).min(end);
            after_dot = false;
            prev_punct = b'"';
            continue;
        }
        if b == b'\'' || b == b'"' {
            i = scan_non_raw_string_vars(
                text,
                i,
                end,
                ctx,
                expr_locals,
                captured,
                seen_captured,
            )?;
            after_dot = false;
            prev_punct = b'"';
            continue;
        }

        // Control statements / expressions with bindings inside `expr`.
        if is_ident_start_byte(b) {
            let word_start = i;
            let mut word_end = i + 1;
            while word_end < end && is_ident_char(bytes[word_end]) {
                word_end += 1;
            }
            let word = &text[word_start..word_end];

            if !after_dot && word == "for" {
                let open_p = skip_ws_and_comments(bytes, word_end);
                if open_p < end
                    && bytes[open_p] == b'('
                    && let Some(close_p) = find_matching_delim(bytes, open_p, end, b'(', b')')
                {
                    let (bindings, scan_ranges) =
                        parse_for_header_parts(text, open_p + 1, close_p);
                    for (r_start, r_end) in scan_ranges {
                        scan_expr_range(
                            text,
                            r_start,
                            r_end,
                            ctx,
                            expr_locals,
                            captured,
                            seen_captured,
                        )?;
                    }
                    let mut frame = HashSet::new();
                    for bname in bindings {
                        frame.insert(bname);
                    }
                    expr_locals.push(frame);
                    let after_p = skip_ws_and_comments(bytes, close_p + 1);
                    let body_end = if after_p < end && bytes[after_p] == b'{' {
                        find_matching_delim(bytes, after_p, end, b'{', b'}')
                            .map(|c| c + 1)
                            .unwrap_or(end)
                    } else {
                        find_for_element_end(bytes, after_p, end)
                    };
                    if after_p < body_end && bytes[after_p] == b'{' {
                        scan_block_statements(
                            text,
                            after_p + 1,
                            body_end - 1,
                            ctx,
                            expr_locals,
                            captured,
                            seen_captured,
                        )?;
                    } else {
                        scan_expr_range(
                            text,
                            after_p,
                            body_end,
                            ctx,
                            expr_locals,
                            captured,
                            seen_captured,
                        )?;
                    }
                    expr_locals.pop();
                    i = body_end;
                    after_dot = false;
                    prev_punct = b')';
                    continue;
                }
            }

            if !after_dot && word == "catch" {
                let open_p = skip_ws_and_comments(bytes, word_end);
                if open_p < end
                    && bytes[open_p] == b'('
                    && let Some(close_p) = find_matching_delim(bytes, open_p, end, b'(', b')')
                {
                    let mut frame = HashSet::new();
                    for cname in parse_catch_bindings(&text[open_p + 1..close_p]) {
                        frame.insert(cname);
                    }
                    expr_locals.push(frame);
                    let after_p = skip_ws_and_comments(bytes, close_p + 1);
                    let body_end = if after_p < end && bytes[after_p] == b'{' {
                        find_matching_delim(bytes, after_p, end, b'{', b'}')
                            .map(|c| c + 1)
                            .unwrap_or(end)
                    } else {
                        end
                    };
                    if after_p < body_end && bytes[after_p] == b'{' {
                        scan_block_statements(
                            text,
                            after_p + 1,
                            body_end - 1,
                            ctx,
                            expr_locals,
                            captured,
                            seen_captured,
                        )?;
                    }
                    expr_locals.pop();
                    i = body_end;
                    after_dot = false;
                    prev_punct = b'}';
                    continue;
                }
            }

            if after_dot {
                after_dot = false;
                i = word_end;
                prev_punct = b'a';
                continue;
            }

            // Skip named argument label (`ident: value`) when directly after `(` or `,`.
            let next_after = skip_ws_and_comments(bytes, word_end);
            if matches!(prev_punct, b'(' | b',')
                && next_after < end
                && bytes[next_after] == b':'
                && bytes.get(next_after + 1) != Some(&b':')
            {
                i = word_end;
                prev_punct = b'a';
                continue;
            }

            if is_expr_local(expr_locals, word) {
                i = word_end;
                after_dot = false;
                prev_punct = b'a';
                continue;
            }

            if matches!(word, "this" | "super" | "widget") {
                return Err(ExtractError::Unsafe(format!(
                    "references enclosing `{word}`"
                )));
            }

            if ctx.class_type_params.contains(word) {
                return Err(ExtractError::Unsafe(format!(
                    "references enclosing class type parameter `{word}`"
                )));
            }

            if DART_EXTRACT_KEYWORDS.contains(&word) {
                i = word_end;
                after_dot = false;
                prev_punct = b'a';
                continue;
            }

            // Free non-keyword identifier.
            let is_pascal = word.bytes().next().is_some_and(|c| c.is_ascii_uppercase());
            if !is_pascal {
                let is_call = is_call_after_ident(bytes, word_end, end);
                if is_call {
                    if let Some(ty_opt) = ctx.lookup(word) {
                        capture_resolved_var(word, ty_opt.as_deref(), ctx, captured, seen_captured)?;
                    } else if ctx.class_methods.contains(word) {
                        return Err(ExtractError::Unsafe(format!(
                            "calls enclosing method `{word}`"
                        )));
                    } else if matches!(word, "print" | "identical")
                        || ctx.top_level_names.contains(word)
                    {
                        // Safe top-level function call.
                    } else {
                        return Err(ExtractError::Unsafe(format!(
                            "calls unresolved or enclosing function `{word}`"
                        )));
                    }
                } else {
                    if has_mutation_around(bytes, start, end, word_start, word_end) {
                        return Err(ExtractError::Unsafe(format!(
                            "mutates free variable `{word}`"
                        )));
                    }
                    if word == "context" {
                        if let Some(ty_opt) = ctx.lookup("context")
                            && ty_opt.as_deref() != Some("BuildContext")
                        {
                            return Err(ExtractError::Unsafe(
                                "local `context` shadows `BuildContext`".into(),
                            ));
                        }
                    } else if let Some(ty_opt) = ctx.lookup(word) {
                        capture_resolved_var(word, ty_opt.as_deref(), ctx, captured, seen_captured)?;
                    } else if ctx.class_methods.contains(word) {
                        return Err(ExtractError::Unsafe(format!(
                            "references enclosing member `{word}`"
                        )));
                    } else if ctx.top_level_names.contains(word) {
                        // Top-level binding in the same file stays valid at top level.
                    } else {
                        return Err(ExtractError::Unsafe(format!(
                            "cannot resolve free identifier `{word}`"
                        )));
                    }
                }
            }

            i = word_end;
            after_dot = false;
            prev_punct = b'a';
            continue;
        }

        if b == b'('
            && let Some(close_p) = find_matching_delim(bytes, i, end, b'(', b')')
        {
            // Check if `(params) => expr` or `(params) { stmts }`.
            let mut after = skip_ws_and_comments(bytes, close_p + 1);
            if after + 5 <= end
                && &bytes[after..after + 5] == b"async"
                && !bytes.get(after + 5).is_some_and(|c| is_ident_char(*c))
            {
                after = skip_ws_and_comments(bytes, after + 5);
            }
            if after + 2 <= end && bytes[after] == b'=' && bytes[after + 1] == b'>' {
                let params = parse_param_list(&text[i + 1..close_p]);
                scan_param_type_refs(text, i + 1, close_p, ctx)?;
                let mut frame = HashSet::new();
                for (pname, _) in params {
                    frame.insert(pname);
                }
                expr_locals.push(frame);
                let body_start = after + 2;
                let body_end = find_arrow_body_end(bytes, body_start, end);
                scan_expr_range(
                    text,
                    body_start,
                    body_end,
                    ctx,
                    expr_locals,
                    captured,
                    seen_captured,
                )?;
                expr_locals.pop();
                i = body_end;
                after_dot = false;
                prev_punct = b')';
                continue;
            }
            if after < end && bytes[after] == b'{' && !matches!(prev_punct, b'a') {
                let params = parse_param_list(&text[i + 1..close_p]);
                scan_param_type_refs(text, i + 1, close_p, ctx)?;
                let close_b = find_matching_delim(bytes, after, end, b'{', b'}')
                    .unwrap_or(end.saturating_sub(1));
                let mut frame = HashSet::new();
                for (pname, _) in params {
                    frame.insert(pname);
                }
                expr_locals.push(frame);
                scan_block_statements(
                    text,
                    after + 1,
                    close_b,
                    ctx,
                    expr_locals,
                    captured,
                    seen_captured,
                )?;
                expr_locals.pop();
                i = (close_b + 1).min(end);
                after_dot = false;
                prev_punct = b'}';
                continue;
            }

            scan_expr_range(
                text,
                i + 1,
                close_p,
                ctx,
                expr_locals,
                captured,
                seen_captured,
            )?;
            i = close_p + 1;
            after_dot = false;
            prev_punct = b')';
            continue;
        }

        if b == b'{'
            && let Some(close_b) = find_matching_delim(bytes, i, end, b'{', b'}')
        {
            if is_block_preceder(prev_punct) {
                expr_locals.push(HashSet::new());
                scan_block_statements(
                    text,
                    i + 1,
                    close_b,
                    ctx,
                    expr_locals,
                    captured,
                    seen_captured,
                )?;
                expr_locals.pop();
            } else {
                scan_expr_range(
                    text,
                    i + 1,
                    close_b,
                    ctx,
                    expr_locals,
                    captured,
                    seen_captured,
                )?;
            }
            i = close_b + 1;
            after_dot = false;
            prev_punct = b'}';
            continue;
        }

        if b == b'.' {
            after_dot = true;
            prev_punct = b'.';
            i += 1;
            continue;
        }

        if b.is_ascii_digit() {
            i = skip_number_bytes(bytes, i, end);
            after_dot = false;
            prev_punct = b'0';
            continue;
        }

        after_dot = false;
        prev_punct = b;
        i += 1;
    }

    Ok(())
}

fn capture_resolved_var(
    name: &str,
    ty: Option<&str>,
    ctx: &ExtractContext,
    captured: &mut Vec<CapturedVar>,
    seen_captured: &mut HashSet<String>,
) -> Result<(), ExtractError> {
    let Some(ty_str) = ty else {
        return Err(ExtractError::Unsafe(format!(
            "cannot infer static type of `{name}`"
        )));
    };
    if !ctx.class_type_params.is_empty() && type_mentions_any(ty_str, &ctx.class_type_params) {
        return Err(ExtractError::Unsafe(format!(
            "type `{ty_str}` of `{name}` references an enclosing class type parameter"
        )));
    }
    if seen_captured.insert(name.to_owned()) {
        captured.push(CapturedVar {
            name: name.to_owned(),
            ty: ty_str.to_owned(),
        });
    }
    Ok(())
}

fn scan_param_type_refs(
    text: &str,
    start: usize,
    end: usize,
    ctx: &ExtractContext,
) -> Result<(), ExtractError> {
    if ctx.class_type_params.is_empty() {
        return Ok(());
    }
    let slice = &text[start..end];
    for tp in &ctx.class_type_params {
        if contains_word(slice, tp) {
            return Err(ExtractError::Unsafe(format!(
                "references enclosing class type parameter `{tp}`"
            )));
        }
    }
    Ok(())
}

fn scan_block_statements(
    text: &str,
    start: usize,
    end: usize,
    ctx: &ExtractContext,
    expr_locals: &mut Vec<HashSet<String>>,
    captured: &mut Vec<CapturedVar>,
    seen_captured: &mut HashSet<String>,
) -> Result<(), ExtractError> {
    let bytes = text.as_bytes();
    let mut stmt_start = start;
    let mut i = start;
    let mut dp = 0usize;
    let mut db = 0usize;
    let mut dk = 0usize;

    while i < end {
        let b = bytes[i];
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            i = skip_line_comment_bytes(bytes, i).min(end);
            continue;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i = skip_block_comment_bytes(bytes, i).min(end);
            continue;
        }
        if is_string_start(bytes, i) {
            i = skip_any_dart_string(bytes, i).min(end);
            continue;
        }
        match b {
            b'(' => dp += 1,
            b')' => dp = dp.saturating_sub(1),
            b'[' => dk += 1,
            b']' => dk = dk.saturating_sub(1),
            b'{' => db += 1,
            b'}' => {
                if db == 0 {
                    break;
                }
                db -= 1;
                if dp == 0 && db == 0 && dk == 0 {
                    scan_expr_range(
                        text,
                        stmt_start,
                        i + 1,
                        ctx,
                        expr_locals,
                        captured,
                        seen_captured,
                    )?;
                    stmt_start = i + 1;
                }
            }
            b';' if dp == 0 && db == 0 && dk == 0 => {
                let stmt = &text[stmt_start..i];
                let decls = parse_statement_declarations(stmt);
                if let Some(eq_rel) = find_top_level_equals(stmt) {
                    scan_expr_range(
                        text,
                        stmt_start + eq_rel + 1,
                        i,
                        ctx,
                        expr_locals,
                        captured,
                        seen_captured,
                    )?;
                } else {
                    scan_expr_range(
                        text,
                        stmt_start,
                        i,
                        ctx,
                        expr_locals,
                        captured,
                        seen_captured,
                    )?;
                }
                if let Some(frame) = expr_locals.last_mut() {
                    for (name, _) in decls {
                        frame.insert(name);
                    }
                }
                stmt_start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if stmt_start < end {
        scan_expr_range(
            text,
            stmt_start,
            end,
            ctx,
            expr_locals,
            captured,
            seen_captured,
        )?;
    }
    Ok(())
}

fn scan_non_raw_string_vars(
    text: &str,
    start: usize,
    end: usize,
    ctx: &ExtractContext,
    expr_locals: &mut Vec<HashSet<String>>,
    captured: &mut Vec<CapturedVar>,
    seen_captured: &mut HashSet<String>,
) -> Result<usize, ExtractError> {
    let bytes = text.as_bytes();
    let quote = bytes[start];
    let triple = bytes.get(start + 1) == Some(&quote) && bytes.get(start + 2) == Some(&quote);
    let mut i = start + if triple { 3 } else { 1 };

    while i < end {
        if bytes[i] == b'\\' {
            i = (i + 2).min(end);
            continue;
        }
        if bytes[i] == b'$' {
            if bytes.get(i + 1) == Some(&b'{') {
                if let Some(close_b) = find_matching_delim(bytes, i + 1, end, b'{', b'}') {
                    scan_expr_range(
                        text,
                        i + 2,
                        close_b,
                        ctx,
                        expr_locals,
                        captured,
                        seen_captured,
                    )?;
                    i = close_b + 1;
                    continue;
                }
            } else if bytes.get(i + 1).is_some_and(|c| is_ident_start_byte(*c)) {
                let id_start = i + 1;
                let mut id_end = id_start + 1;
                while id_end < end && is_ident_char(bytes[id_end]) {
                    id_end += 1;
                }
                let word = &text[id_start..id_end];
                if !is_expr_local(expr_locals, word) {
                    if matches!(word, "this" | "super" | "widget") {
                        return Err(ExtractError::Unsafe(format!(
                            "references enclosing `{word}`"
                        )));
                    }
                    if word != "context" {
                        if let Some(ty_opt) = ctx.lookup(word) {
                            capture_resolved_var(
                                word,
                                ty_opt.as_deref(),
                                ctx,
                                captured,
                                seen_captured,
                            )?;
                        } else if !ctx.top_level_names.contains(word) {
                            return Err(ExtractError::Unsafe(format!(
                                "cannot resolve interpolated identifier `{word}`"
                            )));
                        }
                    } else if let Some(ty_opt) = ctx.lookup("context")
                        && ty_opt.as_deref() != Some("BuildContext")
                    {
                        return Err(ExtractError::Unsafe(
                            "local `context` shadows `BuildContext`".into(),
                        ));
                    }
                }
                i = id_end;
                continue;
            }
        }
        if triple {
            if bytes[i] == quote
                && bytes.get(i + 1) == Some(&quote)
                && bytes.get(i + 2) == Some(&quote)
            {
                return Ok((i + 3).min(end));
            }
        } else if bytes[i] == quote {
            return Ok((i + 1).min(end));
        }
        i += 1;
    }
    Ok(end)
}

fn is_expr_local(scopes: &[HashSet<String>], name: &str) -> bool {
    scopes.iter().rev().any(|s| s.contains(name))
}

fn is_block_preceder(prev_punct: u8) -> bool {
    matches!(prev_punct, b')' | b';' | b'{' | b'}') || prev_punct == b'a'
}

fn is_call_after_ident(bytes: &[u8], word_end: usize, limit: usize) -> bool {
    let mut k = skip_ws_and_comments(bytes, word_end).min(limit);
    if k < limit && bytes[k] == b'(' {
        return true;
    }
    if k < limit && bytes[k] == b'<'
        && let Some(gt) = find_matching_angle(bytes, k, limit)
    {
        k = skip_ws_and_comments(bytes, gt + 1).min(limit);
        return k < limit && bytes[k] == b'(';
    }
    false
}

fn has_mutation_around(
    bytes: &[u8],
    range_start: usize,
    range_end: usize,
    word_start: usize,
    word_end: usize,
) -> bool {
    // Prefix ++v / --v
    let mut p = word_start;
    while p > range_start && bytes[p - 1].is_ascii_whitespace() {
        p -= 1;
    }
    if p >= range_start + 2 && (&bytes[p - 2..p] == b"++" || &bytes[p - 2..p] == b"--") {
        return true;
    }
    // Postfix v++ / v-- / v = ... / v += ...
    let q = skip_ws_and_comments(bytes, word_end).min(range_end);
    if q + 1 < range_end && (&bytes[q..q + 2] == b"++" || &bytes[q..q + 2] == b"--") {
        return true;
    }
    if q < range_end && bytes[q] == b'=' && !matches!(bytes.get(q + 1), Some(b'=' | b'>')) {
        return true;
    }
    for op in [
        "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "~/=", "??=", "<<=", ">>=",
    ] {
        let ob = op.as_bytes();
        if q + ob.len() <= range_end && &bytes[q..q + ob.len()] == ob {
            return true;
        }
    }
    q + 4 <= range_end && &bytes[q..q + 4] == b">>>="
}

fn classify_open_brace(text: &str, open_brace: usize, at_top_level: bool) -> ScopeKind {
    let bytes = text.as_bytes();
    let header_start = find_header_start(bytes, open_brace);
    let header = text[header_start..open_brace].trim();

    if at_top_level && let Some((type_params, _is_class)) = parse_type_decl_header(header) {
        return ScopeKind::Class {
            type_params,
            open_brace,
        };
    }

    // Walk back from `open_brace` skipping `async`/`sync`/`*`.
    let mut end = open_brace;
    loop {
        while end > header_start && bytes[end - 1].is_ascii_whitespace() {
            end -= 1;
        }
        let s = text[header_start..end].trim_end();
        if let Some(stripped) = s
            .strip_suffix("async")
            .or_else(|| s.strip_suffix("sync"))
            .filter(|p| p.is_empty() || !is_ident_char(p.as_bytes()[p.len() - 1]))
        {
            end = header_start + stripped.len();
            continue;
        }
        if let Some(stripped) = s.strip_suffix('*') {
            end = header_start + stripped.len();
            continue;
        }
        break;
    }

    if end > header_start && bytes[end - 1] == b')' {
        let close_p = end - 1;
        if let Some(open_p) = find_matching_paren_backward(bytes, header_start, close_p) {
            let before_paren = text[header_start..open_p].trim_end();
            let trailing_kw = before_paren
                .rsplit(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .next()
                .unwrap_or("");
            match trailing_kw {
                "for" => {
                    let (names, _) = parse_for_header_parts(text, open_p + 1, close_p);
                    let mut bindings = Vec::new();
                    for name in names {
                        bindings.push((name, None));
                    }
                    return ScopeKind::ForBlock {
                        bindings,
                        open_brace,
                    };
                }
                "catch" => {
                    return ScopeKind::CatchBlock {
                        bindings: parse_catch_bindings(&text[open_p + 1..close_p]),
                        open_brace,
                    };
                }
                "if" | "while" | "switch" => {
                    return ScopeKind::Block { open_brace };
                }
                _ => {
                    let params = parse_param_list(&text[open_p + 1..close_p]);
                    return ScopeKind::Function { params, open_brace };
                }
            }
        }
    }

    let last_word = header
        .rsplit(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .next()
        .unwrap_or("");
    if matches!(last_word, "else" | "try" | "finally") || header.is_empty() {
        return ScopeKind::Block { open_brace };
    }
    if header.starts_with("for") || header.contains(" for ") {
        return ScopeKind::Block { open_brace };
    }

    // Getter `String get foo { ... }`
    if last_word != "const" && header.contains(" get ") {
        return ScopeKind::Function {
            params: Vec::new(),
            open_brace,
        };
    }

    let last_non_ws = header.as_bytes().last().copied().unwrap_or(b';');
    if matches!(
        last_non_ws,
        b'=' | b':' | b',' | b'[' | b'(' | b'>' | b'?'
    ) || last_word == "return"
        || last_word == "const"
    {
        return ScopeKind::Literal;
    }

    ScopeKind::Block { open_brace }
}

fn find_header_start(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos;
    let mut dp = 0usize;
    let mut dk = 0usize;
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b')' => dp += 1,
            b'(' => dp = dp.saturating_sub(1),
            b']' => dk += 1,
            b'[' => dk = dk.saturating_sub(1),
            b';' | b'{' | b'}' if dp == 0 && dk == 0 => return i + 1,
            _ => {}
        }
    }
    0
}

fn find_matching_paren_backward(bytes: &[u8], min_pos: usize, close_p: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = close_p + 1;
    while i > min_pos {
        i -= 1;
        match bytes[i] {
            b')' => depth += 1,
            b'(' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn is_control_header_before(text: &str, open_p: usize) -> bool {
    let prefix = text[..open_p].trim_end();
    let kw = prefix
        .rsplit(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .next()
        .unwrap_or("");
    matches!(kw, "if" | "for" | "while" | "switch" | "catch")
}

fn parse_type_decl_header(header: &str) -> Option<(HashSet<String>, bool)> {
    let cleaned = header.trim();
    for kw in ["class ", "mixin ", "extension "] {
        if let Some(idx) = cleaned.find(kw) {
            let before = &cleaned[..idx];
            if idx == 0 || before.ends_with(char::is_whitespace) {
                let rest = cleaned[idx + kw.len()..].trim_start();
                let mut type_params = HashSet::new();
                if let Some(lt) = rest.find('<') {
                    let prefix_name = rest[..lt].trim();
                    if prefix_name
                        .bytes()
                        .all(is_ident_char)
                        && let Some(gt) = find_matching_angle(rest.as_bytes(), lt, rest.len())
                    {
                        for seg in split_top_level(&rest[lt + 1..gt], b',') {
                            let seg = seg.trim();
                            if let Some(tp) = seg.split(|c: char| !c.is_ascii_alphanumeric() && c != '_').next()
                                && !tp.is_empty()
                                && tp.bytes().next().is_some_and(is_ident_start_byte)
                            {
                                type_params.insert(tp.to_owned());
                            }
                        }
                    }
                }
                return Some((type_params, true));
            }
        }
    }
    None
}

fn collect_top_level_names(text: &str) -> HashSet<String> {
    let mut names = HashSet::new();
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;
    let mut depth = 0usize;
    let mut dp = 0usize;
    let mut dk = 0usize;
    let mut stmt_start = 0usize;

    while i < len {
        let b = bytes[i];
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            i = skip_line_comment_bytes(bytes, i);
            continue;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i = skip_block_comment_bytes(bytes, i);
            continue;
        }
        if is_string_start(bytes, i) {
            i = skip_any_dart_string(bytes, i);
            continue;
        }
        match b {
            b'(' => dp += 1,
            b')' => dp = dp.saturating_sub(1),
            b'[' => dk += 1,
            b']' => dk = dk.saturating_sub(1),
            b'{' => {
                if depth == 0 && dp == 0 && dk == 0 {
                    extract_top_level_header_name(text[stmt_start..i].trim(), &mut names);
                }
                depth += 1;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && dp == 0 && dk == 0 {
                    stmt_start = i + 1;
                }
            }
            b';' if depth == 0 && dp == 0 && dk == 0 => {
                extract_top_level_header_name(text[stmt_start..i].trim(), &mut names);
                stmt_start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    names
}

fn extract_top_level_header_name(header: &str, out: &mut HashSet<String>) {
    let h = strip_leading_annotations_and_comments(header);
    if h.is_empty()
        || h.starts_with("import ")
        || h.starts_with("export ")
        || h.starts_with("part ")
        || h.starts_with("library ")
    {
        return;
    }
    for kw in ["class ", "mixin ", "enum ", "typedef ", "extension "] {
        if let Some(pos) = h.find(kw) {
            let after = h[pos + kw.len()..].trim_start();
            if let Some(name) = after
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .next()
                && !name.is_empty()
                && name != "on"
            {
                out.insert(name.to_owned());
            }
            return;
        }
    }
    for (name, _) in parse_statement_declarations(h) {
        out.insert(name);
    }
    if let Some(fn_name) = parse_callable_decl_name(h) {
        out.insert(fn_name);
    }
}

fn scan_class_members(
    body: &str,
    fields: &mut HashMap<String, Option<String>>,
    methods: &mut HashSet<String>,
) {
    let bytes = body.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;
    let mut depth = 0usize;
    let mut dp = 0usize;
    let mut dk = 0usize;
    let mut stmt_start = 0usize;

    while i < len {
        let b = bytes[i];
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            i = skip_line_comment_bytes(bytes, i);
            continue;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i = skip_block_comment_bytes(bytes, i);
            continue;
        }
        if is_string_start(bytes, i) {
            i = skip_any_dart_string(bytes, i);
            continue;
        }
        match b {
            b'(' => dp += 1,
            b')' => dp = dp.saturating_sub(1),
            b'[' => dk += 1,
            b']' => dk = dk.saturating_sub(1),
            b'{' => {
                if depth == 0 && dp == 0 && dk == 0
                    && let Some(mname) = parse_callable_decl_name(body[stmt_start..i].trim())
                {
                    methods.insert(mname);
                }
                depth += 1;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && dp == 0 && dk == 0 {
                    stmt_start = i + 1;
                }
            }
            b';' if depth == 0 && dp == 0 && dk == 0 => {
                let stmt = strip_leading_annotations_and_comments(body[stmt_start..i].trim());
                if let Some(mname) = parse_callable_decl_name(stmt) {
                    methods.insert(mname);
                } else {
                    for (fname, fty) in parse_statement_declarations(stmt) {
                        fields.insert(fname, fty);
                    }
                }
                stmt_start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
}

fn parse_callable_decl_name(header: &str) -> Option<String> {
    let h = strip_leading_annotations_and_comments(header);
    if h.is_empty() {
        return None;
    }
    let bytes = h.as_bytes();
    // Look for `get <name>` before `=>` or `{` or end.
    let head = if let Some(arrow) = find_top_level_arrow(h) {
        h[..arrow].trim()
    } else {
        h
    };
    if let Some(paren) = find_top_level_char(head, b'(') {
        let before = head[..paren].trim_end();
        let name = before
            .rsplit(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .next()?;
        if name.is_empty()
            || !name.bytes().next().is_some_and(is_ident_start_byte)
            || DART_EXTRACT_KEYWORDS.contains(&name)
            || name.bytes().next().is_some_and(|c| c.is_ascii_uppercase())
        {
            return None;
        }
        return Some(name.to_owned());
    }
    // Getter `Type get name`
    let tokens: Vec<&str> = head.split_whitespace().collect();
    if tokens.len() >= 2 && tokens[tokens.len() - 2] == "get" {
        let name = tokens[tokens.len() - 1];
        if !name.is_empty() && name.bytes().next().is_some_and(is_ident_start_byte) {
            return Some(name.to_owned());
        }
    }
    let _ = bytes;
    None
}

fn scan_block_declarations(src: &str, out: &mut HashMap<String, Option<String>>) {
    let bytes = src.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;
    let mut dp = 0usize;
    let mut db = 0usize;
    let mut dk = 0usize;
    let mut stmt_start = 0usize;

    while i < len {
        let b = bytes[i];
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            i = skip_line_comment_bytes(bytes, i);
            continue;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i = skip_block_comment_bytes(bytes, i);
            continue;
        }
        if is_string_start(bytes, i) {
            i = skip_any_dart_string(bytes, i);
            continue;
        }
        match b {
            b'(' => dp += 1,
            b')' => dp = dp.saturating_sub(1),
            b'[' => dk += 1,
            b']' => dk = dk.saturating_sub(1),
            b'{' => db += 1,
            b'}' => {
                db = db.saturating_sub(1);
                if dp == 0 && db == 0 && dk == 0 {
                    stmt_start = i + 1;
                }
            }
            b';' if dp == 0 && db == 0 && dk == 0 => {
                for (name, ty) in parse_statement_declarations(&src[stmt_start..i]) {
                    out.insert(name, ty);
                }
                stmt_start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
}

fn parse_statement_declarations(stmt: &str) -> Vec<(String, Option<String>)> {
    let s = strip_leading_annotations_and_comments(stmt);
    if s.is_empty() {
        return Vec::new();
    }
    let first_word = s
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .next()
        .unwrap_or("");
    if matches!(
        first_word,
        "return"
            | "throw"
            | "rethrow"
            | "yield"
            | "await"
            | "if"
            | "else"
            | "for"
            | "while"
            | "do"
            | "switch"
            | "case"
            | "default"
            | "break"
            | "continue"
            | "try"
            | "catch"
            | "finally"
            | "assert"
            | "class"
            | "enum"
            | "mixin"
            | "typedef"
            | "extension"
            | "import"
            | "export"
            | "part"
            | "library"
    ) {
        return Vec::new();
    }

    // Split multiple declarators separated by `,` outside `<...>(...)[...]{...}` and before `=>`
    if find_top_level_arrow(s).is_some() {
        return Vec::new();
    }
    let lhs_all = if let Some(eq) = find_top_level_equals(s) {
        &s[..eq]
    } else {
        s
    };
    // Function declaration without body is not a variable.
    if find_top_level_char(lhs_all, b'(').is_some() && !lhs_all.contains("Function(") {
        return Vec::new();
    }

    let mut items = split_top_level(s, b',');
    if items.is_empty() {
        return Vec::new();
    }
    let first_part = items.remove(0);
    let (prefix_lhs, first_rhs) = match find_top_level_equals(first_part) {
        Some(eq) => (first_part[..eq].trim(), Some(first_part[eq + 1..].trim())),
        None => (first_part.trim(), None),
    };
    let Some((shared_ty, first_name)) = parse_typed_binding(prefix_lhs, true) else {
        return Vec::new();
    };
    let inferred = shared_ty.clone().or_else(|| first_rhs.and_then(infer_literal_type));
    let mut out = vec![(first_name, inferred)];

    for extra in items {
        let (extra_lhs, extra_rhs) = match find_top_level_equals(extra) {
            Some(eq) => (extra[..eq].trim(), Some(extra[eq + 1..].trim())),
            None => (extra.trim(), None),
        };
        if is_simple_ident(extra_lhs) && !DART_EXTRACT_KEYWORDS.contains(&extra_lhs) {
            let ty = shared_ty
                .clone()
                .or_else(|| extra_rhs.and_then(infer_literal_type));
            out.push((extra_lhs.to_owned(), ty));
        }
    }
    out
}

fn parse_param_list(raw: &str) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return out;
    }
    for part in split_top_level(trimmed, b',') {
        let p = part.trim();
        let inner = p
            .trim_start_matches('{')
            .trim_start_matches('[')
            .trim_end_matches('}')
            .trim_end_matches(']')
            .trim();
        if inner.is_empty() {
            continue;
        }
        for sub in split_top_level(inner, b',') {
            let sub = sub.trim();
            if sub.is_empty() {
                continue;
            }
            let lhs = match find_top_level_equals(sub) {
                Some(eq) => sub[..eq].trim(),
                None => sub,
            };
            if let Some((ty, name)) = parse_typed_binding(lhs, false) {
                out.push((name, ty));
            }
        }
    }
    out
}

fn parse_typed_binding(lhs: &str, require_decl_or_type: bool) -> Option<(Option<String>, String)> {
    let mut rest = strip_leading_annotations_and_comments(lhs);
    let mut saw_decl_kw = false;
    loop {
        let trimmed = rest.trim_start();
        let word = trimmed
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .next()
            .unwrap_or("");
        if matches!(
            word,
            "static" | "late" | "final" | "const" | "var" | "required" | "covariant"
        ) && (trimmed.len() == word.len()
            || !is_ident_char(trimmed.as_bytes()[word.len()]))
        {
            if matches!(word, "late" | "final" | "const" | "var") {
                saw_decl_kw = true;
            }
            rest = trimmed[word.len()..].trim_start();
            continue;
        }
        break;
    }

    let rest = rest.trim();
    if rest.is_empty() || rest.contains("this.") || rest.contains("super.") {
        return None;
    }

    // Old-style function parameter: `Ret name(Args)`
    if let Some(paren_idx) = find_top_level_char(rest, b'(')
        && !rest.contains("Function(")
        && let Some(close_p) = find_matching_delim(rest.as_bytes(), paren_idx, rest.len(), b'(', b')')
    {
        let before_p = rest[..paren_idx].trim_end();
        let name_start = before_p
            .rfind(|c: char| !is_ident_char(c as u8))
            .map(|p| p + 1)
            .unwrap_or(0);
        let name = &before_p[name_start..];
        let ret_ty = before_p[..name_start].trim();
        let ret_str = if ret_ty.is_empty() { "dynamic" } else { ret_ty };
        let args_str = &rest[paren_idx..=close_p];
        let null_suffix = rest[close_p + 1..].trim();
        if is_simple_ident(name) && !DART_EXTRACT_KEYWORDS.contains(&name) {
            return Some((
                Some(format!("{ret_str} Function{args_str}{null_suffix}")),
                name.to_owned(),
            ));
        }
    }

    // Split last identifier (the variable/param name) from preceding type expression.
    let bytes = rest.as_bytes();
    let mut name_start = rest.len();
    while name_start > 0 && is_ident_char(bytes[name_start - 1]) {
        name_start -= 1;
    }
    let name = rest[name_start..].trim();
    if !is_simple_ident(name) || DART_EXTRACT_KEYWORDS.contains(&name) {
        return None;
    }
    let ty_part = rest[..name_start].trim();
    if ty_part.is_empty() {
        return if saw_decl_or_param(saw_decl_kw, require_decl_or_type) {
            Some((None, name.to_owned()))
        } else {
            None
        };
    }

    if !looks_like_dart_type(ty_part) {
        return None;
    }
    Some((Some(normalize_spaces(ty_part)), name.to_owned()))
}

fn saw_decl_or_param(saw_decl_kw: bool, require_decl_or_type: bool) -> bool {
    saw_decl_kw || !require_decl_or_type
}

fn looks_like_dart_type(ty: &str) -> bool {
    let t = ty.trim();
    if t.is_empty() {
        return false;
    }
    let first = t
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '.')
        .next()
        .unwrap_or("");
    let last_seg = first.rsplit('.').next().unwrap_or(first);
    matches!(
        last_seg,
        "bool" | "int" | "double" | "num" | "String" | "dynamic" | "void" | "Object" | "Never"
            | "Function"
    ) || last_seg.bytes().next().is_some_and(|b| b.is_ascii_uppercase())
        || t.starts_with('(')
}

fn infer_literal_type(rhs: &str) -> Option<String> {
    let mut r = rhs.trim();
    while r.starts_with('(') && r.ends_with(')') {
        if find_matching_delim(r.as_bytes(), 0, r.len(), b'(', b')') == Some(r.len() - 1) {
            r = r[1..r.len() - 1].trim();
        } else {
            break;
        }
    }
    if let Some(rest) = r.strip_prefix("const ") {
        r = rest.trim_start();
    } else if let Some(rest) = r.strip_prefix("new ") {
        r = rest.trim_start();
    }
    if r.is_empty() {
        return None;
    }
    let b = r.as_bytes();
    if is_string_start(b, 0) && skip_any_dart_string(b, 0) == b.len() {
        return Some("String".into());
    }
    if r == "true" || r == "false" {
        return Some("bool".into());
    }
    if is_int_literal(r) {
        return Some("int".into());
    }
    if is_double_literal(r) {
        return Some("double".into());
    }
    if r.starts_with("Colors.") || r.starts_with("CupertinoColors.") {
        return Some("Color".into());
    }
    if r.starts_with("Icons.") || r.starts_with("CupertinoIcons.") {
        return Some("IconData".into());
    }
    if r.starts_with('<')
        && let Some(gt) = find_matching_angle(b, 0, b.len())
    {
        let args = r[1..gt].trim();
        let after = r[gt + 1..].trim_start();
        if after.starts_with('[') {
            return Some(format!("List<{args}>"));
        }
        if after.starts_with('{') {
            let parts = split_top_level(args, b',');
            return if parts.len() == 2 {
                Some(format!("Map<{args}>"))
            } else {
                Some(format!("Set<{args}>"))
            };
        }
    }
    // Constructor call `Foo(...)` / `Foo<T>(...)` / `Foo.named(...)`
    if b[0].is_ascii_uppercase() {
        let mut idx = 1usize;
        while idx < b.len() && is_ident_char(b[idx]) {
            idx += 1;
        }
        let base_type = &r[..idx];
        let mut cursor = skip_ws_and_comments(b, idx);
        let mut generics = None;
        if cursor < b.len()
            && b[cursor] == b'<'
            && let Some(gt) = find_matching_angle(b, cursor, b.len())
        {
            generics = Some(&r[cursor..=gt]);
            cursor = skip_ws_and_comments(b, gt + 1);
        }
        if cursor < b.len() && b[cursor] == b'.' {
            cursor += 1;
            while cursor < b.len() && is_ident_char(b[cursor]) {
                cursor += 1;
            }
            cursor = skip_ws_and_comments(b, cursor);
        }
        if cursor < b.len()
            && b[cursor] == b'('
            && find_matching_delim(b, cursor, b.len(), b'(', b')') == Some(b.len() - 1)
        {
            return Some(match generics {
                Some(g) => format!("{base_type}{g}"),
                None => base_type.to_owned(),
            });
        }
    }
    None
}

fn parse_for_header_parts(
    text: &str,
    start: usize,
    end: usize,
) -> (Vec<String>, Vec<(usize, usize)>) {
    let header = &text[start..end];
    if let Some(in_rel) = find_top_level_keyword(header, "in") {
        let lhs = header[..in_rel].trim();
        let mut names = Vec::new();
        if let Some((_, n)) = parse_typed_binding(lhs, false) {
            names.push(n);
        }
        return (names, vec![(start + in_rel + 2, end)]);
    }
    let semis: Vec<usize> = header
        .bytes()
        .enumerate()
        .filter(|&(_, b)| b == b';')
        .map(|(idx, _)| start + idx)
        .collect();
    if semis.len() == 2 {
        let decls = parse_statement_declarations(&text[start..semis[0]]);
        let names = decls.into_iter().map(|(n, _)| n).collect();
        return (names, vec![(start, end)]);
    }
    (Vec::new(), vec![(start, end)])
}

fn find_for_element_end(bytes: &[u8], start: usize, limit: usize) -> usize {
    let mut i = start;
    let mut dp = 0usize;
    let mut db = 0usize;
    let mut dk = 0usize;
    while i < limit {
        let b = bytes[i];
        if is_string_start(bytes, i) {
            i = skip_any_dart_string(bytes, i).min(limit);
            continue;
        }
        match b {
            b'(' => dp += 1,
            b')' => {
                if dp == 0 {
                    return i;
                }
                dp -= 1;
            }
            b'{' => db += 1,
            b'}' => {
                if db == 0 {
                    return i;
                }
                db -= 1;
            }
            b'[' => dk += 1,
            b']' => {
                if dk == 0 {
                    return i;
                }
                dk -= 1;
            }
            b',' if dp == 0 && db == 0 && dk == 0 => return i,
            _ => {}
        }
        i += 1;
    }
    limit
}

fn find_arrow_body_end(bytes: &[u8], start: usize, limit: usize) -> usize {
    find_for_element_end(bytes, start, limit)
}

fn parse_catch_bindings(raw: &str) -> Vec<String> {
    raw.split(',')
        .filter_map(|p| {
            let t = p.trim();
            if is_simple_ident(t) {
                Some(t.to_owned())
            } else {
                None
            }
        })
        .collect()
}

fn strip_leading_annotations_and_comments(mut s: &str) -> &str {
    loop {
        s = s.trim_start();
        if let Some(rest) = s.strip_prefix("//") {
            s = rest.split_once('\n').map(|(_, tail)| tail).unwrap_or("");
            continue;
        }
        if let Some(rest) = s.strip_prefix("/*") {
            let close = skip_block_comment_bytes(s.as_bytes(), 0);
            s = &s[close..];
            let _ = rest;
            continue;
        }
        if s.starts_with('@') {
            let b = s.as_bytes();
            let mut i = 1usize;
            while i < b.len() && (is_ident_char(b[i]) || b[i] == b'.') {
                i += 1;
            }
            let after = skip_ws_and_comments(b, i);
            if after < b.len()
                && b[after] == b'('
                && let Some(close) = find_matching_delim(b, after, b.len(), b'(', b')')
            {
                s = &s[close + 1..];
                continue;
            }
            s = &s[i..];
            continue;
        }
        return s.trim();
    }
}

fn split_top_level(s: &str, sep: u8) -> Vec<&str> {
    let bytes = s.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut dp = 0usize;
    let mut db = 0usize;
    let mut dk = 0usize;
    let mut da = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if is_string_start(bytes, i) {
            i = skip_any_dart_string(bytes, i);
            continue;
        }
        match b {
            b'(' => dp += 1,
            b')' => dp = dp.saturating_sub(1),
            b'{' => db += 1,
            b'}' => db = db.saturating_sub(1),
            b'[' => dk += 1,
            b']' => dk = dk.saturating_sub(1),
            b'<' if !matches!(bytes.get(i + 1), Some(b'=' | b'<')) => da += 1,
            b'>' if da > 0 && !matches!(bytes.get(i.saturating_sub(1)), Some(b'=')) => {
                da = da.saturating_sub(1)
            }
            c if c == sep && dp == 0 && db == 0 && dk == 0 && da == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start <= s.len() {
        parts.push(&s[start..]);
    }
    parts
}

fn find_top_level_equals(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    let mut dp = 0usize;
    let mut db = 0usize;
    let mut dk = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if is_string_start(bytes, i) {
            i = skip_any_dart_string(bytes, i);
            continue;
        }
        match b {
            b'(' => dp += 1,
            b')' => dp = dp.saturating_sub(1),
            b'{' => db += 1,
            b'}' => db = db.saturating_sub(1),
            b'[' => dk += 1,
            b']' => dk = dk.saturating_sub(1),
            b'=' if dp == 0
                && db == 0
                && dk == 0
                && !matches!(bytes.get(i + 1), Some(b'=' | b'>'))
                && !matches!(
                    bytes.get(i.saturating_sub(1)),
                    Some(b'<' | b'>' | b'!' | b'=')
                ) =>
            {
                return Some(i);
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn find_top_level_arrow(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    let mut dp = 0usize;
    let mut db = 0usize;
    let mut dk = 0usize;
    while i + 1 < bytes.len() {
        let b = bytes[i];
        if is_string_start(bytes, i) {
            i = skip_any_dart_string(bytes, i);
            continue;
        }
        match b {
            b'(' => dp += 1,
            b')' => dp = dp.saturating_sub(1),
            b'{' => db += 1,
            b'}' => db = db.saturating_sub(1),
            b'[' => dk += 1,
            b']' => dk = dk.saturating_sub(1),
            b'=' if bytes[i + 1] == b'>' && dp == 0 && db == 0 && dk == 0 => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

fn find_top_level_char(s: &str, needle: u8) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    let mut dp = 0usize;
    let mut db = 0usize;
    let mut dk = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if is_string_start(bytes, i) {
            i = skip_any_dart_string(bytes, i);
            continue;
        }
        if b == needle && dp == 0 && db == 0 && dk == 0 {
            return Some(i);
        }
        match b {
            b'(' => dp += 1,
            b')' => dp = dp.saturating_sub(1),
            b'{' => db += 1,
            b'}' => db = db.saturating_sub(1),
            b'[' => dk += 1,
            b']' => dk = dk.saturating_sub(1),
            _ => {}
        }
        i += 1;
    }
    None
}

fn find_top_level_keyword(s: &str, kw: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    let mut dp = 0usize;
    let mut db = 0usize;
    let mut dk = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if is_string_start(bytes, i) {
            i = skip_any_dart_string(bytes, i);
            continue;
        }
        if dp == 0
            && db == 0
            && dk == 0
            && s[i..].starts_with(kw)
            && (i == 0 || !is_ident_char(bytes[i - 1]))
            && bytes.get(i + kw.len()).is_none_or(|c| !is_ident_char(*c))
        {
            return Some(i);
        }
        match b {
            b'(' => dp += 1,
            b')' => dp = dp.saturating_sub(1),
            b'{' => db += 1,
            b'}' => db = db.saturating_sub(1),
            b'[' => dk += 1,
            b']' => dk = dk.saturating_sub(1),
            _ => {}
        }
        i += 1;
    }
    None
}

fn find_matching_delim(
    bytes: &[u8],
    open_pos: usize,
    limit: usize,
    open: u8,
    close: u8,
) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open_pos;
    while i < limit {
        let b = bytes[i];
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            i = skip_line_comment_bytes(bytes, i);
            continue;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i = skip_block_comment_bytes(bytes, i);
            continue;
        }
        if is_string_start(bytes, i) {
            i = skip_any_dart_string(bytes, i);
            continue;
        }
        if b == open {
            depth += 1;
        } else if b == close {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

fn find_matching_angle(bytes: &[u8], start: usize, limit: usize) -> Option<usize> {
    if bytes.get(start) != Some(&b'<') {
        return None;
    }
    let mut depth = 1usize;
    let mut i = start + 1;
    while i < limit {
        match bytes[i] {
            b'<' => depth += 1,
            b'>' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            b';' | b'{' | b'}' | b'(' | b')' => return None,
            _ => {}
        }
        i += 1;
    }
    None
}

fn is_string_start(bytes: &[u8], i: usize) -> bool {
    matches!(bytes.get(i), Some(b'\'' | b'"')) || is_raw_string_start(bytes, i)
}

fn is_raw_string_start(bytes: &[u8], i: usize) -> bool {
    bytes.get(i) == Some(&b'r')
        && matches!(bytes.get(i + 1), Some(b'\'' | b'"'))
        && (i == 0 || !is_ident_char(bytes[i - 1]))
}

fn skip_any_dart_string(bytes: &[u8], start: usize) -> usize {
    let raw = bytes.get(start) == Some(&b'r');
    let q_idx = if raw { start + 1 } else { start };
    let Some(&quote) = bytes.get(q_idx) else {
        return start + 1;
    };
    let triple = bytes.get(q_idx + 1) == Some(&quote) && bytes.get(q_idx + 2) == Some(&quote);
    let mut i = q_idx + if triple { 3 } else { 1 };
    while i < bytes.len() {
        if !raw && bytes[i] == b'\\' {
            i = (i + 2).min(bytes.len());
            continue;
        }
        if !raw
            && bytes[i] == b'$'
            && bytes.get(i + 1) == Some(&b'{')
            && let Some(close_b) = find_matching_delim(bytes, i + 1, bytes.len(), b'{', b'}')
        {
            i = close_b + 1;
            continue;
        }
        if triple {
            if bytes[i] == quote
                && bytes.get(i + 1) == Some(&quote)
                && bytes.get(i + 2) == Some(&quote)
            {
                return i + 3;
            }
        } else if bytes[i] == quote {
            return i + 1;
        }
        i += 1;
    }
    bytes.len()
}

fn skip_line_comment_bytes(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 2;
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    i
}

fn skip_block_comment_bytes(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 2;
    let mut depth = 1usize;
    while i < bytes.len() && depth > 0 {
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
    i
}

fn skip_ws_and_comments(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
        } else if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
            i = skip_line_comment_bytes(bytes, i);
        } else if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i = skip_block_comment_bytes(bytes, i);
        } else {
            break;
        }
    }
    i
}

fn skip_number_bytes(bytes: &[u8], mut i: usize, limit: usize) -> usize {
    if i + 1 < limit && bytes[i] == b'0' && matches!(bytes[i + 1], b'x' | b'X') {
        i += 2;
        while i < limit && (bytes[i].is_ascii_hexdigit() || bytes[i] == b'_') {
            i += 1;
        }
        return i;
    }
    while i < limit && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
        i += 1;
    }
    if i + 1 < limit && bytes[i] == b'.' && bytes[i + 1].is_ascii_digit() {
        i += 1;
        while i < limit && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
            i += 1;
        }
    }
    if i < limit && matches!(bytes[i], b'e' | b'E') {
        i += 1;
        if i < limit && matches!(bytes[i], b'+' | b'-') {
            i += 1;
        }
        while i < limit && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
            i += 1;
        }
    }
    i
}

fn is_ident_start_byte(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_simple_ident(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().next().is_some_and(is_ident_start_byte)
        && s.bytes().all(is_ident_char)
}

fn normalize_spaces(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn type_mentions_any(ty: &str, set: &HashSet<String>) -> bool {
    set.iter().any(|tp| contains_word(ty, tp))
}

fn contains_word(haystack: &str, word: &str) -> bool {
    let bytes = haystack.as_bytes();
    let mut start = 0usize;
    while let Some(rel) = haystack[start..].find(word) {
        let pos = start + rel;
        let end = pos + word.len();
        let left_ok = pos == 0 || !is_ident_char(bytes[pos - 1]);
        let right_ok = end == bytes.len() || !is_ident_char(bytes[end]);
        if left_ok && right_ok {
            return true;
        }
        start = pos + 1;
    }
    false
}

fn is_int_literal(s: &str) -> bool {
    let s = s.strip_prefix(['+', '-']).unwrap_or(s).trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit() || b == b'_');
    }
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit() || b == b'_')
}

fn is_double_literal(s: &str) -> bool {
    let s = s.strip_prefix(['+', '-']).unwrap_or(s).trim();
    if s.starts_with("0x") || s.starts_with("0X") || s.is_empty() {
        return false;
    }
    if let Some((a, b)) = s.split_once('.') {
        !a.is_empty()
            && a.bytes().all(|c| c.is_ascii_digit() || c == b'_')
            && !b.is_empty()
            && b.bytes()
                .all(|c| c.is_ascii_digit() || c == b'_' || matches!(c, b'e' | b'E' | b'+' | b'-'))
    } else {
        s.contains(['e', 'E'])
            && s.bytes()
                .all(|c| c.is_ascii_digit() || c == b'_' || matches!(c, b'e' | b'E' | b'+' | b'-'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDGET_FILE: &str = "\
class HomePage extends StatelessWidget {
  const HomePage({super.key});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: Center(
        child: Text(
          'Hello',
        ),
      ),
    );
  }
}
";

    #[test]
    fn emits_closing_labels_for_multiline_widgets() {
        let labels = closing_labels(WIDGET_FILE, 2);
        let texts: Vec<_> = labels.iter().map(|l| l.label.as_str()).collect();
        assert!(texts.contains(&" // Text"));
        assert!(texts.contains(&" // Center"));
        assert!(texts.contains(&" // Scaffold"));

        let freezed = "\
class AppSourceAGIChat {
  const factory AppSourceAGIChat({
    required bool enabled,
    required String title,
  }) = _AppSourceAGIChat;
}
";
        assert!(closing_labels(freezed, 2).is_empty());
    }

    #[test]
    fn wraps_unwraps_extracts_and_converts_widgets() {
        let span = widget_at_cursor(WIDGET_FILE, TextPos::new(7, 16)).unwrap();
        assert_eq!(span.name, "Text");
        let wrapped = wrap_with_widget(WIDGET_FILE, &span, WrapKind::Padding);
        assert!(wrapped.contains("Padding(") && wrapped.contains("padding: const EdgeInsets.all(8.0)"));

        let center_span = widget_at_cursor(WIDGET_FILE, TextPos::new(6, 13)).unwrap();
        assert_eq!(center_span.name, "Center");
        let unwrapped = remove_widget_wrapper(WIDGET_FILE, &center_span).unwrap();
        assert!(unwrapped.starts_with("Text("));

        let (call, def) = extract_flutter_widget(WIDGET_FILE, &span, "GreetingText").unwrap();
        assert_eq!(call, "const GreetingText()");
        assert!(def.contains("class GreetingText extends StatelessWidget"));

        let (_, stateful) = convert_stateless_to_stateful(WIDGET_FILE, 0).unwrap();
        assert!(stateful.contains("extends StatefulWidget"));
        assert!(stateful.contains("class _HomePageState extends State<HomePage>"));
        let (_, stateless) = convert_stateful_to_stateless(&stateful, 0).unwrap();
        assert!(stateless.contains("class HomePage extends StatelessWidget"));
    }

    #[test]
    fn extract_widget_captures_free_variables_and_handles_interpolations_and_ternaries() {
        let src = "\
class Home extends StatelessWidget {
  const Home({super.key, required this.title});
  final String title;

  @override
  Widget build(BuildContext context) {
    final message = 'hello';
    final count = 3;
    final bool flag = computeFlag();
    return Text(flag ? '$message ($title: ${count + 1})' : 'fallback');
  }
}
";
        let span = widget_at_cursor(src, TextPos::new(9, 13)).unwrap();
        let (call, def) = extract_flutter_widget(src, &span, "Greeting").unwrap();
        assert_eq!(
            call,
            "Greeting(flag: flag, message: message, title: title, count: count)"
        );
        assert!(def.contains("required this.flag,"));
        assert!(def.contains("required this.message,"));
        assert!(def.contains("required this.title,"));
        assert!(def.contains("required this.count,"));
        assert!(def.contains("final bool flag;"));
        assert!(def.contains("final String message;"));
        assert!(def.contains("final String title;"));
        assert!(def.contains("final int count;"));
    }

    #[test]
    fn extract_widget_handles_arrow_methods_closures_and_private_fields() {
        let arrow_src = "\
class Home extends StatelessWidget {
  const Home({super.key, required this.title});
  final String title;

  @override
  Widget build(BuildContext context) => Text(title);
}
";
        let span = widget_at_cursor(arrow_src, TextPos::new(5, 42)).unwrap();
        let (call, def) = extract_flutter_widget(arrow_src, &span, "TitleText").unwrap();
        assert_eq!(call, "TitleText(title: title)");
        assert!(def.contains("final String title;"));
        assert!(def.contains("const TitleText({super.key, required this.title});"));

        let closure_src = "\
class ListPage extends StatelessWidget {
  final int _offset = 1;
  @override
  Widget build(BuildContext context) {
    final items = <String>['a', 'b'];
    return Column(
      children: items.map((item) => Text('$item $_offset')).toList(),
    );
  }
}
";
        let col_span = widget_at_cursor(closure_src, TextPos::new(5, 13)).unwrap();
        let (call, def) = extract_flutter_widget(closure_src, &col_span, "ItemsColumn").unwrap();
        assert_eq!(call, "ItemsColumn(_offset, items: items)");
        assert!(def.contains("const ItemsColumn(this._offset, {super.key, required this.items});"));
        assert!(def.contains("final List<String> items;"));
        assert!(def.contains("final int _offset;"));
        assert!(!def.contains("this.item,") && !def.contains("final dynamic item;"));
    }

    #[test]
    fn extract_widget_declines_unsafe_captures() {
        // Untyped non-literal local variable -> decline
        let untyped = "\
class Home extends StatelessWidget {
  @override
  Widget build(BuildContext context) {
    final message = compute();
    return Text(message);
  }
}
";
        let span = widget_at_cursor(untyped, TextPos::new(4, 13)).unwrap();
        assert!(matches!(
            extract_flutter_widget(untyped, &span, "Greeting"),
            Err(ExtractError::Unsafe(_))
        ));

        // `this` / `widget` / `setState` / class method / mutation / generic T -> decline
        for (needle, src) in [
            ("Text(", "class H extends StatelessWidget { final String x = ''; @override Widget build(BuildContext c) => Text(this.x); }"),
            ("Text(", "class S extends State<H> { @override Widget build(BuildContext c) => Text(widget.x); }"),
            ("ElevatedButton(", "class S extends State<H> { @override Widget build(BuildContext c) => ElevatedButton(onPressed: () => setState(() {}), child: const Text('a')); }"),
            ("Text(", "class H extends StatelessWidget { String f() => 'a'; @override Widget build(BuildContext c) => Text(f()); }"),
            ("ElevatedButton(", "class H extends StatelessWidget { @override Widget build(BuildContext c) { var n = 0; return ElevatedButton(onPressed: () => n++, child: const Text('a')); } }"),
            ("Text(", "class Box<T> extends StatelessWidget { final T val; const Box(this.val); @override Widget build(BuildContext c) => Text('$val'); }"),
            ("Text(", "class H extends StatelessWidget { @override Widget build(BuildContext c) => Text(missing); }"),
        ] {
            let line = 0u32;
            let col = src.find(needle).unwrap() as u32;
            let s = widget_at_cursor(src, TextPos::new(line, col + 1)).unwrap();
            assert!(
                matches!(can_extract_flutter_widget(src, &s), Err(ExtractError::Unsafe(_))),
                "expected decline for: {src}"
            );
        }
    }
}
