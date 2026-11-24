# Flutter (`xirf.flutter`) for OpenIDEA

Flutter IDE tooling ported from `flutter/flutter-intellij` for [OpenIDEA](https://github.com/l0ng-ai/openidea). Depends on [`xirf.dart`](https://github.com/xirf/openidea-flutter).

Source repository: https://github.com/xirf/openidea-flutter

## Features

- **Widget productivity**: Flutter closing labels (`// Container`, `// Column`, ...), **Extract Flutter Widget...**, Wrap with `Container`/`Padding`/`Center`/`SizedBox`/`Column`/`Row`/`Expanded`/`Flexible`/`Builder`/`StreamBuilder`/`FutureBuilder`, Remove widget wrapper, and `StatelessWidget` <-> `StatefulWidget` conversions.
- **Colors & Icons**: `Color(0x...)`, `Color.fromARGB`, `Color.fromRGBO`, Material `Colors.*`, and `CupertinoColors.*` swatches + presentations; Material `Icons.*` & `CupertinoIcons.*` hover metadata, completions, and gutter markers.
- **Flutter CLI & builds**: `flutter pub get/upgrade/outdated`, `clean`, `doctor`, `gen-l10n`, `upgrade`, `build` targets (`apk`, `appbundle`, `ios`, `ipa`, `web`, `macos`, `linux`, `windows`), `run --profile`/`--release`, `attach`, `widget-preview`, and emulator launcher.
- **Test tree & tool window**: dynamic `LaunchConfigurations` (`flutter-test`, `flutter-run-profile`, `flutter-run-release`, `flutter-attach`, `flutter-integration-test`), streaming `flutter test --machine` parser, **Flutter** right tool window, FVM/SDK & editor settings, Flutter snippets & widget templates, and MCP tools (`flutter_doctor`, `flutter_devices`).

## Capabilities & Why Each Is Requested

- `project.read` *(required)*: Read `pubspec.yaml`, `.fvm`, and `assets/` paths for Flutter project detection, asset validation, and Go-to-Definition.
- `editor.read` *(required)*: Read the active Dart buffer for widget refactorings, closing labels, colors, and icon hints.
- `editor.write` *(required)*: Apply Extract Widget, wrap/unwrap, and `StatelessWidget <-> StatefulWidget` refactorings.
- `events` *(required)*: Subscribe to `file-saved` events to refresh the Flutter tool window when `pubspec.yaml` changes.
- `process:flutter` *(required)*: Execute `flutter` CLI subcommands (`pub`, `clean`, `doctor`, `gen-l10n`, `build`, `run`, `attach`, `test`, `emulators`, `devices`).
- `process:fvm` *(optional)*: Invoke `fvm flutter` when a `.fvm` directory is present and FVM support is enabled.
