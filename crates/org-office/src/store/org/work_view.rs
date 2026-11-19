//! Fallible work-domain organization reads, held on one org.db snapshot per attempt.
use std::sync::Arc;

use org_proto::org::{Area, Member, Position, Project};
use org_work::{OrgView, StoreError};
use rusqlite::{OptionalExtension, Params};
use serde::de::DeserializeOwned;

use super::last_position;
use crate::store::work::assembly::OrgSource;
use crate::store::{ReaderSnapshot, Store};

pub(crate) struct OrgDbSource(Arc<Store>);

impl OrgDbSource {
    pub(crate) fn new(org: Arc<Store>) -> Self {
        Self(org)
    }
}

impl OrgSource for OrgDbSource {
    fn snapshot(&self) -> Result<Box<dyn OrgView + '_>, StoreError> {
        Ok(Box::new(OrgDbView(
            self.0.read_snapshot().map_err(StoreError::new)?,
        )))
    }
}

struct OrgDbView<'a>(ReaderSnapshot<'a>);

impl OrgDbView<'_> {
    fn checked<T>(&self, result: Result<T, StoreError>) -> Result<T, StoreError> {
        if result.is_err() {
            self.0.failed.set(true);
        }
        result
    }

    fn one<T: DeserializeOwned>(
        &self,
        query: &str,
        params: impl Params,
    ) -> Result<Option<T>, StoreError> {
        self.checked((|| {
            let body: Option<String> = self
                .0
                .query_row(query, params, |r| r.get(0))
                .optional()
                .map_err(StoreError::new)?;
            body.map(|body| {
                serde_json::from_str(&body).map_err(|_| {
                    StoreError::new("a stored organization entity could not be decoded")
                })
            })
            .transpose()
        })())
    }
}

impl OrgView for OrgDbView<'_> {
    fn applied_position(&self) -> Result<Position, StoreError> {
        self.checked(
            last_position(&self.0)
                .map_err(StoreError::new)
                .and_then(|p| p.ok_or_else(|| StoreError::new("the organization log is empty"))),
        )
    }

    fn member(&self, id: &str) -> Result<Option<Member>, StoreError> {
        self.one("SELECT body FROM members WHERE id = ?1", [id])
    }

    fn owner_id(&self) -> Result<String, StoreError> {
        self.checked((|| {
            let mut stmt = self
                .0
                .prepare("SELECT id FROM members WHERE handle_key IS NULL ORDER BY id")
                .map_err(StoreError::new)?;
            let ids = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(StoreError::new)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(StoreError::new)?;
            match ids.as_slice() {
                [id] => Ok(id.clone()),
                _ => Err(StoreError::new(
                    "the organization must have exactly one owner",
                )),
            }
        })())
    }

    fn project(&self, id: &str) -> Result<Option<Project>, StoreError> {
        self.one("SELECT body FROM projects WHERE id = ?1", [id])
    }

    fn project_by_key(&self, key: &str) -> Result<Option<Project>, StoreError> {
        if key.is_empty() {
            return Ok(None);
        }
        self.one(
            "SELECT body FROM projects WHERE json_extract(body, '$.key') = ?1 ORDER BY id LIMIT 1",
            [key],
        )
    }

    fn area(&self, id: &str) -> Result<Option<Area>, StoreError> {
        self.one("SELECT body FROM areas WHERE id = ?1", [id])
    }

    fn general_area(&self, project_id: &str) -> Result<Option<String>, StoreError> {
        self.checked(self.0.query_row("SELECT id FROM areas WHERE project_id = ?1 AND json_extract(body, '$.general') = 1 ORDER BY id LIMIT 1", [project_id], |r| r.get(0))
            .optional().map_err(StoreError::new))
    }

    fn project_teams(&self, project_id: &str) -> Result<Vec<String>, StoreError> {
        self.checked((|| {
            let mut stmt = self
                .0
                .prepare("SELECT team_id FROM project_teams WHERE project_id = ?1 ORDER BY team_id")
                .map_err(StoreError::new)?;
            let rows = stmt
                .query_map([project_id], |r| r.get(0))
                .map_err(StoreError::new)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(StoreError::new)
        })())
    }

    fn team_lead(&self, team_id: &str) -> Result<Option<String>, StoreError> {
        self.checked(
            self.0
                .query_row(
                    "SELECT lead_member_id FROM teams WHERE id = ?1",
                    [team_id],
                    |r| r.get(0),
                )
                .optional()
                .map(|lead: Option<Option<String>>| lead.flatten())
                .map_err(StoreError::new),
        )
    }
}

#[cfg(test)]
pub(crate) mod tests;
