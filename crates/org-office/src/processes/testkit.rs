//! In-memory engine adapter for [`ProcessesRead`] and [`ProcessesWrite`].

use std::{any::Any, collections::BTreeMap};

use org_store::{
    testkit::{FamilyTable, MemorySnapshot, MemoryTx},
    EngineError, Id,
};

use super::ports::{ProcessesRead, ProcessesWrite};
use super::records::StructureDocument;

#[derive(Clone, Default, Debug)]
pub struct ProcessesTables {
    docs: BTreeMap<Id, StructureDocument>,
}

impl FamilyTable for ProcessesTables {
    fn name(&self) -> &'static str {
        "processes"
    }

    fn clone_box(&self) -> Box<dyn FamilyTable> {
        Box::new(self.clone())
    }

    fn dump(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .docs
            .values()
            .enumerate()
            .map(|(i, row)| format!("structure_documents {i:06} {row:?}"))
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
    tables: Option<&ProcessesTables>,
    query: impl FnOnce(&ProcessesTables) -> R,
) -> Result<R, EngineError> {
    Ok(tables.map(query).unwrap_or_default())
}

macro_rules! memory_read {
    ($($view:ty),+) => {$(
        impl ProcessesRead for $view {
            fn structure_document(&self, id: &Id) -> Result<Option<StructureDocument>, EngineError> {
                read(self.table(), |t| t.docs.get(id).cloned())
            }

            fn structure_documents(&self) -> Result<Vec<StructureDocument>, EngineError> {
                read(self.table(), |t| t.docs.values().cloned().collect())
            }
        }
    )+};
}

memory_read!(MemoryTx, MemorySnapshot);

impl ProcessesWrite for MemoryTx {
    fn put_structure_document(&mut self, record: &StructureDocument) -> Result<(), EngineError> {
        self.table_mut::<ProcessesTables>()?
            .docs
            .insert(record.id.clone(), record.clone());
        Ok(())
    }

    fn remove_structure_document(&mut self, id: &Id) -> Result<(), EngineError> {
        self.table_mut::<ProcessesTables>()?.docs.remove(id);
        Ok(())
    }
}
