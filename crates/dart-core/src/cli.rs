//! CLI executable resolution (SDK override / FVM) and parsers for Dart/Flutter
//! machine-readable and structured stdout formats.

use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutdatedPackage {
    pub name: String,
    pub current: Option<String>,
    pub upgradable: Option<String>,
    pub resolvable: Option<String>,
    pub latest: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnalyzeSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnalyzeIssue {
    pub severity: AnalyzeSeverity,
    pub kind: String,
    pub code: String,
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub length: u32,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceEntry {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub emulator: bool,
    pub is_supported: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmulatorEntry {
    pub id: String,
    pub name: String,
    pub manufacturer: String,
    pub platform: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DoctorStatus {
    Ok,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DoctorSection {
    pub status: DoctorStatus,
    pub title: String,
    pub details: Vec<String>,
}

/// Resolves `(program, prefix_args)` using FVM (`fvm dart`/`fvm flutter`), an explicit SDK bin/executable
/// override, or the default command on `PATH`.
pub fn resolve_program(
    tool: &str,
    sdk_override: Option<&str>,
    use_fvm: bool,
    has_fvm_dir: bool,
) -> (String, Vec<String>) {
    if let Some(raw) = sdk_override.map(str::trim).filter(|s| !s.is_empty()) {
        let trimmed = raw.trim_end_matches(['/', '\\']);
        let path = if trimmed.ends_with(tool) || trimmed.ends_with(&format!("{tool}.bat")) {
            trimmed.to_owned()
        } else if trimmed.ends_with("/bin") || trimmed.ends_with("\\bin") {
            format!("{trimmed}/{tool}")
        } else {
            format!("{trimmed}/bin/{tool}")
        };
        return (path, Vec::new());
    }
    if use_fvm && has_fvm_dir {
        return ("fvm".into(), vec![tool.to_owned()]);
    }
    (tool.to_owned(), Vec::new())
}

/// Parses `dart pub outdated --json` output.
pub fn parse_pub_outdated_json(json: &str) -> Vec<OutdatedPackage> {
    let Ok(val) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let Some(pkgs) = val.get("packages").and_then(Value::as_array) else {
        return Vec::new();
    };
    let version_of = |pkg: &Value, field: &str| -> Option<String> {
        pkg.get(field)
            .and_then(|v| v.get("version"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    pkgs.iter()
        .filter_map(|pkg| {
            let name = pkg.get("package")?.as_str()?.to_owned();
            Some(OutdatedPackage {
                name,
                current: version_of(pkg, "current"),
                upgradable: version_of(pkg, "upgradable"),
                resolvable: version_of(pkg, "resolvable"),
                latest: version_of(pkg, "latest"),
            })
        })
        .collect()
}

/// Parses `dart analyze --format=machine` lines:
/// `SEVERITY|TYPE|CODE|FILE|LINE|COL|LEN|MESSAGE`
pub fn parse_analyze_machine(output: &str) -> Vec<AnalyzeIssue> {
    let mut issues = Vec::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.splitn(8, '|').collect();
        if parts.len() != 8 {
            continue;
        }
        let severity = match parts[0].trim() {
            "ERROR" => AnalyzeSeverity::Error,
            "WARNING" => AnalyzeSeverity::Warning,
            "INFO" => AnalyzeSeverity::Info,
            _ => continue,
        };
        let Ok(line_num) = parts[4].trim().parse::<u32>() else {
            continue;
        };
        let Ok(col_num) = parts[5].trim().parse::<u32>() else {
            continue;
        };
        let length = parts[6].trim().parse::<u32>().unwrap_or(1).max(1);
        issues.push(AnalyzeIssue {
            severity,
            kind: parts[1].trim().to_owned(),
            code: parts[2].trim().to_owned(),
            file: parts[3].trim().to_owned(),
            line: line_num.saturating_sub(1),
            column: col_num.saturating_sub(1),
            length,
            message: parts[7].trim().to_owned(),
        });
    }
    issues
}

/// Parses `flutter devices --machine` JSON array.
pub fn parse_devices_json(output: &str) -> Vec<DeviceEntry> {
    let start = output.find('[').unwrap_or(0);
    let Ok(arr) = serde_json::from_str::<Vec<Value>>(&output[start..]) else {
        return Vec::new();
    };
    arr.into_iter()
        .filter_map(|v| {
            let id = v.get("id")?.as_str()?.to_owned();
            let name = v.get("name")?.as_str()?.to_owned();
            let platform = v
                .get("targetPlatform")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned();
            let emulator = v.get("emulator").and_then(Value::as_bool).unwrap_or(false);
            let is_supported = v.get("isSupported").and_then(Value::as_bool).unwrap_or(true);
            Some(DeviceEntry {
                id,
                name,
                platform,
                emulator,
                is_supported,
            })
        })
        .collect()
}

/// Parses `flutter emulators` stdout rows (`id • name • manufacturer • platform`).
pub fn parse_emulators_output(output: &str) -> Vec<EmulatorEntry> {
    let mut out = Vec::new();
    for line in output.lines() {
        if !line.contains('•') {
            continue;
        }
        let cols: Vec<&str> = line.split('•').map(str::trim).collect();
        if cols.len() >= 4 && !cols[0].is_empty() && !cols[0].eq_ignore_ascii_case("id") {
            out.push(EmulatorEntry {
                id: cols[0].to_owned(),
                name: cols[1].to_owned(),
                manufacturer: cols[2].to_owned(),
                platform: cols[3].to_owned(),
            });
        }
    }
    out
}

/// Parses `flutter doctor` output into sections.
pub fn parse_doctor_output(output: &str) -> Vec<DoctorSection> {
    let mut sections: Vec<DoctorSection> = Vec::new();
    for raw in output.lines() {
        let trimmed = raw.trim();
        if let Some((status, rest)) = parse_doctor_header(trimmed) {
            sections.push(DoctorSection {
                status,
                title: rest.trim().to_owned(),
                details: Vec::new(),
            });
        } else if let Some(last) = sections.last_mut()
            && (trimmed.starts_with('•') || trimmed.starts_with('!') || trimmed.starts_with('✗'))
        {
            last.details.push(trimmed.trim_start_matches(['•', '!', '✗']).trim().to_owned());
        }
    }
    sections
}

fn parse_doctor_header(line: &str) -> Option<(DoctorStatus, &str)> {
    for (prefix, status) in [
        ("[✓]", DoctorStatus::Ok),
        ("[+]", DoctorStatus::Ok),
        ("[!]", DoctorStatus::Warning),
        ("[✗]", DoctorStatus::Error),
        ("[X]", DoctorStatus::Error),
        ("[x]", DoctorStatus::Error),
    ] {
        if let Some(rest) = line.strip_prefix(prefix) {
            return Some((status, rest));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_sdk_and_fvm() {
        assert_eq!(resolve_program("flutter", None, true, true), ("fvm".into(), vec!["flutter".into()]));
        assert_eq!(resolve_program("dart", Some("/opt/dart-sdk"), true, true), ("/opt/dart-sdk/bin/dart".into(), vec![]));
        assert_eq!(resolve_program("dart", None, false, true), ("dart".into(), vec![]));
    }

    #[test]
    fn parses_outdated_analyze_devices_emulators_and_doctor() {
        let outdated = r#"{"packages":[{"package":"http","current":{"version":"1.1.0"},"upgradable":{"version":"1.2.2"},"resolvable":{"version":"1.2.2"},"latest":{"version":"1.3.0"}}]}"#;
        let pkgs = parse_pub_outdated_json(outdated);
        assert_eq!(pkgs.len(), 1);
        assert_eq!(pkgs[0].latest.as_deref(), Some("1.3.0"));

        let analyze = "WARNING|LINT|prefer_const_constructors|/proj/lib/main.dart|12|5|4|Use const.\n";
        let issues = parse_analyze_machine(analyze);
        assert_eq!(issues[0].severity, AnalyzeSeverity::Warning);
        assert_eq!(issues[0].line, 11);
        assert_eq!(issues[0].column, 4);

        let devices = parse_devices_json("[{\"id\":\"macos\",\"name\":\"macOS\",\"targetPlatform\":\"darwin\",\"emulator\":false,\"isSupported\":true}]");
        assert_eq!(devices[0].id, "macos");

        let emus = parse_emulators_output("Pixel_8_API_35 • Pixel 8 API 35 • Google • android\n");
        assert_eq!(emus[0].id, "Pixel_8_API_35");

        let doc = parse_doctor_output("[✓] Flutter (Channel stable, 3.24.0)\n[!] Android toolchain\n    ! cmdline-tools missing\n");
        assert_eq!(doc.len(), 2);
        assert_eq!(doc[0].status, DoctorStatus::Ok);
        assert_eq!(doc[1].status, DoctorStatus::Warning);
        assert_eq!(doc[1].details, vec!["cmdline-tools missing"]);
    }
}
