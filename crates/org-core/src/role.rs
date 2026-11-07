//! `hr/roles/<id>.toml`: role specs (job descriptions). Unlike hr.py, which
//! rejects a spec with unknown or missing fields, this reads what it can and
//! reports the rest as issues, so the app can still show the catalog.

use std::collections::BTreeMap;

use toml::{Table, Value};

use crate::model::{Issue, Parsed, ProblemKind, Role};

const ROLE_FIELDS: &[&str] = &[
    "title",
    "department",
    "summary",
    "mission",
    "responsibilities",
    "working_style",
    "boundaries",
    "aliases",
    "model",
    "tools",
    "directories",
];

/// Parses a role spec. `id` is the file stem. The value is `None` only if the
/// text isn't valid TOML.
pub fn parse_role(id: &str, text: &str) -> Parsed<Option<Role>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let table: Table = match toml::from_str(text) {
        Ok(table) => table,
        Err(err) => {
            let message = err.to_string();
            let first = message.lines().next().unwrap_or("").trim();
            let detail = message.lines().last().unwrap_or("").trim();
            return Parsed::new(
                None,
                vec![Issue::error(
                    ProblemKind::Malformed,
                    format!("invalid TOML: {first}: {detail}"),
                )],
            );
        }
    };
    let mut issues = Vec::new();

    let mut string = |key: &str, required: bool| -> Option<String> {
        match table.get(key) {
            Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
            Some(Value::String(_)) => {
                issues.push(Issue::warning(
                    ProblemKind::InvalidField,
                    format!("'{key}' is empty"),
                ));
                None
            }
            Some(_) => {
                issues.push(Issue::warning(
                    ProblemKind::InvalidField,
                    format!("'{key}' should be a string"),
                ));
                None
            }
            None => {
                if required {
                    issues.push(Issue::warning(
                        ProblemKind::InvalidField,
                        format!("missing field '{key}'"),
                    ));
                }
                None
            }
        }
    };
    let title = string("title", true);
    let department = string("department", true);
    let summary = string("summary", true);
    let mission = string("mission", true);
    let model = string("model", false).unwrap_or_else(|| "inherit".into());

    let mut list = |key: &str, required: bool| -> Option<Vec<String>> {
        match table.get(key) {
            Some(Value::Array(items)) => {
                let strings: Vec<String> = items
                    .iter()
                    .filter_map(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();
                if strings.len() != items.len() {
                    issues.push(Issue::warning(
                        ProblemKind::InvalidField,
                        format!("'{key}' has entries that aren't non-empty strings"),
                    ));
                }
                Some(strings)
            }
            Some(_) => {
                issues.push(Issue::warning(
                    ProblemKind::InvalidField,
                    format!("'{key}' should be a list of strings"),
                ));
                None
            }
            None => {
                if required {
                    issues.push(Issue::warning(
                        ProblemKind::InvalidField,
                        format!("missing field '{key}'"),
                    ));
                }
                None
            }
        }
    };
    let responsibilities = list("responsibilities", true).unwrap_or_default();
    let working_style = list("working_style", false).unwrap_or_default();
    let boundaries = list("boundaries", false).unwrap_or_default();
    let aliases = list("aliases", false).unwrap_or_default();
    let tools = list("tools", false);

    let mut directories = BTreeMap::new();
    match table.get("directories") {
        Some(Value::Table(dirs)) => {
            for (dir, purpose) in dirs {
                match purpose.as_str() {
                    Some(p) => {
                        directories.insert(dir.clone(), p.trim().to_string());
                    }
                    None => issues.push(Issue::warning(
                        ProblemKind::InvalidField,
                        format!("directory '{dir}' needs a one-line purpose string"),
                    )),
                }
            }
        }
        Some(_) => issues.push(Issue::warning(
            ProblemKind::InvalidField,
            "'directories' should be a table",
        )),
        None => {}
    }

    let mut extra = BTreeMap::new();
    for (key, value) in &table {
        if !ROLE_FIELDS.contains(&key.as_str()) {
            extra.insert(
                key.clone(),
                serde_json::to_value(value).unwrap_or(serde_json::Value::Null),
            );
        }
    }
    if !extra.is_empty() {
        let keys: Vec<&str> = extra.keys().map(String::as_str).collect();
        issues.push(Issue::warning(
            ProblemKind::InvalidField,
            format!("unknown field(s) {} (hr.py rejects these)", keys.join(", ")),
        ));
    }

    let role = Role {
        id: id.to_string(),
        title,
        department,
        summary,
        mission,
        responsibilities,
        working_style,
        boundaries,
        aliases,
        model,
        tools,
        directories,
        extra,
    };
    Parsed::new(Some(role), issues)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_role_and_tolerates_unknown_keys() {
        let text = r#"
title = " Backend Engineer "
department = "Engineering"
summary = "Use for backend work."
mission = """
Build it.
"""
responsibilities = ["Write code", 3]
tools = ["Read"]
salary_band = 4
[directories]
"knowledge/adrs" = "Decisions"
"#;
        let parsed = parse_role("backend-engineer", text);
        let role = parsed.value.unwrap();
        assert_eq!(role.title.as_deref(), Some("Backend Engineer"));
        assert_eq!(role.mission.as_deref(), Some("Build it."));
        assert_eq!(role.responsibilities, ["Write code"]);
        assert_eq!(role.model, "inherit");
        assert_eq!(role.tools, Some(vec!["Read".to_string()]));
        assert_eq!(role.directories["knowledge/adrs"], "Decisions");
        assert_eq!(role.extra["salary_band"], 4);
        assert_eq!(parsed.issues.len(), 2, "{:?}", parsed.issues);
    }

    #[test]
    fn invalid_toml_is_an_error_issue() {
        let parsed = parse_role("x", "title = \n[[[");
        assert!(parsed.value.is_none());
        assert_eq!(parsed.issues[0].kind, ProblemKind::Malformed);
    }
}
