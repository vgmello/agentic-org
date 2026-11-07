//! `project.json` and `agent.json`, the machine-readable profiles hr.py writes.
//! Unknown fields are kept; a field with the wrong type is an issue and is left `None`.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::model::{AgentProfile, AgentStatus, Issue, Parsed, ProblemKind, ProjectProfile, Tools};

/// Reads the fields of a JSON object one by one, recording what's wrong.
struct Fields<'a> {
    object: &'a Map<String, Value>,
    issues: Vec<Issue>,
}

impl<'a> Fields<'a> {
    /// A string field. `null` and a missing field are `None`; a missing required
    /// field and any non-string value are issues.
    fn string(&mut self, key: &str, required: bool) -> Option<String> {
        match self.object.get(key) {
            Some(Value::String(s)) => Some(s.clone()),
            Some(Value::Null) | None => {
                if required {
                    self.issues.push(Issue::warning(
                        ProblemKind::InvalidField,
                        format!("missing field '{key}'"),
                    ));
                }
                None
            }
            Some(other) => {
                self.issues.push(Issue::warning(
                    ProblemKind::InvalidField,
                    format!("'{key}' should be a string, not {}", kind_of(other)),
                ));
                None
            }
        }
    }

    fn extra(&self, known: &[&str]) -> BTreeMap<String, Value> {
        self.object
            .iter()
            .filter(|(k, _)| !known.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "a list",
        Value::Object(_) => "an object",
    }
}

/// Parses JSON text into an object, or explains why it can't.
fn object(text: &str) -> Result<Map<String, Value>, Issue> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(other) => Err(Issue::error(
            ProblemKind::Malformed,
            format!("expected a JSON object, found {}", kind_of(&other)),
        )),
        Err(err) => Err(Issue::error(
            ProblemKind::Malformed,
            format!("invalid JSON: {err}"),
        )),
    }
}

const PROJECT_FIELDS: &[&str] = &["key", "name", "description", "created_on", "home"];

/// Parses `project.json`. The value is `None` only if the text isn't a JSON object.
pub fn parse_project_profile(text: &str) -> Parsed<Option<ProjectProfile>> {
    let map = match object(text) {
        Ok(map) => map,
        Err(issue) => return Parsed::new(None, vec![issue]),
    };
    let mut f = Fields {
        object: &map,
        issues: Vec::new(),
    };
    let profile = ProjectProfile {
        key: f.string("key", true),
        name: f.string("name", true),
        description: f.string("description", false),
        created_on: f.string("created_on", false),
        home: f.string("home", false),
        extra: f.extra(PROJECT_FIELDS),
    };
    Parsed::new(Some(profile), f.issues)
}

const AGENT_FIELDS: &[&str] = &[
    "id",
    "name",
    "project",
    "role",
    "title",
    "department",
    "focus",
    "reports_to",
    "model",
    "tools",
    "status",
    "hired_on",
    "offboarded_on",
    "home",
    "registration",
];

/// Parses `agent.json`. The value is `None` only if the text isn't a JSON object.
pub fn parse_agent_profile(text: &str) -> Parsed<Option<AgentProfile>> {
    let map = match object(text) {
        Ok(map) => map,
        Err(issue) => return Parsed::new(None, vec![issue]),
    };
    let mut f = Fields {
        object: &map,
        issues: Vec::new(),
    };
    let tools = match map.get("tools") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.trim() == "inherit" => Some(Tools::Inherit),
        Some(Value::Array(items)) => {
            let names: Vec<String> = items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
            if names.len() != items.len() {
                f.issues.push(Issue::warning(
                    ProblemKind::InvalidField,
                    "'tools' has entries that aren't strings",
                ));
            }
            Some(Tools::List(names))
        }
        Some(other) => {
            f.issues.push(Issue::warning(
                ProblemKind::InvalidField,
                format!(
                    "'tools' should be \"inherit\" or a list of tool names, not {}",
                    kind_of(other)
                ),
            ));
            None
        }
    };
    let profile = AgentProfile {
        id: f.string("id", true),
        name: f.string("name", true),
        project: f.string("project", true),
        role: f.string("role", true),
        title: f.string("title", true),
        department: f.string("department", false),
        focus: f.string("focus", false),
        reports_to: f.string("reports_to", false),
        model: f.string("model", false),
        tools,
        status: f.string("status", true).map(|s| AgentStatus::parse(&s)),
        hired_on: f.string("hired_on", false),
        offboarded_on: f.string("offboarded_on", false),
        home: f.string("home", false),
        registration: f.string("registration", false),
        extra: f.extra(AGENT_FIELDS),
    };
    Parsed::new(Some(profile), f.issues)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_profile_with_unknown_and_mistyped_fields() {
        let text = r#"{"id": "p-r-a", "name": "A", "project": "p", "role": "r", "title": 7,
            "status": "offboarded", "tools": ["Read", 1], "mood": "great", "focus": null}"#;
        let parsed = parse_agent_profile(text);
        let p = parsed.value.unwrap();
        assert_eq!(p.title, None);
        assert_eq!(p.status, Some(AgentStatus::Offboarded));
        assert_eq!(p.tools, Some(Tools::List(vec!["Read".into()])));
        assert_eq!(p.focus, None);
        assert_eq!(p.extra["mood"], "great");
        assert_eq!(parsed.issues.len(), 2, "{:?}", parsed.issues);
    }

    #[test]
    fn garbage_json_is_an_error_issue() {
        for text in ["", "{", "[1]", "\"x\"", "null"] {
            let parsed = parse_agent_profile(text);
            assert!(parsed.value.is_none());
            assert_eq!(parsed.issues[0].kind, ProblemKind::Malformed);
            assert!(parse_project_profile(text).value.is_none());
        }
    }

    #[test]
    fn project_profile() {
        let parsed = parse_project_profile(
            r#"{"key":"pay","name":"Pay","description":null,"created_on":"2026-11-07","home":"projects/pay"}"#,
        );
        assert!(parsed.issues.is_empty());
        let p = parsed.value.unwrap();
        assert_eq!((p.key.as_deref(), p.description), (Some("pay"), None));
    }
}
