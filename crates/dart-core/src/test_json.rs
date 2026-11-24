//! Streaming parser for `package:test` JSON reporter (`dart test --reporter=json`
//! and `flutter test --machine`).

use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestLocation {
    pub path: String,
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParsedTestEvent {
    SuiteStarted {
        id: String,
        parent: Option<String>,
        name: String,
        location: Option<TestLocation>,
    },
    TestStarted {
        id: String,
        parent: Option<String>,
        name: String,
        location: Option<TestLocation>,
    },
    Passed {
        id: String,
        duration_ms: Option<u32>,
    },
    Failed {
        id: String,
        duration_ms: Option<u32>,
        message: String,
        expected: Option<String>,
        actual: Option<String>,
        location: Option<TestLocation>,
    },
    Skipped {
        id: String,
        reason: Option<String>,
    },
    Output {
        id: String,
        text: String,
    },
}

#[derive(Default)]
struct TestMeta {
    node_id: String,
    name: String,
    start_time: Option<u64>,
    location: Option<TestLocation>,
    hidden: bool,
    errors: Vec<String>,
    skipped: bool,
    skip_reason: Option<String>,
}

/// Stateful per-run parser for chunked `package:test` JSON lines.
#[derive(Default)]
pub struct TestJsonParser {
    pending: String,
    suites: HashMap<u64, String>,
    groups: HashMap<u64, String>,
    emitted_suites: HashSet<String>,
    tests: HashMap<u64, TestMeta>,
}

impl TestJsonParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.pending.clear();
        self.suites.clear();
        self.groups.clear();
        self.emitted_suites.clear();
        self.tests.clear();
    }

    pub fn push_chunk(&mut self, chunk: &str) -> Vec<ParsedTestEvent> {
        self.pending.push_str(chunk);
        let mut events = Vec::new();
        while let Some(newline) = self.pending.find('\n') {
            let line: String = self.pending.drain(..=newline).collect();
            self.parse_line(line.trim(), &mut events);
        }
        if self.pending.len() > 128 * 1024 {
            self.pending.clear();
        }
        events
    }

    fn parse_line(&mut self, line: &str, out: &mut Vec<ParsedTestEvent>) {
        if !line.starts_with('{') {
            return;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return;
        };
        let Some(kind) = value.get("type").and_then(Value::as_str) else {
            return;
        };
        let time = value.get("time").and_then(Value::as_u64);

        match kind {
            "suite" => {
                if let Some(suite) = value.get("suite")
                    && let Some(id) = suite.get("id").and_then(Value::as_u64)
                {
                    let path = suite.get("path").and_then(Value::as_str).unwrap_or("suite");
                    let node_id = format!("suite:{id}");
                    self.suites.insert(id, node_id.clone());
                    if self.emitted_suites.insert(node_id.clone()) {
                        let name = path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned();
                        out.push(ParsedTestEvent::SuiteStarted {
                            id: node_id,
                            parent: None,
                            name,
                            location: (!path.is_empty()).then(|| TestLocation {
                                path: path.to_owned(),
                                line: 0,
                                column: 0,
                            }),
                        });
                    }
                }
            }
            "group" => {
                if let Some(group) = value.get("group")
                    && let Some(id) = group.get("id").and_then(Value::as_u64)
                {
                    let name = group.get("name").and_then(Value::as_str).unwrap_or("");
                    let suite_id = group.get("suiteID").and_then(Value::as_u64);
                    let parent_id = group.get("parentID").and_then(Value::as_u64);
                    // Root implicit group has empty name; map it to the suite node.
                    if name.is_empty() {
                        if let Some(suite_node) = suite_id.and_then(|s| self.suites.get(&s)).cloned() {
                            self.groups.insert(id, suite_node);
                        }
                        return;
                    }
                    let node_id = format!("group:{id}");
                    let parent = parent_id
                        .and_then(|p| self.groups.get(&p).cloned())
                        .or_else(|| suite_id.and_then(|s| self.suites.get(&s).cloned()));
                    self.groups.insert(id, node_id.clone());
                    out.push(ParsedTestEvent::SuiteStarted {
                        id: node_id,
                        parent,
                        name: leaf_group_name(name).to_owned(),
                        location: extract_location(group),
                    });
                }
            }
            "testStart" => {
                if let Some(test) = value.get("test")
                    && let Some(id) = test.get("id").and_then(Value::as_u64)
                {
                    let name = test.get("name").and_then(Value::as_str).unwrap_or("test");
                    let metadata = test.get("metadata");
                    let skip = metadata
                        .and_then(|m| m.get("skip"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let skip_reason = metadata
                        .and_then(|m| m.get("skipReason"))
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    let hidden = name.starts_with("loading ") || name == "(setUpAll)" || name == "(tearDownAll)";
                    let suite_id = test.get("suiteID").and_then(Value::as_u64);
                    let group_ids = test.get("groupIDs").and_then(Value::as_array);
                    let parent = group_ids
                        .and_then(|arr| arr.iter().rev().filter_map(Value::as_u64).find_map(|g| self.groups.get(&g)))
                        .cloned()
                        .or_else(|| suite_id.and_then(|s| self.suites.get(&s).cloned()));
                    let location = extract_location(test);
                    let node_id = format!("test:{id}");
                    let short_name = short_test_name(name);
                    self.tests.insert(
                        id,
                        TestMeta {
                            node_id: node_id.clone(),
                            name: short_name.clone(),
                            start_time: time,
                            location: location.clone(),
                            hidden,
                            errors: Vec::new(),
                            skipped: skip,
                            skip_reason,
                        },
                    );
                    if !hidden {
                        out.push(ParsedTestEvent::TestStarted {
                            id: node_id,
                            parent,
                            name: short_name,
                            location,
                        });
                    }
                }
            }
            "print" => {
                if let Some(id) = value.get("testID").and_then(Value::as_u64)
                    && let Some(msg) = value.get("message").and_then(Value::as_str)
                    && let Some(meta) = self.tests.get(&id)
                    && !meta.hidden
                {
                    out.push(ParsedTestEvent::Output {
                        id: meta.node_id.clone(),
                        text: format!("{msg}\n"),
                    });
                }
            }
            "error" => {
                if let Some(id) = value.get("testID").and_then(Value::as_u64)
                    && let Some(err) = value.get("error").and_then(Value::as_str)
                    && let Some(meta) = self.tests.get_mut(&id)
                {
                    let stack = value.get("stackTrace").and_then(Value::as_str).unwrap_or("");
                    if stack.is_empty() {
                        meta.errors.push(err.to_owned());
                    } else {
                        meta.errors.push(format!("{err}\n{stack}"));
                    }
                    if !meta.hidden {
                        out.push(ParsedTestEvent::Output {
                            id: meta.node_id.clone(),
                            text: format!("{err}\n{stack}\n"),
                        });
                    }
                }
            }
            "testDone" => {
                if let Some(id) = value.get("testID").and_then(Value::as_u64)
                    && let Some(meta) = self.tests.remove(&id)
                {
                    let hidden = meta.hidden
                        || value.get("hidden").and_then(Value::as_bool).unwrap_or(false);
                    if hidden {
                        return;
                    }
                    let skipped = meta.skipped
                        || value.get("skipped").and_then(Value::as_bool).unwrap_or(false);
                    let result = value.get("result").and_then(Value::as_str).unwrap_or("success");
                    let duration_ms = match (meta.start_time, time) {
                        (Some(s), Some(e)) if e >= s => u32::try_from(e - s).ok(),
                        _ => None,
                    };
                    if skipped {
                        out.push(ParsedTestEvent::Skipped {
                            id: meta.node_id,
                            reason: meta.skip_reason,
                        });
                    } else if result == "success" && meta.errors.is_empty() {
                        out.push(ParsedTestEvent::Passed {
                            id: meta.node_id,
                            duration_ms,
                        });
                    } else {
                        let message = if meta.errors.is_empty() {
                            format!("{} failed ({result})", meta.name)
                        } else {
                            meta.errors.join("\n")
                        };
                        let (expected, actual) = extract_expected_actual(&message);
                        out.push(ParsedTestEvent::Failed {
                            id: meta.node_id,
                            duration_ms,
                            message,
                            expected,
                            actual,
                            location: meta.location,
                        });
                    }
                }
            }
            _ => {}
        }
    }
}

/// Converts selected test-tree paths (`[[suite, group..., test]]`) into replacement CLI args.
pub fn build_test_filter_args(base_args: &[&str], tests: &[Vec<String>]) -> Option<Vec<String>> {
    if tests.is_empty() {
        return None;
    }
    let mut args: Vec<String> = base_args.iter().map(|s| (*s).to_owned()).collect();
    let mut names = Vec::new();
    let mut file_target: Option<String> = None;
    for path in tests {
        if let Some(first) = path.first()
            && first.ends_with("_test.dart")
            && file_target.is_none()
            && path.len() == 1
        {
            file_target = Some(first.clone());
        } else if let Some(leaf) = path.last()
            && !leaf.is_empty()
        {
            names.push(leaf.clone());
        }
    }
    if names.len() == 1 {
        args.push("--plain-name".into());
        args.push(names.remove(0));
        return Some(args);
    }
    if names.len() > 1 {
        let pattern = names
            .iter()
            .map(|n| regex_escape(n))
            .collect::<Vec<_>>()
            .join("|");
        args.push("--name".into());
        args.push(pattern);
        return Some(args);
    }
    if let Some(file) = file_target {
        args.push(file);
        return Some(args);
    }
    None
}

fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if matches!(ch, '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn leaf_group_name(full: &str) -> &str {
    full.rsplit(" ").next().filter(|_| false).unwrap_or(full)
}

fn short_test_name(full: &str) -> String {
    full.to_owned()
}

fn extract_location(obj: &Value) -> Option<TestLocation> {
    let url = obj
        .get("root_url")
        .or_else(|| obj.get("url"))
        .and_then(Value::as_str)?;
    let path = url.strip_prefix("file://").unwrap_or(url).to_owned();
    let line = obj
        .get("root_line")
        .or_else(|| obj.get("line"))
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .saturating_sub(1) as u32;
    let column = obj
        .get("root_column")
        .or_else(|| obj.get("column"))
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .saturating_sub(1) as u32;
    Some(TestLocation { path, line, column })
}

fn extract_expected_actual(message: &str) -> (Option<String>, Option<String>) {
    let mut expected = None;
    let mut actual = None;
    for line in message.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Expected:") {
            expected = Some(rest.trim().to_owned());
        } else if let Some(rest) = trimmed.strip_prefix("Actual:") {
            actual = Some(rest.trim().to_owned());
        }
    }
    (expected, actual)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_split_chunks_groups_failures_and_skips() {
        let mut parser = TestJsonParser::new();
        let part1 = r#"{"type":"suite","suite":{"id":0,"path":"test/widget_test.dart"}}
{"type":"group","group":{"id":1,"suiteID":0,"name":""}}
{"type":"group","group":{"id":2,"suiteID":0,"parentID":1,"name":"Counter"}}
{"type":"testStart","test":{"id":3,"name":"loading test/widget_test.dart","suiteID":0,"groupIDs":[]}}
{"type":"testDone","testID":3,"result":"success","hidden":true}
{"type":"testStart","test":{"id":4,"name":"increments","suiteID":0,"groupIDs":[2],"url":"file:///proj/test/widget_test.dart","line":10,"column":3},"time":100}
{"type":"error","testID":4,"error":"Expected: <1>\n  Actual: <0>","stackTrace":"test/widget_test.dart 12:5"}
{"type":"testDone","testID":4,"result":"failure","hidden":false,"time":142}
{"type":"testStart","test":{"id":5,"name":"skipped case","suiteID":0,"groupIDs":[2],"metadata":{"skip":true,"skipReason":"todo"}},"time":150}
{"type":"testDone","testID":5,"result":"success","skipped":true,"hidden":false,"time":151}
"#;
        // Split across a line boundary to test buffering.
        let mid = part1.len() / 2;
        let mut events = parser.push_chunk(&part1[..mid]);
        events.extend(parser.push_chunk(&part1[mid..]));

        assert!(events.iter().any(|e| matches!(e, ParsedTestEvent::SuiteStarted { name, .. } if name == "widget_test.dart")));
        assert!(events.iter().any(|e| matches!(e, ParsedTestEvent::SuiteStarted { name, .. } if name == "Counter")));
        assert!(!events.iter().any(|e| matches!(e, ParsedTestEvent::TestStarted { name, .. } if name.starts_with("loading "))));
        assert!(events.iter().any(|e| matches!(
            e,
            ParsedTestEvent::Failed { duration_ms: Some(42), expected: Some(exp), actual: Some(act), .. }
            if exp == "<1>" && act == "<0>"
        )));
        assert!(events.iter().any(|e| matches!(
            e,
            ParsedTestEvent::Skipped { reason: Some(r), .. } if r == "todo"
        )));
    }

    #[test]
    fn builds_single_and_multi_test_filter_args() {
        assert_eq!(
            build_test_filter_args(&["test", "--machine"], &[vec!["Counter".into(), "adds one".into()]]),
            Some(vec!["test".into(), "--machine".into(), "--plain-name".into(), "adds one".into()])
        );
        assert_eq!(
            build_test_filter_args(&["test", "--reporter=json"], &[vec!["a (1)".into()], vec!["b".into()]]),
            Some(vec!["test".into(), "--reporter=json".into(), "--name".into(), r"a \(1\)|b".into()])
        );
    }
}
