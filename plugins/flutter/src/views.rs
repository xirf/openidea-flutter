use crate::commands::BUILD_TARGETS;
use dart_core::cli::DoctorStatus;
use dart_core::pubspec::Pubspec;
use openidea_plugin::views::{Body, Button, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{Error, Invocation, host};

pub const FLUTTER_TOOLS_VIEW: &str = "flutter-tools";

impl Views for crate::FlutterPlugin {
    fn render(&mut self, view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        if view != FLUTTER_TOOLS_VIEW {
            return Err(Error::NotFound(format!("unknown view {view}")));
        }
        let project = ctx
            .project()
            .or_else(|| host::projects().ok().and_then(|mut p| p.pop()));
        let Some(project) = project else {
            return Ok(View::message("Open a Flutter project to view devices, emulators, and build targets."));
        };

        let pubspec_text = project.read_file("pubspec.yaml").unwrap_or_default();
        let spec = Pubspec::parse(&pubspec_text);
        let mut items = Vec::new();

        // 1. Doctor & SDK section
        items.push(
            TreeItem::new("sec:doctor", "SDK & Doctor")
                .icon("info")
                .detail(if self.doctor.is_empty() {
                    "Click Doctor to check"
                } else {
                    "Last check"
                })
                .expandable(true),
        );
        if self.doctor.is_empty() {
            items.push(
                TreeItem::new("cmd:doctor", "Run flutter doctor")
                    .parent("sec:doctor")
                    .icon("run")
                    .detail("Inspect Android, iOS, web & desktop toolchains"),
            );
        } else {
            for (idx, sec) in self.doctor.iter().enumerate() {
                let badge = match sec.status {
                    DoctorStatus::Ok => "OK",
                    DoctorStatus::Warning => "Warn",
                    DoctorStatus::Error => "Error",
                };
                let icon = match sec.status {
                    DoctorStatus::Ok => "check",
                    DoctorStatus::Warning => "warning",
                    DoctorStatus::Error => "error",
                };
                items.push(
                    TreeItem::new(&format!("doc:{idx}"), &sec.title)
                        .parent("sec:doctor")
                        .icon(icon)
                        .detail(badge),
                );
            }
        }

        // 2. Devices
        items.push(
            TreeItem::new("sec:devices", "Devices")
                .icon("device")
                .detail(&format!("{}", self.devices.len()))
                .expandable(true),
        );
        if self.devices.is_empty() {
            items.push(
                TreeItem::new("cmd:refresh-devices", "Scan connected devices")
                    .parent("sec:devices")
                    .icon("refresh"),
            );
        } else {
            for dev in &self.devices {
                items.push(
                    TreeItem::new(&format!("dev:{}", dev.id), &dev.name)
                        .parent("sec:devices")
                        .icon("device")
                        .detail(&format!("{} · {}", dev.id, dev.platform)),
                );
            }
        }

        // 3. Emulators
        items.push(
            TreeItem::new("sec:emulators", "Emulators")
                .icon("mobile")
                .detail(&format!("{}", self.emulators.len()))
                .expandable(true),
        );
        if self.emulators.is_empty() {
            items.push(
                TreeItem::new("cmd:launch-emulator", "Choose & launch emulator...")
                    .parent("sec:emulators")
                    .icon("run"),
            );
        } else {
            for emu in &self.emulators {
                items.push(
                    TreeItem::new(&format!("emu:{}", emu.id), &emu.name)
                        .parent("sec:emulators")
                        .icon("run")
                        .detail(&format!("{} · {}", emu.manufacturer, emu.platform)),
                );
            }
        }

        // 4. Build Targets
        items.push(
            TreeItem::new("sec:builds", "Build Targets")
                .icon("build")
                .expandable(true),
        );
        for (target, label) in BUILD_TARGETS {
            items.push(
                TreeItem::new(&format!("build:{target}"), label)
                    .parent("sec:builds")
                    .icon("run")
                    .detail(&format!("flutter build {target}")),
            );
        }

        // 5. Quick Actions
        items.push(
            TreeItem::new("sec:actions", "Quick Actions")
                .icon("settings")
                .expandable(true),
        );
        for (cmd, label, detail) in [
            ("pub-get", "Flutter Pub Get", "flutter pub get"),
            ("pub-upgrade", "Flutter Pub Upgrade", "flutter pub upgrade"),
            ("gen-l10n", "Generate Localizations", "flutter gen-l10n"),
            ("clean", "Flutter Clean", "flutter clean"),
            ("widget-preview", "Start Widget Preview", "flutter widget-preview start"),
        ] {
            items.push(
                TreeItem::new(&format!("cmd:{cmd}"), label)
                    .parent("sec:actions")
                    .icon("run")
                    .detail(detail),
            );
        }

        let pkg = spec.name.as_ref().map(|(n, _)| n.as_str()).unwrap_or("Flutter");
        let status = if spec.is_flutter_project() {
            format!("{pkg} • {} devices", self.devices.len())
        } else {
            format!("{pkg} (no Flutter dependency in pubspec.yaml)")
        };

        Ok(View::new(Body::Tree(items))
            .button(Button::new("refresh", "Refresh").icon("refresh"))
            .button(Button::new("doctor", "Doctor").icon("info"))
            .button(Button::new("pub-get", "Pub Get").icon("download"))
            .button(Button::new("clean", "Clean").icon("remove"))
            .status(&status))
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        if view != FLUTTER_TOOLS_VIEW {
            return Ok(());
        }
        match event {
            ViewEvent::Button(id) => match id.as_str() {
                "refresh" => {
                    if let Ok(project) = crate::commands::active_project(ctx) {
                        crate::commands::refresh_devices_and_emulators(self, &project);
                    }
                    host::refresh_view(FLUTTER_TOOLS_VIEW);
                }
                "doctor" => {
                    crate::commands::run_doctor(self, ctx)?;
                }
                "pub-get" => {
                    crate::commands::run_flutter_pub(ctx, "get")?;
                }
                "clean" => {
                    crate::commands::run_simple(ctx, "flutter clean", &["clean"])?;
                }
                _ => {}
            },
            ViewEvent::Activate(key) => {
                if let Some(target) = key.strip_prefix("build:") {
                    crate::commands::run_build(ctx, target)?;
                } else if let Some(emu_id) = key.strip_prefix("emu:") {
                    let project = crate::commands::active_project(ctx)?;
                    let _ = crate::commands::run_flutter_captured(
                        &project,
                        &["emulators".into(), "--launch".into(), emu_id.to_owned()],
                    )?;
                } else if let Some(cmd) = key.strip_prefix("cmd:") {
                    match cmd {
                        "refresh-devices" => {
                            if let Ok(project) = crate::commands::active_project(ctx) {
                                crate::commands::refresh_devices_and_emulators(self, &project);
                            }
                            host::refresh_view(FLUTTER_TOOLS_VIEW);
                        }
                        "doctor" => {
                            crate::commands::run_doctor(self, ctx)?;
                        }
                        "launch-emulator" => {
                            crate::commands::launch_emulator(self, ctx)?;
                        }
                        "pub-get" => {
                            crate::commands::run_flutter_pub(ctx, "get")?;
                        }
                        "pub-upgrade" => {
                            crate::commands::run_flutter_pub(ctx, "upgrade")?;
                        }
                        "gen-l10n" => {
                            crate::commands::run_simple(ctx, "flutter gen-l10n", &["gen-l10n"])?;
                        }
                        "clean" => {
                            crate::commands::run_simple(ctx, "flutter clean", &["clean"])?;
                        }
                        "widget-preview" => {
                            crate::commands::run_widget_preview(ctx)?;
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
