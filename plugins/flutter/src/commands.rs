use crate::highlighting::{setting_bool, setting_str};
use dart_core::TextPos;
use dart_core::cli::{
    parse_devices_json, parse_doctor_output, parse_emulators_output, resolve_program,
};
use dart_core::flutter_widgets::{extract_flutter_widget, widget_at_cursor};
use openidea_plugin::host;
use openidea_plugin::{
    Error, Invocation, NotificationLevel, PickItem, Position, ProcessOutput, Project, Range,
    TextEdit,
};

pub const FLUTTER_OUTPUT: &str = "Flutter";

pub const BUILD_TARGETS: &[(&str, &str)] = &[
    ("apk", "Android APK"),
    ("appbundle", "Android App Bundle (AAB)"),
    ("ios", "iOS App"),
    ("ipa", "iOS IPA"),
    ("web", "Web App"),
    ("macos", "macOS Desktop"),
    ("linux", "Linux Desktop"),
    ("windows", "Windows Desktop"),
];

pub fn resolve_flutter_tool(project: &Project) -> (String, Vec<String>) {
    let sdk = setting_str("sdkPath");
    let use_fvm = setting_bool("useFvm", true);
    let has_fvm = project.exists(".fvm").unwrap_or(false);
    resolve_program("flutter", sdk.as_deref(), use_fvm, has_fvm)
}

pub fn active_project(ctx: &Invocation) -> Result<Project, Error> {
    ctx.project()
        .or_else(|| host::projects().ok().and_then(|mut p| p.pop()))
        .ok_or_else(|| Error::Failed("Open a Flutter project first.".into()))
}

pub fn run_flutter_captured(
    project: &Project,
    args: &[String],
) -> Result<ProcessOutput, Error> {
    let (prog, mut full_args) = resolve_flutter_tool(project);
    full_args.extend(args.iter().cloned());
    let cmd = host::Command::new(&prog).args(full_args.iter().map(String::as_str));
    host::run_command(&cmd, Some(project), None)
}

pub fn run_flutter_pub(ctx: &Invocation, subcommand: &str) -> Result<Option<String>, Error> {
    let project = active_project(ctx)?;
    let (prog, mut args) = resolve_flutter_tool(&project);
    args.push("pub".into());
    args.push(subcommand.into());

    let task = host::begin_task(&format!("flutter pub {subcommand}"), true);
    task.report(0, None, Some("Running flutter pub..."));
    let _ = host::output_clear(FLUTTER_OUTPUT);
    let _ = host::output_append(FLUTTER_OUTPUT, &format!("$ {prog} {}\n", args.join(" ")));
    let _ = host::output_reveal(FLUTTER_OUTPUT);

    let cmd = host::Command::new(&prog).args(args.iter().map(String::as_str));
    let out = host::run_command(&cmd, Some(&project), None)?;
    append_output(&out);
    host::refresh_view(crate::views::FLUTTER_TOOLS_VIEW);

    if out.status == Some(0) {
        host::notify(
            NotificationLevel::Info,
            &format!("flutter pub {subcommand}"),
            "Completed successfully.",
        );
        Ok(None)
    } else {
        Err(Error::Failed(format!(
            "flutter pub {subcommand} failed (exit {:?}); see Flutter output.",
            out.status
        )))
    }
}

pub fn run_simple(ctx: &Invocation, title: &str, args: &[&str]) -> Result<Option<String>, Error> {
    let project = active_project(ctx)?;
    let task = host::begin_task(title, true);
    task.report(0, None, Some(title));
    let _ = host::output_clear(FLUTTER_OUTPUT);
    let _ = host::output_reveal(FLUTTER_OUTPUT);

    let owned: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
    let out = run_flutter_captured(&project, &owned)?;
    append_output(&out);
    host::refresh_view(crate::views::FLUTTER_TOOLS_VIEW);

    if out.status == Some(0) {
        host::notify(NotificationLevel::Info, title, "Completed successfully.");
        Ok(None)
    } else {
        Err(Error::Failed(format!("{title} failed (exit {:?})", out.status)))
    }
}

pub fn run_doctor(plugin: &mut crate::FlutterPlugin, ctx: &Invocation) -> Result<Option<String>, Error> {
    let project = active_project(ctx)?;
    let task = host::begin_task("flutter doctor", true);
    task.report(0, None, Some("Inspecting Flutter toolchains..."));
    let _ = host::output_clear(FLUTTER_OUTPUT);
    let _ = host::output_reveal(FLUTTER_OUTPUT);

    let out = run_flutter_captured(&project, &["doctor".into()])?;
    append_output(&out);
    plugin.doctor = parse_doctor_output(&out.stdout);
    host::refresh_view(crate::views::FLUTTER_TOOLS_VIEW);
    Ok(None)
}

pub fn run_build(ctx: &Invocation, target: &str) -> Result<Option<String>, Error> {
    let project = active_project(ctx)?;
    let mut args = vec!["build".to_owned(), target.to_owned()];
    if let Some(flavor) = setting_str("defaultFlavor")
        && matches!(target, "apk" | "appbundle" | "ios" | "ipa")
    {
        args.push("--flavor".into());
        args.push(flavor);
    }
    let task = host::begin_task(&format!("flutter build {target}"), true);
    task.report(0, None, Some(&format!("Building {target}...")));
    let _ = host::output_clear(FLUTTER_OUTPUT);
    let _ = host::output_reveal(FLUTTER_OUTPUT);

    let out = run_flutter_captured(&project, &args)?;
    append_output(&out);
    if out.status == Some(0) {
        host::notify(
            NotificationLevel::Info,
            &format!("flutter build {target}"),
            "Build succeeded.",
        );
        Ok(None)
    } else {
        Err(Error::Failed(format!("flutter build {target} failed (exit {:?})", out.status)))
    }
}

pub fn pick_and_build(ctx: &Invocation) -> Result<Option<String>, Error> {
    let items: Vec<PickItem> = BUILD_TARGETS
        .iter()
        .map(|(k, label)| PickItem::new(k, label))
        .collect();
    let target = host::pick("Select Flutter Build Target", &items)?.ok_or(Error::Cancelled)?;
    run_build(ctx, &target)
}

pub fn run_launch_mode(ctx: &Invocation, mode: &str) -> Result<Option<String>, Error> {
    let project = active_project(ctx)?;
    let (prog, mut args) = resolve_flutter_tool(&project);
    args.push("run".into());
    args.push(format!("--{mode}"));
    if let Some(flavor) = setting_str("defaultFlavor") {
        args.push("--flavor".into());
        args.push(flavor);
    }
    if let Some(extra) = setting_str("additionalRunArgs") {
        args.extend(extra.split_whitespace().map(str::to_owned));
    }
    host::open_terminal(&format!("Flutter ({mode})"), &prog, &args)?;
    Ok(None)
}

pub fn run_attach(ctx: &Invocation) -> Result<Option<String>, Error> {
    let project = active_project(ctx)?;
    let (prog, mut args) = resolve_flutter_tool(&project);
    args.push("attach".into());
    host::open_terminal("Flutter Attach", &prog, &args)?;
    Ok(None)
}

pub fn run_flutter_test(ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
    let mut args = vec!["test".to_owned(), "--machine".to_owned()];
    if let Some(extra) = setting_str("additionalRunArgs") {
        args.extend(extra.split_whitespace().map(str::to_owned));
    }
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
    host::start_run("flutter-test", &args, false).map(Some)
}

pub fn run_widget_preview(ctx: &Invocation) -> Result<Option<String>, Error> {
    let project = active_project(ctx)?;
    let (prog, mut args) = resolve_flutter_tool(&project);
    args.push("widget-preview".into());
    args.push("start".into());
    host::open_terminal("Flutter Widget Preview", &prog, &args)?;
    Ok(None)
}

pub fn launch_emulator(plugin: &mut crate::FlutterPlugin, ctx: &Invocation) -> Result<Option<String>, Error> {
    let project = active_project(ctx)?;
    let out = run_flutter_captured(&project, &["emulators".into()])?;
    let emulators = parse_emulators_output(&out.stdout);
    plugin.emulators = emulators.clone();
    if emulators.is_empty() {
        return Ok(Some("No Flutter emulators found.".into()));
    }
    let items: Vec<PickItem> = emulators
        .iter()
        .map(|e| {
            PickItem::new(&e.id, &e.name).description(&format!("{} ({})", e.manufacturer, e.platform))
        })
        .collect();
    let chosen = host::pick("Launch Flutter Emulator", &items)?.ok_or(Error::Cancelled)?;
    let launch_out = run_flutter_captured(&project, &["emulators".into(), "--launch".into(), chosen.clone()])?;
    append_output(&launch_out);
    host::notify(NotificationLevel::Info, "Flutter Emulator", &format!("Launching {chosen}..."));
    Ok(None)
}

pub fn refresh_devices_and_emulators(plugin: &mut crate::FlutterPlugin, project: &Project) {
    if let Ok(out) = run_flutter_captured(project, &["devices".into(), "--machine".into()]) {
        let parsed = parse_devices_json(&out.stdout);
        if !parsed.is_empty() {
            plugin.devices = parsed;
        }
    }
    if let Ok(out) = run_flutter_captured(project, &["emulators".into()]) {
        let parsed = parse_emulators_output(&out.stdout);
        if !parsed.is_empty() {
            plugin.emulators = parsed;
        }
    }
}

pub fn extract_widget_command(ctx: &Invocation) -> Result<Option<String>, Error> {
    let editor = ctx
        .editor()
        .ok_or_else(|| Error::Failed("Focus a Dart file on a widget to extract.".into()))?;
    let doc = editor.document()?;
    let sels = editor.selections()?;
    let cursor = sels.first().map(|r| r.start).unwrap_or_default();
    let span = widget_at_cursor(&doc.text, TextPos::new(cursor.line, cursor.character))
        .ok_or_else(|| Error::Failed("Place the cursor inside a Flutter widget constructor.".into()))?;
    let name = host::input("New StatelessWidget name", Some("ExtractedWidget"))?
        .ok_or(Error::Cancelled)?;
    let (call, class_def) = extract_flutter_widget(&doc.text, &span, &name)
        .ok_or_else(|| Error::Invalid("Widget name must be a valid PascalCase identifier.".into()))?;

    let lines: Vec<&str> = doc.text.split('\n').collect();
    let last_line = lines.len().saturating_sub(1);
    let last_col = dart_core::utf16_len(lines[last_line].strip_suffix('\r').unwrap_or(lines[last_line]));
    let end_pos = Position::new(last_line as u32, last_col);

    editor.apply_edits(
        doc.version,
        &[
            TextEdit {
                range: Range::new(
                    Position::new(span.span.start.line, span.span.start.character),
                    Position::new(span.span.end.line, span.span.end.character),
                ),
                text: call,
            },
            TextEdit {
                range: Range::new(end_pos, end_pos),
                text: class_def,
            },
        ],
    )?;
    Ok(None)
}

pub fn open_devtools_browser(ctx: &Invocation) -> Result<Option<String>, Error> {
    let project = active_project(ctx)?;
    let (prog, mut args) = resolve_flutter_tool(&project);
    args.extend(["pub".into(), "global".into(), "run".into(), "devtools".into()]);
    host::open_terminal("Flutter DevTools", &prog, &args)?;
    Ok(None)
}

fn append_output(out: &ProcessOutput) {
    if !out.stdout.is_empty() {
        let _ = host::output_append(FLUTTER_OUTPUT, &out.stdout);
        if !out.stdout.ends_with('\n') {
            let _ = host::output_append(FLUTTER_OUTPUT, "\n");
        }
    }
    if !out.stderr.is_empty() {
        let _ = host::output_append(FLUTTER_OUTPUT, &out.stderr);
        if !out.stderr.ends_with('\n') {
            let _ = host::output_append(FLUTTER_OUTPUT, "\n");
        }
    }
}
