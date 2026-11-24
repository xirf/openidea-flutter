//! `xirf.flutter` — OpenIDEA plugin porting Flutter IntelliJ support.

mod commands;
mod editor;
mod execution;
mod highlighting;
mod mcp;
mod views;

use dart_core::cli::{DeviceEntry, DoctorSection, EmulatorEntry};
use dart_core::test_json::TestJsonParser;
use openidea_plugin::{
    ConsoleRule, Context, Error, Event, EventKind, Invocation, Plugin, host, register_plugin,
};

#[derive(Default)]
pub struct FlutterPlugin {
    pub devices: Vec<DeviceEntry>,
    pub emulators: Vec<EmulatorEntry>,
    pub doctor: Vec<DoctorSection>,
    pub test_parser: TestJsonParser,
    pub last_test_config: Option<String>,
}

impl Plugin for FlutterPlugin {
    fn activate(&mut self, _ctx: &Context) -> Result<(), Error> {
        let _ = host::subscribe(EventKind::FileSaved);
        let _ = host::set_console_rules(&[
            ConsoleRule::new(r"════════ Exception caught by [^═]+")
                .foreground("red")
                .style("bold"),
            ConsoleRule::new(r"A RenderFlex overflowed by [^\n]+")
                .foreground("yellow")
                .style("bold"),
            ConsoleRule::new(r"(Hot reload performed|Reloaded \d+ of \d+ libraries|Restarted application)")
                .foreground("green")
                .style("bold"),
            ConsoleRule::new(r"package:flutter/[^:\s)]+\.dart:\d+:\d+")
                .foreground("cyan")
                .style("underline"),
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
            "pub-get" => commands::run_flutter_pub(ctx, "get"),
            "pub-upgrade" => commands::run_flutter_pub(ctx, "upgrade"),
            "pub-outdated" => commands::run_flutter_pub(ctx, "outdated"),
            "clean" => commands::run_simple(ctx, "flutter clean", &["clean"]),
            "doctor" => commands::run_doctor(self, ctx),
            "gen-l10n" => commands::run_simple(ctx, "flutter gen-l10n", &["gen-l10n"]),
            "upgrade" => commands::run_simple(ctx, "flutter upgrade", &["upgrade"]),
            "build-apk" => commands::run_build(ctx, "apk"),
            "build-appbundle" => commands::run_build(ctx, "appbundle"),
            "build-ios" => commands::run_build(ctx, "ios"),
            "build-ipa" => commands::run_build(ctx, "ipa"),
            "build-web" => commands::run_build(ctx, "web"),
            "build-macos" => commands::run_build(ctx, "macos"),
            "build-linux" => commands::run_build(ctx, "linux"),
            "build-windows" => commands::run_build(ctx, "windows"),
            "build-pick" => commands::pick_and_build(ctx),
            "run-profile" => commands::run_launch_mode(ctx, "profile"),
            "run-release" => commands::run_launch_mode(ctx, "release"),
            "attach" => commands::run_attach(ctx),
            "test" | "run-widget-test" => commands::run_flutter_test(ctx, arg),
            "widget-preview" => commands::run_widget_preview(ctx),
            "launch-emulator" => commands::launch_emulator(self, ctx),
            "extract-widget" => commands::extract_widget_command(ctx),
            "open-devtools-browser" => commands::open_devtools_browser(ctx),
            _ => Err(Error::NotFound(format!("unknown Flutter command {id}"))),
        }
    }

    fn handle_events(&mut self, events: &[Event], _ctx: &Invocation) {
        for ev in events {
            if let Event::FileSaved(path) = ev
                && path.ends_with("pubspec.yaml")
            {
                host::refresh_view(views::FLUTTER_TOOLS_VIEW);
            }
        }
    }
}

register_plugin!(
    FlutterPlugin,
    providers,
    semantic_tokens,
    inlay_hints,
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
    use openidea_plugin::hover::HoverProvider;
    use openidea_plugin::inlay_hints::{Document as InlayDoc, InlayHints};
    use openidea_plugin::language_features::{Document as LfDoc, LanguageFeatures};
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::providers::{Document, Providers};
    use openidea_plugin::semantic_tokens::SemanticTokens;
    use openidea_plugin::views::{ViewEvent, Views};
    use openidea_plugin::{LaunchConfigurations, McpProvider, PluginTestEvent, Position, Range, TestResults};

    const SAMPLE_WIDGET: &str = "\
import 'package:flutter/material.dart';

class CounterCard extends StatelessWidget {
  const CounterCard({super.key});

  @override
  Widget build(BuildContext context) {
    return Container(
      color: Colors.blue,
      child: const Icon(
        Icons.favorite,
      ),
    );
  }
}
";

    #[test]
    fn activates_and_emits_closing_labels_colors_icons_and_hover() {
        let host = MockHost::install().with_capabilities(&["events", "project.read"]);
        let mut plugin = FlutterPlugin::default();
        plugin.activate(&Context::default()).unwrap();
        assert_eq!(host.console_rules().len(), 4);

        let hints = plugin.inlay_hints(
            &InlayDoc {
                path: "/app/lib/counter.dart".into(),
                language: "dart".into(),
                version: 1,
                text: SAMPLE_WIDGET.into(),
            },
            Range::new(Position::new(0, 0), Position::new(20, 0)),
        );
        assert!(hints.iter().any(|h| h.label == " // Icon"));
        assert!(hints.iter().any(|h| h.label == " // Container"));

        let colors = plugin.colors(&LfDoc {
            path: "/app/lib/counter.dart".into(),
            language: "dart".into(),
            version: 1,
            text: SAMPLE_WIDGET.into(),
        });
        assert_eq!(colors.len(), 1);
        assert!(colors[0].blue > 0.9);

        let hover = plugin
            .hover(
                &Document {
                    path: "/app/lib/counter.dart".into(),
                    language: "dart".into(),
                    version: 1,
                    text: SAMPLE_WIDGET.into(),
                },
                Position::new(10, 10),
            )
            .unwrap();
        assert!(hover.markdown.contains("Material Icon") && hover.markdown.contains("Icons.favorite"));

        let tokens = plugin.semantic_tokens(&openidea_plugin::semantic_tokens::Document {
            path: "/app/lib/counter.dart".into(),
            language: "dart".into(),
            version: 1,
            text: SAMPLE_WIDGET.into(),
        });
        assert!(!tokens.is_empty());
    }

    #[test]
    fn provides_completions_widget_code_actions_and_state_conversion() {
        let host = MockHost::install().with_capabilities(&["project.read"]);
        host.set_project_root(Some("/app"));
        let mut plugin = FlutterPlugin::default();

        let icon_items = plugin.completion(
            &Document {
                path: "/app/lib/main.dart".into(),
                language: "dart".into(),
                version: 1,
                text: "Icon(Icons.".into(),
            },
            Position::new(0, 11),
            Some("."),
        );
        assert!(icon_items.iter().any(|i| i.label == "favorite"));

        let doc = Document {
            path: "/app/lib/counter.dart".into(),
            language: "dart".into(),
            version: 1,
            text: SAMPLE_WIDGET.into(),
        };
        let actions = plugin.code_actions(&doc, Range::new(Position::new(9, 14), Position::new(9, 14)));
        assert!(actions.iter().any(|a| a.title == "Wrap with Padding"));
        assert!(actions.iter().any(|a| a.title.starts_with("Extract Flutter Widget")));
        assert!(actions.iter().any(|a| a.title == "Convert to StatefulWidget"));

        let extract = actions
            .iter()
            .find(|a| a.title.starts_with("Extract Flutter Widget"))
            .unwrap()
            .clone();
        host.answer_next_prompt(Some(&["HeartIcon"]));
        let resolved = plugin.resolve_code_action(&doc, extract).unwrap();
        assert_eq!(resolved.edits[0].1.len(), 2);
        assert!(resolved.edits[0].1[1].text.contains("class HeartIcon extends StatelessWidget"));
    }

    #[test]
    fn renders_flutter_view_launch_configs_and_test_tree() {
        let host = MockHost::install().with_capabilities(&["project.read", "process:flutter"]);
        host.set_project_root(Some("/app"));
        host.add_file(
            "pubspec.yaml",
            "name: my_flutter_app\ndependencies:\n  flutter:\n    sdk: flutter\n",
        );
        host.add_dir("integration_test");

        let mut plugin = FlutterPlugin::default();
        let configs = plugin.launch_configurations("/app");
        assert!(configs.iter().any(|c| c.config.id == "flutter-test"));
        assert!(configs.iter().any(|c| c.config.id == "flutter-run-profile"));
        assert!(configs.iter().any(|c| c.config.id == "flutter-integration-test"));

        let evs = plugin.parse(
            "flutter-test",
            "{\"type\":\"testStart\",\"test\":{\"id\":1,\"name\":\"renders counter\"}}\n{\"type\":\"testDone\",\"testID\":1,\"result\":\"success\",\"hidden\":false}\n",
        );
        assert!(matches!(&evs[..], [PluginTestEvent::TestStarted(_), PluginTestEvent::Passed(_)]));

        let view = plugin.render(views::FLUTTER_TOOLS_VIEW, None, &host.invocation()).unwrap();
        assert!(view.status.unwrap().contains("my_flutter_app"));
        plugin
            .handle(views::FLUTTER_TOOLS_VIEW, ViewEvent::Button("refresh".into()), &host.invocation())
            .unwrap();
    }

    #[test]
    fn validates_manifest_and_json_assets() {
        let manifest = include_str!("../plugin.toml");
        assert!(manifest.contains("id = \"xirf.flutter\""));
        assert!(manifest.contains("id = \"xirf.dart\""));

        for raw in [
            include_str!("../settings/sdk.schema.json"),
            include_str!("../settings/editor.schema.json"),
            include_str!("../mcp/flutter_doctor.schema.json"),
            include_str!("../mcp/flutter_devices.schema.json"),
        ] {
            let v: serde_json::Value = serde_json::from_str(raw).unwrap();
            assert!(v.is_object());
        }
        let snippets: serde_json::Value = serde_json::from_str(include_str!("../snippets/flutter.json")).unwrap();
        for (_, entry) in snippets.as_object().unwrap() {
            let obj = entry.as_object().unwrap();
            assert!(obj.contains_key("prefix") && obj.contains_key("body"));
            assert!(obj.keys().all(|k| matches!(k.as_str(), "prefix" | "body" | "description")));
        }

        let host = MockHost::install().with_capabilities(&["project.read", "process:flutter"]);
        host.set_project_root(Some("/app"));
        let mut plugin = FlutterPlugin::default();
        assert!(plugin.call("unknown", "{}", &host.invocation()).is_err());
    }

    #[test]
    fn extract_widget_command_captures_local_variables_and_declines_unsafe() {
        // 1. Regression check: free local `message` must be captured via field + constructor arg (non-const call).
        let src = "\
import 'package:flutter/material.dart';

class Home extends StatelessWidget {
  const Home({super.key});

  @override
  Widget build(BuildContext context) {
    final message = 'hello';
    return Text(message);
  }
}
";
        let host = MockHost::install().with_capabilities(&[
            "project.read",
            "editor.read",
            "editor.write",
        ]);
        host.set_project_root(Some("/app"));
        host.open_editor("/app/lib/home.dart", "dart", src, &[(8, 12, 8, 12)]);
        host.answer_next_prompt(Some(&["Greeting"]));

        let mut plugin = FlutterPlugin::default();
        plugin
            .run_command("extract-widget", &host.invocation(), None)
            .unwrap();
        let updated = host.editor_text();
        assert!(updated.contains("return Greeting(message: message);"));
        assert!(!updated.contains("return const Greeting("));
        assert!(updated.contains("class Greeting extends StatelessWidget"));
        assert!(updated.contains("const Greeting({super.key, required this.message});"));
        assert!(updated.contains("final String message;"));

        // 2. Unsafe extraction declines without modifying the editor and hides the code action.
        let unsafe_src = "\
import 'package:flutter/material.dart';

class Home extends StatelessWidget {
  const Home({super.key});

  @override
  Widget build(BuildContext context) {
    final message = compute();
    return Text(message);
  }
}
";
        host.open_editor("/app/lib/home.dart", "dart", unsafe_src, &[(8, 12, 8, 12)]);
        assert!(
            plugin
                .run_command("extract-widget", &host.invocation(), None)
                .is_err()
        );
        assert_eq!(host.editor_text(), unsafe_src);

        let actions = plugin.code_actions(
            &Document {
                path: "/app/lib/home.dart".into(),
                language: "dart".into(),
                version: 1,
                text: unsafe_src.into(),
            },
            Range::new(Position::new(8, 12), Position::new(8, 12)),
        );
        assert!(!actions.iter().any(|a| a.title.starts_with("Extract Flutter Widget")));
    }
}
