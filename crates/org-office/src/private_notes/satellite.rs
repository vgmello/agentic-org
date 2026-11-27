//! Fail-closed [`PrivateNotesStore`] for satellites (`doc-79`: notes stay at HQ only).

use std::collections::BTreeMap;

use org_proto::{ApiError, ErrorCode};

use crate::error::OfficeError;

use super::{
    BeginPublishCmd, CreateNoteCmd, DeleteNoteCmd, EditNoteCmd, NotePage, NoteQuery, NoteResult,
    PrivateNoteRecord, PrivateNotesStore, PublishAttempt,
};

type OptNote = Result<Option<PrivateNoteRecord>, OfficeError>;
type NoteMap = Result<BTreeMap<String, PrivateNoteRecord>, OfficeError>;
type PubRes = Result<Result<PublishAttempt, ApiError>, OfficeError>;
type UnitRes = Result<(), OfficeError>;

#[derive(Debug, Default, Clone, Copy)]
pub struct SatelliteRefusedPrivateNotesStore;

impl SatelliteRefusedPrivateNotesStore {
    pub fn refused() -> ApiError {
        ApiError::new(
            ErrorCode::Unavailable,
            "private notes live only on HQ and are not replicated to satellite offices",
        )
    }

    fn err<T>() -> Result<Result<T, ApiError>, OfficeError> {
        Ok(Err(Self::refused()))
    }
}

impl PrivateNotesStore for SatelliteRefusedPrivateNotesStore {
    fn available(&self) -> bool {
        false
    }
    fn create_note(&self, _: CreateNoteCmd<'_>) -> NoteResult {
        Self::err()
    }
    fn upsert_checkpoint_note(&self, _: CreateNoteCmd<'_>) -> NoteResult {
        Self::err()
    }
    fn edit_note(&self, _: EditNoteCmd<'_>) -> NoteResult {
        Self::err()
    }
    fn delete_note(&self, _: DeleteNoteCmd<'_>) -> NoteResult {
        Self::err()
    }
    fn get_note(&self, _: &str, _: &str, _: bool, _: &str) -> OptNote {
        Ok(None)
    }
    fn list_notes(&self, _: &NoteQuery<'_>) -> Result<NotePage, OfficeError> {
        Ok(NotePage {
            notes: Vec::new(),
            total: 0,
            next_cursor: None,
        })
    }
    fn latest_author_notes_for_tasks(&self, _: &str, _: &str, _: &[String]) -> NoteMap {
        Ok(BTreeMap::new())
    }
    fn begin_publish(&self, _: BeginPublishCmd<'_>) -> PubRes {
        Self::err()
    }
    fn finish_publish(&self, _: &str, _: &str, _: &str, _: Option<&str>, _: &str) -> UnitRes {
        Ok(())
    }
    fn mark_task_deleted(&self, _: &str, _: &str) -> UnitRes {
        Ok(())
    }
}
