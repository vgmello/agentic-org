//! [`DeliveryWrite`] implementation for [`SqliteTx`].

use org_docs::delivery::records::{
    DeploymentRecord, EntityLinkRecord, FeatureFlagRecord, FeatureRecord, LinkEntityKind,
    MilestoneRecord, NumberCounterRecord, ReleaseRecord, ScopedChangelogRecord,
};
use org_docs::project_tree::canonical_json;
use org_office::delivery::ports::{DeliveryRead, DeliveryWrite};
use org_store::{EngineError, Id};
use rusqlite::types::Value;

use super::push_meta_values;
use crate::organization::{optional, put, put_record_ref, run, text, update, whole, View, META};
use crate::SqliteTx;

fn parse_id(raw: &str) -> Result<Id, EngineError> {
    Id::parse(raw).map_err(|_| EngineError::new(format!("invalid id `{raw}`")))
}

impl DeliveryWrite for SqliteTx {
    fn put_feature(&mut self, rec: &FeatureRecord) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let mut values = vec![
            text(scope),
            text(&rec.id),
            text(&rec.project_id),
            whole(rec.number)?,
            text(&rec.local_slug),
            optional(rec.owner_employee_id.as_deref()),
            optional(rec.work_type.map(|w| w.as_str())),
            text(&rec.title),
            text(rec.status.as_str()),
            text(&rec.body_markdown),
            optional(rec.delivered_at.as_deref()),
            optional(rec.retired_reason.as_deref()),
            optional(rec.replaced_by_feature_id.as_deref()),
        ];
        push_meta_values(&rec.meta, &mut values)?;
        let cols = format!(
            "organization_id, id, project_id, number, local_slug, owner_employee_id, work_type, \
             title, status, body_markdown, delivered_at, retired_reason, replaced_by_feature_id, \
             schema_version, {META}"
        );
        put(db, "features", &cols, 2, values)?;
        put_record_ref(db, scope, "feature", &parse_id(&rec.id)?)
    }

    fn remove_feature(&mut self, id: &Id) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        run(
            db,
            "DELETE FROM features WHERE organization_id = ?1 AND id = ?2",
            vec![text(scope), text(id.as_str())],
        )?;
        run(
            db,
            "DELETE FROM record_refs WHERE organization_id = ?1 AND kind = 'feature' AND id = ?2",
            vec![text(scope), text(id.as_str())],
        )
    }

    fn put_release(&mut self, rec: &ReleaseRecord) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let mut values = vec![
            text(scope),
            text(&rec.id),
            text(&rec.project_id),
            whole(rec.number)?,
            text(&rec.title),
            optional(rec.version_label.as_deref()),
            text(&rec.body_markdown),
        ];
        push_meta_values(&rec.meta, &mut values)?;
        let cols = format!(
            "organization_id, id, project_id, number, title, version_label, body_markdown, \
             schema_version, {META}"
        );
        put(db, "releases", &cols, 2, values)?;
        put_record_ref(db, scope, "release", &parse_id(&rec.id)?)?;

        run(
            db,
            "DELETE FROM release_repository_commits WHERE organization_id = ?1 AND release_id = ?2",
            vec![text(scope), text(&rec.id)],
        )?;
        for c in &rec.repository_commits {
            run(
                db,
                "INSERT INTO release_repository_commits (organization_id, release_id, project_id, repository_id, commit_sha) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                vec![text(scope), text(&rec.id), text(&rec.project_id), text(&c.repository_id), text(&c.commit_sha)],
            )?;
        }
        run(
            db,
            "DELETE FROM release_features WHERE organization_id = ?1 AND release_id = ?2",
            vec![text(scope), text(&rec.id)],
        )?;
        for fid in &rec.feature_ids {
            run(
                db,
                "INSERT INTO release_features (organization_id, release_id, project_id, feature_id) \
                 VALUES (?1, ?2, ?3, ?4)",
                vec![text(scope), text(&rec.id), text(&rec.project_id), text(fid)],
            )?;
        }
        run(
            db,
            "DELETE FROM release_tasks WHERE organization_id = ?1 AND release_id = ?2",
            vec![text(scope), text(&rec.id)],
        )?;
        for tid in &rec.task_ids {
            run(
                db,
                "INSERT INTO release_tasks (organization_id, release_id, project_id, task_id, task_kind) \
                 VALUES (?1, ?2, ?3, ?4, 'task')",
                vec![text(scope), text(&rec.id), text(&rec.project_id), text(tid)],
            )?;
        }
        run(
            db,
            "DELETE FROM release_flag_settings WHERE organization_id = ?1 AND release_id = ?2",
            vec![text(scope), text(&rec.id)],
        )?;
        for s in &rec.flag_settings {
            run(
                db,
                "INSERT INTO release_flag_settings \
                 (organization_id, release_id, project_id, flag_id, environment, target, value_json, rollout_rules_json) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                vec![
                    text(scope),
                    text(&rec.id),
                    text(&rec.project_id),
                    text(&s.flag_id),
                    text(&s.environment),
                    text(&s.target),
                    text(&canonical_json(&s.value)),
                    s.rollout_rules.as_ref().map_or(Value::Null, |v| text(&canonical_json(v))),
                ],
            )?;
        }
        Ok(())
    }

    fn remove_release(&mut self, id: &Id) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        for tbl in [
            "release_repository_commits",
            "release_features",
            "release_tasks",
            "release_flag_settings",
        ] {
            run(
                db,
                &format!("DELETE FROM {tbl} WHERE organization_id = ?1 AND release_id = ?2"),
                vec![text(scope), text(id.as_str())],
            )?;
        }
        run(
            db,
            "DELETE FROM releases WHERE organization_id = ?1 AND id = ?2",
            vec![text(scope), text(id.as_str())],
        )?;
        run(
            db,
            "DELETE FROM record_refs WHERE organization_id = ?1 AND kind = 'release' AND id = ?2",
            vec![text(scope), text(id.as_str())],
        )
    }

    fn put_milestone(&mut self, rec: &MilestoneRecord) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let mut values = vec![
            text(scope),
            text(&rec.id),
            text(&rec.project_id),
            text(&rec.release_id),
            whole(rec.number)?,
            text(&rec.title),
            text(&rec.body_markdown),
        ];
        push_meta_values(&rec.meta, &mut values)?;
        let cols = format!(
            "organization_id, id, project_id, release_id, number, title, body_markdown, \
             schema_version, {META}"
        );
        put(db, "milestones", &cols, 2, values)?;
        put_record_ref(db, scope, "milestone", &parse_id(&rec.id)?)
    }

    fn remove_milestone(&mut self, id: &Id) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        run(
            db,
            "DELETE FROM milestones WHERE organization_id = ?1 AND id = ?2",
            vec![text(scope), text(id.as_str())],
        )?;
        run(
            db,
            "DELETE FROM record_refs WHERE organization_id = ?1 AND kind = 'milestone' AND id = ?2",
            vec![text(scope), text(id.as_str())],
        )
    }

    fn put_deployment(&mut self, rec: &DeploymentRecord) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let mut values = vec![
            text(scope),
            text(&rec.id),
            text(&rec.project_id),
            text(&rec.release_id),
            text(&rec.environment),
            text(&rec.target),
            text(rec.operation.as_str()),
            optional(rec.rollback_of_id.as_deref()),
            optional(rec.baseline_deployment_id.as_deref()),
            text(rec.status.as_str()),
            optional(rec.started_at.as_deref()),
            optional(rec.finished_at.as_deref()),
            optional(rec.external_url.as_deref()),
            text(&rec.body_markdown),
        ];
        push_meta_values(&rec.meta, &mut values)?;
        let cols = format!(
            "organization_id, id, project_id, release_id, environment, target, operation, \
             rollback_of_id, baseline_deployment_id, status, started_at, finished_at, \
             external_url, body_markdown, schema_version, {META}"
        );
        put(db, "deployments", &cols, 2, values)?;
        put_record_ref(db, scope, "deployment", &parse_id(&rec.id)?)?;

        run(
            db,
            "DELETE FROM deployment_repository_commits WHERE organization_id = ?1 AND deployment_id = ?2",
            vec![text(scope), text(&rec.id)],
        )?;
        for c in &rec.repository_commits {
            run(
                db,
                "INSERT INTO deployment_repository_commits (organization_id, deployment_id, project_id, repository_id, commit_sha) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                vec![text(scope), text(&rec.id), text(&rec.project_id), text(&c.repository_id), text(&c.commit_sha)],
            )?;
        }
        run(
            db,
            "DELETE FROM deployment_flag_settings WHERE organization_id = ?1 AND deployment_id = ?2",
            vec![text(scope), text(&rec.id)],
        )?;
        for s in &rec.flag_settings {
            run(
                db,
                "INSERT INTO deployment_flag_settings \
                 (organization_id, deployment_id, project_id, flag_id, value_json, rollout_rules_json, applied_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                vec![
                    text(scope),
                    text(&rec.id),
                    text(&rec.project_id),
                    text(&s.flag_id),
                    text(&canonical_json(&s.value)),
                    s.rollout_rules.as_ref().map_or(Value::Null, |v| text(&canonical_json(v))),
                    optional(s.applied_at.as_deref()),
                ],
            )?;
        }
        Ok(())
    }

    fn remove_deployment(&mut self, id: &Id) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        for tbl in ["deployment_repository_commits", "deployment_flag_settings"] {
            run(
                db,
                &format!("DELETE FROM {tbl} WHERE organization_id = ?1 AND deployment_id = ?2"),
                vec![text(scope), text(id.as_str())],
            )?;
        }
        run(
            db,
            "DELETE FROM deployments WHERE organization_id = ?1 AND id = ?2",
            vec![text(scope), text(id.as_str())],
        )?;
        run(
            db,
            "DELETE FROM record_refs WHERE organization_id = ?1 AND kind = 'deployment' AND id = ?2",
            vec![text(scope), text(id.as_str())],
        )
    }

    fn put_flag(&mut self, rec: &FeatureFlagRecord) -> Result<(), EngineError> {
        let existing = self.flag(&parse_id(&rec.id)?)?;
        if existing.as_ref() == Some(rec) {
            return Ok(());
        }
        let (db, scope) = self.view()?;
        let mut values = vec![
            text(scope),
            text(&rec.id),
            text(&rec.project_id),
            optional(rec.feature_id.as_deref()),
            text(&rec.key),
            optional(rec.owner_employee_id.as_deref()),
            text(rec.status.as_str()),
            text(rec.value_type.as_str()),
            text(&canonical_json(&rec.default_value)),
            text(&rec.body_markdown),
        ];
        push_meta_values(&rec.meta, &mut values)?;
        let cols = format!(
            "organization_id, id, project_id, feature_id, key, owner_employee_id, status, \
             value_type, default_value_json, body_markdown, schema_version, {META}"
        );
        if existing.is_some() {
            // Explicit UPDATE so `used_flag_identity_immutable` fires fail-closed on identity changes.
            update(db, "feature_flags", &cols, 2, values)
        } else {
            put(db, "feature_flags", &cols, 2, values)
        }
    }

    fn remove_flag(&mut self, id: &Id) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        run(
            db,
            "DELETE FROM feature_flags WHERE organization_id = ?1 AND id = ?2",
            vec![text(scope), text(id.as_str())],
        )
    }

    fn put_scoped_changelog(&mut self, rec: &ScopedChangelogRecord) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let mut values = vec![
            text(scope),
            text(&rec.id),
            text(&rec.project_id),
            optional(rec.release_id.as_deref()),
            optional(rec.deployment_id.as_deref()),
            text("changelog"),
            text(&rec.title),
            text(rec.status.as_str()),
            optional(rec.owner_employee_id.as_deref()),
            text(&rec.body_markdown),
        ];
        push_meta_values(&rec.meta, &mut values)?;
        let cols = format!(
            "organization_id, id, project_id, release_id, deployment_id, kind, title, status, \
             owner_employee_id, body_markdown, schema_version, {META}"
        );
        put(db, "documents", &cols, 2, values)?;
        put_record_ref(db, scope, "document", &parse_id(&rec.id)?)
    }

    fn remove_release_changelog(&mut self, release: &Id) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        run(
            db,
            "DELETE FROM documents WHERE organization_id = ?1 AND release_id = ?2 AND kind = 'changelog'",
            vec![text(scope), text(release.as_str())],
        )?;
        run(
            db,
            "DELETE FROM record_refs WHERE organization_id = ?1 AND kind = 'document' \
             AND NOT EXISTS (SELECT 1 FROM documents d WHERE d.organization_id = ?1 AND d.id = record_refs.id)",
            vec![text(scope)],
        )
    }

    fn remove_deployment_changelog(&mut self, deployment: &Id) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        run(
            db,
            "DELETE FROM documents WHERE organization_id = ?1 AND deployment_id = ?2 AND kind = 'changelog'",
            vec![text(scope), text(deployment.as_str())],
        )?;
        run(
            db,
            "DELETE FROM record_refs WHERE organization_id = ?1 AND kind = 'document' \
             AND NOT EXISTS (SELECT 1 FROM documents d WHERE d.organization_id = ?1 AND d.id = record_refs.id)",
            vec![text(scope)],
        )
    }

    fn put_entity_link(&mut self, rec: &EntityLinkRecord) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        put(
            db,
            "links",
            "organization_id, source_kind, source_id, id, kind, target_kind, target_id",
            4,
            vec![
                text(scope),
                text(rec.source_kind.as_str()),
                text(&rec.source_id),
                text(&rec.id),
                text(rec.kind.as_str()),
                text(rec.target_kind.as_str()),
                text(&rec.target_id),
            ],
        )
    }

    fn remove_entity_link(
        &mut self,
        source_kind: LinkEntityKind,
        source_id: &Id,
        link_id: &Id,
    ) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        run(
            db,
            "DELETE FROM links WHERE organization_id = ?1 AND source_kind = ?2 AND source_id = ?3 AND id = ?4",
            vec![
                text(scope),
                text(source_kind.as_str()),
                text(source_id.as_str()),
                text(link_id.as_str()),
            ],
        )
    }

    fn put_delivery_counter(
        &mut self,
        scope_kind: &str,
        scope_id: &Id,
        entity_kind: &str,
        counter: &NumberCounterRecord,
    ) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        put(
            db,
            "reference_counters",
            "organization_id, scope_kind, scope_id, entity_kind, last_number",
            4,
            vec![
                text(scope),
                text(scope_kind),
                text(scope_id.as_str()),
                text(entity_kind),
                whole(counter.next_number.saturating_sub(1))?,
            ],
        )?;
        for a in &counter.allocations {
            put(
                db,
                "reference_allocations",
                "organization_id, scope_kind, scope_id, entity_kind, number, entity_id, allocated_at",
                5,
                vec![
                    text(scope),
                    text(scope_kind),
                    text(scope_id.as_str()),
                    text(entity_kind),
                    whole(a.number)?,
                    text(&a.entity_id),
                    text("1970-01-01T00:00:00Z"),
                ],
            )?;
        }
        Ok(())
    }

    fn remove_delivery_counter(
        &mut self,
        scope_kind: &str,
        scope_id: &Id,
        entity_kind: &str,
    ) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        run(
            db,
            "DELETE FROM reference_allocations \
             WHERE organization_id = ?1 AND scope_kind = ?2 AND scope_id = ?3 AND entity_kind = ?4",
            vec![
                text(scope),
                text(scope_kind),
                text(scope_id.as_str()),
                text(entity_kind),
            ],
        )?;
        run(
            db,
            "DELETE FROM reference_counters \
             WHERE organization_id = ?1 AND scope_kind = ?2 AND scope_id = ?3 AND entity_kind = ?4",
            vec![
                text(scope),
                text(scope_kind),
                text(scope_id.as_str()),
                text(entity_kind),
            ],
        )
    }
}
