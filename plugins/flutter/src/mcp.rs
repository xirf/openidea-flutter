use crate::commands::{active_project, run_flutter_captured};
use dart_core::cli::{DoctorStatus, parse_devices_json, parse_doctor_output};
use openidea_plugin::{Error, Invocation, McpProvider};
use serde_json::{Value, json};

impl McpProvider for crate::FlutterPlugin {
    fn call(&mut self, tool: &str, arguments: &str, ctx: &Invocation) -> Result<String, Error> {
        let project = active_project(ctx)?;
        let args_val: Value = serde_json::from_str(arguments).unwrap_or_else(|_| json!({}));

        match tool {
            "flutter_doctor" => {
                let verbose = args_val
                    .get("verbose")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let mut args = vec!["doctor".to_owned()];
                if verbose {
                    args.push("-v".into());
                }
                let out = run_flutter_captured(&project, &args)?;
                let sections = parse_doctor_output(&out.stdout);
                let payload = json!({
                    "exitCode": out.status,
                    "sections": sections.iter().map(|s| json!({
                        "status": match s.status {
                            DoctorStatus::Ok => "ok",
                            DoctorStatus::Warning => "warning",
                            DoctorStatus::Error => "error",
                        },
                        "title": s.title,
                        "details": s.details,
                    })).collect::<Vec<_>>(),
                    "raw": out.stdout,
                });
                Ok(payload.to_string())
            }
            "flutter_devices" => {
                let out = run_flutter_captured(&project, &["devices".into(), "--machine".into()])?;
                let devices = parse_devices_json(&out.stdout);
                let payload = json!({
                    "count": devices.len(),
                    "devices": devices.iter().map(|d| json!({
                        "id": d.id,
                        "name": d.name,
                        "platform": d.platform,
                        "emulator": d.emulator,
                        "isSupported": d.is_supported,
                    })).collect::<Vec<_>>()
                });
                Ok(payload.to_string())
            }
            _ => Err(Error::NotFound(format!("unknown MCP tool {tool}"))),
        }
    }
}
