//! [`OrganizationRead`] for the writer's transaction and for a snapshot.

use org_store::{EngineError, Id};

use super::super::{SqliteSnapshot, SqliteTx};
use super::rows::{
    agent_execution_row, employee_row, hire_request_row, model_profile_base_row, office_code_row,
    office_row, organization_row, profile_connections, provider_connection_row, record_ref_row,
    repository_row, role_row, team_allocation_row, team_counter_row, team_row, watching_model_row,
};
use super::{
    all, one, select, text, whole, View, AGENT_EXEC, EMPLOYEE, HIRE_REQUEST, META, MODEL_PROFILE,
    OFFICE, ORGANIZATION, PROVIDER_CONN, REPOSITORY, ROLE, TEAM, TEAM_SCOPE,
};
use crate::organization::{
    ports::{OrganizationRead, RecordRef},
    records::{
        AgentExecutionConfig, Employee, HireRequest, ModelProfile, Office, OfficeCode,
        Organization, ProviderConnection, Repository, Role, Team, TeamAllocation, TeamCounter,
        WatchingModelSetting,
    },
};

macro_rules! sqlite_read {
    ($($view:ty),+) => {$(
        impl OrganizationRead for $view {
            fn organization(&self) -> Result<Option<Organization>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!("SELECT {ORGANIZATION}, {META} FROM organizations WHERE id = ?1");
                one(db, &sql, vec![text(scope)], organization_row)
            }

            fn repositories(&self) -> Result<Vec<Repository>, EngineError> {
                let (db, scope) = self.view()?;
                // Written whole and in the file's order, so the row order is the file's.
                let sql = select(REPOSITORY, "repositories", "", "rowid");
                all(db, &sql, vec![text(scope)], repository_row)
            }

            fn office(&self, id: &Id) -> Result<Option<Office>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = select(OFFICE, "offices", " AND id = ?2", "id");
                one(db, &sql, vec![text(scope), text(id.as_str())], office_row)
            }

            fn offices(&self) -> Result<Vec<Office>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = select(OFFICE, "offices", "", "id");
                all(db, &sql, vec![text(scope)], office_row)
            }

            fn office_codes(&self) -> Result<Vec<OfficeCode>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = "SELECT code, office_id, issued_at, retired_at FROM office_codes \
                           WHERE organization_id = ?1 ORDER BY rowid";
                all(db, sql, vec![text(scope)], office_code_row)
            }

            fn role(&self, id: &Id) -> Result<Option<Role>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = select(ROLE, "roles", " AND id = ?2", "id");
                one(db, &sql, vec![text(scope), text(id.as_str())], role_row)
            }

            fn roles(&self) -> Result<Vec<Role>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = select(ROLE, "roles", "", "id");
                all(db, &sql, vec![text(scope)], role_row)
            }

            fn team(&self, id: &Id) -> Result<Option<Team>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = select(TEAM, "teams", " AND id = ?2", "id");
                one(db, &sql, vec![text(scope), text(id.as_str())], team_row)
            }

            fn team_by_number(&self, number: u64) -> Result<Option<Team>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = select(TEAM, "teams", " AND number = ?2", "id");
                one(db, &sql, vec![text(scope), whole(number)?], team_row)
            }

            fn teams(&self) -> Result<Vec<Team>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = select(TEAM, "teams", "", "id");
                all(db, &sql, vec![text(scope)], team_row)
            }

            fn employee(&self, id: &Id) -> Result<Option<Employee>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = select(EMPLOYEE, "employees", " AND id = ?2", "id");
                one(db, &sql, vec![text(scope), text(id.as_str())], employee_row)
            }

            fn employees(&self) -> Result<Vec<Employee>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = select(EMPLOYEE, "employees", "", "id");
                all(db, &sql, vec![text(scope)], employee_row)
            }

            fn handle_in_use(&self, handle: &str) -> Result<bool, EngineError> {
                let (db, scope) = self.view()?;
                let sql = "SELECT 1 FROM employees WHERE organization_id = ?1 AND handle = ?2";
                let found = one(db, sql, vec![text(scope), text(handle)], |_| Ok(()))?;
                Ok(found.is_some())
            }

            fn hire_request(&self, id: &Id) -> Result<Option<HireRequest>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {HIRE_REQUEST} FROM hire_requests \
                     WHERE organization_id = ?1 AND id = ?2"
                );
                one(db, &sql, vec![text(scope), text(id.as_str())], hire_request_row)
            }

            fn hire_requests(&self) -> Result<Vec<HireRequest>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {HIRE_REQUEST} FROM hire_requests \
                     WHERE organization_id = ?1 ORDER BY id"
                );
                all(db, &sql, vec![text(scope)], hire_request_row)
            }

            fn agent_execution(
                &self,
                employee_id: &Id,
            ) -> Result<Option<AgentExecutionConfig>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = select(
                    AGENT_EXEC,
                    "agent_execution_configs",
                    " AND employee_id = ?2",
                    "employee_id",
                );
                one(
                    db,
                    &sql,
                    vec![text(scope), text(employee_id.as_str())],
                    agent_execution_row,
                )
            }

            fn agent_executions(&self) -> Result<Vec<AgentExecutionConfig>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = select(AGENT_EXEC, "agent_execution_configs", "", "employee_id");
                all(db, &sql, vec![text(scope)], agent_execution_row)
            }

            fn provider_connection(
                &self,
                id: &Id,
            ) -> Result<Option<ProviderConnection>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = select(PROVIDER_CONN, "provider_connections", " AND id = ?2", "id");
                one(
                    db,
                    &sql,
                    vec![text(scope), text(id.as_str())],
                    provider_connection_row,
                )
            }

            fn provider_connections(&self) -> Result<Vec<ProviderConnection>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = select(PROVIDER_CONN, "provider_connections", "", "id");
                all(db, &sql, vec![text(scope)], provider_connection_row)
            }

            fn model_profile(&self, id: &Id) -> Result<Option<ModelProfile>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = select(MODEL_PROFILE, "model_profiles", " AND id = ?2", "id");
                let Some(mut profile) = one(
                    db,
                    &sql,
                    vec![text(scope), text(id.as_str())],
                    model_profile_base_row,
                )?
                else {
                    return Ok(None);
                };
                profile.connection_ids = profile_connections(db, scope, &profile.id)?;
                Ok(Some(profile))
            }

            fn model_profiles(&self) -> Result<Vec<ModelProfile>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = select(MODEL_PROFILE, "model_profiles", "", "id");
                let mut profiles = all(db, &sql, vec![text(scope)], model_profile_base_row)?;
                for profile in &mut profiles {
                    profile.connection_ids = profile_connections(db, scope, &profile.id)?;
                }
                Ok(profiles)
            }

            fn watching_model(&self) -> Result<Option<WatchingModelSetting>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT model_profile_id, {META} FROM watching_model_settings \
                     WHERE organization_id = ?1"
                );
                one(db, &sql, vec![text(scope)], watching_model_row)
            }

            fn team_counter(&self) -> Result<Option<TeamCounter>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT organization_id, last_number FROM reference_counters \
                     WHERE organization_id = ?1 AND {TEAM_SCOPE}"
                );
                one(db, &sql, vec![text(scope)], team_counter_row)
            }

            fn team_allocations(&self) -> Result<Vec<TeamAllocation>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT organization_id, number, entity_id, allocated_at \
                     FROM reference_allocations \
                     WHERE organization_id = ?1 AND {TEAM_SCOPE} ORDER BY number"
                );
                all(db, &sql, vec![text(scope)], team_allocation_row)
            }

            fn record_refs(&self) -> Result<Vec<RecordRef>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = "SELECT kind, id, is_deleted FROM record_refs \
                           WHERE organization_id = ?1 ORDER BY kind, id";
                all(db, sql, vec![text(scope)], record_ref_row)
            }
        }
    )+};
}

sqlite_read!(SqliteTx, SqliteSnapshot);
