//! The projects family on the memory engine (`MemoryTx` / `MemorySnapshot`), plus the
//! engine-independent test suite run on both Memory and SQLite.

use std::{any::Any, collections::BTreeMap};

use org_store::{
    testkit::{FamilyTable, MemorySnapshot, MemoryTx},
    EngineError, Id,
};

use super::{
    ports::{ProjectsRead, ProjectsWrite},
    records::{Project, ProjectAllocation, ProjectCounter},
};

#[derive(Clone, Default, Debug)]
pub struct ProjectsTables {
    projects: BTreeMap<Id, Project>,
    project_counter: Option<ProjectCounter>,
    project_allocations: BTreeMap<u64, ProjectAllocation>,
}

impl FamilyTable for ProjectsTables {
    fn name(&self) -> &'static str {
        "projects"
    }

    fn clone_box(&self) -> Box<dyn FamilyTable> {
        Box::new(self.clone())
    }

    fn dump(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for (i, row) in self.projects.values().enumerate() {
            lines.push(format!("projects {i:06} {row:?}"));
        }
        if let Some(counter) = &self.project_counter {
            lines.push(format!("project_counter 000000 {counter:?}"));
        }
        for (i, row) in self.project_allocations.values().enumerate() {
            lines.push(format!("project_allocations {i:06} {row:?}"));
        }
        lines.sort();
        lines
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

fn read<R: Default>(
    tables: Option<&ProjectsTables>,
    query: impl FnOnce(&ProjectsTables) -> R,
) -> Result<R, EngineError> {
    Ok(tables.map(query).unwrap_or_default())
}

macro_rules! memory_read {
    ($($view:ty),+) => {$(
        impl ProjectsRead for $view {
            fn project(&self, id: &Id) -> Result<Option<Project>, EngineError> {
                read(self.table(), |t| t.projects.get(id).cloned())
            }

            fn project_by_number(&self, number: u64) -> Result<Option<Project>, EngineError> {
                read(self.table(), |t| {
                    t.projects.values().find(|p| p.number == number).cloned()
                })
            }

            fn project_by_slug(&self, slug: &str) -> Result<Option<Project>, EngineError> {
                read(self.table(), |t| {
                    t.projects.values().find(|p| p.slug == slug).cloned()
                })
            }

            fn projects(&self) -> Result<Vec<Project>, EngineError> {
                read(self.table(), |t| {
                    let mut list: Vec<Project> = t.projects.values().cloned().collect();
                    list.sort_by(|a, b| a.number.cmp(&b.number).then_with(|| a.id.cmp(&b.id)));
                    list
                })
            }

            fn project_counter(&self) -> Result<Option<ProjectCounter>, EngineError> {
                read(self.table(), |t| t.project_counter.clone())
            }

            fn project_allocations(&self) -> Result<Vec<ProjectAllocation>, EngineError> {
                read(self.table(), |t| {
                    t.project_allocations.values().cloned().collect()
                })
            }
        }
    )+};
}

memory_read!(MemoryTx, MemorySnapshot);

impl ProjectsWrite for MemoryTx {
    fn put_project(&mut self, record: &Project) -> Result<(), EngineError> {
        let mut normalized = record.clone();
        normalized.contributing_team_ids.sort();
        let tables: &mut ProjectsTables = self.table_mut()?;
        tables.projects.insert(normalized.id.clone(), normalized);
        let org: &mut crate::organization::testkit::OrganizationTables = self.table_mut()?;
        org.insert_record_ref("project", &record.id);
        Ok(())
    }

    fn remove_project(&mut self, id: &Id) -> Result<(), EngineError> {
        let tables: &mut ProjectsTables = self.table_mut()?;
        tables.projects.remove(id);
        let org: &mut crate::organization::testkit::OrganizationTables = self.table_mut()?;
        org.remove_record_ref("project", id);
        Ok(())
    }

    fn put_project_counter(&mut self, record: &ProjectCounter) -> Result<(), EngineError> {
        let tables: &mut ProjectsTables = self.table_mut()?;
        tables.project_counter = Some(record.clone());
        Ok(())
    }

    fn remove_project_counter(&mut self) -> Result<(), EngineError> {
        let tables: &mut ProjectsTables = self.table_mut()?;
        tables.project_counter = None;
        Ok(())
    }

    fn put_project_allocation(&mut self, record: &ProjectAllocation) -> Result<(), EngineError> {
        let tables: &mut ProjectsTables = self.table_mut()?;
        tables
            .project_allocations
            .insert(record.number, record.clone());
        Ok(())
    }

    fn remove_project_allocation(&mut self, number: u64) -> Result<(), EngineError> {
        let tables: &mut ProjectsTables = self.table_mut()?;
        tables.project_allocations.remove(&number);
        Ok(())
    }
}
