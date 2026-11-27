//! Organization-state integrity scanner for `agentic-org doctor` (AC #4).
//!
//! Safely walks a `durable/state/` tree (or explicit `--org-dir`) with `O_NOFOLLOW` and strict
//! file/byte caps, indexing tasks, projects, features, docs, decisions, and JSON records.
//! Check evaluation and SQLite parity reporting live in `org_integrity_checks.rs`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::doctor::DoctorCheck;
use super::org_integrity_checks::{
    build_cross_link_check, build_display_key_check, build_sqlite_parity_check, build_syntax_check,
    resolve_office_db,
};
use super::paths::{normalize_lstat_path, read_regular_bounded, validate_plain_path};
use super::yaml_front::{parse_markdown_front_matter, parse_yaml_mapping};

const MAX_SCAN_ENTRIES: usize = 10_000;
const MAX_SCAN_DEPTH: usize = 32;
const MAX_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Default)]
pub(super) struct StateIndex {
    pub(super) entries_scanned: usize,
    pub(super) cap_hit: bool,
    pub(super) files_scanned: usize,
    pub(super) syntax_errors: Vec<String>,
    pub(super) task_keys: BTreeMap<String, Vec<String>>,
    pub(super) task_ids: BTreeMap<String, Vec<String>>,
    pub(super) doc_keys: BTreeMap<String, Vec<String>>,
    pub(super) doc_ids: BTreeMap<String, Vec<String>>,
    pub(super) decision_keys: BTreeMap<String, Vec<String>>,
    pub(super) decision_ids: BTreeMap<String, Vec<String>>,
    pub(super) known_tasks: BTreeSet<String>,
    pub(super) known_projects: BTreeSet<String>,
    pub(super) known_features: BTreeSet<String>,
    pub(super) known_docs: BTreeSet<String>,
    pub(super) known_decisions: BTreeSet<String>,
    pub(super) task_count: usize,
    pub(super) link_checks: Vec<LinkExpectation>,
}

pub(super) struct LinkExpectation {
    pub(super) source_path: String,
    pub(super) field: String,
    pub(super) target_kind: TargetKind,
    pub(super) target: String,
}

#[derive(Clone, Copy)]
pub(super) enum TargetKind {
    Task,
    Project,
    Feature,
    Doc,
    Decision,
    AnyEntity,
}

fn syntax_fail(scope: &str, msg: impl Into<String>, fix: &str, path: &Path) -> Vec<DoctorCheck> {
    vec![DoctorCheck::fail(
        format!("{scope}.syntax"),
        "org_integrity",
        msg,
        Some(fix.to_owned()),
        Some(path.display().to_string()),
    )]
}

/// Runs all four organization-state integrity checks against `org_dir`.
pub(crate) fn check_org_state(
    org_dir: &Path,
    office_db_override: Option<&Path>,
    scope_label: &str,
) -> Vec<DoctorCheck> {
    if let Err(err) = validate_plain_path(org_dir) {
        return syntax_fail(
            scope_label,
            err.to_string(),
            "Pass a clean path without `..` or NUL bytes.",
            org_dir,
        );
    }
    let state_root = resolve_state_root(org_dir);
    let norm_root = normalize_lstat_path(&state_root);
    match fs::symlink_metadata(&norm_root) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => {
            return syntax_fail(
                scope_label,
                format!(
                    "`{}` is not a real directory (symlinks are refused)",
                    norm_root.display()
                ),
                "Point --org-dir at a real durable/state directory.",
                &norm_root,
            );
        }
        Err(e) => {
            return syntax_fail(
                scope_label,
                format!("cannot inspect `{}`: {e}", norm_root.display()),
                "Verify the organization state directory exists and is readable.",
                &norm_root,
            );
        }
    }

    let mut index = StateIndex::default();
    walk_state_tree(&norm_root, &norm_root, &mut index);

    let mut checks = vec![
        build_syntax_check(&index, scope_label, &norm_root),
        build_display_key_check(&index, scope_label, &norm_root),
        build_cross_link_check(&index, scope_label, &norm_root),
    ];
    if let Some(db_path) = resolve_office_db(org_dir, &norm_root, office_db_override) {
        checks.push(build_sqlite_parity_check(&index, &db_path, scope_label));
    }
    checks
}

fn resolve_state_root(org_dir: &Path) -> PathBuf {
    let durable_state = org_dir.join("durable").join("state");
    if fs::symlink_metadata(&durable_state).is_ok_and(|m| m.is_dir()) {
        return durable_state;
    }
    let direct_state = org_dir.join("state");
    if fs::symlink_metadata(&direct_state).is_ok_and(|m| m.is_dir()) {
        return direct_state;
    }
    org_dir.to_path_buf()
}

fn walk_state_tree(root: &Path, dir: &Path, index: &mut StateIndex) {
    walk_state_tree_bounded(root, dir, 0, MAX_SCAN_DEPTH, MAX_SCAN_ENTRIES, index);
}

pub(super) fn walk_state_tree_bounded(
    root: &Path,
    dir: &Path,
    depth: usize,
    max_depth: usize,
    max_entries: usize,
    index: &mut StateIndex,
) {
    if depth > max_depth {
        index.syntax_errors.push(format!(
            "{}: directory nesting exceeds maximum depth of {max_depth}",
            rel_str(root, dir)
        ));
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            index.syntax_errors.push(format!(
                "{}: cannot list directory: {e}",
                rel_str(root, dir)
            ));
            return;
        }
    };
    let mut paths = Vec::new();
    for entry in entries {
        match entry {
            Ok(e) => paths.push(e.path()),
            Err(e) => index.syntax_errors.push(format!(
                "{}: directory entry read failed: {e}",
                rel_str(root, dir)
            )),
        }
    }
    paths.sort();
    for path in paths {
        if index.entries_scanned >= max_entries {
            if !index.cap_hit {
                index.cap_hit = true;
                index.syntax_errors.push(format!(
                    "state tree exceeded scan cap of {max_entries} entries"
                ));
            }
            return;
        }
        index.entries_scanned += 1;
        let rel = rel_str(root, &path);
        let meta = match fs::symlink_metadata(normalize_lstat_path(&path)) {
            Ok(m) => m,
            Err(e) => {
                index.syntax_errors.push(format!("{rel}: stat failed: {e}"));
                continue;
            }
        };
        if meta.file_type().is_symlink() {
            index
                .syntax_errors
                .push(format!("{rel}: symlink is forbidden inside durable state"));
            continue;
        }
        if meta.is_dir() {
            walk_state_tree_bounded(root, &path, depth + 1, max_depth, max_entries, index);
            if index.cap_hit {
                return;
            }
            continue;
        }
        if !meta.is_file() {
            index.syntax_errors.push(format!(
                "{rel}: non-regular file is forbidden inside durable state"
            ));
            continue;
        }
        index.files_scanned += 1;
        inspect_file(&path, &rel, index);
    }
}

fn rel_str(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn inspect_file(path: &Path, rel: &str, index: &mut StateIndex) {
    let parts: Vec<&str> = rel.split('/').collect();
    if is_task_md(&parts) {
        inspect_task_md(path, rel, &parts, index);
    } else if is_project_file(&parts) {
        inspect_project_file(path, rel, &parts, index);
    } else if is_feature_file(&parts) {
        inspect_feature_file(path, rel, &parts, index);
    } else if is_decision_md(&parts) {
        inspect_doc_or_decision_md(path, rel, true, index);
    } else if is_doc_md(&parts) {
        inspect_doc_or_decision_md(path, rel, false, index);
    } else if rel.ends_with(".json") && is_checked_json_area(&parts) {
        inspect_json_file(path, rel, index);
    }
}

fn is_task_md(parts: &[&str]) -> bool {
    parts.len() >= 3 && parts[parts.len() - 1] == "task.md" && parts[parts.len() - 3] == "tasks"
}

fn is_project_file(parts: &[&str]) -> bool {
    match parts {
        [.., "projects", _, name] if *name == "project.yaml" || *name == "project.json" => true,
        [.., "projects", name] if name.ends_with(".yaml") || name.ends_with(".json") => true,
        _ => false,
    }
}

fn is_feature_file(parts: &[&str]) -> bool {
    match parts {
        [.., "features", _, name]
            if *name == "feature.yaml" || *name == "feature.json" || *name == "feature.md" =>
        {
            true
        }
        [.., "features", name]
            if name.ends_with(".yaml") || name.ends_with(".json") || name.ends_with(".md") =>
        {
            true
        }
        _ => false,
    }
}

fn is_doc_md(parts: &[&str]) -> bool {
    parts.contains(&"docs") && parts.last().is_some_and(|n| n.ends_with(".md"))
}

fn is_decision_md(parts: &[&str]) -> bool {
    parts.contains(&"decisions") && parts.last().is_some_and(|n| n.ends_with(".md"))
}

fn is_checked_json_area(parts: &[&str]) -> bool {
    parts
        .iter()
        .any(|p| matches!(*p, "tasks" | "docs" | "decisions" | "work-history"))
}

fn read_utf8_file(path: &Path, rel: &str, index: &mut StateIndex) -> Option<String> {
    let bytes = match read_regular_bounded(path, MAX_FILE_BYTES) {
        Ok(b) => b,
        Err(e) => {
            index.syntax_errors.push(format!("{rel}: read failed: {e}"));
            return None;
        }
    };
    match String::from_utf8(bytes) {
        Ok(text) => Some(text),
        Err(e) => {
            index
                .syntax_errors
                .push(format!("{rel}: invalid UTF-8: {e}"));
            None
        }
    }
}

fn inspect_task_md(path: &Path, rel: &str, parts: &[&str], index: &mut StateIndex) {
    let Some(text) = read_utf8_file(path, rel, index) else {
        return;
    };
    let parsed = match parse_markdown_front_matter(&text) {
        Ok(doc) => doc,
        Err(e) => {
            index
                .syntax_errors
                .push(format!("{rel}: invalid YAML front matter: {e}"));
            return;
        }
    };
    let Some(fm) = parsed.front_matter else {
        index.syntax_errors.push(format!(
            "{rel}: task.md is missing required YAML front matter"
        ));
        return;
    };
    index.task_count += 1;
    let folder_id = parts[parts.len() - 2];
    index.known_tasks.insert(folder_id.to_owned());
    if let Some(id) = str_field(&fm, "id") {
        index.known_tasks.insert(id.to_owned());
        index
            .task_ids
            .entry(id.to_owned())
            .or_default()
            .push(rel.to_owned());
    }
    if let Some(key) = task_display_key(&fm) {
        index.known_tasks.insert(key.clone());
        index.task_keys.entry(key).or_default().push(rel.to_owned());
    }
    collect_task_links(&fm, rel, index);
}

fn task_display_key(fm: &Map<String, Value>) -> Option<String> {
    if let Some(k) = str_field(fm, "key") {
        return Some(k.to_owned());
    }
    let code = str_field(fm, "office_code")?;
    let num = fm.get("number").and_then(Value::as_i64)?;
    (num > 0).then(|| format!("T-{code}{num}"))
}

fn collect_task_links(fm: &Map<String, Value>, rel: &str, index: &mut StateIndex) {
    for dep in string_list_field(fm, "depends_on") {
        push_link(index, rel, "depends_on", TargetKind::Task, dep);
    }
    if let Some(pid) = str_field(fm, "project_id") {
        push_link(
            index,
            rel,
            "project_id",
            TargetKind::Project,
            pid.to_owned(),
        );
    }
    if let Some(fid) = str_field(fm, "feature_id") {
        push_link(
            index,
            rel,
            "feature_id",
            TargetKind::Feature,
            fid.to_owned(),
        );
    }
    for rel_ref in string_list_field(fm, "relates_to") {
        push_link(index, rel, "relates_to", TargetKind::AnyEntity, rel_ref);
    }
    let Some(Value::Array(links)) = fm.get("links") else {
        return;
    };
    for item in links {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let Some(target_id) = str_field(obj, "target_id") else {
            continue;
        };
        let kind = match str_field(obj, "target_kind").unwrap_or("task") {
            "task" => TargetKind::Task,
            "project" => TargetKind::Project,
            "feature" => TargetKind::Feature,
            "document" | "doc" => TargetKind::Doc,
            "decision" => TargetKind::Decision,
            _ => TargetKind::AnyEntity,
        };
        push_link(index, rel, "links", kind, target_id.to_owned());
    }
}

fn push_link(index: &mut StateIndex, source: &str, field: &str, kind: TargetKind, target: String) {
    index.link_checks.push(LinkExpectation {
        source_path: source.to_owned(),
        field: field.to_owned(),
        target_kind: kind,
        target,
    });
}

fn inspect_doc_or_decision_md(path: &Path, rel: &str, is_decision: bool, index: &mut StateIndex) {
    let Some(text) = read_utf8_file(path, rel, index) else {
        return;
    };
    let parsed = match parse_markdown_front_matter(&text) {
        Ok(doc) => doc,
        Err(e) => {
            index
                .syntax_errors
                .push(format!("{rel}: invalid YAML front matter: {e}"));
            return;
        }
    };
    let stem = Path::new(rel)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let target_set = if is_decision {
        &mut index.known_decisions
    } else {
        &mut index.known_docs
    };
    target_set.insert(rel.to_owned());
    if !stem.is_empty() {
        target_set.insert(stem.to_owned());
    }
    let Some(fm) = parsed.front_matter else {
        return;
    };
    record_doc_identity(&fm, rel, is_decision, index);
    for rel_ref in string_list_field(&fm, "relates_to") {
        push_link(index, rel, "relates_to", TargetKind::AnyEntity, rel_ref);
    }
    if let Some(pid) = str_field(&fm, "project_id") {
        push_link(
            index,
            rel,
            "project_id",
            TargetKind::Project,
            pid.to_owned(),
        );
    }
    if let Some(fid) = str_field(&fm, "feature_id") {
        push_link(
            index,
            rel,
            "feature_id",
            TargetKind::Feature,
            fid.to_owned(),
        );
    }
}

fn record_doc_identity(
    fm: &Map<String, Value>,
    rel: &str,
    is_decision: bool,
    index: &mut StateIndex,
) {
    let (known, ids, keys) = if is_decision {
        (
            &mut index.known_decisions,
            &mut index.decision_ids,
            &mut index.decision_keys,
        )
    } else {
        (
            &mut index.known_docs,
            &mut index.doc_ids,
            &mut index.doc_keys,
        )
    };
    if let Some(id) = str_field(fm, "id") {
        known.insert(id.to_owned());
        ids.entry(id.to_owned()).or_default().push(rel.to_owned());
    }
    if let Some(key) = str_field(fm, "key") {
        known.insert(key.to_owned());
        keys.entry(key.to_owned()).or_default().push(rel.to_owned());
    }
}

fn inspect_project_file(path: &Path, rel: &str, parts: &[&str], index: &mut StateIndex) {
    let Some(text) = read_utf8_file(path, rel, index) else {
        return;
    };
    let map = match parse_yaml_or_json(rel, &text) {
        Ok(m) => m,
        Err(e) => {
            index.syntax_errors.push(e);
            return;
        }
    };
    if let [.., "projects", folder, _] = parts {
        index.known_projects.insert((*folder).to_owned());
    }
    for key in ["id", "key", "slug"] {
        if let Some(val) = str_field(&map, key) {
            index.known_projects.insert(val.to_owned());
        }
    }
    if let Some(num) = map.get("number").and_then(Value::as_i64).filter(|&n| n > 0) {
        index.known_projects.insert(format!("P-{num}"));
    }
}

fn inspect_feature_file(path: &Path, rel: &str, parts: &[&str], index: &mut StateIndex) {
    let Some(text) = read_utf8_file(path, rel, index) else {
        return;
    };
    let map = if rel.ends_with(".md") {
        match parse_markdown_front_matter(&text) {
            Ok(doc) => doc.front_matter.unwrap_or_default(),
            Err(e) => {
                index
                    .syntax_errors
                    .push(format!("{rel}: invalid YAML front matter: {e}"));
                return;
            }
        }
    } else {
        match parse_yaml_or_json(rel, &text) {
            Ok(m) => m,
            Err(e) => {
                index.syntax_errors.push(e);
                return;
            }
        }
    };
    if let [.., "features", folder, _] = parts {
        index.known_features.insert((*folder).to_owned());
    } else if let Some(stem) = Path::new(rel).file_stem().and_then(|s| s.to_str()) {
        index.known_features.insert(stem.to_owned());
    }
    for key in ["id", "key"] {
        if let Some(val) = str_field(&map, key) {
            index.known_features.insert(val.to_owned());
        }
    }
    if let Some(num) = map.get("number").and_then(Value::as_i64).filter(|&n| n > 0) {
        index.known_features.insert(format!("F-{num}"));
    }
}

fn inspect_json_file(path: &Path, rel: &str, index: &mut StateIndex) {
    let Some(text) = read_utf8_file(path, rel, index) else {
        return;
    };
    if let Err(e) = serde_json::from_str::<Value>(&text) {
        index
            .syntax_errors
            .push(format!("{rel}: malformed JSON: {e}"));
    }
}

fn parse_yaml_or_json(rel: &str, text: &str) -> Result<Map<String, Value>, String> {
    if rel.ends_with(".json") {
        match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(map)) => Ok(map),
            Ok(_) => Err(format!("{rel}: expected JSON object root")),
            Err(e) => Err(format!("{rel}: malformed JSON: {e}")),
        }
    } else {
        parse_yaml_mapping(text).map_err(|e| format!("{rel}: invalid YAML: {e}"))
    }
}

fn str_field<'a>(map: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    map.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn string_list_field(map: &Map<String, Value>, key: &str) -> Vec<String> {
    match map.get(key) {
        Some(Value::String(s)) if !s.trim().is_empty() => vec![s.trim().to_owned()],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}
