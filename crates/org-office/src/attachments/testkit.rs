//! The shared attachments family on the memory engine (`MemoryTx` / `MemorySnapshot`).

use std::{any::Any, collections::BTreeMap};

use org_proto::attachments::AttachmentTargetKind;
use org_store::{
    testkit::{FamilyTable, MemorySnapshot, MemoryTx},
    EngineError, Id,
};

use super::{
    ports::{AttachmentsRead, AttachmentsWrite},
    records::AttachmentRecord,
};

#[derive(Clone, Default, Debug)]
pub struct AttachmentsTables {
    attachments: BTreeMap<Id, AttachmentRecord>,
}

impl FamilyTable for AttachmentsTables {
    fn name(&self) -> &'static str {
        "attachments"
    }

    fn clone_box(&self) -> Box<dyn FamilyTable> {
        Box::new(self.clone())
    }

    fn dump(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .attachments
            .values()
            .enumerate()
            .map(|(i, row)| format!("attachments {i:06} {row:?}"))
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
    tables: Option<&AttachmentsTables>,
    query: impl FnOnce(&AttachmentsTables) -> R,
) -> Result<R, EngineError> {
    Ok(tables.map(query).unwrap_or_default())
}

macro_rules! memory_read {
    ($($view:ty),+) => {$(
        impl AttachmentsRead for $view {
            fn attachment(&self, id: &Id) -> Result<Option<AttachmentRecord>, EngineError> {
                read(self.table(), |t| t.attachments.get(id).cloned())
            }

            fn attachments_for_target(
                &self,
                kind: AttachmentTargetKind,
                target_id: Option<&Id>,
                after: Option<&Id>,
                limit: usize,
            ) -> Result<Vec<AttachmentRecord>, EngineError> {
                read(self.table(), |t| {
                    t.attachments
                        .values()
                        .filter(|r| r.target_kind == kind && r.target_id.as_ref() == target_id)
                        .filter(|r| after.is_none_or(|after| r.id > *after))
                        .take(limit)
                        .cloned()
                        .collect()
                })
            }

            fn attachments(&self) -> Result<Vec<AttachmentRecord>, EngineError> {
                read(self.table(), |t| t.attachments.values().cloned().collect())
            }
        }
    )+};
}

memory_read!(MemoryTx, MemorySnapshot);

impl AttachmentsWrite for MemoryTx {
    fn put_attachment(&mut self, record: &AttachmentRecord) -> Result<(), EngineError> {
        let tables: &mut AttachmentsTables = self.table_mut()?;
        tables.attachments.insert(record.id.clone(), record.clone());
        Ok(())
    }

    fn remove_attachment(&mut self, id: &Id) -> Result<(), EngineError> {
        let tables: &mut AttachmentsTables = self.table_mut()?;
        tables.attachments.remove(id);
        Ok(())
    }
}
