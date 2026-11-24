//! `xirf.dart` — OpenIDEA plugin porting Dart IntelliJ support.

mod commands;
mod editor;
mod execution;
mod highlighting;
mod mcp;
mod views;

use dart_core::cli::OutdatedPackage;
use dart_core::test_json::TestJsonParser;
use openidea_plugin::{
    ConsoleRule, Context, Error, Event, EventKind, Invocation, Plugin, host, register_plugin,
};
use std::collections::HashMap;

#[derive(Default)]
pub struct DartPlugin {
    pub outdated: HashMap<String, OutdatedPackage>,
    pub test_parser: TestJsonParser,
    pub last_test_config: Option<String>,
}

impl Plugin for DartPlugin {
    fn activate(&mut self, _ctx: &Context) -> Result<(), Error> {
        let _ = host::subscribe(EventKind::FileSaved);
        let _ = host::set_console_rules(&[
            ConsoleRule::new(r"^(\s*Error:|Unhandled exception:)")
                .foreground("red")
                .style("bold"),
            ConsoleRule::new(r"^(\s*Warning:|INFO \|)")
                .foreground("yellow"),
            ConsoleRule::new(r"package:[^:\s)]+\.dart:\d+:\d+")
                .foreground("cyan")
                .style("underline"),
            ConsoleRule::new(r"^(\+?\d+\s*:.*All tests passed!|\s*✓)")
                .foreground("green")
                .style("bold"),
            ConsoleRule::new(r"(Some tests failed\.|\bFAILED\b)")
                .foreground("red")
                .style("bold"),
        ]);
        Ok(())
    }

    fn run_command(
        &mut self,
        id: &str,
        ctx: &Invocation,
        arg: Option<&str>,
    ) -> Result<Option<String>, Error> {
        match id {
            "pub-get" => commands::run_pub_command(self, ctx, "get"),
            "pub-upgrade" => commands::run_pub_command(self, ctx, "upgrade"),
            "pub-outdated" => commands::run_pub_outdated(self, ctx),
            "pub-deps" => commands::run_pub_command(self, ctx, "deps"),
            "pub-add" => commands::run_pub_add(ctx),
            "pub-remove" => commands::run_pub_remove(ctx),
            "dart-analyze" => commands::run_dart_analyze(ctx),
            "dart-format" => commands::run_dart_format(ctx),
            "dart-fix" => commands::run_dart_fix(ctx),
            "dart-run" => commands::run_dart_file(ctx),
            "dart-test" => commands::run_dart_test(ctx, arg),
            "run-target" => commands::run_target(ctx, arg),
            "open-pubspec" => commands::open_pubspec(ctx),
            "sort-pubspec" => commands::sort_pubspec(ctx),
            "create-test-file" => commands::create_test_file(ctx),
            _ => Err(Error::NotFound(format!("unknown Dart command {id}"))),
        }
    }

    fn handle_events(&mut self, events: &[Event], ctx: &Invocation) {
        for ev in events {
            if let Event::FileSaved(path) = ev {
                if path.ends_with("pubspec.yaml") {
                    host::refresh_view(views::PUB_PACKAGES_VIEW);
                } else if path.ends_with(".dart")
                    && highlighting::setting_bool("formatOnSave", false)
                    && ctx.editor().is_some_and(|e| e.path() == *path)
                {
                    let _ = commands::run_dart_format(ctx);
                }
            }
        }
    }
}

register_plugin!(
    DartPlugin,
    providers,
    semantic_tokens,
    hover,
    diagnostics,
    file_actions,
    language_features,
    launch_configurations,
    test_results,
    views,
    mcp
);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::diagnostics::DiagnosticsProvider;
    use openidea_plugin::language_features::{Document as LfDoc, LanguageFeatures};
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::providers::{Document, Providers};
    use openidea_plugin::semantic_tokens::SemanticTokens;
    use openidea_plugin::views::{ViewEvent, Views};
    use openidea_plugin::{LaunchConfigurations, McpProvider, PluginTestEvent, Position, Range, TestResults};

    #[test]
    fn activates_with_console_rules_and_pubspec_refresh() {
        let host = MockHost::install().with_capabilities(&["events", "project.read"]);
        let mut plugin = DartPlugin::default();
        plugin.activate(&Context::default()).unwrap();
        assert!(host.subscriptions().contains(&EventKind::FileSaved));
        assert_eq!(host.console_rules().len(), 5);

        plugin.handle_events(&[Event::FileSaved("/proj/pubspec.yaml".into())], &host.invocation());
        assert!(host.refreshed_views().contains(&views::PUB_PACKAGES_VIEW.to_owned()));
    }

    #[test]
    fn highlights_dart_and_pubspec_and_validates_pubspec() {
        let host = MockHost::install().with_capabilities(&["project.read"]);
        host.set_project_root(Some("/proj"));
        host.add_file("/proj/pubspec.yaml", "name: BadPkg\ndependencies:\n  zeta: ^1.0.0\n  alpha: ^1.0.0\n");
        let mut plugin = DartPlugin::default();

        let dart_tokens = plugin.semantic_tokens(&openidea_plugin::semantic_tokens::Document {
            path: "/proj/lib/main.dart".into(),
            language: "dart".into(),
            version: 1,
            text: "\
AppSourceAGIChat appSourceAGIChatFromMap(Map<String, dynamic> json) {
  if (a < b && c > (d)) return AppSourceAGIChat();
  readBool(data: json['enabled']);
  readList<String>(data: json['allowedAccesses']);
  readMap<String, AppSourceAGIChatSuggestion>(data: json['onboarding']);
  IcaModelOption.fromJson(json: json);
}"
            .into(),
        });
        assert!(dart_tokens.iter().any(|t| {
            t.kind == openidea_plugin::semantic_tokens::TokenKind::Type
                && t.modifiers
                    .contains(&openidea_plugin::semantic_tokens::TokenModifier::DefaultLibrary)
        }));
        assert_eq!(
            dart_tokens
                .iter()
                .filter(|t| matches!(
                    t.kind,
                    openidea_plugin::semantic_tokens::TokenKind::Function
                        | openidea_plugin::semantic_tokens::TokenKind::Method
                ))
                .count(),
            5
        );

        let doc = Document {
            path: "/proj/pubspec.yaml".into(),
            language: "yaml".into(),
            version: 1,
            text: "name: BadPkg\ndependencies:\n  zeta: ^1.0.0\n  alpha: ^1.0.0\n".into(),
        };
        let diags = plugin.diagnostics(&doc);
        assert!(diags.iter().any(|d| d.code.as_deref() == Some("invalid_package_name")));

        let actions = plugin.code_actions(&doc, Range::default());
        assert_eq!(actions.len(), 1);
        assert!(actions[0].edits[0].1[0].text.find("alpha").unwrap() < actions[0].edits[0].1[0].text.find("zeta").unwrap());
    }

    #[test]
    fn renders_pub_packages_view_and_launches_tests() {
        let host = MockHost::install().with_capabilities(&["project.read", "process:dart"]);
        host.set_project_root(Some("/proj"));
        host.add_file(
            "pubspec.yaml",
            "name: demo\nenvironment:\n  sdk: ^3.5.0\ndependencies:\n  http: ^1.2.0\ndev_dependencies:\n  build_runner: ^2.4.0\n",
        );
        host.add_file("lib/main.dart", "void main() {}\n");

        let mut plugin = DartPlugin::default();
        let view = plugin.render(views::PUB_PACKAGES_VIEW, None, &host.invocation()).unwrap();
        assert!(!view.toolbar.is_empty());
        assert!(view.status.unwrap().contains("demo"));

        let configs = plugin.launch_configurations("/proj");
        assert!(configs.iter().any(|c| c.config.id == "dart-test"));
        assert!(configs.iter().any(|c| c.config.id == "build-runner-watch"));

        let evs = plugin.parse(
            "dart-test",
            "{\"type\":\"testStart\",\"test\":{\"id\":1,\"name\":\"adds\"}}\n{\"type\":\"testDone\",\"testID\":1,\"result\":\"success\",\"hidden\":false}\n",
        );
        assert!(matches!(&evs[..], [PluginTestEvent::TestStarted(_), PluginTestEvent::Passed(_)]));

        let lenses = plugin.code_lenses(&LfDoc {
            path: "/proj/lib/main.dart".into(),
            language: "dart".into(),
            version: 1,
            text: "void main() {}".into(),
        });
        assert_eq!(lenses[0].command, "run-target");

        plugin
            .handle(views::PUB_PACKAGES_VIEW, ViewEvent::Activate("dep:4:http".into()), &host.invocation())
            .unwrap();
    }

    #[test]
    fn validates_manifest_and_json_assets() {
        let manifest = include_str!("../plugin.toml");
        assert!(manifest.contains("id = \"xirf.dart\""));
        assert!(manifest.contains("[[extension-points]]"));

        for raw in [
            include_str!("../settings/sdk.schema.json"),
            include_str!("../settings/editor.schema.json"),
            include_str!("../mcp/dart_analyze.schema.json"),
            include_str!("../mcp/pub_outdated.schema.json"),
        ] {
            let v: serde_json::Value = serde_json::from_str(raw).unwrap();
            assert!(v.is_object());
        }
        let snippets: serde_json::Value = serde_json::from_str(include_str!("../snippets/dart.json")).unwrap();
        for (_, entry) in snippets.as_object().unwrap() {
            let obj = entry.as_object().unwrap();
            assert!(obj.contains_key("prefix") && obj.contains_key("body"));
            assert!(obj.keys().all(|k| matches!(k.as_str(), "prefix" | "body" | "description")));
        }

        // Verify McpProvider handles unknown tools cleanly.
        let host = MockHost::install().with_capabilities(&["project.read", "process:dart"]);
        host.set_project_root(Some("/proj"));
        let mut plugin = DartPlugin::default();
        assert!(plugin.call("unknown_tool", "{}", &host.invocation()).is_err());
        let _ = Position::default();
    }

    #[test]
    fn dart_format_formats_live_dirty_buffer_via_stdin_not_disk() {
        let host = MockHost::install().with_capabilities(&[
            "project.read",
            "editor.read",
            "editor.write",
            "process:dart",
        ]);
        host.set_project_root(Some("/proj"));
        host.add_file("lib/main.dart", "void main() {\n  print('saved');\n}\n");
        host.open_editor(
            "/proj/lib/main.dart",
            "dart",
            "void main(){print('UNSAVED-USER-EDIT');}\n",
            &[(0, 0, 0, 0)],
        );
        host.on_process("dart", |args, stdin| {
            assert!(args.contains(&"format".to_owned()));
            assert!(args.contains(&"--output=show".to_owned()));
            assert!(args.contains(&"--summary=none".to_owned()));
            assert!(args.contains(&"--show=none".to_owned()));
            let stdin_name_pos = args.iter().position(|a| a == "--stdin-name").unwrap();
            assert_eq!(args.get(stdin_name_pos + 1).map(String::as_str), Some("/proj/lib/main.dart"));
            assert_eq!(args.last().map(String::as_str), Some("/proj/lib/main.dart"));
            assert_eq!(args.iter().filter(|a| *a == "/proj/lib/main.dart").count(), 1);
            let source = stdin.expect("dart format must receive live editor buffer via stdin");
            assert!(source.contains("UNSAVED-USER-EDIT"));
            Ok(openidea_plugin::ProcessOutput {
                status: Some(0),
                stdout: "void main() {\n  print('UNSAVED-USER-EDIT');\n}\n".into(),
                stderr: String::new(),
            })
        });

        let mut plugin = DartPlugin::default();
        let res = plugin.run_command("dart-format", &host.invocation(), None).unwrap();
        assert_eq!(res, None);
        assert_eq!(host.editor_text(), "void main() {\n  print('UNSAVED-USER-EDIT');\n}\n");
        assert!(!host.editor_text().contains("saved"));
    }

    #[test]
    fn dart_format_respects_document_version_and_capabilities() {
        // 1. Missing editor.write capability preserves dirty buffer.
        let host = MockHost::install().with_capabilities(&["project.read", "editor.read", "process:dart"]);
        host.set_project_root(Some("/proj"));
        host.add_file("lib/main.dart", "void main() {\n  print('saved');\n}\n");
        host.open_editor("/proj/lib/main.dart", "dart", "void main(){print('dirty');}\n", &[(0, 0, 0, 0)]);
        host.on_process("dart", |_, stdin| {
            assert_eq!(stdin, Some("void main(){print('dirty');}\n"));
            Ok(openidea_plugin::ProcessOutput {
                status: Some(0),
                stdout: "void main() {\n  print('dirty');\n}\n".into(),
                stderr: String::new(),
            })
        });
        let mut plugin = DartPlugin::default();
        assert!(plugin.run_command("dart-format", &host.invocation(), None).is_err());
        assert_eq!(host.editor_text(), "void main(){print('dirty');}\n");

        // 2. Typing advances document version; formatting formats latest buffer and bumps version.
        let host = MockHost::install().with_capabilities(&[
            "project.read",
            "editor.read",
            "editor.write",
            "process:dart",
        ]);
        host.set_project_root(Some("/proj"));
        host.open_editor("/proj/lib/main.dart", "dart", "void main(){", &[(0, 0, 0, 0)]);
        host.type_in_editor("print('v2');}\n");
        assert_eq!(host.editor_version(), 2);
        host.on_process("dart", |_, stdin| {
            assert_eq!(stdin, Some("void main(){print('v2');}\n"));
            Ok(openidea_plugin::ProcessOutput {
                status: Some(0),
                stdout: "void main() {\n  print('v2');\n}\n".into(),
                stderr: String::new(),
            })
        });
        plugin.run_command("dart-format", &host.invocation(), None).unwrap();
        assert_eq!(host.editor_text(), "void main() {\n  print('v2');\n}\n");
        assert_eq!(host.editor_version(), 3);

        // Unchanged output is a no-op that does not bump version.
        host.on_process("dart", |_, stdin| {
            Ok(openidea_plugin::ProcessOutput {
                status: Some(0),
                stdout: stdin.unwrap().to_owned(),
                stderr: String::new(),
            })
        });
        plugin.run_command("dart-format", &host.invocation(), None).unwrap();
        assert_eq!(host.editor_version(), 3);

        // 3. Formatter error exit preserves dirty buffer.
        host.open_editor("/proj/lib/main.dart", "dart", "void main( {\n", &[(0, 0, 0, 0)]);
        host.on_process("dart", |_, _| {
            Ok(openidea_plugin::ProcessOutput {
                status: Some(65),
                stdout: String::new(),
                stderr: "Could not format".into(),
            })
        });
        assert!(plugin.run_command("dart-format", &host.invocation(), None).is_err());
        assert_eq!(host.editor_text(), "void main( {\n");
    }
}
