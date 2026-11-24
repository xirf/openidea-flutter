# Dart (`xirf.dart`) for OpenIDEA

Dart IDE tooling ported from `flutter/dart-intellij-third-party` for [OpenIDEA](https://github.com/l0ng-ai/openidea).

Source repository: https://github.com/xirf/openidea-flutter

## Features

- **Pub package management**: `dart pub get`, `pub upgrade`, `pub outdated`, `pub deps`, `pub add`, `pub remove`, and the **Pub Packages** bottom tool window.
- **`pubspec.yaml` intelligence**: diagnostics for invalid package names, duplicate dependencies, and missing `path:` dependencies; alphabetical dependency sorting quick fix; hover & path-dependency navigation; code lenses.
- **Test & run integration**: dynamic `LaunchConfigurations` (`dart-test`, `dart-run`, `build_runner build/watch`), streaming `dart test --reporter=json` tree & single/multi-test rerun filtering, and gutter run actions on `main()`, `test()`, and `group()`.
- **Dart tooling & editor**: `dart analyze --format=machine` diagnostics, `dart format`, `dart fix --apply`, semantic highlighting overlay for generic/static calls and `/// [SymbolRef]` links, import/region/doc folding, Dart snippets & file templates, FVM/SDK settings, and MCP tools (`dart_analyze`, `pub_outdated`).

## Capabilities & Why Each Is Requested

- `project.read` *(required)*: Read `pubspec.yaml`, `.fvm`, and `.dart` files for Pub inspection, `pubspec.yaml` diagnostics, and navigation.
- `editor.read` *(required)*: Read the active editor buffer for `dart format`, `sort-pubspec`, and semantic/provider hooks.
- `editor.write` *(required)*: Apply `dart format` and `sort-pubspec` edits to the focused editor.
- `events` *(required)*: Subscribe to `file-saved` events to refresh the Pub Packages view and optionally format on save.
- `process:dart` *(required)*: Execute `dart` CLI subcommands (`pub`, `analyze`, `format`, `fix`, `run`, `test`).
- `project.write` *(optional)*: Create paired `test/*_test.dart` files when requested.
- `process:fvm` *(optional)*: Invoke `fvm dart` when a `.fvm` directory is present and FVM support is enabled.
