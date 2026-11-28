//! Durable budget files: loading, employee mirror inference, atomic writes and rebuilding
//! `budgets.db` from them.

use std::fs;
use std::path::Path;

use rusqlite::Connection;
use serde::Serialize;

use org_budgets::BudgetAggregate;
use org_budgets::BudgetError;
use org_office::private_fs::{full_sync, private_dir_synced, write_private_atomic};

use super::project::project_aggregate;

pub(super) fn reconcile_durable_conn(
    conn: &mut Connection,
    durable_root: &Path,
    org_id: &str,
) -> Result<(), BudgetError> {
    let aggregates = load_all_durable(durable_root, org_id)?;
    if aggregates.is_empty() {
        return Ok(());
    }
    let known_agents: std::collections::BTreeSet<String> = aggregates
        .iter()
        .filter_map(|a| a.budget.agent_id.clone())
        .collect();
    let tx = conn.transaction().map_err(BudgetError::storage)?;
    for agg in &aggregates {
        let inferred_emps = infer_employee_mirrors(agg, &known_agents);
        project_aggregate(&tx, agg, &inferred_emps)?;
    }
    tx.commit().map_err(BudgetError::storage)?;
    Ok(())
}

fn load_all_durable(root: &Path, org_id: &str) -> Result<Vec<BudgetAggregate>, BudgetError> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(root).map_err(BudgetError::storage)? {
        let dir = entry.map_err(BudgetError::storage)?.path();
        let agg_path = dir.join("aggregate.v1.json");
        if !agg_path.is_file() {
            continue;
        }
        // Durably flush each recovered aggregate file and its ancestor chain before
        // projecting its receipts so an interrupted rename/dir-sync can never be
        // acknowledged without a completed flush (`TASK-97` / Pass 4 Finding #1).
        let f = fs::File::open(&agg_path).map_err(BudgetError::storage)?;
        full_sync(&f).map_err(BudgetError::storage)?;
        private_dir_synced(root, &dir).map_err(BudgetError::storage)?;
        let mut agg: BudgetAggregate =
            serde_json::from_slice(&fs::read(&agg_path).map_err(BudgetError::storage)?)
                .map_err(BudgetError::storage)?;
        if agg.budget.organization_id != org_id {
            continue;
        }
        agg.versions.sort_by_key(|v| v.revision);
        agg.periods.sort_by(|a, b| a.period_key.cmp(&b.period_key));
        agg.events.sort_by(|a, b| {
            a.occurred_at
                .cmp(&b.occurred_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        agg.notices.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        agg.holds.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        out.push(agg);
    }
    Ok(out)
}

fn infer_employee_mirrors(
    agg: &BudgetAggregate,
    known_agents: &std::collections::BTreeSet<String>,
) -> Vec<(String, &'static str)> {
    let mut ids = std::collections::BTreeSet::new();
    ids.extend(agg.budget.agent_id.iter().cloned());
    ids.extend(agg.budget.time_zone_subject_ref.iter().cloned());
    for v in &agg.versions {
        ids.insert(v.changed_by_id.clone());
        ids.extend(v.time_zone_subject_ref.iter().cloned());
    }
    ids.extend(agg.events.iter().filter_map(|e| e.actor_id.clone()));
    ids.extend(agg.notices.iter().filter_map(|n| n.recipient_id.clone()));
    ids.into_iter()
        .map(|id| {
            let kind = if known_agents.contains(&id) {
                "agent"
            } else {
                "person"
            };
            (id, kind)
        })
        .collect()
}

pub(super) fn write_json_atomic<T: Serialize>(path: &Path, val: &T) -> Result<(), BudgetError> {
    let bytes = serde_json::to_vec_pretty(val).map_err(BudgetError::storage)?;
    write_private_atomic(path, &bytes).map_err(BudgetError::storage)
}
