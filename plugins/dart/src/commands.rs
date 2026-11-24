use crate::highlighting::setting_bool;
use dart_core::cli::{
    AnalyzeSeverity, OutdatedPackage, parse_analyze_machine, parse_pub_outdated_json, resolve_program,
};
use dart_core::pubspec::{Pubspec, sort_dependencies_edit};
use openidea_plugin::host;
use openidea_plugin::{
    Diagnostic, Error, Invocation, NotificationLevel, PickItem, Position, ProcessOutput, Project,
    Range, Severity, TextEdit,
};
use std::collections::HashMap;

pub const DART_OUTPUT: &str = "Dart";
pub const PUB_OUTPUT: &str = "Pub";

pub fn setting_str(key: &str) -> Option<String> {
    host::setting(key)
        .and_then(|raw| serde_json::from_str::<String>(&raw).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

pub fn split_extra_args(key: &str) -> Vec<String> {
    setting_str(key)
        .map(|s| s.split_whitespace().map(str::to_owned).collect())
        .unwrap_or_default()
}

pub fn resolve_dart_tool(project: &Project) -> (String, Vec<String>) {
    let sdk = setting_str("sdkPath");
    let use_fvm = setting_bool("useFvm", true);
    let has_fvm = project.exists(".fvm").unwrap_or(false);
    resolve_program("dart", sdk.as_deref(), use_fvm, has_fvm)
}

pub fn run_tool_captured(
    project: &Project,
    program: &str,
    args: &[String],
) -> Result<ProcessOutput, Error> {
    let cmd = host::Command::new(program).args(args.iter().map(String::as_str));
    host::run_command(&cmd, Some(project), None)
}

pub fn active_project(ctx: &Invocation) -> Result<Project, Error> {
    ctx.project()
        .or_else(|| host::projects().ok().and_then(|mut p| p.pop()))
        .ok_or_else(|| Error::Failed("Open a Dart project first.".into()))
}

pub fn run_pub_command(
    plugin: &mut crate::DartPlugin,
    ctx: &Invocation,
    subcommand: &str,
) -> Result<Option<String>, Error> {
    let project = active_project(ctx)?;
    let (prog, mut args) = resolve_dart_tool(&project);
    args.push("pub".into());
    args.push(subcommand.into());
    if setting_bool("pubOffline", false) && matches!(subcommand, "get" | "upgrade") {
        args.push("--offline".into());
    }

    let task = host::begin_task(&format!("dart pub {subcommand}"), true);
    task.report(0, None, Some("Running pub..."));
    let _ = host::output_clear(PUB_OUTPUT);
    let _ = host::output_append(PUB_OUTPUT, &format!("$ {prog} {}\n", args.join(" ")));
    let _ = host::output_reveal(PUB_OUTPUT);

    let out = run_tool_captured(&project, &prog, &args)?;
    append_output(PUB_OUTPUT, &out);
    host::refresh_view(crate::views::PUB_PACKAGES_VIEW);

    if out.status == Some(0) {
        if subcommand == "get" || subcommand == "upgrade" {
            plugin.outdated.clear();
        }
        host::notify(
            NotificationLevel::Info,
            &format!("dart pub {subcommand}"),
            "Completed successfully.",
        );
        Ok(None)
    } else {
        Err(Error::Failed(format!(
            "dart pub {subcommand} failed (exit {:?}); see Pub output.",
            out.status
        )))
    }
}

pub fn run_pub_outdated(
    plugin: &mut crate::DartPlugin,
    ctx: &Invocation,
) -> Result<Option<String>, Error> {
    let project = active_project(ctx)?;
    let (prog, mut args) = resolve_dart_tool(&project);
    args.extend(["pub".into(), "outdated".into(), "--json".into()]);

    let task = host::begin_task("dart pub outdated", true);
    task.report(0, None, Some("Checking pub.dev for newer versions..."));

    let out = run_tool_captured(&project, &prog, &args)?;
    let pkgs = parse_pub_outdated_json(&out.stdout);
    let mut map: HashMap<String, OutdatedPackage> = HashMap::new();
    let _ = host::output_clear(PUB_OUTPUT);
    let _ = host::output_append(PUB_OUTPUT, "Package                 Current    Upgradable Latest\n");
    let mut upgradable_count = 0usize;
    for p in pkgs {
        let curr = p.current.as_deref().unwrap_or("-");
        let upg = p.upgradable.as_deref().unwrap_or("-");
        let lat = p.latest.as_deref().unwrap_or("-");
        if p.latest.is_some() && p.current != p.latest {
            upgradable_count += 1;
        }
        let _ = host::output_append(
            PUB_OUTPUT,
            &format!("{:<24}{:<11}{:<11}{}\n", p.name, curr, upg, lat),
        );
        map.insert(p.name.clone(), p);
    }
    plugin.outdated = map;
    host::refresh_view(crate::views::PUB_PACKAGES_VIEW);
    let _ = host::output_reveal(PUB_OUTPUT);
    host::notify(
        NotificationLevel::Info,
        "dart pub outdated",
        &format!("{upgradable_count} package(s) have newer versions."),
    );
    Ok(None)
}

pub fn run_pub_add(ctx: &Invocation) -> Result<Option<String>, Error> {
    let project = active_project(ctx)?;
    let pkg = host::input("Package name to add (prefix with dev: for dev_dependencies)", None)?
        .ok_or(Error::Cancelled)?;
    let pkg = pkg.trim();
    if pkg.is_empty() {
        return Err(Error::Cancelled);
    }
    let (prog, mut args) = resolve_dart_tool(&project);
    args.extend(["pub".into(), "add".into(), pkg.to_owned()]);
    let _ = host::output_clear(PUB_OUTPUT);
    let out = run_tool_captured(&project, &prog, &args)?;
    append_output(PUB_OUTPUT, &out);
    host::refresh_view(crate::views::PUB_PACKAGES_VIEW);
    if out.status == Some(0) {
        Ok(Some(format!("Added {pkg}")))
    } else {
        Err(Error::Failed(format!("dart pub add {pkg} failed")))
    }
}

pub fn run_pub_remove(ctx: &Invocation) -> Result<Option<String>, Error> {
    let project = active_project(ctx)?;
    let yaml = project.read_file("pubspec.yaml")?;
    let spec = Pubspec::parse(&yaml);
    if spec.dependencies.is_empty() {
        return Ok(Some("No dependencies in pubspec.yaml".into()));
    }
    let items: Vec<PickItem> = spec
        .dependencies
        .iter()
        .map(|d| {
            PickItem::new(&d.name, &d.name).description(d.section.as_str())
        })
        .collect();
    let chosen = host::pick("Remove dependency", &items)?.ok_or(Error::Cancelled)?;
    let (prog, mut args) = resolve_dart_tool(&project);
    args.extend(["pub".into(), "remove".into(), chosen.clone()]);
    let _ = host::output_clear(PUB_OUTPUT);
    let out = run_tool_captured(&project, &prog, &args)?;
    append_output(PUB_OUTPUT, &out);
    host::refresh_view(crate::views::PUB_PACKAGES_VIEW);
    if out.status == Some(0) {
        Ok(Some(format!("Removed {chosen}")))
    } else {
        Err(Error::Failed(format!("dart pub remove {chosen} failed")))
    }
}

pub fn run_dart_analyze(ctx: &Invocation) -> Result<Option<String>, Error> {
    let project = active_project(ctx)?;
    let (prog, mut args) = resolve_dart_tool(&project);
    args.extend(["analyze".into(), "--format=machine".into()]);
    args.extend(split_extra_args("additionalAnalyzeArgs"));

    let task = host::begin_task("dart analyze", true);
    task.report(0, None, Some("Analyzing Dart project..."));
    let out = run_tool_captured(&project, &prog, &args)?;
    let combined = format!("{}\n{}", out.stdout, out.stderr);
    let issues = parse_analyze_machine(&combined);

    let _ = host::output_clear(DART_OUTPUT);
    let mut by_file: HashMap<String, Vec<Diagnostic>> = HashMap::new();
    for issue in &issues {
        let severity = match issue.severity {
            AnalyzeSeverity::Error => Severity::Error,
            AnalyzeSeverity::Warning => Severity::Warning,
            AnalyzeSeverity::Info => Severity::Info,
        };
        let pos = Position::new(issue.line, issue.column);
        let end = Position::new(issue.line, issue.column + issue.length);
        let mut diag = Diagnostic::new(Range::new(pos, end), severity, &issue.message);
        diag.code = Some(issue.code.clone());
        diag.source = Some("dart analyze".into());
        by_file.entry(issue.file.clone()).or_default().push(diag);
        let _ = host::output_append(
            DART_OUTPUT,
            &format!(
                "{}: {}:{}:{} [{}] {}\n",
                issue.kind,
                issue.file,
                issue.line + 1,
                issue.column + 1,
                issue.code,
                issue.message
            ),
        );
    }

    for (file, diags) in &by_file {
        let _ = host::publish_diagnostics(file, diags);
    }

    if issues.is_empty() {
        let _ = host::output_append(DART_OUTPUT, "No issues found.\n");
        host::notify(NotificationLevel::Info, "dart analyze", "No issues found.");
    } else {
        let _ = host::output_reveal(DART_OUTPUT);
        host::notify(
            NotificationLevel::Warning,
            "dart analyze",
            &format!("Found {} issue(s) across {} file(s).", issues.len(), by_file.len()),
        );
    }
    Ok(None)
}

pub fn run_dart_format(ctx: &Invocation) -> Result<Option<String>, Error> {
    let project = active_project(ctx)?;
    let editor = ctx
        .editor()
        .ok_or_else(|| Error::Failed("Focus a Dart editor to format.".into()))?;
    let path = editor.path();
    let doc = editor.document()?;
    let (prog, mut args) = resolve_dart_tool(&project);
    args.extend(["format".into(), "--output=show".into(), path.clone()]);
    let out = run_tool_captured(&project, &prog, &args)?;
    if out.status != Some(0) {
        return Err(Error::Failed(format!("dart format failed: {}", out.stderr.trim())));
    }
    if out.stdout.is_empty() || out.stdout == doc.text {
        return Ok(None);
    }
    let lines: Vec<&str> = doc.text.split('\n').collect();
    let last_line = lines.len().saturating_sub(1);
    let last_col = dart_core::utf16_len(lines[last_line].strip_suffix('\r').unwrap_or(lines[last_line]));
    editor.apply_edits(
        doc.version,
        &[TextEdit {
            range: Range::new(Position::new(0, 0), Position::new(last_line as u32, last_col)),
            text: out.stdout,
        }],
    )?;
    Ok(None)
}

pub fn run_dart_fix(ctx: &Invocation) -> Result<Option<String>, Error> {
    let project = active_project(ctx)?;
    let (prog, mut args) = resolve_dart_tool(&project);
    args.extend(["fix".into(), "--apply".into()]);
    let _ = host::output_clear(DART_OUTPUT);
    let _ = host::output_reveal(DART_OUTPUT);
    let out = run_tool_captured(&project, &prog, &args)?;
    append_output(DART_OUTPUT, &out);
    if out.status == Some(0) {
        Ok(Some("Applied dart fix".into()))
    } else {
        Err(Error::Failed(format!("dart fix failed (exit {:?})", out.status)))
    }
}

pub fn run_dart_file(ctx: &Invocation) -> Result<Option<String>, Error> {
    let project = active_project(ctx)?;
    let target = ctx
        .editor()
        .map(|e| e.path())
        .filter(|p| p.ends_with(".dart"))
        .unwrap_or_else(|| "lib/main.dart".into());
    let (prog, mut args) = resolve_dart_tool(&project);
    args.extend(["run".into(), target.clone()]);
    let _ = project;
    host::open_terminal(&format!("Dart: {target}"), &prog, &args)?;
    Ok(None)
}

pub fn run_dart_test(ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
    let mut args = vec!["test".to_owned(), "--reporter=json".to_owned()];
    args.extend(split_extra_args("additionalTestArgs"));
    if let Some(arg) = arg.map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(name) = arg.strip_prefix("name:") {
            args.push("--plain-name".into());
            args.push(name.to_owned());
        } else {
            args.push(arg.to_owned());
        }
    } else if let Some(ed) = ctx.editor()
        && ed.path().ends_with("_test.dart")
    {
        args.push(ed.path());
    }
    host::start_run("dart-test", &args, false).map(Some)
}

pub fn run_target(ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
    let project = active_project(ctx)?;
    let (prog, mut args) = resolve_dart_tool(&project);
    let target = arg
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| ctx.editor().map(|e| e.path()))
        .ok_or_else(|| Error::Failed("No Dart file to run".into()))?;
    args.extend(["run".into(), target.clone()]);
    host::open_terminal(&format!("Dart: {target}"), &prog, &args)?;
    Ok(None)
}

pub fn open_pubspec(ctx: &Invocation) -> Result<Option<String>, Error> {
    let project = active_project(ctx)?;
    let root = project.root()?;
    let path = format!("{}/pubspec.yaml", root.trim_end_matches('/'));
    host::open_editor(&path, None)?;
    Ok(None)
}

pub fn sort_pubspec(ctx: &Invocation) -> Result<Option<String>, Error> {
    let editor = ctx
        .editor()
        .ok_or_else(|| Error::Failed("Open pubspec.yaml to sort dependencies.".into()))?;
    let doc = editor.document()?;
    let Some((span, text)) = sort_dependencies_edit(&doc.text) else {
        return Ok(Some("Dependencies are already sorted.".into()));
    };
    editor.apply_edits(
        doc.version,
        &[TextEdit {
            range: Range::new(
                Position::new(span.start.line, span.start.character),
                Position::new(span.end.line, span.end.character),
            ),
            text,
        }],
    )?;
    Ok(None)
}

pub fn create_test_file(ctx: &Invocation) -> Result<Option<String>, Error> {
    let project = active_project(ctx)?;
    let editor = ctx
        .editor()
        .ok_or_else(|| Error::Failed("Focus a Dart source file first.".into()))?;
    let path = editor.path();
    let root_owned = project.root()?;
    let root = root_owned.trim_end_matches('/');
    let rel = path.strip_prefix(root).unwrap_or(&path).trim_start_matches('/');
    let test_rel = if let Some(inner) = rel.strip_prefix("lib/") {
        format!("test/{}", inner.trim_end_matches(".dart")) + "_test.dart"
    } else {
        format!("test/{}_test.dart", rel.trim_end_matches(".dart"))
    };
    if !project.exists(&test_rel).unwrap_or(false) {
        let pkg = project
            .read_file("pubspec.yaml")
            .ok()
            .and_then(|y| Pubspec::parse(&y).name.map(|(n, _)| n))
            .unwrap_or_else(|| "app".into());
        let import_line = if let Some(inner) = rel.strip_prefix("lib/") {
            format!("import 'package:{pkg}/{inner}';\n")
        } else {
            String::new()
        };
        let stub = format!(
            "import 'package:test/test.dart';\n{import_line}\nvoid main() {{\n  test('works', () {{\n    // TODO: implement test\n  }});\n}}\n"
        );
        project.write_file(&test_rel, &stub)?;
    }
    let abs = format!("{root}/{test_rel}");
    host::open_editor(&abs, None)?;
    Ok(None)
}

fn append_output(channel: &str, out: &ProcessOutput) {
    if !out.stdout.is_empty() {
        let _ = host::output_append(channel, &out.stdout);
        if !out.stdout.ends_with('\n') {
            let _ = host::output_append(channel, "\n");
        }
    }
    if !out.stderr.is_empty() {
        let _ = host::output_append(channel, &out.stderr);
        if !out.stderr.ends_with('\n') {
            let _ = host::output_append(channel, "\n");
        }
    }
}
