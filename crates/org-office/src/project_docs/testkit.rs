//! The documentation tree family on the memory engine (`MemoryTx` / `MemorySnapshot`). Rows are
//! keyed by `(project, path)`, as files are, so a removed file removes the row at its path.

use std::{any::Any, collections::BTreeMap};

use org_store::{
    testkit::{FamilyTable, MemorySnapshot, MemoryTx},
    EngineError, Id,
};

use super::ports::{ProjectDocsRead, ProjectDocsWrite};
use super::records::{DocEntry, DocMedia, ProjectDoc};

#[derive(Clone, Default, Debug)]
pub struct ProjectDocsTables {
    docs: BTreeMap<(Id, String), ProjectDoc>,
    media: BTreeMap<(Id, String), DocMedia>,
}

impl FamilyTable for ProjectDocsTables {
    fn name(&self) -> &'static str {
        "project_docs"
    }

    fn clone_box(&self) -> Box<dyn FamilyTable> {
        Box::new(self.clone())
    }

    fn dump(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .docs
            .values()
            .map(|row| format!("documents {} {row:?}", row.id))
            .chain(
                self.media
                    .values()
                    .map(|row| format!("documentation_media {} {row:?}", row.id)),
            )
            .collect();
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
    tables: Option<&ProjectDocsTables>,
    query: impl FnOnce(&ProjectDocsTables) -> R,
) -> Result<R, EngineError> {
    Ok(tables.map(query).unwrap_or_default())
}

macro_rules! memory_read {
    ($($view:ty),+) => {$(
        impl ProjectDocsRead for $view {
            fn project_doc_entries(&self, project: &Id) -> Result<Vec<DocEntry>, EngineError> {
                read(self.table(), |t| {
                    t.docs.values().filter(|d| d.project_id == *project).map(ProjectDoc::entry).collect()
                })
            }

            fn project_doc(&self, id: &Id) -> Result<Option<ProjectDoc>, EngineError> {
                read(self.table(), |t| t.docs.values().find(|d| d.id == *id).cloned())
            }

            fn project_doc_media(&self, project: &Id) -> Result<Vec<DocMedia>, EngineError> {
                read(self.table(), |t| {
                    t.media.values().filter(|m| m.project_id == *project).cloned().collect()
                })
            }

            fn projects_with_docs(&self) -> Result<Vec<Id>, EngineError> {
                read(self.table(), |t: &ProjectDocsTables| {
                    let mut ids: Vec<Id> = t
                        .docs
                        .keys()
                        .chain(t.media.keys())
                        .map(|(project, _)| project.clone())
                        .collect();
                    ids.sort();
                    ids.dedup();
                    ids
                })
            }

            fn document_link_ids(&self, doc_id: &Id) -> Result<Vec<String>, EngineError> {
                use org_docs::delivery::records::LinkEntityKind;
                let mut ids: Vec<String> = self
                    .table::<crate::delivery::testkit::DeliveryTables>()
                    .map(|t| {
                        t.links
                            .values()
                            .filter(|l| {
                                (l.source_kind == LinkEntityKind::Document
                                    && l.source_id == doc_id.as_str())
                                    || (l.target_kind == LinkEntityKind::Document
                                        && l.target_id == doc_id.as_str())
                            })
                            .map(|l| l.id.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                ids.sort();
                ids.dedup();
                Ok(ids)
            }
        }
    )+};
}

memory_read!(MemoryTx, MemorySnapshot);

impl ProjectDocsWrite for MemoryTx {
    fn put_project_doc(&mut self, doc: &ProjectDoc) -> Result<(), EngineError> {
        let tables = self.table_mut::<ProjectDocsTables>()?;
        tables.docs.insert(
            (doc.project_id.clone(), doc.path.as_str().to_owned()),
            doc.clone(),
        );
        Ok(())
    }

    fn remove_project_doc_at(&mut self, project: &Id, path: &str) -> Result<(), EngineError> {
        let tables = self.table_mut::<ProjectDocsTables>()?;
        tables.docs.remove(&(project.clone(), path.to_owned()));
        Ok(())
    }

    fn put_doc_media(&mut self, media: &DocMedia) -> Result<(), EngineError> {
        let tables = self.table_mut::<ProjectDocsTables>()?;
        tables.media.insert(
            (media.project_id.clone(), media.file.rel_path()),
            media.clone(),
        );
        Ok(())
    }

    fn remove_doc_media_at(&mut self, project: &Id, path: &str) -> Result<(), EngineError> {
        let tables = self.table_mut::<ProjectDocsTables>()?;
        tables.media.remove(&(project.clone(), path.to_owned()));
        Ok(())
    }
}
