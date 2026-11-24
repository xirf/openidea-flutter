//! Flutter widget tree scanner, closing labels, wrap/unwrap/extract, and
//! StatelessWidget <-> StatefulWidget conversions.

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

/// Extracts `span` into a new `StatelessWidget` class appended to the file.
/// Returns `(call_replacement, class_definition_to_append)`.
pub fn extract_flutter_widget(text: &str, span: &WidgetSpan, widget_name: &str) -> Option<(String, String)> {
    let clean_name = widget_name.trim();
    if clean_name.is_empty()
        || !clean_name.bytes().next()?.is_ascii_uppercase()
        || !clean_name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return None;
    }
    let expr = &text[span.start_byte..=span.close_paren_byte];
    let expr_indent = line_indent_at(text, span.start_byte);
    let target_indent = "    ";
    let body_expr = reindent_to(expr, &expr_indent, target_indent);
    let call = format!("const {clean_name}()");
    let class_def = format!(
        "\n\nclass {clean_name} extends StatelessWidget {{\n  const {clean_name}({{super.key}});\n\n  @override\n  Widget build(BuildContext context) {{\n    return {body_expr};\n  }}\n}}\n"
    );
    Some((call, class_def))
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
}
