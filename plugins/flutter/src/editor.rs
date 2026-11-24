use crate::highlighting::{setting_bool, setting_u32};
use dart_core::TextPos;
use dart_core::dart_syntax::{RunnableKind, find_runnables};
use dart_core::flutter_catalog::{
    CUPERTINO_COLORS, FLUTTER_ICONS, MATERIAL_COLORS, find_flutter_colors, find_flutter_icons,
    format_color_presentations,
};
use dart_core::flutter_widgets::{
    ExtractError, WrapKind, can_extract_flutter_widget, closing_labels,
    convert_stateful_to_stateless, convert_stateless_to_stateful, extract_flutter_widget,
    remove_widget_wrapper, widget_at_cursor, wrap_with_widget,
};
use dart_core::pubspec::Pubspec;
use openidea_plugin::diagnostics::DiagnosticsProvider;
use openidea_plugin::file_actions::{FileAction, FileActions, FileOperation};
use openidea_plugin::hover::{Hover, HoverProvider};
use openidea_plugin::inlay_hints::{InlayHint, InlayHints, InlayKind};
use openidea_plugin::language_features::{
    CodeLens, ColorInfo, Document as LfDoc, DocumentLink, GutterAction, LanguageFeatures,
};
use openidea_plugin::providers::{
    CodeAction, CompletionItem, CompletionKind, Document, LineMarker, Providers,
};
use openidea_plugin::{Diagnostic, Error, Location, Position, Range, Severity, TextEdit, host};

fn is_dart(path: &str, lang: &str) -> bool {
    lang == "dart" || path.ends_with(".dart")
}

fn is_pubspec(path: &str) -> bool {
    path.ends_with("/pubspec.yaml") || path == "pubspec.yaml" || path.ends_with("\\pubspec.yaml")
}

fn to_range(span: dart_core::TextSpan) -> Range {
    Range::new(
        Position::new(span.start.line, span.start.character),
        Position::new(span.end.line, span.end.character),
    )
}

fn pos_in_span(at: Position, span: dart_core::TextSpan) -> bool {
    at.line == span.start.line
        && at.character >= span.start.character
        && at.character <= span.end.character
}

impl InlayHints for crate::FlutterPlugin {
    fn inlay_hints(&mut self, doc: &openidea_plugin::inlay_hints::Document, range: Range) -> Vec<InlayHint> {
        if !setting_bool("closingLabels", true) || !is_dart(&doc.path, &doc.language) {
            return Vec::new();
        }
        let min_lines = setting_u32("closingLabelsMinLines", 2);
        closing_labels(&doc.text, min_lines)
            .into_iter()
            .filter(|l| l.pos.line >= range.start.line && l.pos.line <= range.end.line)
            .map(|l| {
                InlayHint::new(Position::new(l.pos.line, l.pos.character), l.label)
                    .kind(InlayKind::Other)
                    .tooltip(l.tooltip)
                    .padding_left()
            })
            .collect()
    }
}

impl LanguageFeatures for crate::FlutterPlugin {
    fn colors(&mut self, doc: &LfDoc) -> Vec<ColorInfo> {
        if !setting_bool("colorPreviews", true) || !is_dart(&doc.path, &doc.language) {
            return Vec::new();
        }
        find_flutter_colors(&doc.text)
            .into_iter()
            .map(|c| {
                let (r, g, b, a) = c.rgba_f32();
                ColorInfo {
                    range: to_range(c.span),
                    red: r,
                    green: g,
                    blue: b,
                    alpha: a,
                }
            })
            .collect()
    }

    fn color_presentations(&mut self, _doc: &LfDoc, color: ColorInfo) -> Vec<String> {
        format_color_presentations(color.red, color.green, color.blue, color.alpha)
    }

    fn gutter_actions(&mut self, doc: &LfDoc) -> Vec<GutterAction> {
        if !is_dart(&doc.path, &doc.language) {
            return Vec::new();
        }
        let mut actions = Vec::new();
        for r in find_runnables(&doc.text) {
            if r.kind == RunnableKind::TestWidget || r.kind == RunnableKind::Group {
                let arg = r
                    .test_name
                    .map(|n| format!("name:{n}"))
                    .unwrap_or_else(|| doc.path.clone());
                actions.push(GutterAction {
                    line: r.line,
                    icon: Some("run".into()),
                    tooltip: r.label,
                    command: "run-widget-test".into(),
                    argument: Some(arg),
                });
            }
        }
        actions
    }

    fn code_lenses(&mut self, doc: &LfDoc) -> Vec<CodeLens> {
        if is_pubspec(&doc.path) {
            let spec = Pubspec::parse(&doc.text);
            if !spec.is_flutter_project() {
                return Vec::new();
            }
            let line = spec.flutter_section_line.unwrap_or(0);
            let range = Range::new(Position::new(line, 0), Position::new(line, 0));
            return vec![
                CodeLens {
                    range,
                    title: "Flutter pub get".into(),
                    command: "pub-get".into(),
                    argument: None,
                },
                CodeLens {
                    range,
                    title: "Flutter clean".into(),
                    command: "clean".into(),
                    argument: None,
                },
                CodeLens {
                    range,
                    title: "Flutter doctor".into(),
                    command: "doctor".into(),
                    argument: None,
                },
            ];
        }
        if is_dart(&doc.path, &doc.language) {
            return find_runnables(&doc.text)
                .into_iter()
                .filter(|r| r.kind == RunnableKind::TestWidget)
                .map(|r| CodeLens {
                    range: Range::new(Position::new(r.line, 0), Position::new(r.line, 0)),
                    title: "Run widget test".into(),
                    command: "run-widget-test".into(),
                    argument: Some(
                        r.test_name
                            .map(|n| format!("name:{n}"))
                            .unwrap_or_else(|| doc.path.clone()),
                    ),
                })
                .collect();
        }
        Vec::new()
    }

    fn document_links(&mut self, doc: &LfDoc) -> Vec<DocumentLink> {
        if !is_pubspec(&doc.path) {
            return Vec::new();
        }
        let Some(project) = host::project_of(&doc.path) else {
            return Vec::new();
        };
        let Ok(root_owned) = project.root() else {
            return Vec::new();
        };
        let root = root_owned.trim_end_matches('/');
        let spec = Pubspec::parse(&doc.text);
        spec.assets
            .into_iter()
            .filter_map(|a| {
                let rel = a.value.trim_start_matches('/');
                if project.exists(rel).unwrap_or(false) {
                    Some(DocumentLink {
                        range: to_range(a.span),
                        target: format!("{root}/{rel}"),
                    })
                } else {
                    None
                }
            })
            .collect()
    }
}

impl HoverProvider for crate::FlutterPlugin {
    fn hover(&mut self, doc: &Document, at: Position) -> Option<Hover> {
        if !is_dart(&doc.path, &doc.language) {
            return None;
        }
        if setting_bool("colorPreviews", true) {
            for c in find_flutter_colors(&doc.text) {
                if pos_in_span(at, c.span) {
                    let (r, g, b, a) = c.rgba_f32();
                    let md = format!(
                        "**Flutter Color** `{}`\n\n- ARGB: `0x{:08X}`\n- RGB: `({}, {}, {})` · Alpha: `{:.0}%`",
                        c.label,
                        c.argb,
                        (r * 255.0).round() as u8,
                        (g * 255.0).round() as u8,
                        (b * 255.0).round() as u8,
                        a * 100.0
                    );
                    return Some(Hover::new(md).range(to_range(c.span)));
                }
            }
        }
        if setting_bool("iconPreviews", true) {
            for icon in find_flutter_icons(&doc.text) {
                if pos_in_span(at, icon.span) {
                    let md = match icon.entry {
                        Some(e) => format!(
                            "**{} Icon** `{}`\n\n- Codepoint: `U+{:04X}`\n- Category: {}",
                            e.family, icon.full_name, e.codepoint, e.category
                        ),
                        None => format!("**Flutter Icon** `{}`", icon.full_name),
                    };
                    return Some(Hover::new(md).range(to_range(icon.span)));
                }
            }
        }
        None
    }
}

impl Providers for crate::FlutterPlugin {
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        if is_pubspec(&doc.path) {
            let line = doc.text.lines().nth(at.line as usize).unwrap_or("");
            if line.trim_start().starts_with('-')
                && let Some(project) = host::project_of(&doc.path)
                && let Ok(files) = project.find_files("assets/**", 40)
            {
                return files
                    .into_iter()
                    .map(|f| {
                        let mut item = CompletionItem::new(&f, CompletionKind::File);
                        item.detail = Some("Flutter asset".into());
                        item
                    })
                    .collect();
            }
            return [
                ("flutter", "flutter:\n  uses-material-design: true\n  assets:\n    - $0"),
                ("uses-material-design", "uses-material-design: true$0"),
                ("assets", "assets:\n    - assets/$0"),
                ("fonts", "fonts:\n    - family: ${1:FontName}\n      fonts:\n        - asset: assets/fonts/${2:font}.ttf$0"),
            ]
            .into_iter()
            .map(|(label, snip)| {
                let mut item = CompletionItem::new(label, CompletionKind::Property);
                item.insert_text = snip.into();
                item.snippet = true;
                item.detail = Some("Flutter pubspec key".into());
                item
            })
            .collect();
        }

        if !is_dart(&doc.path, &doc.language) {
            return Vec::new();
        }
        let line = doc.text.lines().nth(at.line as usize).unwrap_or("");
        let byte_col = dart_core::utf16_col_to_byte(line, at.character);
        let before = &line[..byte_col.min(line.len())];

        if before.ends_with("Colors.") {
            return MATERIAL_COLORS
                .iter()
                .map(|(name, argb)| {
                    let mut item = CompletionItem::new(name, CompletionKind::Color);
                    item.detail = Some(format!("#{argb:08X}"));
                    item
                })
                .collect();
        }
        if before.ends_with("CupertinoColors.") {
            return CUPERTINO_COLORS
                .iter()
                .map(|(name, argb)| {
                    let mut item = CompletionItem::new(name, CompletionKind::Color);
                    item.detail = Some(format!("#{argb:08X}"));
                    item
                })
                .collect();
        }
        if before.ends_with("Icons.") && setting_bool("iconPreviews", true) {
            return FLUTTER_ICONS
                .iter()
                .filter(|e| e.family == "Material")
                .map(|e| {
                    let mut item = CompletionItem::new(e.name, CompletionKind::EnumMember);
                    item.detail = Some(format!("Material Icon · U+{:04X}", e.codepoint));
                    item
                })
                .collect();
        }
        if before.ends_with("CupertinoIcons.") && setting_bool("iconPreviews", true) {
            return FLUTTER_ICONS
                .iter()
                .filter(|e| e.family == "Cupertino")
                .map(|e| {
                    let mut item = CompletionItem::new(e.name, CompletionKind::EnumMember);
                    item.detail = Some(format!("Cupertino Icon · U+{:04X}", e.codepoint));
                    item
                })
                .collect();
        }
        Vec::new()
    }

    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        if !setting_bool("widgetCodeActions", true) || !is_dart(&doc.path, &doc.language) {
            return Vec::new();
        }
        let mut actions = Vec::new();
        let pos = TextPos::new(selection.start.line, selection.start.character);

        if let Some(span) = widget_at_cursor(&doc.text, pos) {
            if can_extract_flutter_widget(&doc.text, &span).is_ok() {
                // Deferred Extract Widget (uses host::input in resolve_code_action)
                actions.push(CodeAction {
                    title: format!("Extract Flutter Widget ({})...", span.name),
                    kind: Some("refactor".into()),
                    preferred: false,
                    edits: Vec::new(),
                    data: Some(format!("extract:{}:{}", selection.start.line, selection.start.character)),
                });
            }

            if let Some(unwrapped) = remove_widget_wrapper(&doc.text, &span) {
                actions.push(CodeAction {
                    title: format!("Remove {} widget", span.name),
                    kind: Some("refactor".into()),
                    preferred: false,
                    edits: vec![(
                        doc.path.clone(),
                        vec![TextEdit {
                            range: to_range(span.span),
                            text: unwrapped,
                        }],
                    )],
                    data: None,
                });
            }

            for kind in WrapKind::ALL {
                actions.push(CodeAction {
                    title: kind.title().into(),
                    kind: Some("refactor".into()),
                    preferred: matches!(kind, WrapKind::Container | WrapKind::Padding | WrapKind::Center),
                    edits: vec![(
                        doc.path.clone(),
                        vec![TextEdit {
                            range: to_range(span.span),
                            text: wrap_with_widget(&doc.text, &span, kind),
                        }],
                    )],
                    data: None,
                });
            }
        }

        if let Some((span, text)) = convert_stateless_to_stateful(&doc.text, selection.start.line) {
            actions.push(CodeAction {
                title: "Convert to StatefulWidget".into(),
                kind: Some("refactor".into()),
                preferred: false,
                edits: vec![(
                    doc.path.clone(),
                    vec![TextEdit {
                        range: to_range(span),
                        text,
                    }],
                )],
                data: None,
            });
        } else if let Some((span, text)) = convert_stateful_to_stateless(&doc.text, selection.start.line) {
            actions.push(CodeAction {
                title: "Convert to StatelessWidget".into(),
                kind: Some("refactor".into()),
                preferred: false,
                edits: vec![(
                    doc.path.clone(),
                    vec![TextEdit {
                        range: to_range(span),
                        text,
                    }],
                )],
                data: None,
            });
        }

        actions
    }

    fn resolve_code_action(&mut self, doc: &Document, mut action: CodeAction) -> Result<CodeAction, Error> {
        let Some(data) = action.data.take() else {
            return Ok(action);
        };
        if let Some(coords) = data.strip_prefix("extract:")
            && let Some((line_s, col_s)) = coords.split_once(':')
            && let (Ok(line), Ok(col)) = (line_s.parse::<u32>(), col_s.parse::<u32>())
            && let Some(span) = widget_at_cursor(&doc.text, TextPos::new(line, col))
        {
            let name = host::input("New StatelessWidget name", Some("ExtractedWidget"))?
                .ok_or(Error::Cancelled)?;
            let (call, class_def) = extract_flutter_widget(&doc.text, &span, &name).map_err(|e| match e {
                ExtractError::InvalidName => Error::Invalid("Widget name must be PascalCase".into()),
                ExtractError::Unsafe(reason) => {
                    Error::Failed(format!("Cannot safely extract widget: {reason}"))
                }
            })?;
            let lines: Vec<&str> = doc.text.split('\n').collect();
            let last_line = lines.len().saturating_sub(1);
            let last_col =
                dart_core::utf16_len(lines[last_line].strip_suffix('\r').unwrap_or(lines[last_line]));
            let eof = Position::new(last_line as u32, last_col);
            action.edits = vec![(
                doc.path.clone(),
                vec![
                    TextEdit {
                        range: to_range(span.span),
                        text: call,
                    },
                    TextEdit {
                        range: Range::new(eof, eof),
                        text: class_def,
                    },
                ],
            )];
        }
        Ok(action)
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        let Some(project) = host::project_of(&doc.path) else {
            return Vec::new();
        };
        let Ok(root_owned) = project.root() else {
            return Vec::new();
        };
        let root = root_owned.trim_end_matches('/');
        if is_pubspec(&doc.path) {
            let spec = Pubspec::parse(&doc.text);
            for asset in spec.assets {
                if pos_in_span(at, asset.span) {
                    let rel = asset.value.trim_start_matches('/');
                    if project.exists(rel).unwrap_or(false) {
                        return vec![Location {
                            path: format!("{root}/{rel}"),
                            range: Range::default(),
                        }];
                    }
                }
            }
        } else if is_dart(&doc.path, &doc.language) {
            let line = doc.text.lines().nth(at.line as usize).unwrap_or("");
            if let Some(asset) = quoted_asset_at(line, at.character)
                && project.exists(asset).unwrap_or(false)
            {
                return vec![Location {
                    path: format!("{root}/{asset}"),
                    range: Range::default(),
                }];
            }
        }
        Vec::new()
    }

    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        if !is_dart(&doc.path, &doc.language) || !setting_bool("iconPreviews", true) {
            return Vec::new();
        }
        let mut markers = Vec::new();
        for icon in find_flutter_icons(&doc.text) {
            let tooltip = match icon.entry {
                Some(e) => format!("{} icon {} (U+{:04X})", e.family, icon.full_name, e.codepoint),
                None => format!("Flutter icon {}", icon.full_name),
            };
            markers.push(LineMarker {
                line: icon.span.start.line,
                icon: Some("flutter-widget".into()),
                tooltip,
                target: None,
            });
        }
        markers
    }
}

impl DiagnosticsProvider for crate::FlutterPlugin {
    fn diagnostics(&mut self, doc: &Document) -> Vec<Diagnostic> {
        let Some(project) = host::project_of(&doc.path) else {
            return Vec::new();
        };
        if is_dart(&doc.path, &doc.language)
            && doc.text.contains("Icons.")
            && let Ok(pub_yaml) = project.read_file("pubspec.yaml")
        {
            let spec = Pubspec::parse(&pub_yaml);
            if spec.is_flutter_project()
                && spec.uses_material_design.is_some_and(|(enabled, _)| !enabled)
                && let Some(first_icon) = find_flutter_icons(&doc.text).into_iter().find(|i| i.full_name.starts_with("Icons."))
            {
                let mut diag = Diagnostic::new(
                    to_range(first_icon.span),
                    Severity::Warning,
                    "Material `Icons.*` used while `flutter.uses-material-design` is false in pubspec.yaml",
                );
                diag.code = Some("uses_material_design_false".into());
                diag.source = Some("flutter".into());
                return vec![diag];
            }
        }
        Vec::new()
    }
}

impl FileActions for crate::FlutterPlugin {
    fn file_actions(&mut self, doc: &Document, _selection: Range) -> Vec<FileAction> {
        if !is_dart(&doc.path, &doc.language) || doc.path.ends_with("_test.dart") {
            return Vec::new();
        }
        let Some(project) = host::project_of(&doc.path) else {
            return Vec::new();
        };
        let Ok(yaml) = project.read_file("pubspec.yaml") else {
            return Vec::new();
        };
        let spec = Pubspec::parse(&yaml);
        if !spec.is_flutter_project() {
            return Vec::new();
        }
        let Ok(root_owned) = project.root() else {
            return Vec::new();
        };
        let root = root_owned.trim_end_matches('/');
        let rel = doc.path.strip_prefix(root).unwrap_or(&doc.path).trim_start_matches('/');
        let Some(inner) = rel.strip_prefix("lib/") else {
            return Vec::new();
        };
        let test_rel = format!("test/{}_widget_test.dart", inner.trim_end_matches(".dart"));
        if project.exists(&test_rel).unwrap_or(false) {
            return Vec::new();
        }
        let pkg = spec.name.map(|(n, _)| n).unwrap_or_else(|| "app".into());
        let stub = format!(
            "import 'package:flutter/material.dart';\nimport 'package:flutter_test/flutter_test.dart';\nimport 'package:{pkg}/{inner}';\n\nvoid main() {{\n  testWidgets('renders', (WidgetTester tester) async {{\n    await tester.pumpWidget(const MaterialApp(home: Scaffold()));\n  }});\n}}\n"
        );
        vec![FileAction {
            title: format!("Create Flutter widget test ({test_rel})"),
            kind: Some("refactor".into()),
            preferred: false,
            operations: vec![FileOperation::Create {
                path: test_rel.clone(),
                text: stub,
                overwrite: false,
            }],
            edits: Vec::new(),
            open: Some(format!("{root}/{test_rel}")),
            data: None,
        }]
    }
}

fn quoted_asset_at(line: &str, col: u32) -> Option<&str> {
    let byte_col = dart_core::utf16_col_to_byte(line, col);
    for (start, b) in line.bytes().enumerate() {
        if (b == b'\'' || b == b'"')
            && let Some(rel_end) = line[start + 1..].find(b as char)
        {
            let end = start + 1 + rel_end;
            if byte_col >= start && byte_col <= end {
                let candidate = &line[start + 1..end];
                if candidate.starts_with("assets/") || candidate.starts_with("packages/") {
                    return Some(candidate);
                }
            }
        }
    }
    None
}
