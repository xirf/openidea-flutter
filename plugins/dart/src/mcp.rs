use crate::commands::{resolve_dart_tool, run_tool_captured};
use dart_core::cli::{AnalyzeSeverity, parse_analyze_machine, parse_pub_outdated_json};
use openidea_plugin::{Error, Invocation, McpProvider, host};
use serde_json::{Value, json};

impl McpProvider for crate::DartPlugin {
    fn call(&mut self, tool: &str, arguments: &str, ctx: &Invocation) -> Result<String, Error> {
        let project = ctx
            .project()
            .or_else(|| host::projects().ok().and_then(|mut p| p.pop()))
            .ok_or_else(|| Error::Failed("No open Dart project".into()))?;
        let args_val: Value = serde_json::from_str(arguments).unwrap_or_else(|_| json!({}));

        match tool {
            "dart_analyze" => {
                let (prog, prefix) = resolve_dart_tool(&project);
                let mut args = prefix;
                args.extend(["analyze".into(), "--format=machine".into()]);
                if let Some(rel) = args_val.get("path").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    args.push(rel.to_owned());
                }
                let out = run_tool_captured(&project, &prog, &args)?;
                let combined = format!("{}\n{}", out.stdout, out.stderr);
                let issues = parse_analyze_machine(&combined);
                let payload = json!({
                    "exitCode": out.status,
                    "count": issues.len(),
                    "issues": issues.iter().map(|i| json!({
                        "severity": match i.severity {
                            AnalyzeSeverity::Error => "error",
                            AnalyzeSeverity::Warning => "warning",
                            AnalyzeSeverity::Info => "info",
                        },
                        "code": i.code,
                        "file": i.file,
                        "line": i.line + 1,
                        "column": i.column + 1,
                        "message": i.message,
                    })).collect::<Vec<_>>()
                });
                Ok(payload.to_string())
            }
            "pub_outdated" => {
                let (prog, prefix) = resolve_dart_tool(&project);
                let mut args = prefix;
                args.extend(["pub".into(), "outdated".into(), "--json".into()]);
                let out = run_tool_captured(&project, &prog, &args)?;
                let pkgs = parse_pub_outdated_json(&out.stdout);
                let payload = json!({
                    "packages": pkgs.iter().map(|p| json!({
                        "package": p.name,
                        "current": p.current,
                        "upgradable": p.upgradable,
                        "resolvable": p.resolvable,
                        "latest": p.latest,
                    })).collect::<Vec<_>>()
                });
                Ok(payload.to_string())
            }
            _ => Err(Error::NotFound(format!("unknown MCP tool {tool}"))),
        }
    }
}
