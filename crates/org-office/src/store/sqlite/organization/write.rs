//! [`OrganizationWrite`] for the writer's transaction.

use org_store::{EngineError, Id};
use rusqlite::types::Value;

use super::super::SqliteTx;
use super::rows::office_code_row;
use super::{
    all, meta_values, optional, optional_id_value, put, put_record_ref, run, text, whole, View,
    AGENT_EXEC, EMPLOYEE, META, MODEL_PROFILE, OFFICE, ORGANIZATION, PROVIDER_CONN, REPOSITORY,
    ROLE, TEAM, TEAM_SCOPE,
};
use crate::organization::{
    ports::OrganizationWrite,
    records::{
        yaml::canonical_json, AgentExecutionConfig, BuiltinKey, Employee, HireRequest,
        ModelProfile, Office, OfficeCode, Organization, ProviderConnection, Repository, Role, Team,
        TeamAllocation, TeamCounter, WatchingModelSetting,
    },
};

/// `default_dod_json`: the items as a canonical JSON array, or null (`TASK-88`).
pub(in crate::store::sqlite) fn default_dod_value(items: Option<&Vec<String>>) -> Value {
    items.map_or(Value::Null, |items| {
        text(&canonical_json(&serde_json::json!(items)))
    })
}

impl OrganizationWrite for SqliteTx {
    fn put_organization(&mut self, record: &Organization) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let mut values = vec![
            text(record.id.as_str()),
            text(&record.slug),
            text(&record.name),
            text(record.organization_repository_id.as_str()),
            text(&record.state_branch),
            default_dod_value(record.default_dod.as_ref()),
        ];
        meta_values(&record.meta, &mut values)?;
        let columns = format!("{ORGANIZATION}, schema_version, {META}");
        put(db, "organizations", &columns, 1, values)?;
        let others = "DELETE FROM record_refs WHERE organization_id = ?1 \
                      AND kind = 'organization' AND id <> ?2";
        run(db, others, vec![text(scope), text(record.id.as_str())])?;
        put_record_ref(db, scope, "organization", &record.id)
    }

    fn remove_organization(&mut self) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let refs = "DELETE FROM record_refs WHERE organization_id = ?1 AND kind = 'organization'";
        run(db, refs, vec![text(scope)])?;
        run(
            db,
            "DELETE FROM organizations WHERE id = ?1",
            vec![text(scope)],
        )
    }

    fn put_repositories(&mut self, records: &[Repository]) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let clear = "DELETE FROM repositories WHERE organization_id = ?1";
        run(db, clear, vec![text(scope)])?;
        let sql = format!(
            "INSERT INTO repositories (organization_id, {REPOSITORY}, schema_version, {META}) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)"
        );
        for record in records {
            let mut values = vec![
                text(scope),
                text(record.id.as_str()),
                optional(record.key.as_deref()),
                text(record.kind.as_str()),
                optional(record.remote_url.as_deref()),
            ];
            meta_values(&record.meta, &mut values)?;
            run(db, &sql, values)?;
        }
        Ok(())
    }

    fn put_office(&mut self, record: &Office) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let mut values = vec![
            text(scope),
            text(record.id.as_str()),
            text(&record.slug),
            text(&record.name),
            text(record.role.as_str()),
        ];
        meta_values(&record.meta, &mut values)?;
        let columns = format!("organization_id, {OFFICE}, schema_version, {META}");
        put(db, "offices", &columns, 2, values)
    }

    fn remove_office(&mut self, id: &Id) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let sql = "DELETE FROM offices WHERE organization_id = ?1 AND id = ?2";
        run(db, sql, vec![text(scope), text(id.as_str())])
    }

    fn put_office_codes(&mut self, records: &[OfficeCode]) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let existing_sql = "SELECT code, office_id, issued_at, retired_at FROM office_codes \
                            WHERE organization_id = ?1 ORDER BY rowid";
        let existing = all(db, existing_sql, vec![text(scope)], office_code_row)?;
        for old in &existing {
            if !records.iter().any(|r| r.code == old.code) {
                return Err(EngineError::new("office codes must be retained"));
            }
        }
        let insert_sql = "INSERT INTO office_codes (organization_id, code, office_id, issued_at, \
                          retired_at) VALUES (?1, ?2, ?3, ?4, ?5)";
        let update_sql = "UPDATE office_codes SET office_id = ?3, issued_at = ?4, retired_at = ?5 \
                          WHERE organization_id = ?1 AND code = ?2";
        for record in records {
            if let Some(old) = existing.iter().find(|e| e.code == record.code) {
                if old == record {
                    continue;
                }
                let values = vec![
                    text(scope),
                    text(&record.code),
                    text(record.office_id.as_str()),
                    text(&record.issued_at),
                    optional(record.retired_at.as_deref()),
                ];
                run(db, update_sql, values)?;
            } else {
                let values = vec![
                    text(scope),
                    text(&record.code),
                    text(record.office_id.as_str()),
                    text(&record.issued_at),
                    optional(record.retired_at.as_deref()),
                ];
                run(db, insert_sql, values)?;
            }
        }
        Ok(())
    }

    fn put_role(&mut self, record: &Role) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let mut values = vec![
            text(scope),
            text(record.id.as_str()),
            optional(record.key.as_deref()),
            text(&record.name),
            text(&record.body),
        ];
        meta_values(&record.meta, &mut values)?;
        let columns = format!("organization_id, {ROLE}, schema_version, {META}");
        put(db, "roles", &columns, 2, values)
    }

    fn remove_role(&mut self, id: &Id) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let sql = "DELETE FROM roles WHERE organization_id = ?1 AND id = ?2";
        run(db, sql, vec![text(scope), text(id.as_str())])
    }

    fn put_team(&mut self, record: &Team) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let mut values = vec![
            text(scope),
            text(record.id.as_str()),
            text(&record.name),
            whole(record.number)?,
            optional_id_value(record.parent_team_id.as_ref()),
            optional_id_value(record.lead_employee_id.as_ref()),
            text(record.status.as_str()),
            whole(record.max_members)?,
        ];
        meta_values(&record.meta, &mut values)?;
        let columns = format!("organization_id, {TEAM}, schema_version, {META}");
        put(db, "teams", &columns, 2, values)?;
        put_record_ref(db, scope, "team", &record.id)
    }

    fn remove_team(&mut self, id: &Id) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let values = || vec![text(scope), text(id.as_str())];
        let refs = "DELETE FROM record_refs WHERE organization_id = ?1 AND kind = 'team' \
                    AND id = ?2";
        run(db, refs, values())?;
        let sql = "DELETE FROM teams WHERE organization_id = ?1 AND id = ?2";
        run(db, sql, values())
    }

    fn put_employee(&mut self, record: &Employee) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let mut values = vec![
            text(scope),
            text(record.id.as_str()),
            text(&record.handle),
            text(record.kind.as_str()),
            text(&record.name),
            text(record.home_team_id.as_str()),
            optional_id_value(record.role_id.as_ref()),
            optional_id_value(record.reports_to_employee_id.as_ref()),
            text(record.status.as_str()),
            optional(record.contact.as_deref()),
            optional(record.title.as_deref()),
            optional(record.focus.as_deref()),
            optional(record.role_text.as_deref()),
            optional(record.title_in_team.as_deref()),
            optional(record.offboarded_at.as_deref()),
            Value::Integer(i64::from(record.may_hire)),
            optional(record.builtin_key.map(BuiltinKey::as_str)),
            text(&record.body),
        ];
        meta_values(&record.meta, &mut values)?;
        let columns = format!("organization_id, {EMPLOYEE}, schema_version, {META}");
        put(db, "employees", &columns, 2, values)
    }

    fn remove_employee(&mut self, id: &Id) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let sql = "DELETE FROM employees WHERE organization_id = ?1 AND id = ?2";
        run(db, sql, vec![text(scope), text(id.as_str())])
    }

    fn put_hire_request(&mut self, record: &HireRequest) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let proposal_val = serde_json::to_value(&record.proposal)
            .map_err(|e| EngineError::new(format!("proposal json: {e}")))?;
        let values = vec![
            text(scope),
            text(record.id.as_str()),
            Value::Integer(1),
            whole(record.revision)?,
            text(record.state.as_str()),
            text(record.requested_by_employee_id.as_str()),
            text(record.home_team_id.as_str()),
            optional_id_value(record.role_id.as_ref()),
            text(&canonical_json(&proposal_val)),
            text(&record.reason),
            optional(record.confirmed_at.as_deref()),
            optional(record.closed_reason.as_deref()),
            optional_id_value(record.employee_id.as_ref()),
            optional(record.failed_step.as_deref()),
            text(&record.created_at),
            text(&record.updated_at),
        ];
        let columns = "organization_id, id, schema_version, revision, state, \
                       requested_by_employee_id, home_team_id, role_id, proposal_json, reason, \
                       confirmed_at, closed_reason, employee_id, failed_step, created_at, \
                       updated_at";
        put(db, "hire_requests", columns, 2, values)
    }

    fn remove_hire_request(&mut self, id: &Id) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let sql = "DELETE FROM hire_requests WHERE organization_id = ?1 AND id = ?2";
        run(db, sql, vec![text(scope), text(id.as_str())])
    }

    fn put_agent_execution(&mut self, record: &AgentExecutionConfig) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let tools_json = serde_json::to_string(&record.tools_config)
            .map_err(|e| EngineError::new(format!("tools json: {e}")))?;
        let mut values = vec![
            text(scope),
            text(record.employee_id.as_str()),
            text(record.model_profile_id.as_str()),
            text(&record.model_id),
            text(&record.system_prompt_markdown),
            text(&tools_json),
            text(record.permission_preset.as_str()),
            Value::Integer(i64::from(record.enabled)),
            optional(record.effort.map(|e| e.as_str())),
        ];
        meta_values(&record.meta, &mut values)?;
        let columns = format!("organization_id, {AGENT_EXEC}, schema_version, {META}");
        put(db, "agent_execution_configs", &columns, 2, values)
    }

    fn remove_agent_execution(&mut self, employee_id: &Id) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let sql =
            "DELETE FROM agent_execution_configs WHERE organization_id = ?1 AND employee_id = ?2";
        run(db, sql, vec![text(scope), text(employee_id.as_str())])
    }

    fn put_provider_connection(&mut self, record: &ProviderConnection) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let mut values = vec![
            text(scope),
            text(record.id.as_str()),
            text(&record.key),
            text(&record.name),
            text(&record.provider_type),
            text(record.auth_mode.as_str()),
            optional(record.subscription_identity.as_deref()),
            text(&record.runtime_driver),
            optional(record.endpoint_url.as_deref()),
            Value::Integer(i64::from(record.enabled)),
            Value::Integer(i64::from(record.archived)),
            whole(record.max_concurrent_turns)?,
        ];
        meta_values(&record.meta, &mut values)?;
        let columns = format!("organization_id, {PROVIDER_CONN}, schema_version, {META}");
        put(db, "provider_connections", &columns, 2, values)
    }

    fn remove_provider_connection(&mut self, id: &Id) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let sql = "DELETE FROM provider_connections WHERE organization_id = ?1 AND id = ?2";
        run(db, sql, vec![text(scope), text(id.as_str())])
    }

    fn put_model_profile(&mut self, record: &ModelProfile) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let mut values = vec![
            text(scope),
            text(record.id.as_str()),
            text(&record.key),
            text(&record.name),
            text(&record.description),
            Value::Integer(i64::from(record.is_auto_generated)),
            optional_id_value(record.generated_for_connection_id.as_ref()),
            Value::Integer(i64::from(record.archived)),
        ];
        meta_values(&record.meta, &mut values)?;
        let columns = format!("organization_id, {MODEL_PROFILE}, schema_version, {META}");
        put(db, "model_profiles", &columns, 2, values)?;
        run(
            db,
            "DELETE FROM model_profile_connections WHERE organization_id = ?1 AND model_profile_id = ?2",
            vec![text(scope), text(record.id.as_str())],
        )?;
        for (idx, conn_id) in record.connection_ids.iter().enumerate() {
            let priority = i64::try_from(idx + 1).map_err(|_| EngineError::new("priority"))?;
            run(
                db,
                "INSERT INTO model_profile_connections (organization_id, model_profile_id, connection_id, priority) VALUES (?1, ?2, ?3, ?4)",
                vec![
                    text(scope),
                    text(record.id.as_str()),
                    text(conn_id.as_str()),
                    Value::Integer(priority),
                ],
            )?;
        }
        Ok(())
    }

    fn remove_model_profile(&mut self, id: &Id) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        run(
            db,
            "DELETE FROM model_profile_connections WHERE organization_id = ?1 AND model_profile_id = ?2",
            vec![text(scope), text(id.as_str())],
        )?;
        let sql = "DELETE FROM model_profiles WHERE organization_id = ?1 AND id = ?2";
        run(db, sql, vec![text(scope), text(id.as_str())])
    }

    fn put_watching_model(&mut self, record: &WatchingModelSetting) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let mut values = vec![
            text(scope),
            optional_id_value(record.model_profile_id.as_ref()),
            Value::Integer(1),
        ];
        meta_values(&record.meta, &mut values)?;
        let columns = format!("organization_id, model_profile_id, schema_version, {META}");
        put(db, "watching_model_settings", &columns, 1, values)
    }

    fn remove_watching_model(&mut self) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        run(
            db,
            "DELETE FROM watching_model_settings WHERE organization_id = ?1",
            vec![text(scope)],
        )
    }

    fn put_team_counter(&mut self, record: &TeamCounter) -> Result<(), EngineError> {
        let (db, _) = self.view()?;
        let organization = record.organization_id.as_str();
        let values = vec![
            text(organization),
            text("organization"),
            text(organization),
            text("team"),
            whole(record.last_number)?,
        ];
        let columns = "organization_id, scope_kind, scope_id, entity_kind, last_number";
        put(db, "reference_counters", columns, 4, values)
    }

    fn remove_team_counter(&mut self) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let sql =
            format!("DELETE FROM reference_counters WHERE organization_id = ?1 AND {TEAM_SCOPE}");
        run(db, &sql, vec![text(scope)])
    }

    fn put_team_allocation(&mut self, record: &TeamAllocation) -> Result<(), EngineError> {
        let (db, _) = self.view()?;
        let organization = record.organization_id.as_str();
        let values = vec![
            text(organization),
            text("organization"),
            text(organization),
            text("team"),
            whole(record.number)?,
            text(record.team_id.as_str()),
            text(&record.allocated_at),
        ];
        let columns = "organization_id, scope_kind, scope_id, entity_kind, number, entity_id, \
                       allocated_at";
        put(db, "reference_allocations", columns, 5, values)
    }

    fn remove_team_allocation(&mut self, number: u64) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let sql = format!(
            "DELETE FROM reference_allocations WHERE organization_id = ?1 AND {TEAM_SCOPE} \
             AND number = ?2"
        );
        run(db, &sql, vec![text(scope), whole(number)?])
    }
}
