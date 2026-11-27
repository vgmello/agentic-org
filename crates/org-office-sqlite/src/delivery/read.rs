//! [`DeliveryRead`] implementation for [`SqliteTx`] and [`SqliteSnapshot`].

use std::collections::BTreeSet;

use org_docs::delivery::records::{
    DeploymentFlagSettingRecord, DeploymentOperation, DeploymentRecord, DeploymentStatus,
    EntityLinkRecord, FeatureFlagRecord, FeatureRecord, FeatureStatus, FeatureWorkType, FlagStatus,
    FlagValueType, LinkEntityKind, LinkKind, MilestoneRecord, NumberAllocation,
    NumberCounterRecord, ReleaseFlagSettingRecord, ReleaseRecord, RepoCommitRecord,
    ScopedChangelogRecord,
};
use org_docs::project_tree::DocStatus;
use org_office::delivery::ports::DeliveryRead;
use org_store::{EngineError, Id};
use rusqlite::{Connection, Row};

use super::{foreign, read_meta};
use crate::organization::{all, id, number, one, text, View, META};
use crate::{SqliteSnapshot, SqliteTx};

const FEATURE_COLS: &str = "id, project_id, number, local_slug, owner_employee_id, work_type, \
    title, status, body_markdown, delivered_at, retired_reason, replaced_by_feature_id";
const RELEASE_COLS: &str = "id, project_id, number, title, version_label, body_markdown";
const MILESTONE_COLS: &str = "id, project_id, release_id, number, title, body_markdown";
const DEPLOYMENT_COLS: &str = "id, project_id, release_id, environment, target, operation, \
    rollback_of_id, baseline_deployment_id, status, started_at, finished_at, external_url, body_markdown";
const FLAG_COLS: &str = "id, project_id, feature_id, key, owner_employee_id, status, \
    value_type, default_value_json, body_markdown";
const CHANGELOG_COLS: &str = "id, project_id, release_id, deployment_id, title, status, \
    owner_employee_id, body_markdown";

fn feature_row(row: &Row<'_>) -> rusqlite::Result<FeatureRecord> {
    let wt: Option<String> = row.get(5)?;
    let work_type = wt
        .map(|s| FeatureWorkType::parse(&s).ok_or_else(|| foreign(5, "invalid work_type")))
        .transpose()?;
    let status_s: String = row.get(7)?;
    let status =
        FeatureStatus::parse(&status_s).ok_or_else(|| foreign(7, "invalid feature status"))?;
    Ok(FeatureRecord {
        id: row.get(0)?,
        project_id: row.get(1)?,
        number: number(row, 2)?,
        local_slug: row.get(3)?,
        owner_employee_id: row.get(4)?,
        work_type,
        title: row.get(6)?,
        status,
        body_markdown: row.get(8)?,
        delivered_at: row.get(9)?,
        retired_reason: row.get(10)?,
        replaced_by_feature_id: row.get(11)?,
        meta: read_meta(row, 12)?,
    })
}

fn release_base_row(row: &Row<'_>) -> rusqlite::Result<ReleaseRecord> {
    Ok(ReleaseRecord {
        id: row.get(0)?,
        project_id: row.get(1)?,
        number: number(row, 2)?,
        title: row.get(3)?,
        version_label: row.get(4)?,
        body_markdown: row.get(5)?,
        repository_commits: Vec::new(),
        feature_ids: Vec::new(),
        task_ids: Vec::new(),
        flag_settings: Vec::new(),
        meta: read_meta(row, 6)?,
    })
}

fn hydrate_release(
    db: &Connection,
    scope: &str,
    mut rel: ReleaseRecord,
) -> Result<ReleaseRecord, EngineError> {
    rel.repository_commits = all(
        db,
        "SELECT repository_id, commit_sha FROM release_repository_commits \
         WHERE organization_id = ?1 AND release_id = ?2 ORDER BY repository_id",
        vec![text(scope), text(&rel.id)],
        |r| {
            Ok(RepoCommitRecord {
                repository_id: r.get(0)?,
                commit_sha: r.get(1)?,
            })
        },
    )?;
    rel.feature_ids = all(
        db,
        "SELECT feature_id FROM release_features \
         WHERE organization_id = ?1 AND release_id = ?2 ORDER BY feature_id",
        vec![text(scope), text(&rel.id)],
        |r| r.get(0),
    )?;
    rel.task_ids = all(
        db,
        "SELECT task_id FROM release_tasks \
         WHERE organization_id = ?1 AND release_id = ?2 ORDER BY task_id",
        vec![text(scope), text(&rel.id)],
        |r| r.get(0),
    )?;
    rel.flag_settings = all(
        db,
        "SELECT flag_id, environment, target, value_json, rollout_rules_json \
         FROM release_flag_settings \
         WHERE organization_id = ?1 AND release_id = ?2 \
         ORDER BY environment, target, flag_id",
        vec![text(scope), text(&rel.id)],
        |r| {
            let val_s: String = r.get(3)?;
            let value = serde_json::from_str(&val_s).map_err(|_| foreign(3, "bad json"))?;
            let rules_s: Option<String> = r.get(4)?;
            let rollout_rules = rules_s
                .map(|s| serde_json::from_str(&s).map_err(|_| foreign(4, "bad json")))
                .transpose()?;
            Ok(ReleaseFlagSettingRecord {
                flag_id: r.get(0)?,
                environment: r.get(1)?,
                target: r.get(2)?,
                value,
                rollout_rules,
            })
        },
    )?;
    Ok(rel)
}

fn milestone_row(row: &Row<'_>) -> rusqlite::Result<MilestoneRecord> {
    Ok(MilestoneRecord {
        id: row.get(0)?,
        project_id: row.get(1)?,
        release_id: row.get(2)?,
        number: number(row, 3)?,
        title: row.get(4)?,
        body_markdown: row.get(5)?,
        meta: read_meta(row, 6)?,
    })
}

fn deployment_base_row(row: &Row<'_>) -> rusqlite::Result<DeploymentRecord> {
    let op_s: String = row.get(5)?;
    let operation = DeploymentOperation::parse(&op_s).ok_or_else(|| foreign(5, "bad op"))?;
    let st_s: String = row.get(8)?;
    let status = DeploymentStatus::parse(&st_s).ok_or_else(|| foreign(8, "bad status"))?;
    Ok(DeploymentRecord {
        id: row.get(0)?,
        project_id: row.get(1)?,
        release_id: row.get(2)?,
        environment: row.get(3)?,
        target: row.get(4)?,
        operation,
        rollback_of_id: row.get(6)?,
        baseline_deployment_id: row.get(7)?,
        status,
        started_at: row.get(9)?,
        finished_at: row.get(10)?,
        external_url: row.get(11)?,
        body_markdown: row.get(12)?,
        repository_commits: Vec::new(),
        flag_settings: Vec::new(),
        meta: read_meta(row, 13)?,
    })
}

fn hydrate_deployment(
    db: &Connection,
    scope: &str,
    mut dep: DeploymentRecord,
) -> Result<DeploymentRecord, EngineError> {
    dep.repository_commits = all(
        db,
        "SELECT repository_id, commit_sha FROM deployment_repository_commits \
         WHERE organization_id = ?1 AND deployment_id = ?2 ORDER BY repository_id",
        vec![text(scope), text(&dep.id)],
        |r| {
            Ok(RepoCommitRecord {
                repository_id: r.get(0)?,
                commit_sha: r.get(1)?,
            })
        },
    )?;
    dep.flag_settings = all(
        db,
        "SELECT flag_id, value_json, rollout_rules_json, applied_at \
         FROM deployment_flag_settings \
         WHERE organization_id = ?1 AND deployment_id = ?2 ORDER BY flag_id",
        vec![text(scope), text(&dep.id)],
        |r| {
            let val_s: String = r.get(1)?;
            let value = serde_json::from_str(&val_s).map_err(|_| foreign(1, "bad json"))?;
            let rules_s: Option<String> = r.get(2)?;
            let rollout_rules = rules_s
                .map(|s| serde_json::from_str(&s).map_err(|_| foreign(2, "bad json")))
                .transpose()?;
            Ok(DeploymentFlagSettingRecord {
                flag_id: r.get(0)?,
                value,
                rollout_rules,
                applied_at: r.get(3)?,
            })
        },
    )?;
    Ok(dep)
}

fn flag_row(row: &Row<'_>) -> rusqlite::Result<FeatureFlagRecord> {
    let st_s: String = row.get(5)?;
    let status = FlagStatus::parse(&st_s).ok_or_else(|| foreign(5, "bad flag status"))?;
    let vt_s: String = row.get(6)?;
    let value_type = FlagValueType::parse(&vt_s).ok_or_else(|| foreign(6, "bad value_type"))?;
    let dv_s: String = row.get(7)?;
    let default_value = serde_json::from_str(&dv_s).map_err(|_| foreign(7, "bad json"))?;
    Ok(FeatureFlagRecord {
        id: row.get(0)?,
        project_id: row.get(1)?,
        feature_id: row.get(2)?,
        key: row.get(3)?,
        owner_employee_id: row.get(4)?,
        status,
        value_type,
        default_value,
        body_markdown: row.get(8)?,
        meta: read_meta(row, 9)?,
    })
}

fn changelog_row(row: &Row<'_>) -> rusqlite::Result<ScopedChangelogRecord> {
    let st_s: String = row.get(5)?;
    let status = DocStatus::parse(&st_s).ok_or_else(|| foreign(5, "bad doc status"))?;
    Ok(ScopedChangelogRecord {
        id: row.get(0)?,
        project_id: row.get(1)?,
        release_id: row.get(2)?,
        deployment_id: row.get(3)?,
        title: row.get(4)?,
        status,
        owner_employee_id: row.get(6)?,
        body_markdown: row.get(7)?,
        meta: read_meta(row, 8)?,
    })
}

fn link_row(row: &Row<'_>) -> rusqlite::Result<EntityLinkRecord> {
    let sk: String = row.get(1)?;
    let k: String = row.get(3)?;
    let tk: String = row.get(4)?;
    Ok(EntityLinkRecord {
        id: row.get(0)?,
        project_id: row.get(6)?,
        source_kind: LinkEntityKind::parse(&sk).ok_or_else(|| foreign(1, "bad source_kind"))?,
        source_id: row.get(2)?,
        kind: LinkKind::parse(&k).ok_or_else(|| foreign(3, "bad link kind"))?,
        target_kind: LinkEntityKind::parse(&tk).ok_or_else(|| foreign(4, "bad target_kind"))?,
        target_id: row.get(5)?,
    })
}

macro_rules! sqlite_delivery_read {
    ($($view:ty),+) => {$(
        impl DeliveryRead for $view {
            fn project_features(&self, project: &Id) -> Result<Vec<FeatureRecord>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {FEATURE_COLS}, {META} FROM features \
                     WHERE organization_id = ?1 AND project_id = ?2 ORDER BY number, id"
                );
                all(db, &sql, vec![text(scope), text(project.as_str())], feature_row)
            }
            fn feature(&self, fid: &Id) -> Result<Option<FeatureRecord>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {FEATURE_COLS}, {META} FROM features \
                     WHERE organization_id = ?1 AND id = ?2"
                );
                one(db, &sql, vec![text(scope), text(fid.as_str())], feature_row)
            }
            fn all_features(&self) -> Result<Vec<FeatureRecord>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {FEATURE_COLS}, {META} FROM features \
                     WHERE organization_id = ?1 ORDER BY project_id, number, id"
                );
                all(db, &sql, vec![text(scope)], feature_row)
            }
            fn project_releases(&self, project: &Id) -> Result<Vec<ReleaseRecord>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {RELEASE_COLS}, {META} FROM releases \
                     WHERE organization_id = ?1 AND project_id = ?2 ORDER BY number, id"
                );
                let rows = all(db, &sql, vec![text(scope), text(project.as_str())], release_base_row)?;
                rows.into_iter().map(|r| hydrate_release(db, scope, r)).collect()
            }
            fn release(&self, rid: &Id) -> Result<Option<ReleaseRecord>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {RELEASE_COLS}, {META} FROM releases \
                     WHERE organization_id = ?1 AND id = ?2"
                );
                let row = one(db, &sql, vec![text(scope), text(rid.as_str())], release_base_row)?;
                row.map(|r| hydrate_release(db, scope, r)).transpose()
            }
            fn release_milestones(&self, release: &Id) -> Result<Vec<MilestoneRecord>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {MILESTONE_COLS}, {META} FROM milestones \
                     WHERE organization_id = ?1 AND release_id = ?2 ORDER BY number, id"
                );
                all(db, &sql, vec![text(scope), text(release.as_str())], milestone_row)
            }
            fn milestone(&self, mid: &Id) -> Result<Option<MilestoneRecord>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {MILESTONE_COLS}, {META} FROM milestones \
                     WHERE organization_id = ?1 AND id = ?2"
                );
                one(db, &sql, vec![text(scope), text(mid.as_str())], milestone_row)
            }
            fn project_deployments(&self, project: &Id) -> Result<Vec<DeploymentRecord>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {DEPLOYMENT_COLS}, {META} FROM deployments \
                     WHERE organization_id = ?1 AND project_id = ?2 ORDER BY id"
                );
                let rows = all(db, &sql, vec![text(scope), text(project.as_str())], deployment_base_row)?;
                rows.into_iter().map(|d| hydrate_deployment(db, scope, d)).collect()
            }
            fn deployment(&self, did: &Id) -> Result<Option<DeploymentRecord>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {DEPLOYMENT_COLS}, {META} FROM deployments \
                     WHERE organization_id = ?1 AND id = ?2"
                );
                let row = one(db, &sql, vec![text(scope), text(did.as_str())], deployment_base_row)?;
                row.map(|d| hydrate_deployment(db, scope, d)).transpose()
            }
            fn project_flags(&self, project: &Id) -> Result<Vec<FeatureFlagRecord>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {FLAG_COLS}, {META} FROM feature_flags \
                     WHERE organization_id = ?1 AND project_id = ?2 ORDER BY key, id"
                );
                all(db, &sql, vec![text(scope), text(project.as_str())], flag_row)
            }
            fn flag(&self, flid: &Id) -> Result<Option<FeatureFlagRecord>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {FLAG_COLS}, {META} FROM feature_flags \
                     WHERE organization_id = ?1 AND id = ?2"
                );
                one(db, &sql, vec![text(scope), text(flid.as_str())], flag_row)
            }
            fn used_flag_ids(&self, project: &Id) -> Result<BTreeSet<String>, EngineError> {
                let (db, scope) = self.view()?;
                let ids: Vec<String> = all(
                    db,
                    "SELECT flag_id FROM release_flag_settings \
                     WHERE organization_id = ?1 AND project_id = ?2 \
                     UNION SELECT flag_id FROM deployment_flag_settings \
                     WHERE organization_id = ?1 AND project_id = ?2",
                    vec![text(scope), text(project.as_str())],
                    |r| r.get(0),
                )?;
                Ok(ids.into_iter().collect())
            }
            fn release_changelog(&self, release: &Id) -> Result<Option<ScopedChangelogRecord>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {CHANGELOG_COLS}, {META} FROM documents \
                     WHERE organization_id = ?1 AND release_id = ?2 AND kind = 'changelog'"
                );
                one(db, &sql, vec![text(scope), text(release.as_str())], changelog_row)
            }
            fn deployment_changelog(&self, deployment: &Id) -> Result<Option<ScopedChangelogRecord>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {CHANGELOG_COLS}, {META} FROM documents \
                     WHERE organization_id = ?1 AND deployment_id = ?2 AND kind = 'changelog'"
                );
                one(db, &sql, vec![text(scope), text(deployment.as_str())], changelog_row)
            }
            fn project_links(&self, project: &Id) -> Result<Vec<EntityLinkRecord>, EngineError> {
                let (db, scope) = self.view()?;
                let p = project.as_str();
                let sql = "SELECT l.id, l.source_kind, l.source_id, l.kind, l.target_kind, l.target_id, ?2 \
                    FROM links l WHERE l.organization_id = ?1 AND l.kind <> 'depends_on' AND ( \
                      (l.source_kind = 'project' AND l.source_id = ?2) \
                      OR (l.source_kind = 'feature' AND EXISTS (SELECT 1 FROM features f WHERE f.organization_id = ?1 AND f.id = l.source_id AND f.project_id = ?2)) \
                      OR (l.source_kind = 'task' AND EXISTS (SELECT 1 FROM tasks t WHERE t.organization_id = ?1 AND t.id = l.source_id AND t.project_id = ?2)) \
                      OR (l.source_kind = 'document' AND EXISTS (SELECT 1 FROM documents d WHERE d.organization_id = ?1 AND d.id = l.source_id AND d.project_id = ?2)) \
                    ) ORDER BY l.source_kind, l.source_id, l.id";
                let mut rows = all(db, sql, vec![text(scope), text(p)], link_row)?;
                rows.sort();
                Ok(rows)
            }
            fn delivery_counter(
                &self,
                scope_kind: &str,
                scope_id: &Id,
                entity_kind: &str,
            ) -> Result<Option<NumberCounterRecord>, EngineError> {
                let (db, scope) = self.view()?;
                let last: Option<u64> = one(
                    db,
                    "SELECT last_number FROM reference_counters \
                     WHERE organization_id = ?1 AND scope_kind = ?2 AND scope_id = ?3 AND entity_kind = ?4",
                    vec![text(scope), text(scope_kind), text(scope_id.as_str()), text(entity_kind)],
                    |r| number(r, 0),
                )?;
                let Some(last_number) = last else {
                    return Ok(None);
                };
                let allocations = all(
                    db,
                    "SELECT number, entity_id FROM reference_allocations \
                     WHERE organization_id = ?1 AND scope_kind = ?2 AND scope_id = ?3 AND entity_kind = ?4 \
                     ORDER BY number",
                    vec![text(scope), text(scope_kind), text(scope_id.as_str()), text(entity_kind)],
                    |r| Ok(NumberAllocation { number: number(r, 0)?, entity_id: r.get(1)? }),
                )?;
                Ok(Some(NumberCounterRecord {
                    next_number: last_number.saturating_add(1),
                    allocations,
                }))
            }
            fn projects_with_delivery(&self) -> Result<Vec<Id>, EngineError> {
                let (db, scope) = self.view()?;
                all(
                    db,
                    "SELECT project_id FROM features WHERE organization_id = ?1 \
                     UNION SELECT project_id FROM releases WHERE organization_id = ?1 \
                     UNION SELECT project_id FROM deployments WHERE organization_id = ?1 \
                     UNION SELECT project_id FROM feature_flags WHERE organization_id = ?1 \
                     ORDER BY 1",
                    vec![text(scope)],
                    |r| id(r, 0),
                )
            }
        }
    )+};
}

sqlite_delivery_read!(SqliteTx, SqliteSnapshot);
