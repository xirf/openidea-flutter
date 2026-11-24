# OpenIDEA Dart & Flutter Plugins (`xirf.dart` & `xirf.flutter`)

Standalone [OpenIDEA](https://github.com/l0ng-ai/openidea) WebAssembly component plugins (`wasm32-wasip2`, OpenIDEA Plugin API `^1.12`, built against `v0.1.50`) porting the IDE features of [`flutter/dart-intellij-third-party`](https://github.com/flutter/dart-intellij-third-party) and [`flutter/flutter-intellij`](https://github.com/flutter/flutter-intellij).

Repository: https://github.com/xirf/openidea-flutter

## What OpenIDEA Core Already Ships vs. What These Plugins Port

OpenIDEA (`l0ng-ai/openidea`) natively includes:
- `languages/dart.toml` (`dart language-server --protocol=lsp`, `dart` / `flutter debug_adapter`, Tree-sitter Dart outline/locals)
- `crates/ide-flutter` & `src/features/*` (`flutter daemon` device chooser, `flutter run` config, Hot Reload / Hot Restart, Flutter Structure widget tree, embedded DevTools tool windows, `dart` / `flutter` project generators)

These two plugins mirror IntelliJ's `Dart` -> `io.flutter` dependency split and add the missing workflows:

| Area | `xirf.dart` (`plugins/dart`) | `xirf.flutter` (`plugins/flutter`) |
| --- | --- | --- |
| **Pub & CLI** | `pub get/upgrade/outdated/deps/add/remove`, `dart analyze/format/fix/run/test`, `sort-pubspec`, paired test creation | `flutter pub get/upgrade/outdated`, `clean`, `doctor`, `gen-l10n`, `upgrade`, `build <target>` (APK/AAB/iOS/IPA/Web/macOS/Linux/Windows), `run --profile`/`--release`, `attach`, `widget-preview`, `launch-emulator` |
| **Run & Test Tree** | Dynamic `LaunchConfigurations` (`dart-test`, `dart-run`, `build_runner build/watch`) + `dart test --reporter=json` `TestResults` parser & single/multi-test filtering | Dynamic `LaunchConfigurations` (`flutter-test`, `flutter-run-profile`, `flutter-run-release`, `flutter-attach`, `flutter-integration-test`) + `flutter test --machine` `TestResults` parser & filtering |
| **Highlighting** | Non-clobbering Dart overlay for function/method declarations & calls (including generic calls like `readList<String>(...)` and static calls), `/// [Ref]` doc links, and `pubspec.yaml` semantic tokens | Non-clobbering Flutter overlay for widget classes, `Colors.*`, `CupertinoColors.*`, `Icons.*`, and `CupertinoIcons.*` |
| **Editor Intelligence** | `pubspec.yaml` diagnostics & dependency sort quick fix, `pubspec.yaml` hover & path-dep Go-to-Definition, import/region/doc folding, Code Lenses, `main()`/`test()` gutter actions | Closing labels (`// Container`, `// Column`, ...), `Color(0x...)`/`Colors.*`/`CupertinoColors.*` swatches & presentations, `Icons.*`/`CupertinoIcons.*` completion/hover/markers, `Extract Flutter Widget...`, Wrap/Remove widget, `StatelessWidget <-> StatefulWidget`, asset links & Go-to-Definition |
| **Tool Windows & Settings** | **Pub Packages** bottom tool window + `xirf.dart.tools` extension point + SDK/FVM & Editor settings pages | **Flutter** right tool window (Doctor, Devices, Emulators, Build Targets, Quick Actions) + SDK/Builds & Editor/Widgets settings pages |
| **Snippets & Templates** | `main`, `amain`, `test`, `atest`, `group`, `setup`, `teardown`, `switche`, `sealed` + 3 file templates | `stless`, `stful`, `stanim`, `build`, `initS`, `dis`, `didChangeD`, `didUpdateW`, `setS`, `streamBldr`, `futureBldr`, `listViewB`, `gridViewB`, `customPainter`, `testW`, `inhWidget` + 3 widget/test file templates |
| **MCP Tools** | `dart_analyze`, `pub_outdated` | `flutter_doctor`, `flutter_devices` |

## Capabilities & Security

Both plugins are sandboxed WebAssembly components (`wasm32-wasip2`) that request only the minimum capabilities required for Dart/Flutter IDE workflows:

- **`project.read`** *(required, both)*: Read `pubspec.yaml`, `.fvm`, `.dart` files, and Flutter asset paths inside the open project.
- **`project.write`** *(optional, `xirf.dart`)*: Create paired `test/*_test.dart` files via the **Create Paired Test File** action.
- **`editor.read` / `editor.write`** *(required, both)*: Read the active editor buffer and apply user-triggered edits (`dart format`, `sort-pubspec`, **Extract Flutter Widget**, Wrap/Remove widget, `StatelessWidget <-> StatefulWidget`).
- **`events`** *(required, both)*: Listen for `file-saved` events to refresh the **Pub Packages** / **Flutter** tool windows and run optional format-on-save.
- **`process:dart`** *(required, `xirf.dart`)*: Invoke the `dart` CLI (`pub`, `analyze`, `format`, `fix`, `run`, `test`).
- **`process:flutter`** *(required, `xirf.flutter`)*: Invoke the `flutter` CLI (`pub`, `clean`, `doctor`, `gen-l10n`, `build`, `run`, `attach`, `test`, `emulators`, `devices`).
- **`process:fvm`** *(optional, both)*: Invoke `fvm dart` / `fvm flutter` when a `.fvm` directory is present and FVM support is enabled in settings.

## Workspace Layout

- `crates/dart-core` — Shared pure-Rust `pubspec.yaml` parser/validator/sorter, `package:test` JSON reporter parser, Dart/pubspec lexer & overlay semantic token generator, Flutter widget closing-label & AST-less refactoring transforms, Material/Cupertino Color & Icon catalogs, and CLI/FVM parsers.
- `plugins/dart` — `xirf.dart` WASM component plugin.
- `plugins/flutter` — `xirf.flutter` WASM component plugin (depends on `xirf.dart`).
- `scripts/build.sh` — Builds both release `wasm32-wasip2` components into `plugins/*/dist/plugin.wasm` and runs `openidea-plugin check` if installed.

## Build, Test & Install in OpenIDEA

```sh
# Run lints and all unit + MockHost integration tests
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Build wasm32-wasip2 components into plugins/dart/dist/plugin.wasm and plugins/flutter/dist/plugin.wasm
./scripts/build.sh

# Link both plugins locally during development
openidea-plugin link plugins/dart
openidea-plugin link plugins/flutter
```
