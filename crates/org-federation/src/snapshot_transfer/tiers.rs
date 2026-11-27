//! Snapshot dependency tiers: organization-core paths, employee profiles, and the cycle-breaking
//! rewrite of employee references in organization-core files.

pub(super) fn is_org_core_put(path: &str, is_put: bool) -> bool {
    is_put
        && matches!(
            org_office::classify_replicated_snapshot_path(path),
            org_office::SnapshotPathClass::OrgCoreStructure
                | org_office::SnapshotPathClass::OrgCoreEmployee
        )
}

pub(super) fn snapshot_dependency_tier(path: &str, is_put: bool) -> (u8, usize) {
    use org_office::SnapshotPathClass as Class;
    let depth = path.bytes().filter(|&b| b == b'/').count();
    let class = org_office::classify_replicated_snapshot_path(path);
    if !is_put {
        return match class {
            Class::WorkSidecar => (7, depth),
            Class::WorkCore => (8, depth),
            _ => (9, usize::MAX - depth),
        };
    }
    match class {
        Class::OrgCoreStructure => (0, depth),
        Class::OrgCoreEmployee => (1, depth),
        Class::ProvidersAndHires => (2, depth),
        Class::ModelProfilesAndProjects => (3, depth),
        Class::DependentExecutionDocsAndBudgets => (4, depth),
        Class::WorkCore => (5, depth),
        Class::WorkSidecar => (6, depth),
    }
}

pub(super) fn is_employee_profile_path(path: &str) -> bool {
    org_office::classify_replicated_snapshot_path(path)
        == org_office::SnapshotPathClass::OrgCoreEmployee
}

pub(super) fn strip_org_core_employee_refs(path: &str, bytes: &[u8]) -> Vec<u8> {
    if let Some(typed) = org_office::strip_org_core_employee_refs_typed(path, bytes) {
        return typed;
    }
    let is_markdown = path.ends_with(".md");
    let Ok(text) = std::str::from_utf8(bytes) else {
        return bytes.to_vec();
    };
    let mut out = String::with_capacity(text.len());
    let mut in_frontmatter = !is_markdown;
    let mut seen_open = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if is_markdown && trimmed == "---" {
            out.push_str(line);
            if !seen_open {
                seen_open = true;
                in_frontmatter = true;
            } else {
                in_frontmatter = false;
            }
            continue;
        }
        if in_frontmatter {
            let indent_len = trimmed.len() - trimmed.trim_start().len();
            let after_ws = &trimmed[indent_len..];
            let (prefix, key_line) = if let Some(rest) = after_ws.strip_prefix("- ") {
                (&trimmed[..indent_len + 2], rest)
            } else {
                (&trimmed[..indent_len], after_ws)
            };
            if let Some(replacement) = rewrite_cycle_breaking_yaml_line(key_line) {
                out.push_str(prefix);
                out.push_str(replacement);
                if line.ends_with("\r\n") {
                    out.push_str("\r\n");
                } else if line.ends_with('\n') {
                    out.push('\n');
                }
                continue;
            }
        }
        out.push_str(line);
    }
    out.into_bytes()
}

fn rewrite_cycle_breaking_yaml_line(trimmed: &str) -> Option<&'static str> {
    if trimmed.starts_with("lead_employee_id:") {
        Some("lead_employee_id: null")
    } else if trimmed.starts_with("reports_to_employee_id:") {
        Some("reports_to_employee_id: null")
    } else if trimmed.starts_with("created_by_kind:") {
        Some("created_by_kind: \"system\"")
    } else if trimmed.starts_with("created_by_id:") {
        Some("created_by_id: null")
    } else if trimmed.starts_with("updated_by_kind:") {
        Some("updated_by_kind: \"system\"")
    } else if trimmed.starts_with("updated_by_id:") {
        Some("updated_by_id: null")
    } else {
        None
    }
}
