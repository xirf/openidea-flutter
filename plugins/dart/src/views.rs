use dart_core::pubspec::{DepSection, Pubspec};
use openidea_plugin::views::{Body, Button, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{Error, Invocation, Position, Range, host};

pub const PUB_PACKAGES_VIEW: &str = "pub-packages";

impl Views for crate::DartPlugin {
    fn render(&mut self, view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        if view != PUB_PACKAGES_VIEW {
            return Err(Error::NotFound(format!("unknown view {view}")));
        }

        let project = ctx
            .project()
            .or_else(|| host::projects().ok().and_then(|mut p| p.pop()));
        let Some(project) = project else {
            return Ok(View::message("Open a Dart or Flutter project to inspect Pub packages."));
        };

        let Ok(yaml) = project.read_file("pubspec.yaml") else {
            return Ok(View::message("No pubspec.yaml found in project root."));
        };

        let spec = Pubspec::parse(&yaml);
        let mut items = Vec::new();
        let pkg_name = spec
            .name
            .as_ref()
            .map(|(n, _)| n.as_str())
            .unwrap_or("pubspec.yaml");

        items.push(
            TreeItem::new("sec:deps", "Dependencies")
                .icon("folder")
                .detail(&count_detail(&spec, DepSection::Dependencies))
                .expandable(true),
        );
        items.push(
            TreeItem::new("sec:dev", "Dev Dependencies")
                .icon("folder")
                .detail(&count_detail(&spec, DepSection::DevDependencies))
                .expandable(true),
        );
        if spec
            .dependencies
            .iter()
            .any(|d| d.section == DepSection::DependencyOverrides)
        {
            items.push(
                TreeItem::new("sec:overrides", "Dependency Overrides")
                    .icon("folder")
                    .detail(&count_detail(&spec, DepSection::DependencyOverrides))
                    .expandable(true),
            );
        }

        for dep in &spec.dependencies {
            let parent = match dep.section {
                DepSection::Dependencies => "sec:deps",
                DepSection::DevDependencies => "sec:dev",
                DepSection::DependencyOverrides => "sec:overrides",
            };
            let mut detail = dep
                .constraint
                .clone()
                .or_else(|| dep.sdk.as_ref().map(|s| format!("sdk: {s}")))
                .or_else(|| dep.path.as_ref().map(|p| format!("path: {p}")))
                .or_else(|| dep.git_url.as_ref().map(|g| format!("git: {g}")))
                .unwrap_or_else(|| "any".into());

            if let Some(out) = self.outdated.get(&dep.name)
                && let Some(latest) = &out.latest
                && out.current.as_deref() != Some(latest.as_str())
            {
                let curr = out.current.as_deref().unwrap_or("?");
                detail = format!("{detail}  ({curr} -> {latest})");
            }

            items.push(
                TreeItem::new(&format!("dep:{}:{}", dep.line, dep.name), &dep.name)
                    .parent(parent)
                    .icon("package")
                    .detail(&detail),
            );
        }

        if let Ok(contribs) = host::extension_contributions("xirf.dart.tools")
            && !contribs.is_empty()
        {
            items.push(
                TreeItem::new("sec:ext", "Extension Tools")
                    .icon("settings")
                    .expandable(true),
            );
            for (idx, c) in contribs.iter().enumerate() {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&c.data)
                    && let (Some(title), Some(cmd)) = (
                        val.get("title").and_then(|v| v.as_str()),
                        val.get("command").and_then(|v| v.as_str()),
                    )
                {
                    let desc = val.get("description").and_then(|v| v.as_str()).unwrap_or("");
                    items.push(
                        TreeItem::new(&format!("ext:{idx}:{}:{cmd}", c.plugin), title)
                            .parent("sec:ext")
                            .icon("run")
                            .detail(desc),
                    );
                }
            }
        }

        let sdk_str = spec
            .sdk_constraint
            .as_ref()
            .map(|(s, _)| s.as_str())
            .unwrap_or("unspecified");
        Ok(View::new(Body::Tree(items))
            .button(Button::new("pub-get", "Pub Get").icon("download"))
            .button(Button::new("pub-upgrade", "Upgrade").icon("refresh"))
            .button(Button::new("pub-outdated", "Outdated").icon("search"))
            .button(Button::new("pub-add", "Add").icon("add"))
            .button(Button::new("refresh", "Refresh").icon("refresh"))
            .status(&format!("{pkg_name} • sdk: {sdk_str} • {} packages", spec.dependencies.len())))
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        if view != PUB_PACKAGES_VIEW {
            return Ok(());
        }
        match event {
            ViewEvent::Button(id) => match id.as_str() {
                "pub-get" => {
                    crate::commands::run_pub_command(self, ctx, "get")?;
                }
                "pub-upgrade" => {
                    crate::commands::run_pub_command(self, ctx, "upgrade")?;
                }
                "pub-outdated" => {
                    crate::commands::run_pub_outdated(self, ctx)?;
                }
                "pub-add" => {
                    crate::commands::run_pub_add(ctx)?;
                }
                "refresh" => {
                    host::refresh_view(PUB_PACKAGES_VIEW);
                }
                _ => {}
            },
            ViewEvent::Activate(key) => {
                if let Some(rest) = key.strip_prefix("dep:")
                    && let Some((line_str, _)) = rest.split_once(':')
                    && let Ok(line) = line_str.parse::<u32>()
                    && let Some(project) = ctx.project()
                {
                    let root = project.root()?;
                    let pubspec_path = format!("{}/pubspec.yaml", root.trim_end_matches('/'));
                    let pos = Position::new(line, 0);
                    let _ = host::open_editor(&pubspec_path, Some(Range::new(pos, pos)));
                } else if let Some(rest) = key.strip_prefix("ext:")
                    && let Some((_, plugin_and_cmd)) = rest.split_once(':')
                    && let Some((plugin_id, cmd_id)) = plugin_and_cmd.split_once(':')
                {
                    let full = format!("plugin.{plugin_id}.{cmd_id}");
                    let _ = host::execute_command(&full, None);
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn count_detail(spec: &Pubspec, section: DepSection) -> String {
    let n = spec.dependencies.iter().filter(|d| d.section == section).count();
    format!("{n}")
}
