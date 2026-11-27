//! Report builders for organization-state integrity checks (`org_integrity.rs`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::doctor::{CheckStatus, DoctorCheck};
use super::org_integrity::{StateIndex, TargetKind};
use super::sqlite_read::read_projected_tasks;

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
    let missing_in_db: Vec<String> = durable_keys.difference(&sqlite_keys).cloned().collect();
    let extra_in_db: Vec<String> = sqlite_keys.difference(&durable_keys).cloned().collect();
    let count_drift = projected.len() != index.task_count;

    if !count_drift && missing_in_db.is_empty() && extra_in_db.is_empty() {
        return DoctorCheck {
            id: format!("{scope}.sqlite_parity"),
            category: "org_integrity".to_owned(),
            status: CheckStatus::Pass,
            message: format!(
                "SQLite projection matches durable state ({} task(s))",
                index.task_count
            ),
            remediation: None,
            path: Some(db_path.display().to_string()),
        };
    }
    let mut details = Vec::new();
    if count_drift {
        details.push(format!(
            "task count drift: durable={} sqlite={}",
            index.task_count,
            projected.len()
        ));
    }
    if !missing_in_db.is_empty() {
        details.push(format!(
            "missing in office.db: {}",
            join_limited(&missing_in_db, 5)
        ));
    }
    if !extra_in_db.is_empty() {
        details.push(format!(
            "extra in office.db: {}",
            join_limited(&extra_in_db, 5)
        ));
    }
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

fn join_limited(items: &[String], max: usize) -> String {
    if items.len() <= max {
        items.join("; ")
    } else {
        format!("{}; (+{} more)", items[..max].join("; "), items.len() - max)
    }
}
