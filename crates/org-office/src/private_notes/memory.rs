//! In-memory [`PrivateNotesStore`] adapter enforcing the exact `private-notes-schema.sql`
//! state machine, receipt idempotency, tombstones, and `ADR-0031` boundaries for tests (`ENG-2`).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use org_proto::{ApiError, ErrorCode};

use crate::error::OfficeError;

use super::{
    non_author_mutation_error, parse_receipt_error_code, BeginPublishCmd, CreateNoteCmd,
    DeleteNoteCmd, EditNoteCmd, PrivateNoteRecord, PrivateNotesStore, PublishAttempt,
};

#[derive(Debug, Clone)]
struct ReceiptRow {
    request_sha256: String,
    command: &'static str,
    note_id: String,
    result_revision: u64,
    state: ReceiptState,
    shared_comment_id: Option<String>,
    error_code: Option<String>,
    created_at: String,
    finished_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReceiptState {
    Pending,
    Completed,
    Rejected,
}

#[derive(Default)]
struct State {
    authors: BTreeSet<(String, String)>,
    tasks: BTreeMap<(String, String), bool>,
    notes: BTreeMap<(String, String), PrivateNoteRecord>,
    receipts: BTreeMap<(String, String, String), ReceiptRow>,
}

/// Engine-independent in-memory private notes store for `EngineChoice::Memory` tests.
#[derive(Default)]
pub struct MemoryPrivateNotesStore {
    state: Mutex<State>,
}

impl MemoryPrivateNotesStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>, OfficeError> {
        self.state
            .lock()
            .map_err(|_| OfficeError::Invalid("private notes store lock poisoned"))
    }
}

fn check_receipt_reuse(
    existing: &ReceiptRow,
    expected_cmd: &str,
    request_sha256: &str,
) -> Result<(), ApiError> {
    if existing.command != expected_cmd || existing.request_sha256 != request_sha256 {
        return Err(ApiError::new(
            ErrorCode::OperationReused,
            "command_id reused with different private note request",
        ));
    }
    Ok(())
}

fn has_pending_publish(state: &State, org_id: &str, note_id: &str) -> bool {
    state.receipts.iter().any(|((o, _, _), r)| {
        o == org_id && r.note_id == note_id && r.state == ReceiptState::Pending
    })
}

fn monotonic_ts(now: &str, prev: &str) -> String {
    if now >= prev {
        now.to_owned()
    } else {
        prev.to_owned()
    }
}

fn reconstruct_accepted_note(
    current: &PrivateNoteRecord,
    receipt: &ReceiptRow,
    accepted_body: Option<&str>,
) -> PrivateNoteRecord {
    let ts = receipt
        .finished_at
        .clone()
        .unwrap_or_else(|| receipt.created_at.clone());
    PrivateNoteRecord {
        organization_id: current.organization_id.clone(),
        id: current.id.clone(),
        schema_version: current.schema_version,
        revision: receipt.result_revision,
        task_id: current.task_id.clone(),
        author_id: current.author_id.clone(),
        body_markdown: accepted_body.map(str::to_owned),
        created_at: current.created_at.clone(),
        updated_at: ts.clone(),
        deleted_at: accepted_body.is_none().then_some(ts),
    }
}

fn validate_author_mutation<'a>(
    st: &'a State,
    org_id: &str,
    note_id: &str,
    caller_id: &str,
    caller_is_person: bool,
    expected_revision: u64,
    pending_msg: &str,
) -> Result<&'a PrivateNoteRecord, ApiError> {
    let nkey = (org_id.to_owned(), note_id.to_owned());
    let Some(existing) = st.notes.get(&nkey) else {
        return Err(ApiError::new(ErrorCode::NotFound, "private note not found"));
    };
    if existing.deleted_at.is_some() {
        return Err(ApiError::new(ErrorCode::NotFound, "private note not found"));
    }
    if existing.author_id != caller_id {
        return Err(non_author_mutation_error(caller_is_person));
    }
    if has_pending_publish(st, org_id, note_id) {
        return Err(ApiError::new(ErrorCode::Conflict, pending_msg));
    }
    if existing.revision != expected_revision {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            format!(
                "stale expected_revision {expected_revision}: note is at revision {}",
                existing.revision
            ),
        ));
    }
    Ok(existing)
}

impl PrivateNotesStore for MemoryPrivateNotesStore {
    fn create_note(
        &self,
        cmd: CreateNoteCmd<'_>,
    ) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError> {
        let mut st = self.lock()?;
        let rkey = (
            cmd.org_id.to_owned(),
            cmd.author_id.to_owned(),
            cmd.operation_id.to_owned(),
        );
        if let Some(existing) = st.receipts.get(&rkey) {
            if let Err(e) = check_receipt_reuse(existing, "create", cmd.request_sha256) {
                return Ok(Err(e));
            }
            let nkey = (cmd.org_id.to_owned(), existing.note_id.clone());
            let Some(current) = st.notes.get(&nkey) else {
                return Ok(Err(ApiError::new(
                    ErrorCode::StorageError,
                    "replayed note missing",
                )));
            };
            return Ok(Ok(reconstruct_accepted_note(
                current,
                existing,
                Some(cmd.body_markdown),
            )));
        }

        let nkey = (cmd.org_id.to_owned(), cmd.note_id.to_owned());
        if st.notes.contains_key(&nkey) {
            return Ok(Err(ApiError::new(
                ErrorCode::AlreadyExists,
                format!("private note `{}` already exists", cmd.note_id),
            )));
        }

        st.authors
            .insert((cmd.org_id.to_owned(), cmd.author_id.to_owned()));
        st.tasks
            .entry((cmd.org_id.to_owned(), cmd.task_id.to_owned()))
            .or_insert(false);

        let record = PrivateNoteRecord {
            organization_id: cmd.org_id.to_owned(),
            id: cmd.note_id.to_owned(),
            schema_version: 1,
            revision: 1,
            task_id: cmd.task_id.to_owned(),
            author_id: cmd.author_id.to_owned(),
            body_markdown: Some(cmd.body_markdown.to_owned()),
            created_at: cmd.now.to_owned(),
            updated_at: cmd.now.to_owned(),
            deleted_at: None,
        };
        st.notes.insert(nkey, record.clone());
        st.receipts.insert(
            rkey,
            ReceiptRow {
                request_sha256: cmd.request_sha256.to_owned(),
                command: "create",
                note_id: cmd.note_id.to_owned(),
                result_revision: 1,
                state: ReceiptState::Completed,
                shared_comment_id: None,
                error_code: None,
                created_at: cmd.now.to_owned(),
                finished_at: Some(cmd.now.to_owned()),
            },
        );
        Ok(Ok(record))
    }

    fn edit_note(
        &self,
        cmd: EditNoteCmd<'_>,
    ) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError> {
        let mut st = self.lock()?;
        let rkey = (
            cmd.org_id.to_owned(),
            cmd.caller_id.to_owned(),
            cmd.operation_id.to_owned(),
        );
        if let Some(existing) = st.receipts.get(&rkey) {
            if let Err(e) = check_receipt_reuse(existing, "edit", cmd.request_sha256) {
                return Ok(Err(e));
            }
            let nkey = (cmd.org_id.to_owned(), existing.note_id.clone());
            let Some(current) = st.notes.get(&nkey) else {
                return Ok(Err(ApiError::new(
                    ErrorCode::StorageError,
                    "replayed note missing",
                )));
            };
            return Ok(Ok(reconstruct_accepted_note(
                current,
                existing,
                Some(cmd.body_markdown),
            )));
        }

        let existing_note = match validate_author_mutation(
            &st,
            cmd.org_id,
            cmd.note_id,
            cmd.caller_id,
            cmd.caller_is_person,
            cmd.expected_revision,
            "resolve pending publication before editing private note",
        ) {
            Ok(n) => n.clone(),
            Err(e) => return Ok(Err(e)),
        };
        if existing_note.revision >= i64::MAX as u64 {
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                "private note revision limit reached",
            )));
        }

        let updated_at = monotonic_ts(cmd.now, &existing_note.updated_at);
        let next_rev = existing_note.revision + 1;
        let updated = PrivateNoteRecord {
            revision: next_rev,
            body_markdown: Some(cmd.body_markdown.to_owned()),
            updated_at: updated_at.clone(),
            ..existing_note
        };
        st.notes.insert(
            (cmd.org_id.to_owned(), cmd.note_id.to_owned()),
            updated.clone(),
        );
        st.receipts.insert(
            rkey,
            ReceiptRow {
                request_sha256: cmd.request_sha256.to_owned(),
                command: "edit",
                note_id: cmd.note_id.to_owned(),
                result_revision: next_rev,
                state: ReceiptState::Completed,
                shared_comment_id: None,
                error_code: None,
                created_at: updated_at.clone(),
                finished_at: Some(updated_at),
            },
        );
        Ok(Ok(updated))
    }

    fn delete_note(
        &self,
        cmd: DeleteNoteCmd<'_>,
    ) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError> {
        let mut st = self.lock()?;
        let rkey = (
            cmd.org_id.to_owned(),
            cmd.caller_id.to_owned(),
            cmd.operation_id.to_owned(),
        );
        if let Some(existing) = st.receipts.get(&rkey) {
            if let Err(e) = check_receipt_reuse(existing, "delete", cmd.request_sha256) {
                return Ok(Err(e));
            }
            let nkey = (cmd.org_id.to_owned(), existing.note_id.clone());
            let Some(current) = st.notes.get(&nkey) else {
                return Ok(Err(ApiError::new(
                    ErrorCode::StorageError,
                    "replayed note missing",
                )));
            };
            return Ok(Ok(reconstruct_accepted_note(current, existing, None)));
        }

        let existing_note = match validate_author_mutation(
            &st,
            cmd.org_id,
            cmd.note_id,
            cmd.caller_id,
            cmd.caller_is_person,
            cmd.expected_revision,
            "resolve pending publication before deleting private note",
        ) {
            Ok(n) => n.clone(),
            Err(e) => return Ok(Err(e)),
        };
        if existing_note.revision >= i64::MAX as u64 {
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                "private note revision limit reached",
            )));
        }

        let deleted_at = monotonic_ts(cmd.now, &existing_note.updated_at);
        let next_rev = existing_note.revision + 1;
        let tombstone = PrivateNoteRecord {
            revision: next_rev,
            body_markdown: None,
            updated_at: deleted_at.clone(),
            deleted_at: Some(deleted_at.clone()),
            ..existing_note
        };
        st.notes.insert(
            (cmd.org_id.to_owned(), cmd.note_id.to_owned()),
            tombstone.clone(),
        );
        st.receipts.insert(
            rkey,
            ReceiptRow {
                request_sha256: cmd.request_sha256.to_owned(),
                command: "delete",
                note_id: cmd.note_id.to_owned(),
                result_revision: next_rev,
                state: ReceiptState::Completed,
                shared_comment_id: None,
                error_code: None,
                created_at: deleted_at.clone(),
                finished_at: Some(deleted_at),
            },
        );
        Ok(Ok(tombstone))
    }

    fn get_note(
        &self,
        org_id: &str,
        caller_id: &str,
        caller_is_person: bool,
        note_id: &str,
    ) -> Result<Option<PrivateNoteRecord>, OfficeError> {
        let st = self.lock()?;
        let Some(note) = st.notes.get(&(org_id.to_owned(), note_id.to_owned())) else {
            return Ok(None);
        };
        if note.deleted_at.is_some() {
            return Ok(None);
        }
        if !caller_is_person && note.author_id != caller_id {
            return Ok(None);
        }
        Ok(Some(note.clone()))
    }

    fn list_notes(
        &self,
        org_id: &str,
        caller_id: &str,
        caller_is_person: bool,
        task_id: &str,
        author_filter: Option<&str>,
    ) -> Result<Vec<PrivateNoteRecord>, OfficeError> {
        let st = self.lock()?;
        if !caller_is_person {
            if let Some(filter) = author_filter {
                if filter != caller_id {
                    return Ok(Vec::new());
                }
            }
        }
        let mut out: Vec<PrivateNoteRecord> = st
            .notes
            .values()
            .filter(|n| {
                n.organization_id == org_id
                    && n.task_id == task_id
                    && n.deleted_at.is_none()
                    && (caller_is_person || n.author_id == caller_id)
                    && author_filter.is_none_or(|a| n.author_id == a)
            })
            .cloned()
            .collect();
        out.sort_by(|a, b| {
            a.updated_at
                .cmp(&b.updated_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(out)
    }

    fn begin_publish(
        &self,
        cmd: BeginPublishCmd<'_>,
    ) -> Result<Result<PublishAttempt, ApiError>, OfficeError> {
        let mut st = self.lock()?;
        let rkey = (
            cmd.org_id.to_owned(),
            cmd.caller_id.to_owned(),
            cmd.operation_id.to_owned(),
        );
        if let Some(existing) = st.receipts.get(&rkey).cloned() {
            if let Err(e) = check_receipt_reuse(&existing, "publish", cmd.request_sha256) {
                return Ok(Err(e));
            }
            let shared_comment_id = existing.shared_comment_id.unwrap_or_default();
            let note = st
                .notes
                .get(&(cmd.org_id.to_owned(), existing.note_id))
                .cloned()
                .ok_or_else(|| OfficeError::Invalid("replayed publish note missing"))?;
            return match existing.state {
                ReceiptState::Completed => Ok(Ok(PublishAttempt::AlreadyCompleted {
                    note,
                    result_revision: existing.result_revision,
                    shared_comment_id,
                })),
                ReceiptState::Pending => Ok(Ok(PublishAttempt::Pending {
                    note,
                    shared_comment_id,
                })),
                ReceiptState::Rejected => Ok(Err(ApiError::new(
                    parse_receipt_error_code(existing.error_code.as_deref()),
                    "private note publish was rejected",
                ))),
            };
        }

        let note = match validate_author_mutation(
            &st,
            cmd.org_id,
            cmd.note_id,
            cmd.caller_id,
            cmd.caller_is_person,
            cmd.expected_revision,
            "another publication of this private note is already pending",
        ) {
            Ok(n) => n.clone(),
            Err(e) => return Ok(Err(e)),
        };
        let comment_used = st.receipts.iter().any(|((o, _, _), r)| {
            o == cmd.org_id && r.shared_comment_id.as_deref() == Some(cmd.comment_id)
        });
        if comment_used {
            return Ok(Err(ApiError::new(
                ErrorCode::AlreadyExists,
                format!("comment_id `{}` is already reserved", cmd.comment_id),
            )));
        }

        st.receipts.insert(
            rkey,
            ReceiptRow {
                request_sha256: cmd.request_sha256.to_owned(),
                command: "publish",
                note_id: cmd.note_id.to_owned(),
                result_revision: note.revision,
                state: ReceiptState::Pending,
                shared_comment_id: Some(cmd.comment_id.to_owned()),
                error_code: None,
                created_at: cmd.now.to_owned(),
                finished_at: None,
            },
        );
        Ok(Ok(PublishAttempt::Pending {
            note,
            shared_comment_id: cmd.comment_id.to_owned(),
        }))
    }

    fn finish_publish(
        &self,
        org_id: &str,
        author_id: &str,
        operation_id: &str,
        error_code: Option<&str>,
        now: &str,
    ) -> Result<(), OfficeError> {
        let mut st = self.lock()?;
        let rkey = (
            org_id.to_owned(),
            author_id.to_owned(),
            operation_id.to_owned(),
        );
        if let Some(row) = st.receipts.get_mut(&rkey) {
            if row.state == ReceiptState::Pending {
                row.state = if error_code.is_some() {
                    ReceiptState::Rejected
                } else {
                    ReceiptState::Completed
                };
                row.error_code = error_code.map(str::to_owned);
                row.finished_at = Some(monotonic_ts(now, &row.created_at));
            }
        }
        Ok(())
    }

    fn mark_task_deleted(&self, org_id: &str, task_id: &str) -> Result<(), OfficeError> {
        let mut st = self.lock()?;
        st.tasks
            .insert((org_id.to_owned(), task_id.to_owned()), true);
        Ok(())
    }
}
