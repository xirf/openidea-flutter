use crate::highlighting::setting_bool;
use dart_core::dart_syntax::{DartFoldKind, RunnableKind, find_directives, find_fold_ranges, find_runnables};
use dart_core::pubspec::{DiagnosticLevel, Pubspec, sort_dependencies_edit, validate_pubspec};
use openidea_plugin::diagnostics::DiagnosticsProvider;
use openidea_plugin::file_actions::{FileAction, FileActions, FileOperation};
use openidea_plugin::hover::{Hover, HoverProvider};
use openidea_plugin::language_features::{
    CodeLens, Document as LfDoc, DocumentLink, FoldRange, GutterAction, LanguageFeatures,
};
use openidea_plugin::providers::{
    CodeAction, CompletionItem, CompletionKind, Document, LineMarker, Providers,
};
use openidea_plugin::{Diagnostic, Location, Position, Range, Severity, TextEdit, host};

fn is_pubspec(path: &str) -> bool {
    path.ends_with("/pubspec.yaml") || path == "pubspec.yaml" || path.ends_with("\\pubspec.yaml")
}

fn is_dart(doc: &Document) -> bool {
    doc.language == "dart" || doc.path.ends_with(".dart")
}

fn to_range(span: dart_core::TextSpan) -> Range {
    Range::new(
        Position::new(span.start.line, span.start.character),
        Position::new(span.end.line, span.end.character),
    )
}

fn resolve_relative_project_path(doc_path: &str, rel: &str) -> Option<(String, String)> {
    let project = host::project_of(doc_path)?;
    let root_owned = project.root().ok()?;
    let root = root_owned.trim_end_matches('/');
    let dir = doc_path.rsplit_once(['/', '\\']).map(|(d, _)| d).unwrap_or(root);
    let candidate = format!("{dir}/{rel}");
    let rel_to_root = candidate
        .strip_prefix(root)
        .unwrap_or(&candidate)
        .trim_start_matches('/');
    if project.exists(rel_to_root).unwrap_or(false) {
        Some((format!("{root}/{rel_to_root}"), rel_to_root.to_owned()))
    } else if project.exists(rel).unwrap_or(false) {
        Some((format!("{root}/{}", rel.trim_start_matches('/')), rel.to_owned()))
    } else {
        None
    }
}

impl Providers for crate::DartPlugin {
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        if !is_pubspec(&doc.path) {
            return Vec::new();
        }
        let line_text = doc.text.lines().nth(at.line as usize).unwrap_or("");
        let trimmed = line_text.trim_start();
        if trimmed.is_empty() || !trimmed.contains(':') {
            return [
                ("dependencies", "dependencies:\n  $0"),
                ("dev_dependencies", "dev_dependencies:\n  $0"),
                ("dependency_overrides", "dependency_overrides:\n  $0"),
                ("environment", "environment:\n  sdk: ^3.5.0$0"),
                ("sdk", "sdk: flutter$0"),
                ("path", "path: ../$0"),
                ("git", "git:\n      url: $0"),
            ]
            .into_iter()
            .map(|(label, snippet)| {
                let mut item = CompletionItem::new(label, CompletionKind::Property);
                item.insert_text = snippet.into();
                item.snippet = true;
                item.detail = Some("pubspec.yaml key".into());
                item
            })
            .collect();
        }
        Vec::new()
    }

    fn code_actions(&mut self, doc: &Document, _selection: Range) -> Vec<CodeAction> {
        let mut actions = Vec::new();
        if is_pubspec(&doc.path)
            && let Some((span, replacement)) = sort_dependencies_edit(&doc.text)
        {
            actions.push(CodeAction {
                title: "Sort pubspec.yaml dependencies alphabetically".into(),
                kind: Some("quickfix".into()),
                preferred: true,
                edits: vec![(
                    doc.path.clone(),
                    vec![TextEdit {
                        range: to_range(span),
                        text: replacement,
                    }],
                )],
                data: None,
            });
        }
        actions
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        if is_pubspec(&doc.path) {
            let spec = Pubspec::parse(&doc.text);
            for dep in spec.dependencies {
                if let (Some(path), Some(span)) = (&dep.path, dep.path_span)
                    && span.start.line == at.line
                    && at.character >= span.start.character
                    && at.character <= span.end.character
                    && let Some((abs, _)) = resolve_relative_project_path(&doc.path, &format!("{path}/pubspec.yaml"))
                {
                    return vec![Location {
                        path: abs,
                        range: Range::default(),
                    }];
                }
            }
        } else if is_dart(doc) {
            for link in find_directives(&doc.text) {
                if link.span.start.line == at.line
                    && at.character >= link.span.start.character
                    && at.character <= link.span.end.character
                    && !link.uri.contains(':')
                    && let Some((abs, _)) = resolve_relative_project_path(&doc.path, &link.uri)
                {
                    return vec![Location {
                        path: abs,
                        range: Range::default(),
                    }];
                }
            }
        }
        Vec::new()
    }

    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        if !is_dart(doc) {
            return Vec::new();
        }
        find_runnables(&doc.text)
            .into_iter()
            .map(|r| LineMarker {
                line: r.line,
                icon: Some("run".into()),
                tooltip: r.label,
                target: None,
            })
            .collect()
    }
}

impl HoverProvider for crate::DartPlugin {
    fn hover(&mut self, doc: &Document, at: Position) -> Option<Hover> {
        if !is_pubspec(&doc.path) {
            return None;
        }
        let spec = Pubspec::parse(&doc.text);
        let dep = spec
            .dependencies
            .into_iter()
            .find(|d| d.line == at.line && at.character >= d.key_span.start.character && at.character <= d.key_span.end.character)?;
        let mut md = format!("**{}** (`{}`)", dep.name, dep.section.as_str());
        if let Some(c) = &dep.constraint {
            md.push_str(&format!("\n\nConstraint: `{c}`"));
        }
        if let Some(p) = &dep.path {
            md.push_str(&format!("\n\nLocal path: `{p}`"));
        }
        if let Some(out) = self.outdated.get(&dep.name) {
            md.push_str(&format!(
                "\n\nCurrent: `{}` · Upgradable: `{}` · Latest: `{}`",
                out.current.as_deref().unwrap_or("-"),
                out.upgradable.as_deref().unwrap_or("-"),
                out.latest.as_deref().unwrap_or("-")
            ));
        }
        Some(Hover::new(md).range(to_range(dep.key_span)))
    }
}

impl DiagnosticsProvider for crate::DartPlugin {
    fn diagnostics(&mut self, doc: &Document) -> Vec<Diagnostic> {
        if !is_pubspec(&doc.path) || !setting_bool("pubspecDiagnostics", true) {
            return Vec::new();
        }
        let spec = Pubspec::parse(&doc.text);
        let project = host::project_of(&doc.path);
        let dir = doc.path.rsplit_once(['/', '\\']).map(|(d, _)| d.to_owned());
        validate_pubspec(&spec, |rel| {
            if let (Some(proj), Some(base)) = (&project, &dir) {
                let root_owned = proj.root().unwrap_or_default();
                let root = root_owned.trim_end_matches('/');
                let candidate = format!("{base}/{rel}");
                let norm = candidate.strip_prefix(root).unwrap_or(&candidate).trim_start_matches('/');
                proj.exists(norm).unwrap_or(true) || proj.exists(rel).unwrap_or(true)
            } else {
                true
            }
        })
        .into_iter()
        .map(|d| {
            let sev = match d.level {
                DiagnosticLevel::Error => Severity::Error,
                DiagnosticLevel::Warning => Severity::Warning,
                DiagnosticLevel::Info => Severity::Info,
            };
            let mut diag = Diagnostic::new(to_range(d.span), sev, d.message);
            diag.code = Some(d.code.into());
            diag.source = Some("pubspec".into());
            diag
        })
        .collect()
    }
}

impl FileActions for crate::DartPlugin {
    fn file_actions(&mut self, doc: &Document, _selection: Range) -> Vec<FileAction> {
        if !is_dart(doc) || doc.path.ends_with("_test.dart") {
            return Vec::new();
        }
        let Some(project) = host::project_of(&doc.path) else {
            return Vec::new();
        };
        let Ok(root_owned) = project.root() else {
            return Vec::new();
        };
        let root = root_owned.trim_end_matches('/');
        let rel = doc.path.strip_prefix(root).unwrap_or(&doc.path).trim_start_matches('/');
        let Some(inner) = rel.strip_prefix("lib/") else {
            return Vec::new();
        };
        let test_rel = format!("test/{}_test.dart", inner.trim_end_matches(".dart"));
        if project.exists(&test_rel).unwrap_or(false) {
            return Vec::new();
        }
        let pkg = project
            .read_file("pubspec.yaml")
            .ok()
            .and_then(|y| Pubspec::parse(&y).name.map(|(n, _)| n))
            .unwrap_or_else(|| "app".into());
        let stub = format!(
            "import 'package:test/test.dart';\nimport 'package:{pkg}/{inner}';\n\nvoid main() {{\n  test('works', () {{\n    // TODO: implement test\n  }});\n}}\n"
        );
        let abs = format!("{root}/{test_rel}");
        vec![FileAction {
            title: format!("Create paired test file ({test_rel})"),
            kind: Some("refactor".into()),
            preferred: false,
            operations: vec![FileOperation::Create {
                path: test_rel,
                text: stub,
                overwrite: false,
            }],
            edits: Vec::new(),
            open: Some(abs),
            data: None,
        }]
    }
}

impl LanguageFeatures for crate::DartPlugin {
    fn code_lenses(&mut self, doc: &LfDoc) -> Vec<CodeLens> {
        if !setting_bool("showCodeLenses", true) {
            return Vec::new();
        }
        if is_pubspec(&doc.path) {
            let line = doc
                .text
                .lines()
                .position(|l| l.trim_start().starts_with("dependencies:"))
                .unwrap_or(0) as u32;
            let range = Range::new(Position::new(line, 0), Position::new(line, 0));
            return vec![
                CodeLens {
                    range,
                    title: "Pub get".into(),
                    command: "pub-get".into(),
                    argument: None,
                },
                CodeLens {
                    range,
                    title: "Pub upgrade".into(),
                    command: "pub-upgrade".into(),
                    argument: None,
                },
                CodeLens {
                    range,
                    title: "Pub outdated".into(),
                    command: "pub-outdated".into(),
                    argument: None,
                },
            ];
        }
        if !(doc.language == "dart" || doc.path.ends_with(".dart")) {
            return Vec::new();
        }
        find_runnables(&doc.text)
            .into_iter()
            .map(|r| {
                let range = Range::new(Position::new(r.line, 0), Position::new(r.line, 0));
                match r.kind {
                    RunnableKind::Main => CodeLens {
                        range,
                        title: "Run".into(),
                        command: "run-target".into(),
                        argument: Some(doc.path.clone()),
                    },
                    RunnableKind::Test | RunnableKind::TestWidget | RunnableKind::Group => {
                        let arg = r
                            .test_name
                            .map(|n| format!("name:{n}"))
                            .unwrap_or_else(|| doc.path.clone());
                        CodeLens {
                            range,
                            title: "Run test".into(),
                            command: "dart-test".into(),
                            argument: Some(arg),
                        }
                    }
                }
            })
            .collect()
    }

    fn gutter_actions(&mut self, doc: &LfDoc) -> Vec<GutterAction> {
        if !setting_bool("showTestGutterActions", true)
            || !(doc.language == "dart" || doc.path.ends_with(".dart"))
        {
            return Vec::new();
        }
        find_runnables(&doc.text)
            .into_iter()
            .map(|r| {
                let (command, argument) = match r.kind {
                    RunnableKind::Main => ("run-target".into(), Some(doc.path.clone())),
                    _ => (
                        "dart-test".into(),
                        Some(
                            r.test_name
                                .map(|n| format!("name:{n}"))
                                .unwrap_or_else(|| doc.path.clone()),
                        ),
                    ),
                };
                GutterAction {
                    line: r.line,
                    icon: Some("run".into()),
                    tooltip: r.label,
                    command,
                    argument,
                }
            })
            .collect()
    }

    fn document_links(&mut self, doc: &LfDoc) -> Vec<DocumentLink> {
        if is_pubspec(&doc.path) {
            let spec = Pubspec::parse(&doc.text);
            return spec
                .dependencies
                .into_iter()
                .filter_map(|d| {
                    let span = d.path_span?;
                    let path = d.path?;
                    let (abs, _) = resolve_relative_project_path(&doc.path, &format!("{path}/pubspec.yaml"))?;
                    Some(DocumentLink {
                        range: to_range(span),
                        target: abs,
                    })
                })
                .collect();
        }
        if doc.language == "dart" || doc.path.ends_with(".dart") {
            return find_directives(&doc.text)
                .into_iter()
                .filter_map(|d| {
                    if d.uri.contains(':') {
                        return None;
                    }
                    let (abs, _) = resolve_relative_project_path(&doc.path, &d.uri)?;
                    Some(DocumentLink {
                        range: to_range(d.span),
                        target: abs,
                    })
                })
                .collect();
        }
        Vec::new()
    }

    fn folding(&mut self, doc: &LfDoc) -> Vec<FoldRange> {
        if !(doc.language == "dart" || doc.path.ends_with(".dart")) {
            return Vec::new();
        }
        find_fold_ranges(&doc.text)
            .into_iter()
            .map(|f| FoldRange {
                start_line: f.start_line,
                end_line: f.end_line,
                kind: match f.kind {
                    DartFoldKind::Imports => "imports".into(),
                    DartFoldKind::Comment => "comment".into(),
                    DartFoldKind::Region => "region".into(),
                },
            })
            .collect()
    }
}
