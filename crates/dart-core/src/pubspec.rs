//! `pubspec.yaml` parser, validator, and dependency-sorting helper.

use crate::{TextPos, TextSpan, utf16_len};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepSection {
    Dependencies,
    DevDependencies,
    DependencyOverrides,
}

impl DepSection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dependencies => "dependencies",
            Self::DevDependencies => "dev_dependencies",
            Self::DependencyOverrides => "dependency_overrides",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PubspecDependency {
    pub name: String,
    pub section: DepSection,
    pub constraint: Option<String>,
    pub sdk: Option<String>,
    pub path: Option<String>,
    pub git_url: Option<String>,
    pub line: u32,
    pub key_span: TextSpan,
    pub path_span: Option<TextSpan>,
    pub duplicate: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PubspecAsset {
    pub value: String,
    pub line: u32,
    pub span: TextSpan,
    pub is_font: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pubspec {
    pub name: Option<(String, TextSpan)>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub sdk_constraint: Option<(String, TextSpan)>,
    pub flutter_constraint: Option<(String, TextSpan)>,
    pub dependencies: Vec<PubspecDependency>,
    pub has_flutter_section: bool,
    pub uses_material_design: Option<(bool, TextSpan)>,
    pub assets: Vec<PubspecAsset>,
    pub flutter_section_line: Option<u32>,
}

impl Pubspec {
    /// Parses a `pubspec.yaml` preserving 0-based line and UTF-16 column positions.
    pub fn parse(text: &str) -> Self {
        let mut out = Self::default();
        let mut top_section: Option<TopSection> = None;
        let mut current_dep_idx: Option<usize> = None;
        let mut dep_indent: usize = 0;
        let mut flutter_sub: Option<FlutterSub> = None;
        let mut seen_per_section: [HashSet<String>; 3] = Default::default();

        for (line_num, raw_line) in text.split('\n').enumerate() {
            let line_u32 = line_num as u32;
            let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            let stripped = strip_yaml_comment(line);
            let trimmed = stripped.trim();
            if trimmed.is_empty() {
                continue;
            }
            let indent = stripped.len() - stripped.trim_start().len();

            if indent == 0 && !trimmed.starts_with('-') {
                current_dep_idx = None;
                flutter_sub = None;
                if let Some((key, val, val_col)) = split_yaml_kv(stripped, 0) {
                    match key {
                        "name" => {
                            let clean = unquote_yaml(val);
                            if !clean.is_empty() {
                                let span = span_for_substr(line_u32, line, val_col, val.len());
                                out.name = Some((clean.to_owned(), span));
                            }
                            top_section = None;
                        }
                        "version" => {
                            let clean = unquote_yaml(val);
                            if !clean.is_empty() {
                                out.version = Some(clean.to_owned());
                            }
                            top_section = None;
                        }
                        "description" => {
                            let clean = unquote_yaml(val);
                            if !clean.is_empty() {
                                out.description = Some(clean.to_owned());
                            }
                            top_section = None;
                        }
                        "environment" => top_section = Some(TopSection::Environment),
                        "dependencies" => top_section = Some(TopSection::Deps(DepSection::Dependencies)),
                        "dev_dependencies" => top_section = Some(TopSection::Deps(DepSection::DevDependencies)),
                        "dependency_overrides" => {
                            top_section = Some(TopSection::Deps(DepSection::DependencyOverrides))
                        }
                        "flutter" => {
                            out.has_flutter_section = true;
                            out.flutter_section_line = Some(line_u32);
                            top_section = Some(TopSection::Flutter);
                        }
                        _ => top_section = Some(TopSection::Other),
                    }
                }
                continue;
            }

            match top_section {
                Some(TopSection::Environment) => {
                    if let Some((key, val, val_col)) = split_yaml_kv(stripped, indent) {
                        let clean = unquote_yaml(val);
                        let span = span_for_substr(line_u32, line, val_col, val.len());
                        match key {
                            "sdk" if !clean.is_empty() => out.sdk_constraint = Some((clean.to_owned(), span)),
                            "flutter" if !clean.is_empty() => out.flutter_constraint = Some((clean.to_owned(), span)),
                            _ => {}
                        }
                    }
                }
                Some(TopSection::Deps(section)) => {
                    let sec_idx = section as usize;
                    if let Some(prev_idx) = current_dep_idx
                        && indent > dep_indent
                    {
                        if let Some((key, val, val_col)) = split_yaml_kv(stripped, indent) {
                            let clean = unquote_yaml(val);
                            let dep = &mut out.dependencies[prev_idx];
                            match key {
                                "sdk" if !clean.is_empty() => dep.sdk = Some(clean.to_owned()),
                                "path" if !clean.is_empty() => {
                                    dep.path = Some(clean.to_owned());
                                    dep.path_span = Some(span_for_unquoted(line_u32, line, val_col, val));
                                }
                                "url" | "git" if !clean.is_empty() && dep.git_url.is_none() => {
                                    dep.git_url = Some(clean.to_owned());
                                }
                                "version" if !clean.is_empty() && dep.constraint.is_none() => {
                                    dep.constraint = Some(clean.to_owned());
                                }
                                _ => {}
                            }
                        }
                        continue;
                    }

                    if let Some((key, val, _)) = split_yaml_kv(stripped, indent) {
                        dep_indent = indent;
                        let key_col = utf16_len(&line[..indent]);
                        let key_span = TextSpan::new(
                            TextPos::new(line_u32, key_col),
                            TextPos::new(line_u32, key_col + utf16_len(key)),
                        );
                        let clean_val = unquote_yaml(val);
                        let duplicate = !seen_per_section[sec_idx].insert(key.to_owned());
                        out.dependencies.push(PubspecDependency {
                            name: key.to_owned(),
                            section,
                            constraint: (!clean_val.is_empty()).then(|| clean_val.to_owned()),
                            sdk: None,
                            path: None,
                            git_url: None,
                            line: line_u32,
                            key_span,
                            path_span: None,
                            duplicate,
                        });
                        current_dep_idx = Some(out.dependencies.len() - 1);
                    }
                }
                Some(TopSection::Flutter) => {
                    if let Some(rest) = trimmed.strip_prefix('-') {
                        let item = rest.trim();
                        if matches!(flutter_sub, Some(FlutterSub::Assets)) && !item.is_empty() {
                            let raw_start = stripped.find('-').unwrap_or(indent) + 1;
                            let val_offset = raw_start + (stripped[raw_start..].len() - stripped[raw_start..].trim_start().len());
                            let clean = unquote_yaml(item);
                            if !clean.is_empty() {
                                out.assets.push(PubspecAsset {
                                    value: clean.to_owned(),
                                    line: line_u32,
                                    span: span_for_unquoted(line_u32, line, val_offset, item),
                                    is_font: false,
                                });
                            }
                        } else if matches!(flutter_sub, Some(FlutterSub::Fonts))
                            && let Some((k, v, vc)) = split_yaml_kv(item, 0)
                            && k == "asset"
                        {
                            let item_offset = stripped.find(item).unwrap_or(indent);
                            let clean = unquote_yaml(v);
                            if !clean.is_empty() {
                                out.assets.push(PubspecAsset {
                                    value: clean.to_owned(),
                                    line: line_u32,
                                    span: span_for_unquoted(line_u32, line, item_offset + vc, v),
                                    is_font: true,
                                });
                            }
                        }
                        continue;
                    }
                    if let Some((key, val, val_col)) = split_yaml_kv(stripped, indent) {
                        match key {
                            "uses-material-design" => {
                                let b = val == "true";
                                let span = span_for_substr(line_u32, line, val_col, val.len());
                                out.uses_material_design = Some((b, span));
                                flutter_sub = None;
                            }
                            "assets" => flutter_sub = Some(FlutterSub::Assets),
                            "fonts" => flutter_sub = Some(FlutterSub::Fonts),
                            "asset" if matches!(flutter_sub, Some(FlutterSub::Fonts)) => {
                                let clean = unquote_yaml(val);
                                if !clean.is_empty() {
                                    out.assets.push(PubspecAsset {
                                        value: clean.to_owned(),
                                        line: line_u32,
                                        span: span_for_unquoted(line_u32, line, val_col, val),
                                        is_font: true,
                                    });
                                }
                            }
                            _ if indent <= 2 => flutter_sub = None,
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }

    pub fn is_flutter_project(&self) -> bool {
        self.has_flutter_section
            || self.flutter_constraint.is_some()
            || self
                .dependencies
                .iter()
                .any(|d| d.name == "flutter" || d.sdk.as_deref() == Some("flutter"))
    }

    pub fn has_build_runner(&self) -> bool {
        self.dependencies.iter().any(|d| d.name == "build_runner")
    }

    pub fn dependency(&self, name: &str) -> Option<&PubspecDependency> {
        self.dependencies.iter().find(|d| d.name == name)
    }
}

#[derive(Clone, Copy)]
enum TopSection {
    Environment,
    Deps(DepSection),
    Flutter,
    Other,
}

#[derive(Clone, Copy)]
enum FlutterSub {
    Assets,
    Fonts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticLevel {
    Error,
    Warning,
    Info,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PubspecDiagnostic {
    pub span: TextSpan,
    pub level: DiagnosticLevel,
    pub code: &'static str,
    pub message: String,
}

/// Validates a parsed `pubspec.yaml`. `path_exists` checks project-relative paths when available.
pub fn validate_pubspec<F>(spec: &Pubspec, mut path_exists: F) -> Vec<PubspecDiagnostic>
where
    F: FnMut(&str) -> bool,
{
    let mut diags = Vec::new();
    match &spec.name {
        None => diags.push(PubspecDiagnostic {
            span: TextSpan::default(),
            level: DiagnosticLevel::Error,
            code: "missing_name",
            message: "pubspec.yaml is missing a required `name` field".into(),
        }),
        Some((name, span)) if !is_valid_package_name(name) => diags.push(PubspecDiagnostic {
            span: *span,
            level: DiagnosticLevel::Error,
            code: "invalid_package_name",
            message: format!(
                "Package name `{name}` should use lowercase letters, digits, and underscores (`^[a-z][a-z0-9_]*$`)"
            ),
        }),
        _ => {}
    }

    if spec.sdk_constraint.is_none() {
        diags.push(PubspecDiagnostic {
            span: TextSpan::default(),
            level: DiagnosticLevel::Warning,
            code: "missing_sdk_constraint",
            message: "Missing `environment.sdk` constraint".into(),
        });
    }

    let mut prod_names = HashSet::new();
    for dep in &spec.dependencies {
        if dep.duplicate {
            diags.push(PubspecDiagnostic {
                span: dep.key_span,
                level: DiagnosticLevel::Error,
                code: "duplicate_dependency",
                message: format!("Duplicate dependency `{}` in `{}`", dep.name, dep.section.as_str()),
            });
        }
        if dep.section == DepSection::Dependencies {
            prod_names.insert(dep.name.as_str());
        }
    }

    for dep in &spec.dependencies {
        if dep.section == DepSection::DevDependencies && prod_names.contains(dep.name.as_str()) {
            diags.push(PubspecDiagnostic {
                span: dep.key_span,
                level: DiagnosticLevel::Warning,
                code: "dependency_in_both",
                message: format!(
                    "`{}` is listed in both `dependencies` and `dev_dependencies`",
                    dep.name
                ),
            });
        }
        if let (Some(path), Some(span)) = (&dep.path, dep.path_span)
            && !path_exists(path)
        {
            diags.push(PubspecDiagnostic {
                span,
                level: DiagnosticLevel::Warning,
                code: "missing_path_dependency",
                message: format!("Local path dependency `{path}` does not exist"),
            });
        }
    }

    for asset in &spec.assets {
        if !path_exists(&asset.value) {
            diags.push(PubspecDiagnostic {
                span: asset.span,
                level: DiagnosticLevel::Warning,
                code: if asset.is_font { "missing_font_asset" } else { "missing_flutter_asset" },
                message: format!("Flutter asset path `{}` was not found in the project", asset.value),
            });
        }
    }

    diags
}

pub fn is_valid_package_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

/// Returns `(start_line, end_line_exclusive, replacement_text)` to sort entries alphabetically
/// inside `dependencies:` and `dev_dependencies:` (preserving multi-line blocks and `flutter` SDK first).
pub fn sort_dependencies_edit(text: &str) -> Option<(TextSpan, String)> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut new_lines: Vec<String> = lines.iter().map(|l| l.strip_suffix('\r').unwrap_or(l).to_owned()).collect();
    let mut changed = false;
    let mut i = 0;
    while i < new_lines.len() {
        let trimmed = new_lines[i].trim();
        if trimmed == "dependencies:" || trimmed == "dev_dependencies:" {
            let block_start = i + 1;
            let mut j = block_start;
            while j < new_lines.len() {
                let l = &new_lines[j];
                let t = l.trim();
                if t.is_empty() || t.starts_with('#') {
                    j += 1;
                    continue;
                }
                let indent = l.len() - l.trim_start().len();
                if indent == 0 {
                    break;
                }
                j += 1;
            }
            // Trim trailing blank lines from the sortable region
            let mut block_end = j;
            while block_end > block_start && new_lines[block_end - 1].trim().is_empty() {
                block_end -= 1;
            }
            if let Some(sorted) = sort_dep_block(&new_lines[block_start..block_end])
                && sorted != new_lines[block_start..block_end]
            {
                new_lines.splice(block_start..block_end, sorted);
                changed = true;
            }
            i = j;
            continue;
        }
        i += 1;
    }
    if !changed {
        return None;
    }
    let last_idx = lines.len().saturating_sub(1);
    let last_len = utf16_len(lines[last_idx].strip_suffix('\r').unwrap_or(lines[last_idx]));
    let line_sep = if text.contains("\r\n") { "\r\n" } else { "\n" };
    Some((
        TextSpan::new(TextPos::new(0, 0), TextPos::new(last_idx as u32, last_len)),
        new_lines.join(line_sep),
    ))
}

fn sort_dep_block(slice: &[String]) -> Option<Vec<String>> {
    let mut base_indent: Option<usize> = None;
    for l in slice {
        let t = l.trim();
        if !t.is_empty() && !t.starts_with('#') {
            let ind = l.len() - l.trim_start().len();
            base_indent = Some(base_indent.map_or(ind, |m| m.min(ind)));
        }
    }
    let base_indent = base_indent?;
    let mut entries: Vec<(String, Vec<String>)> = Vec::new();
    let mut pending_comments: Vec<String> = Vec::new();
    for l in slice {
        let t = l.trim();
        if t.is_empty() {
            continue;
        }
        if t.starts_with('#') {
            pending_comments.push(l.clone());
            continue;
        }
        let ind = l.len() - l.trim_start().len();
        if ind == base_indent
            && let Some((k, _, _)) = split_yaml_kv(l, ind)
        {
            let mut block = std::mem::take(&mut pending_comments);
            block.push(l.clone());
            entries.push((k.to_owned(), block));
        } else if let Some((_, last_block)) = entries.last_mut() {
            last_block.append(&mut pending_comments);
            last_block.push(l.clone());
        } else {
            return None;
        }
    }
    if entries.len() < 2 {
        return None;
    }
    // Keep `flutter` / `flutter_test` SDK blocks pinned first when sorting, then alphabetical.
    let mut by_key: BTreeMap<(u8, String), Vec<String>> = BTreeMap::new();
    for (k, block) in entries {
        let rank = if k == "flutter" || k == "flutter_test" { 0 } else { 1 };
        by_key.entry((rank, k)).or_default().extend(block);
    }
    let mut out = Vec::with_capacity(slice.len());
    for block in by_key.into_values() {
        out.extend(block);
    }
    out.extend(pending_comments);
    Some(out)
}

fn strip_yaml_comment(line: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;
    let bytes = line.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'#' if !in_single && !in_double && (i == 0 || bytes[i - 1].is_ascii_whitespace()) => {
                return &line[..i];
            }
            _ => {}
        }
    }
    line
}

fn split_yaml_kv(line: &str, indent: usize) -> Option<(&str, &str, usize)> {
    let body = line.get(indent..)?;
    let colon = body.find(':')?;
    let key = body[..colon].trim();
    if key.is_empty() || key.contains(char::is_whitespace) {
        return None;
    }
    let after = &body[colon + 1..];
    let val = after.trim();
    let ws = after.len() - after.trim_start().len();
    let val_col = indent + colon + 1 + ws;
    Some((key, val, val_col))
}

fn unquote_yaml(s: &str) -> &str {
    let s = s.trim();
    if (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
        || (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

fn span_for_substr(line_u32: u32, full_line: &str, byte_col: usize, byte_len: usize) -> TextSpan {
    let start_col = utf16_len(&full_line[..byte_col.min(full_line.len())]);
    let end_col = start_col + utf16_len(&full_line[byte_col.min(full_line.len())..(byte_col + byte_len).min(full_line.len())]);
    TextSpan::new(TextPos::new(line_u32, start_col), TextPos::new(line_u32, end_col))
}

fn span_for_unquoted(line_u32: u32, full_line: &str, byte_col: usize, raw: &str) -> TextSpan {
    let trimmed = raw.trim();
    let quoted = (trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2)
        || (trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2);
    if quoted {
        span_for_substr(line_u32, full_line, byte_col + 1, trimmed.len() - 2)
    } else {
        span_for_substr(line_u32, full_line, byte_col, trimmed.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
name: my_app
version: 1.0.0+1
environment:
  sdk: ^3.5.0
dependencies:
  flutter:
    sdk: flutter
  http: ^1.2.0
  local_pkg:
    path: ../local_pkg
dev_dependencies:
  flutter_test:
    sdk: flutter
  build_runner: ^2.4.0
flutter:
  uses-material-design: true
  assets:
    - assets/images/logo.png
  fonts:
    - family: Roboto
      fonts:
        - asset: assets/fonts/Roboto-Regular.ttf
";

    #[test]
    fn parses_flutter_pubspec_and_assets() {
        let spec = Pubspec::parse(SAMPLE);
        assert_eq!(spec.name.as_ref().unwrap().0, "my_app");
        assert!(spec.is_flutter_project());
        assert!(spec.has_build_runner());
        assert_eq!(spec.dependencies.len(), 5);
        assert_eq!(spec.dependency("local_pkg").unwrap().path.as_deref(), Some("../local_pkg"));
        assert_eq!(spec.assets.len(), 2);
        assert_eq!(spec.assets[0].value, "assets/images/logo.png");
        assert!(!spec.assets[0].is_font);
        assert_eq!(spec.assets[1].value, "assets/fonts/Roboto-Regular.ttf");
        assert!(spec.assets[1].is_font);
    }

    #[test]
    fn validates_duplicates_and_missing_paths() {
        let bad = "\
name: BadName
dependencies:
  http: ^1.0.0
  http: ^1.2.0
dev_dependencies:
  http: ^1.2.0
flutter:
  assets:
    - assets/missing.png
";
        let spec = Pubspec::parse(bad);
        let diags = validate_pubspec(&spec, |_| false);
        let codes: Vec<_> = diags.iter().map(|d| d.code).collect();
        assert!(codes.contains(&"invalid_package_name"));
        assert!(codes.contains(&"missing_sdk_constraint"));
        assert!(codes.contains(&"duplicate_dependency"));
        assert!(codes.contains(&"dependency_in_both"));
        assert!(codes.contains(&"missing_flutter_asset"));
    }

    #[test]
    fn sorts_dependencies_preserving_sdk_first_and_multiline_blocks() {
        let unsorted = "\
name: app
dependencies:
  zeta: ^1.0.0
  flutter:
    sdk: flutter
  alpha:
    path: ../alpha
";
        let (_, replacement) = sort_dependencies_edit(unsorted).expect("needs sorting");
        assert_eq!(
            replacement,
            "\
name: app
dependencies:
  flutter:
    sdk: flutter
  alpha:
    path: ../alpha
  zeta: ^1.0.0
"
        );
    }
}
