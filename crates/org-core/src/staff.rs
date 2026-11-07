//! Staff functions defined as Claude Code agent files, such as HR
//! (`.claude/agents/hr.md`): YAML frontmatter (`name`, `description`, `tools`,
//! `model`) followed by the agent's instructions.

use crate::markdown::frontmatter;
use crate::model::{Issue, Parsed, ProblemKind, StaffFunction};

/// Parses an agent definition. `path` is its workspace-relative path. Never fails;
/// missing frontmatter or fields are issues.
pub fn parse_staff_function(path: &str, text: &str) -> Parsed<StaffFunction> {
    let mut issues = Vec::new();
    let fm = frontmatter(text).unwrap_or_else(|| {
        issues.push(Issue::error(
            ProblemKind::Malformed,
            "no `---` frontmatter with name and description",
        ));
        Default::default()
    });
    let non_empty = |key: &str| {
        fm.get(key)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    let name = non_empty("name");
    let description = non_empty("description");
    if !fm.is_empty() {
        for (key, value) in [("name", &name), ("description", &description)] {
            if value.is_none() {
                issues.push(Issue::warning(
                    ProblemKind::InvalidField,
                    format!("missing field '{key}'"),
                ));
            }
        }
    }
    // hr.py writes `tools: A, B`; a hand edit may use a YAML flow list `[A, "B"]`.
    let tools = non_empty("tools").map(|t| {
        let t = t
            .strip_prefix('[')
            .and_then(|t| t.strip_suffix(']'))
            .unwrap_or(&t)
            .to_string();
        t.split(',')
            .map(|s| s.trim().trim_matches(['"', '\'']).trim())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    });
    let staff = StaffFunction {
        path: path.to_string(),
        name,
        description,
        model: non_empty("model"),
        tools,
    };
    Parsed::new(staff, issues)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_hr_frontmatter() {
        let text = "---\nname: hr\ndescription: \"HR agent. Hires \\\"agents\\\".\"\ntools: Read, Write , Bash\nmodel: inherit\n---\n\nYou are HR.\n";
        let parsed = parse_staff_function(".claude/agents/hr.md", text);
        assert!(parsed.issues.is_empty());
        let hr = parsed.value;
        assert_eq!(hr.name.as_deref(), Some("hr"));
        assert_eq!(
            hr.description.as_deref(),
            Some("HR agent. Hires \"agents\".")
        );
        assert_eq!(
            hr.tools,
            Some(vec!["Read".into(), "Write".into(), "Bash".into()])
        );
        assert_eq!(hr.model.as_deref(), Some("inherit"));
    }

    #[test]
    fn missing_frontmatter_is_an_issue() {
        let parsed = parse_staff_function("hr.md", "You are HR.");
        assert_eq!(parsed.value.name, None);
        assert_eq!(parsed.issues.len(), 1);
        let parsed = parse_staff_function("hr.md", "---\nname: hr\ntools: [Read, \"Bash\"]\n---\n");
        assert_eq!(parsed.value.tools, Some(vec!["Read".into(), "Bash".into()]));
        let parsed = parse_staff_function("hr.md", "---\nmodel: x\n---\n");
        assert_eq!(parsed.issues.len(), 2);
    }
}
