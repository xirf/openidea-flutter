use dart_core::cli::resolve_program;
use dart_core::pubspec::Pubspec;
use dart_core::test_json::{ParsedTestEvent, build_test_filter_args};
use openidea_plugin::{
    Debugger, LaunchConfigurations, Location, PluginLaunchConfiguration, PluginRunConfiguration,
    PluginTestEvent, Position, Range, TestFailed, TestPassed, TestResults, TestSkipped, TestStart,
    TestOutput, host,
};

impl LaunchConfigurations for crate::DartPlugin {
    fn launch_configurations(&mut self, project_root: &str) -> Vec<PluginLaunchConfiguration> {
        let Some(project) = host::project_of(project_root) else {
            return Vec::new();
        };
        let pubspec_text = project.read_file("pubspec.yaml").unwrap_or_default();
        let spec = Pubspec::parse(&pubspec_text);
        let sdk_override = crate::commands::setting_str("sdkPath");
        let use_fvm = crate::highlighting::setting_bool("useFvm", true);
        let has_fvm = project.exists(".fvm").unwrap_or(false);
        let (dart_prog, mut dart_prefix) =
            resolve_program("dart", sdk_override.as_deref(), use_fvm, has_fvm);

        let mut configs = Vec::new();

        // Dart test configuration (skipped if Flutter project owns widget tests, still available if pure Dart or package)
        let mut test_args = dart_prefix.clone();
        test_args.extend(["test".into(), "--reporter=json".into()]);
        configs.push(PluginLaunchConfiguration {
            config: PluginRunConfiguration {
                id: "dart-test".into(),
                name: "Dart Tests".into(),
                program: dart_prog.clone(),
                args: test_args,
                cwd: None,
                test: true,
                framework: Some("plugin".into()),
            },
            env: Vec::new(),
            debugger: Some(Debugger::Launch { language: "dart".into() }),
        });

        // Dart CLI entry point if bin/ or lib/main.dart exists
        let entry = ["bin/main.dart", "lib/main.dart"]
            .into_iter()
            .find(|p| project.exists(p).unwrap_or(false));
        if let Some(entry) = entry {
            let mut run_args = dart_prefix.clone();
            run_args.extend(["run".into(), entry.into()]);
            configs.push(PluginLaunchConfiguration {
                config: PluginRunConfiguration {
                    id: "dart-run".into(),
                    name: format!("Dart Run ({entry})"),
                    program: dart_prog.clone(),
                    args: run_args,
                    cwd: None,
                    test: false,
                    framework: None,
                },
                env: Vec::new(),
                debugger: Some(Debugger::Launch { language: "dart".into() }),
            });
        }

        if spec.has_build_runner() {
            let mut br_build = dart_prefix.clone();
            br_build.extend([
                "run".into(),
                "build_runner".into(),
                "build".into(),
                "--delete-conflicting-outputs".into(),
            ]);
            configs.push(PluginLaunchConfiguration {
                config: PluginRunConfiguration {
                    id: "build-runner-build".into(),
                    name: "build_runner build".into(),
                    program: dart_prog.clone(),
                    args: br_build,
                    cwd: None,
                    test: false,
                    framework: None,
                },
                env: Vec::new(),
                debugger: None,
            });

            dart_prefix.extend([
                "run".into(),
                "build_runner".into(),
                "watch".into(),
                "--delete-conflicting-outputs".into(),
            ]);
            configs.push(PluginLaunchConfiguration {
                config: PluginRunConfiguration {
                    id: "build-runner-watch".into(),
                    name: "build_runner watch".into(),
                    program: dart_prog,
                    args: dart_prefix,
                    cwd: None,
                    test: false,
                    framework: None,
                },
                env: Vec::new(),
                debugger: None,
            });
        }

        configs
    }
}

impl TestResults for crate::DartPlugin {
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
        if config != "dart-test" {
            return None;
        }
        build_test_filter_args(&["test", "--reporter=json"], tests)
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
