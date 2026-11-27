//! Report builders for organization-state integrity checks (`org_integrity.rs`).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::doctor_types::DoctorCheck;
use super::org_integrity::{walk_state_tree, StateIndex, TargetKind};
use super::paths::{normalize_lstat_path, validate_plain_path};
use super::sqlite_read::read_projected_tasks;

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

pub(super) fn resolve_office_db(
    org_dir: &Path,
    state_root: &Path,
    explicit: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return Some(path.to_path_buf());
    }
    let mut candidates = vec![org_dir.join("office.db"), state_root.join("office.db")];
    if let Some(p1) = state_root.parent() {
        candidates.push(p1.join("office.db"));
        candidates.push(p1.join("runtime").join("office.db"));
        if let Some(p2) = p1.parent() {
            candidates.push(p2.join("office.db"));
            candidates.push(p2.join("runtime").join("office.db"));
            collect_runtime_office_dbs(&p2.join("runtime"), &mut candidates);
        }
        collect_runtime_office_dbs(&p1.join("runtime"), &mut candidates);
    }
    collect_runtime_office_dbs(&org_dir.join("runtime"), &mut candidates);
    candidates
        .into_iter()
        .find(|p| fs::symlink_metadata(p).is_ok_and(|m| m.is_file()))
}

fn collect_runtime_office_dbs(runtime_dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(runtime_dir) else {
        return;
    };
    let mut dirs: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    dirs.sort();
    for dir in dirs {
        if fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) {
            out.push(dir.join("office.db"));
        }
    }
}

pub(super) fn build_syntax_check(index: &StateIndex, scope: &str, root: &Path) -> DoctorCheck {
    if index.syntax_errors.is_empty() {
        DoctorCheck::pass(
            format!("{scope}.syntax"),
            "org_integrity",
            format!(
                "YAML front matter and JSON syntax valid across {} file(s) ({} task(s))",
                index.files_scanned, index.task_count
            ),
            Some(root.display().to_string()),
        )
    } else {
        let preview = join_limited(&index.syntax_errors, 5);
        DoctorCheck::fail(
            format!("{scope}.syntax"),
            "org_integrity",
            format!(
                "{} syntax/structure error(s): {preview}",
                index.syntax_errors.len()
            ),
            Some(
                "Fix the malformed YAML front matter, JSON, or forbidden symlinks reported above."
                    .to_owned(),
            ),
            Some(root.display().to_string()),
        )
    }
}

pub(super) fn build_display_key_check(index: &StateIndex, scope: &str, root: &Path) -> DoctorCheck {
    let mut clashes = Vec::new();
    collect_clashes("task key", &index.task_keys, &mut clashes);
    collect_clashes("task id", &index.task_ids, &mut clashes);
    collect_clashes("doc key", &index.doc_keys, &mut clashes);
    collect_clashes("doc id", &index.doc_ids, &mut clashes);
    collect_clashes("decision key", &index.decision_keys, &mut clashes);
    collect_clashes("decision id", &index.decision_ids, &mut clashes);

    if clashes.is_empty() {
        DoctorCheck::pass(
            format!("{scope}.display_keys"),
            "org_integrity",
            "no duplicate task, doc, or decision keys detected".to_owned(),
            Some(root.display().to_string()),
        )
    } else {
        let preview = join_limited(&clashes, 5);
        DoctorCheck::fail(
            format!("{scope}.display_keys"),
            "org_integrity",
            format!("{} display-key/ID clash(es): {preview}", clashes.len()),
            Some(
                "Resolve duplicate `key` or `id` values so each task, doc, and decision is unique."
                    .to_owned(),
            ),
            Some(root.display().to_string()),
        )
    }
}

fn collect_clashes(label: &str, map: &BTreeMap<String, Vec<String>>, out: &mut Vec<String>) {
    for (key, paths) in map {
        if paths.len() > 1 {
            out.push(format!("duplicate {label} `{key}` in {}", paths.join(", ")));
        }
    }
}

pub(super) fn build_cross_link_check(index: &StateIndex, scope: &str, root: &Path) -> DoctorCheck {
    let mut broken = Vec::new();
    for check in &index.link_checks {
        let exists = match check.target_kind {
            TargetKind::Task => index.known_tasks.contains(&check.target),
            TargetKind::Project => index.known_projects.contains(&check.target),
            TargetKind::Feature => index.known_features.contains(&check.target),
            TargetKind::Doc => index.known_docs.contains(&check.target),
            TargetKind::Decision => index.known_decisions.contains(&check.target),
            TargetKind::AnyEntity => {
                index.known_tasks.contains(&check.target)
                    || index.known_docs.contains(&check.target)
                    || index.known_decisions.contains(&check.target)
                    || index.known_projects.contains(&check.target)
                    || index.known_features.contains(&check.target)
            }
        };
        if !exists {
            broken.push(format!(
                "{} `{}` references missing `{}`",
                check.source_path, check.field, check.target
            ));
        }
    }
    if broken.is_empty() {
        DoctorCheck::pass(
            format!("{scope}.cross_links"),
            "org_integrity",
            format!(
                "all {} cross-link reference(s) resolved",
                index.link_checks.len()
            ),
            Some(root.display().to_string()),
        )
    } else {
        let preview = join_limited(&broken, 5);
        DoctorCheck::fail(
            format!("{scope}.cross_links"),
            "org_integrity",
            format!("{} broken cross-link(s): {preview}", broken.len()),
            Some(
                "Update or remove dangling `depends_on`, `project_id`, `feature_id`, `relates_to`, or `links` targets."
                    .to_owned(),
            ),
            Some(root.display().to_string()),
        )
    }
}

pub(super) fn build_sqlite_parity_check(
    index: &StateIndex,
    db_path: &Path,
    scope: &str,
) -> DoctorCheck {
    let projected = match read_projected_tasks(db_path) {
        Ok(rows) => rows,
        Err(e) => {
            return DoctorCheck::fail(
                format!("{scope}.sqlite_parity"),
                "org_integrity",
                format!("failed reading SQLite projection: {e}"),
                Some(
                    "Stop the office, remove the corrupt `office.db` projection, and restart to rebuild from durable state."
                        .to_owned(),
                ),
                Some(db_path.display().to_string()),
            );
        }
    };
    let durable_keys: BTreeSet<String> = index.task_keys.keys().cloned().collect();
    let sqlite_keys: BTreeSet<String> = projected.iter().filter_map(|t| t.key.clone()).collect();
    let missing_keys: Vec<String> = durable_keys.difference(&sqlite_keys).cloned().collect();
    let extra_keys: Vec<String> = sqlite_keys.difference(&durable_keys).cloned().collect();

    let durable_ids: BTreeSet<String> = index.task_ids.keys().cloned().collect();
    let sqlite_ids: BTreeSet<String> = projected.iter().filter_map(|t| t.id.clone()).collect();
    let missing_ids: Vec<String> = durable_ids.difference(&sqlite_ids).cloned().collect();
    let extra_ids: Vec<String> = sqlite_ids.difference(&durable_ids).cloned().collect();
    let count_drift = projected.len() != index.task_count;

    if !count_drift
        && missing_keys.is_empty()
        && extra_keys.is_empty()
        && missing_ids.is_empty()
        && extra_ids.is_empty()
    {
        return DoctorCheck::pass(
            format!("{scope}.sqlite_parity"),
            "org_integrity",
            format!(
                "SQLite projection matches durable state ({} task(s))",
                index.task_count
            ),
            Some(db_path.display().to_string()),
        );
    }
    let mut details = Vec::new();
    if count_drift {
        details.push(format!(
            "task count drift: durable={} sqlite={}",
            index.task_count,
            projected.len()
        ));
    }
    push_diff_detail(&mut details, "missing keys in office.db", &missing_keys);
    push_diff_detail(&mut details, "extra keys in office.db", &extra_keys);
    push_diff_detail(&mut details, "missing task IDs in office.db", &missing_ids);
    push_diff_detail(&mut details, "extra task IDs in office.db", &extra_ids);
    DoctorCheck::fail(
        format!("{scope}.sqlite_parity"),
        "org_integrity",
        details.join("; "),
        Some(
            "Stop the office, remove `office.db` (+ `-wal`/`-shm`), and restart so the projection rebuilds cleanly from durable files."
                .to_owned(),
        ),
        Some(db_path.display().to_string()),
    )
}

fn push_diff_detail(details: &mut Vec<String>, label: &str, items: &[String]) {
    if !items.is_empty() {
        details.push(format!("{label}: {}", join_limited(items, 5)));
    }
}

fn join_limited(items: &[String], max: usize) -> String {
    if items.len() <= max {
        items.join("; ")
    } else {
        format!("{}; (+{} more)", items[..max].join("; "), items.len() - max)
    }
}
