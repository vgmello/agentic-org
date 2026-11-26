//! Concrete file-target extraction from runtime tool-call inputs (edits, moves, patches).

use org_agent_runtime::{ApprovalKind, ApprovalRequest, ToolCapability};

pub(super) fn collect_json_strings(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) => out.push(s.clone()),
        serde_json::Value::Array(items) => {
            for item in items {
                collect_json_strings(item, out);
            }
        }
        serde_json::Value::Object(map) => {
            for v in map.values() {
                collect_json_strings(v, out);
            }
        }
        _ => {}
    }
}

pub(super) fn is_shell_request(request: &ApprovalRequest) -> bool {
    request.capability == Some(ToolCapability::Shell) || request.kind == ApprovalKind::Command
}

pub(super) fn is_file_target_request(request: &ApprovalRequest) -> bool {
    request.kind == ApprovalKind::FileChange
        || matches!(
            request.capability,
            Some(ToolCapability::Edit | ToolCapability::Read)
        )
}

pub(super) const MOVE_DEST_KEYS: &[&str] = &[
    "move_path",
    "movePath",
    "new_path",
    "newPath",
    "destination",
];

pub(super) fn extract_optional_path_keys(
    map: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
    targets: &mut Vec<String>,
    opaque: &mut bool,
) {
    for key in keys {
        if let Some(val) = map.get(*key).filter(|v| !v.is_null()) {
            match val.as_str().filter(|s| !s.trim().is_empty()) {
                Some(s) => targets.push(s.to_owned()),
                None => *opaque = true,
            }
        }
    }
}

pub(super) fn extract_change_dest_paths(
    map: &serde_json::Map<String, serde_json::Value>,
    targets: &mut Vec<String>,
    opaque: &mut bool,
) {
    extract_optional_path_keys(map, MOVE_DEST_KEYS, targets, opaque);
    if let Some(kind_map) = map.get("kind").and_then(serde_json::Value::as_object) {
        extract_optional_path_keys(kind_map, MOVE_DEST_KEYS, targets, opaque);
    }
}

pub(super) const CHANGE_ITEM_PATH_KEYS: &[&str] = &[
    "path",
    "file_path",
    "filePath",
    "notebook_path",
    "notebookPath",
    "TargetFile",
    "AbsolutePath",
    "target_file",
];

pub(super) fn extract_change_array_item(
    item: &serde_json::Value,
    targets: &mut Vec<String>,
    opaque: &mut bool,
) {
    let Some(map) = item.as_object() else {
        *opaque = true;
        return;
    };
    let before = targets.len();
    extract_optional_path_keys(map, CHANGE_ITEM_PATH_KEYS, targets, opaque);
    if targets.len() == before {
        *opaque = true;
    }
    extract_change_dest_paths(map, targets, opaque);
}

pub(super) fn extract_changes_targets(
    changes: &serde_json::Value,
    targets: &mut Vec<String>,
    opaque: &mut bool,
) {
    match changes {
        serde_json::Value::Array(items) if !items.is_empty() => {
            for item in items {
                extract_change_array_item(item, targets, opaque);
            }
        }
        serde_json::Value::Object(map) if !map.is_empty() => {
            for (k, v) in map {
                if k.trim().is_empty() {
                    *opaque = true;
                } else {
                    targets.push(k.clone());
                }
                if let Some(vmap) = v.as_object() {
                    extract_optional_path_keys(vmap, CHANGE_ITEM_PATH_KEYS, targets, opaque);
                    extract_change_dest_paths(vmap, targets, opaque);
                }
            }
        }
        _ => *opaque = true,
    }
}

pub(super) fn extract_paths_array(
    paths_val: &serde_json::Value,
    targets: &mut Vec<String>,
    opaque: &mut bool,
) {
    let Some(paths) = paths_val.as_array().filter(|a| !a.is_empty()) else {
        *opaque = true;
        return;
    };
    for v in paths {
        match v.as_str().filter(|s| !s.trim().is_empty()) {
            Some(s) => targets.push(s.to_owned()),
            None => *opaque = true,
        }
    }
}

pub(super) fn extract_concrete_file_targets(request: &ApprovalRequest) -> (Vec<String>, bool) {
    let mut targets = Vec::new();
    let mut opaque = false;
    if let Some(bp) = request
        .blocked_path
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        targets.push(bp.to_owned());
    }
    let Some(obj) = request.input.as_object() else {
        if !request.input.is_null() {
            opaque = true;
        }
        return (targets, opaque);
    };
    const SINGLE_PATH_KEYS: &[&str] = &[
        "file_path",
        "path",
        "TargetFile",
        "AbsolutePath",
        "filePath",
        "target_file",
        "notebook_path",
        "notebookPath",
        "move_path",
        "movePath",
        "new_path",
        "newPath",
        "destination",
    ];
    extract_optional_path_keys(obj, SINGLE_PATH_KEYS, &mut targets, &mut opaque);
    if let Some(paths_val) = obj.get("paths").filter(|v| !v.is_null()) {
        extract_paths_array(paths_val, &mut targets, &mut opaque);
    }
    for changes_key in ["changes", "fileChanges"] {
        if let Some(changes) = obj.get(changes_key).filter(|v| !v.is_null()) {
            extract_changes_targets(changes, &mut targets, &mut opaque);
        }
    }
    (targets, opaque)
}

pub(super) fn has_concrete_file_target(request: &ApprovalRequest) -> bool {
    let (targets, opaque) = extract_concrete_file_targets(request);
    !targets.is_empty() && !opaque
}
