use crate::highlighting::{setting_bool, setting_str};
use dart_core::cli::resolve_program;
use dart_core::pubspec::Pubspec;
use dart_core::test_json::{ParsedTestEvent, build_test_filter_args};
use openidea_plugin::{
    Debugger, LaunchConfigurations, Location, PluginLaunchConfiguration, PluginRunConfiguration,
    PluginTestEvent, Position, Range, TestFailed, TestOutput, TestPassed, TestResults, TestSkipped,
    TestStart, host,
};

impl LaunchConfigurations for crate::FlutterPlugin {
    fn launch_configurations(&mut self, project_root: &str) -> Vec<PluginLaunchConfiguration> {
        let Some(project) = host::project_of(project_root) else {
            return Vec::new();
        };
        let pubspec_text = project.read_file("pubspec.yaml").unwrap_or_default();
        let spec = Pubspec::parse(&pubspec_text);
        if !spec.is_flutter_project() {
            return Vec::new();
        }
        let sdk = setting_str("sdkPath");
        let use_fvm = setting_bool("useFvm", true);
        let has_fvm = project.exists(".fvm").unwrap_or(false);
        let (flutter_prog, prefix) = resolve_program("flutter", sdk.as_deref(), use_fvm, has_fvm);

        let with_prefix = |rest: &[&str]| -> Vec<String> {
            let mut args = prefix.clone();
            args.extend(rest.iter().map(|s| (*s).to_owned()));
            args
        };

        let mut configs = vec![
            PluginLaunchConfiguration {
                config: PluginRunConfiguration {
                    id: "flutter-test".into(),
                    name: "Flutter Tests".into(),
                    program: flutter_prog.clone(),
                    args: with_prefix(&["test", "--machine"]),
                    cwd: None,
                    test: true,
                    framework: Some("plugin".into()),
                },
                env: Vec::new(),
                debugger: Some(Debugger::Launch { language: "dart".into() }),
            },
            PluginLaunchConfiguration {
                config: PluginRunConfiguration {
                    id: "flutter-run-profile".into(),
                    name: "Flutter Run (Profile)".into(),
                    program: flutter_prog.clone(),
                    args: with_prefix(&["run", "--profile"]),
                    cwd: None,
                    test: false,
                    framework: None,
                },
                env: Vec::new(),
                debugger: Some(Debugger::Launch { language: "dart".into() }),
            },
            PluginLaunchConfiguration {
                config: PluginRunConfiguration {
                    id: "flutter-run-release".into(),
                    name: "Flutter Run (Release)".into(),
                    program: flutter_prog.clone(),
                    args: with_prefix(&["run", "--release"]),
                    cwd: None,
                    test: false,
                    framework: None,
                },
                env: Vec::new(),
                debugger: None,
            },
            PluginLaunchConfiguration {
                config: PluginRunConfiguration {
                    id: "flutter-attach".into(),
                    name: "Flutter Attach".into(),
                    program: flutter_prog.clone(),
                    args: with_prefix(&["attach"]),
                    cwd: None,
                    test: false,
                    framework: None,
                },
                env: Vec::new(),
                debugger: Some(Debugger::Launch { language: "dart".into() }),
            },
        ];

        if project.exists("integration_test").unwrap_or(false) {
            configs.push(PluginLaunchConfiguration {
                config: PluginRunConfiguration {
                    id: "flutter-integration-test".into(),
                    name: "Flutter Integration Tests".into(),
                    program: flutter_prog,
                    args: with_prefix(&["test", "--machine", "integration_test"]),
                    cwd: None,
                    test: true,
                    framework: Some("plugin".into()),
                },
                env: Vec::new(),
                debugger: Some(Debugger::Launch { language: "dart".into() }),
            });
        }

        configs
    }
}

impl TestResults for crate::FlutterPlugin {
    fn parse(&mut self, config: &str, chunk: &str) -> Vec<PluginTestEvent> {
        if self.last_test_config.as_deref() != Some(config) {
            self.last_test_config = Some(config.to_owned());
            self.test_parser.reset();
        }
        self.test_parser
            .push_chunk(chunk)
            .into_iter()
            .map(convert_test_event)
            .collect()
    }

    fn select_tests(&mut self, config: &str, tests: &[Vec<String>]) -> Option<Vec<String>> {
        if config != "flutter-test" && config != "flutter-integration-test" {
            return None;
        }
        build_test_filter_args(&["test", "--machine"], tests)
    }
}

fn convert_test_event(ev: ParsedTestEvent) -> PluginTestEvent {
    let to_loc = |l: dart_core::test_json::TestLocation| Location {
        path: l.path,
        range: Range::new(Position::new(l.line, l.column), Position::new(l.line, l.column)),
    };
    match ev {
        ParsedTestEvent::SuiteStarted { id, parent, name, location } => {
            PluginTestEvent::SuiteStarted(TestStart {
                id,
                parent,
                name,
                location: location.map(to_loc),
            })
        }
        ParsedTestEvent::TestStarted { id, parent, name, location } => {
            PluginTestEvent::TestStarted(TestStart {
                id,
                parent,
                name,
                location: location.map(to_loc),
            })
        }
        ParsedTestEvent::Passed { id, duration_ms } => {
            PluginTestEvent::Passed(TestPassed { id, duration_ms })
        }
        ParsedTestEvent::Failed {
            id,
            duration_ms,
            message,
            expected,
            actual,
            location,
        } => PluginTestEvent::Failed(TestFailed {
            id,
            duration_ms,
            message,
            expected,
            actual,
            location: location.map(to_loc),
        }),
        ParsedTestEvent::Skipped { id, reason } => {
            PluginTestEvent::Skipped(TestSkipped { id, reason })
        }
        ParsedTestEvent::Output { id, text } => {
            PluginTestEvent::Output(TestOutput { id, text })
        }
    }
}
