//! The engine-independent projector of the shared attachments family (`AttachmentFiles`):
//! each canonical file under `state/files/` and `state/projects/<id>/files/` becomes one row
//! of `attachments`, so deleting the projection and rebuilding it loses nothing (`doc-7` #1).

use org_proto::attachments::AttachmentTargetKind;
use org_store::{DurablePath, Engine, ProjectionError, Projector};

use super::{
    ports::{AttachmentsRead, AttachmentsWrite},
    records::{self, FileKind},
};

#[derive(Default)]
pub struct AttachmentFiles;

impl AttachmentFiles {
    pub fn new() -> Self {
        Self
    }
}

fn refuse(path: &DurablePath, reason: impl Into<String>) -> ProjectionError {
    ProjectionError::new(path, reason)
}

fn agree(path: &DurablePath, agrees: bool, what: &str) -> Result<(), ProjectionError> {
    if agrees {
        Ok(())
    } else {
        Err(refuse(path, format!("{what} does not match the path")))
    }
}

impl<E: Engine> Projector<E> for AttachmentFiles
where
    E::Tx: AttachmentsWrite,
{
    fn owns(&self, path: &DurablePath) -> bool {
        records::kind_of(path).is_some()
    }

    fn apply(
        &self,
        tx: &mut E::Tx,
        path: &DurablePath,
        bytes: Option<&[u8]>,
    ) -> Result<(), ProjectionError> {
        let Some(kind) = records::kind_of(path) else {
            return Err(refuse(path, "not an attachment file"));
        };
        let Some(bytes) = bytes else {
            tx.remove_attachment(kind.attachment())?;
            return Ok(());
        };
        let record = records::decode(bytes).map_err(|error| refuse(path, error.to_string()))?;
        agree(path, &record.id == kind.attachment(), "id")?;
        match &kind {
            FileKind::Project { project, .. } => {
                agree(
                    path,
                    record.target_kind == AttachmentTargetKind::Project,
                    "target_kind",
                )?;
                agree(
                    path,
                    record.target_id.as_ref() == Some(project),
                    "target_id",
                )?;
            }
            FileKind::Organization { .. } => {
                agree(
                    path,
                    record.target_kind == AttachmentTargetKind::Organization
                        && record.target_id.is_none(),
                    "target",
                )?;
            }
        }
        // One attachment has one file: a copy under another target (moved by hand) is refused
        // instead of silently replacing the row the first file made.
        if let Some(existing) = tx.attachment(&record.id)? {
            agree(
                path,
                existing.target_kind == record.target_kind
                    && existing.target_id == record.target_id,
                "an attachment ID already bound elsewhere",
            )?;
        }
        tx.put_attachment(&record)?;
        Ok(())
    }
}
