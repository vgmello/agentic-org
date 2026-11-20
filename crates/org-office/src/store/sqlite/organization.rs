//! The organization family's two ports in SQL, on the data contract's tables
//! (`organizations`, `repositories`, `offices`, `office_codes`, `roles`, `teams`, `employees`,
//! `reference_counters`, `reference_allocations`, `record_refs`).
//!
//! No rule is here: the decisions and the projector's `validate` hold them. The tables'
//! constraints are the backstop, and one that fires is an engine failure, never a refusal.
//! Every row is scoped by the organization the engine was opened for, because a team's,
//! an office's and an employee's file do not name it.

use org_store::{EngineError, Id};
use rusqlite::{
    params_from_iter,
    types::{Type, Value},
    Connection, Row,
};

use super::{failed, SqliteSnapshot, SqliteTx};
use crate::organization::{
    ports::{OrganizationRead, OrganizationWrite, RecordRef},
    records::{
        BuiltinKey, By, Employee, EmployeeKind, EmployeeStatus, Meta, Office, OfficeCode,
        OfficeRole, Organization, Repository, RepositoryKind, Role, Team, TeamAllocation,
        TeamCounter, TeamStatus,
    },
};

/// The columns of document 08's fields, as every attributed table has them.
const META: &str = "revision, created_at, created_by_kind, created_by_id, updated_at, \
                    updated_by_kind, updated_by_id";

/// The one counter of this family: team numbers, scoped by the organization.
const TEAM_SCOPE: &str = "scope_kind = 'organization' AND scope_id = ?1 AND entity_kind = 'team'";

/// A transaction's or a snapshot's connection, and the organization it is scoped to.
trait View {
    fn view(&self) -> Result<(&Connection, &str), EngineError>;
}

fn unscoped() -> EngineError {
    EngineError::new("the projection was opened for no organization")
}

impl View for SqliteTx {
    fn view(&self) -> Result<(&Connection, &str), EngineError> {
        let organization = self.organization.as_ref().ok_or_else(unscoped)?;
        Ok((self.db(), organization.as_str()))
    }
}

impl View for SqliteSnapshot {
    fn view(&self) -> Result<(&Connection, &str), EngineError> {
        let organization = self.organization.as_ref().ok_or_else(unscoped)?;
        Ok((self.db(), organization.as_str()))
    }
}

// ---- reading ----

/// A stored value this code did not write.
fn foreign(column: usize, what: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, Type::Text, what.into())
}

fn id(row: &Row<'_>, column: usize) -> rusqlite::Result<Id> {
    let text: String = row.get(column)?;
    Id::parse(&text).map_err(|_| foreign(column, "not an ID"))
}

fn optional_id(row: &Row<'_>, column: usize) -> rusqlite::Result<Option<Id>> {
    let text: Option<String> = row.get(column)?;
    let parsed = text.map(|text| Id::parse(&text).map_err(|_| foreign(column, "not an ID")));
    parsed.transpose()
}

fn number(row: &Row<'_>, column: usize) -> rusqlite::Result<u64> {
    let stored: i64 = row.get(column)?;
    u64::try_from(stored).map_err(|_| foreign(column, "a negative number"))
}

/// The variant whose text is stored.
fn one_of<T: Copy>(
    row: &Row<'_>,
    column: usize,
    all: &[T],
    text: fn(T) -> &'static str,
) -> rusqlite::Result<T> {
    let stored: String = row.get(column)?;
    let found = all.iter().find(|variant| text(**variant) == stored);
    found
        .copied()
        .ok_or_else(|| foreign(column, "not a known value"))
}

fn by(row: &Row<'_>, column: usize) -> rusqlite::Result<By> {
    let kind: String = row.get(column)?;
    match (kind.as_str(), optional_id(row, column + 1)?) {
        ("employee", Some(id)) => Ok(By::Employee(id)),
        ("system", None) => Ok(By::System),
        ("unknown", None) => Ok(By::Unknown),
        _ => Err(foreign(column, "not an attribution")),
    }
}

/// The [`META`] columns, the first of them at `column`.
fn meta(row: &Row<'_>, column: usize) -> rusqlite::Result<Meta> {
    Ok(Meta {
        revision: number(row, column)?,
        created_at: row.get(column + 1)?,
        created_by: by(row, column + 2)?,
        updated_at: row.get(column + 4)?,
        updated_by: by(row, column + 5)?,
    })
}

fn all<T>(
    db: &Connection,
    sql: &str,
    values: Vec<Value>,
    read: fn(&Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>, EngineError> {
    let query = || -> rusqlite::Result<Vec<T>> {
        let mut query = db.prepare_cached(sql)?;
        let rows = query.query_map(params_from_iter(values), read)?;
        let rows: rusqlite::Result<Vec<T>> = rows.collect();
        rows
    };
    query().map_err(|error| failed(db, "reading organization rows", &error))
}

fn one<T>(
    db: &Connection,
    sql: &str,
    values: Vec<Value>,
    read: fn(&Row<'_>) -> rusqlite::Result<T>,
) -> Result<Option<T>, EngineError> {
    Ok(all(db, sql, values, read)?.pop())
}

fn text(value: &str) -> Value {
    Value::Text(value.to_owned())
}

fn optional(value: Option<&str>) -> Value {
    value.map_or(Value::Null, text)
}

fn whole(value: u64) -> Result<Value, EngineError> {
    let stored = i64::try_from(value);
    stored
        .map(Value::Integer)
        .map_err(|_| EngineError::new("a number is too large to store"))
}

fn organization_row(row: &Row<'_>) -> rusqlite::Result<Organization> {
    Ok(Organization {
        id: id(row, 0)?,
        slug: row.get(1)?,
        name: row.get(2)?,
        organization_repository_id: id(row, 3)?,
        state_branch: row.get(4)?,
        meta: meta(row, 5)?,
    })
}

fn repository_row(row: &Row<'_>) -> rusqlite::Result<Repository> {
    const KINDS: [RepositoryKind; 2] = [RepositoryKind::Remote, RepositoryKind::Local];
    Ok(Repository {
        id: id(row, 0)?,
        key: row.get(1)?,
        kind: one_of(row, 2, &KINDS, RepositoryKind::as_str)?,
        remote_url: row.get(3)?,
        meta: meta(row, 4)?,
    })
}

fn office_row(row: &Row<'_>) -> rusqlite::Result<Office> {
    const ROLES: [OfficeRole; 2] = [OfficeRole::Hq, OfficeRole::Satellite];
    Ok(Office {
        id: id(row, 0)?,
        slug: row.get(1)?,
        name: row.get(2)?,
        role: one_of(row, 3, &ROLES, OfficeRole::as_str)?,
        meta: meta(row, 4)?,
    })
}

fn office_code_row(row: &Row<'_>) -> rusqlite::Result<OfficeCode> {
    Ok(OfficeCode {
        code: row.get(0)?,
        office_id: id(row, 1)?,
        issued_at: row.get(2)?,
        retired_at: row.get(3)?,
    })
}

fn team_row(row: &Row<'_>) -> rusqlite::Result<Team> {
    const STATUSES: [TeamStatus; 2] = [TeamStatus::Active, TeamStatus::Archived];
    Ok(Team {
        id: id(row, 0)?,
        name: row.get(1)?,
        number: number(row, 2)?,
        parent_team_id: optional_id(row, 3)?,
        lead_employee_id: optional_id(row, 4)?,
        status: one_of(row, 5, &STATUSES, TeamStatus::as_str)?,
        max_members: number(row, 6)?,
        meta: meta(row, 7)?,
    })
}

fn employee_row(row: &Row<'_>) -> rusqlite::Result<Employee> {
    const KINDS: [EmployeeKind; 2] = [EmployeeKind::Person, EmployeeKind::Agent];
    const STATUSES: [EmployeeStatus; 3] = [
        EmployeeStatus::Active,
        EmployeeStatus::Paused,
        EmployeeStatus::Inactive,
    ];
    Ok(Employee {
        id: id(row, 0)?,
        handle: row.get(1)?,
        kind: one_of(row, 2, &KINDS, EmployeeKind::as_str)?,
        name: row.get(3)?,
        home_team_id: id(row, 4)?,
        role_id: optional_id(row, 5)?,
        reports_to_employee_id: optional_id(row, 6)?,
        status: one_of(row, 7, &STATUSES, EmployeeStatus::as_str)?,
        contact: row.get(8)?,
        title: row.get(9)?,
        focus: row.get(10)?,
        role_text: row.get(11)?,
        title_in_team: row.get(12)?,
        offboarded_at: row.get(13)?,
        may_hire: row.get(14)?,
        builtin_key: optional_builtin_key(row, 15)?,
        body: row.get(16)?,
        meta: meta(row, 17)?,
    })
}

fn optional_builtin_key(row: &Row<'_>, column: usize) -> rusqlite::Result<Option<BuiltinKey>> {
    let stored: Option<String> = row.get(column)?;
    match stored.as_deref() {
        None => Ok(None),
        Some(text) if text == BuiltinKey::Hr.as_str() => Ok(Some(BuiltinKey::Hr)),
        Some(_) => Err(foreign(column, "not a known value")),
    }
}

fn role_row(row: &Row<'_>) -> rusqlite::Result<Role> {
    Ok(Role {
        id: id(row, 0)?,
        key: row.get(1)?,
        name: row.get(2)?,
        body: row.get(3)?,
        meta: meta(row, 4)?,
    })
}

fn team_counter_row(row: &Row<'_>) -> rusqlite::Result<TeamCounter> {
    Ok(TeamCounter {
        organization_id: id(row, 0)?,
        last_number: number(row, 1)?,
    })
}

fn team_allocation_row(row: &Row<'_>) -> rusqlite::Result<TeamAllocation> {
    Ok(TeamAllocation {
        organization_id: id(row, 0)?,
        number: number(row, 1)?,
        team_id: id(row, 2)?,
        allocated_at: row.get(3)?,
    })
}

fn record_ref_row(row: &Row<'_>) -> rusqlite::Result<RecordRef> {
    Ok(RecordRef {
        kind: row.get(0)?,
        id: id(row, 1)?,
        is_deleted: row.get(2)?,
    })
}

const ORGANIZATION: &str = "id, slug, name, organization_repository_id, state_branch";
const REPOSITORY: &str = "id, key, kind, remote_url";
const OFFICE: &str = "id, slug, name, role";
const ROLE: &str = "id, key, name, body_markdown";
const TEAM: &str = "id, name, number, parent_team_id, lead_employee_id, status, max_members";
const EMPLOYEE: &str = "id, handle, kind, name, home_team_id, role_id, reports_to_employee_id, \
                        status, contact, title, focus, role_text, title_in_team, offboarded_at, \
                        may_hire, builtin_key, body_markdown";

/// Every row of `table` in the organization, or those `filter` (on `?2`) keeps.
fn select(columns: &str, table: &str, filter: &str, order: &str) -> String {
    format!(
        "SELECT {columns}, {META} FROM {table} WHERE organization_id = ?1{filter} ORDER BY {order}"
    )
}

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

// ---- writing ----

fn run(db: &Connection, sql: &str, values: Vec<Value>) -> Result<(), EngineError> {
    db.prepare_cached(sql)
        .and_then(|mut write| write.execute(params_from_iter(values)))
        .map_err(|error| failed(db, "writing an organization row", &error))?;
    Ok(())
}

/// Inserts a row, or replaces every other column of the row with the same key: the first
/// `key` columns. A row is never deleted to be replaced, so what refers to it stays
/// resolved.
fn put(
    db: &Connection,
    table: &str,
    columns: &str,
    key: usize,
    values: Vec<Value>,
) -> Result<(), EngineError> {
    let names: Vec<&str> = columns.split(',').map(str::trim).collect();
    debug_assert_eq!(names.len(), values.len(), "{table}");
    let places: Vec<String> = (1..=names.len()).map(|n| format!("?{n}")).collect();
    let rest: Vec<String> = names[key..]
        .iter()
        .map(|name| format!("{name} = excluded.{name}"))
        .collect();
    let sql = format!(
        "INSERT INTO {table} ({}) VALUES ({}) ON CONFLICT ({}) DO UPDATE SET {}",
        names.join(", "),
        places.join(", "),
        names[..key].join(", "),
        rest.join(", "),
    );
    run(db, &sql, values)
}

fn by_values(by: &By, values: &mut Vec<Value>) {
    let (kind, id) = match by {
        By::Employee(id) => ("employee", Some(id.as_str())),
        By::System => ("system", None),
        By::Unknown => ("unknown", None),
    };
    values.push(text(kind));
    values.push(optional(id));
}

/// The values of `schema_version` and the [`META`] columns.
fn meta_values(meta: &Meta, values: &mut Vec<Value>) -> Result<(), EngineError> {
    values.push(Value::Integer(1));
    values.push(whole(meta.revision)?);
    values.push(optional(meta.created_at.as_deref()));
    by_values(&meta.created_by, values);
    values.push(optional(meta.updated_at.as_deref()));
    by_values(&meta.updated_by, values);
    Ok(())
}

fn optional_id_value(id: Option<&Id>) -> Value {
    optional(id.map(Id::as_str))
}

/// The row other families refer to a record of this one by.
fn put_record_ref(db: &Connection, scope: &str, kind: &str, id: &Id) -> Result<(), EngineError> {
    let values = vec![
        text(scope),
        text(kind),
        text(id.as_str()),
        Value::Integer(0),
    ];
    put(
        db,
        "record_refs",
        "organization_id, kind, id, is_deleted",
        3,
        values,
    )
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
        let clear = "DELETE FROM office_codes WHERE organization_id = ?1";
        run(db, clear, vec![text(scope)])?;
        let sql = "INSERT INTO office_codes (organization_id, code, office_id, issued_at, \
                   retired_at) VALUES (?1, ?2, ?3, ?4, ?5)";
        for record in records {
            let values = vec![
                text(scope),
                text(&record.code),
                text(record.office_id.as_str()),
                text(&record.issued_at),
                optional(record.retired_at.as_deref()),
            ];
            run(db, sql, values)?;
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

#[cfg(test)]
mod tests {
    use crate::store::sqlite::SqliteEngine;

    #[test]
    fn a_projection_opened_for_no_organization_refuses_the_family() {
        use crate::organization::ports::OrganizationRead;
        use org_store::Engine;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("office.db");
        let engine = SqliteEngine::open(&path).unwrap();
        assert!(engine.snapshot().unwrap().teams().is_err());
        assert!(engine.begin().unwrap().organization().is_err());
    }
}
