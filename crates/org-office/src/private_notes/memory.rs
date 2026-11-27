//! In-memory [`PrivateNotesStore`] adapter enforcing the exact `private-notes-schema.sql`
//! state machine, tags, receipt idempotency, tombstones, and `ADR-0031` boundaries for tests
//! (`ENG-2`).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, MutexGuard};

use org_proto::{ApiError, ErrorCode};

use crate::error::OfficeError;

use super::query::newest_first;
use super::{
    check_author_mutation, check_receipt_reuse, monotonic_ts, next_revision,
    parse_receipt_error_code, replayed_note, require_publishable, Accepted, BeginPublishCmd,
    CreateNoteCmd, DeleteNoteCmd, EditNoteCmd, NotePage, NoteQuery, PrivateNoteRecord,
    PrivateNotesStore, PublishAttempt,
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

impl ReceiptRow {
    fn completed(cmd: (&'static str, &str), note_id: &str, result_revision: u64, at: &str) -> Self {
        Self {
            request_sha256: cmd.1.to_owned(),
            command: cmd.0,
            note_id: note_id.to_owned(),
            result_revision,
            state: ReceiptState::Completed,
            shared_comment_id: None,
            error_code: None,
            created_at: at.to_owned(),
            finished_at: Some(at.to_owned()),
        }
    }

    fn time(&self) -> &str {
        self.finished_at.as_deref().unwrap_or(&self.created_at)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReceiptState {
    Pending,
    Completed,
    Rejected,
}

type ReceiptKey = (String, String, String);

#[derive(Default)]
struct State {
    authors: BTreeSet<(String, String)>,
    tasks: BTreeMap<(String, String), bool>,
    notes: BTreeMap<(String, String), PrivateNoteRecord>,
    receipts: BTreeMap<ReceiptKey, ReceiptRow>,
}

impl State {
    fn note(&self, org_id: &str, note_id: &str) -> Option<&PrivateNoteRecord> {
        self.notes.get(&(org_id.to_owned(), note_id.to_owned()))
    }

    fn put(&mut self, note: PrivateNoteRecord) {
        self.notes
            .insert((note.organization_id.clone(), note.id.clone()), note);
    }

    fn has_pending_publish(&self, org_id: &str, note_id: &str) -> bool {
        self.receipts.iter().any(|((o, _, _), r)| {
            o == org_id && r.note_id == note_id && r.state == ReceiptState::Pending
        })
    }

    /// The note an exact replay of `existing` answers, or the refusal of a reused command.
    fn replay(
        &self,
        org_id: &str,
        existing: &ReceiptRow,
        cmd: (&str, &str),
        accepted: Accepted<'_>,
    ) -> Result<PrivateNoteRecord, ApiError> {
        check_receipt_reuse(existing.command, &existing.request_sha256, cmd.0, cmd.1)?;
        let current = self
            .note(org_id, &existing.note_id)
            .ok_or_else(|| ApiError::new(ErrorCode::StorageError, "replayed note missing"))?;
        Ok(replayed_note(
            current,
            existing.result_revision,
            existing.time(),
            accepted,
        ))
    }

    /// Validates an author mutation against the stored note.
    fn author_mutation(
        &self,
        org_id: &str,
        note_id: &str,
        caller: (&str, bool),
        expected_revision: u64,
        pending_msg: &str,
    ) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError> {
        check_author_mutation(
            self.note(org_id, note_id).cloned(),
            caller.0,
            caller.1,
            expected_revision,
            || Ok(self.has_pending_publish(org_id, note_id)),
            pending_msg,
        )
    }
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

    /// Every active note of `org_id`, every author's: what the memory search index reads,
    /// which applies `ADR-0031` itself before it scores anything.
    pub fn active_notes(&self, org_id: &str) -> Result<Vec<PrivateNoteRecord>, OfficeError> {
        Ok(self
            .lock()?
            .notes
            .values()
            .filter(|n| n.organization_id == org_id && n.deleted_at.is_none())
            .cloned()
            .collect())
    }

    fn lock(&self) -> Result<MutexGuard<'_, State>, OfficeError> {
        self.state
            .lock()
            .map_err(|_| OfficeError::Invalid("private notes store lock poisoned"))
    }
}

fn receipt_key(org_id: &str, author_id: &str, operation_id: &str) -> ReceiptKey {
    (
        org_id.to_owned(),
        author_id.to_owned(),
        operation_id.to_owned(),
    )
}

impl PrivateNotesStore for MemoryPrivateNotesStore {
    fn create_note(
        &self,
        cmd: CreateNoteCmd<'_>,
    ) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError> {
        let mut st = self.lock()?;
        let rkey = receipt_key(cmd.org_id, cmd.author_id, cmd.operation_id);
        if let Some(existing) = st.receipts.get(&rkey) {
            let request = ("create", cmd.request_sha256);
            return Ok(st.replay(
                cmd.org_id,
                existing,
                request,
                Accepted::Created(cmd.content),
            ));
        }
        if st.note(cmd.org_id, cmd.note_id).is_some() {
            return Ok(Err(ApiError::new(
                ErrorCode::AlreadyExists,
                format!("private note `{}` already exists", cmd.note_id),
            )));
        }
        st.authors
            .insert((cmd.org_id.to_owned(), cmd.author_id.to_owned()));
        if let Some(task_id) = cmd.task_id {
            st.tasks
                .entry((cmd.org_id.to_owned(), task_id.to_owned()))
                .or_insert(false);
        }
        let record = PrivateNoteRecord::new_active(&cmd);
        st.put(record.clone());
        let receipt =
            ReceiptRow::completed(("create", cmd.request_sha256), cmd.note_id, 1, cmd.now);
        st.receipts.insert(rkey, receipt);
        Ok(Ok(record))
    }

    fn edit_note(
        &self,
        cmd: EditNoteCmd<'_>,
    ) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError> {
        let mut st = self.lock()?;
        let rkey = receipt_key(cmd.org_id, cmd.caller_id, cmd.operation_id);
        if let Some(existing) = st.receipts.get(&rkey) {
            let request = ("edit", cmd.request_sha256);
            return Ok(st.replay(cmd.org_id, existing, request, Accepted::Edited(cmd.patch)));
        }
        let existing = match st.author_mutation(
            cmd.org_id,
            cmd.note_id,
            (cmd.caller_id, cmd.caller_is_person),
            cmd.expected_revision,
            "resolve pending publication before editing private note",
        )? {
            Ok(note) => note,
            Err(e) => return Ok(Err(e)),
        };
        let next_rev = match next_revision(existing.revision) {
            Ok(rev) => rev,
            Err(e) => return Ok(Err(e)),
        };
        let Some(content) = cmd.patch.apply(&existing) else {
            return Err(OfficeError::Invalid("active private note without a body"));
        };
        let updated_at = monotonic_ts(cmd.now, &existing.updated_at);
        let updated = existing.with_content(&content, next_rev, &updated_at);
        st.put(updated.clone());
        let receipt = ReceiptRow::completed(
            ("edit", cmd.request_sha256),
            cmd.note_id,
            next_rev,
            &updated_at,
        );
        st.receipts.insert(rkey, receipt);
        Ok(Ok(updated))
    }

    fn delete_note(
        &self,
        cmd: DeleteNoteCmd<'_>,
    ) -> Result<Result<PrivateNoteRecord, ApiError>, OfficeError> {
        let mut st = self.lock()?;
        let rkey = receipt_key(cmd.org_id, cmd.caller_id, cmd.operation_id);
        if let Some(existing) = st.receipts.get(&rkey) {
            let request = ("delete", cmd.request_sha256);
            return Ok(st.replay(cmd.org_id, existing, request, Accepted::Deleted));
        }
        let existing = match st.author_mutation(
            cmd.org_id,
            cmd.note_id,
            (cmd.caller_id, cmd.caller_is_person),
            cmd.expected_revision,
            "resolve pending publication before deleting private note",
        )? {
            Ok(note) => note,
            Err(e) => return Ok(Err(e)),
        };
        let next_rev = match next_revision(existing.revision) {
            Ok(rev) => rev,
            Err(e) => return Ok(Err(e)),
        };
        let deleted_at = monotonic_ts(cmd.now, &existing.updated_at);
        let tombstone = existing.tombstone(next_rev, &deleted_at);
        st.put(tombstone.clone());
        let receipt = ReceiptRow::completed(
            ("delete", cmd.request_sha256),
            cmd.note_id,
            next_rev,
            &deleted_at,
        );
        st.receipts.insert(rkey, receipt);
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
        Ok(st
            .note(org_id, note_id)
            .filter(|n| n.deleted_at.is_none())
            .filter(|n| caller_is_person || n.author_id == caller_id)
            .cloned())
    }

    fn list_notes(&self, query: &NoteQuery<'_>) -> Result<NotePage, OfficeError> {
        let st = self.lock()?;
        let mut matched: Vec<&PrivateNoteRecord> =
            st.notes.values().filter(|n| query.matches(n)).collect();
        matched.sort_by(|a, b| newest_first(a, b));
        let total = matched.len() as u64;
        let candidates = matched
            .into_iter()
            .filter(|n| query.is_after_cursor(n))
            .take(query.limit.saturating_add(1))
            .cloned()
            .collect();
        Ok(NotePage::from_candidates(candidates, total, query.limit))
    }

    fn begin_publish(
        &self,
        cmd: BeginPublishCmd<'_>,
    ) -> Result<Result<PublishAttempt, ApiError>, OfficeError> {
        let mut st = self.lock()?;
        let rkey = receipt_key(cmd.org_id, cmd.caller_id, cmd.operation_id);
        if let Some(existing) = st.receipts.get(&rkey).cloned() {
            if let Err(e) = check_receipt_reuse(
                existing.command,
                &existing.request_sha256,
                "publish",
                cmd.request_sha256,
            ) {
                return Ok(Err(e));
            }
            let shared_comment_id = existing.shared_comment_id.unwrap_or_default();
            let note = st
                .note(cmd.org_id, &existing.note_id)
                .cloned()
                .ok_or(OfficeError::Invalid("replayed publish note missing"))?;
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

        let note = match st.author_mutation(
            cmd.org_id,
            cmd.note_id,
            (cmd.caller_id, cmd.caller_is_person),
            cmd.expected_revision,
            "another publication of this private note is already pending",
        )? {
            Ok(note) => note,
            Err(e) => return Ok(Err(e)),
        };
        if let Err(e) = require_publishable(&note) {
            return Ok(Err(e));
        }
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
        let rkey = receipt_key(org_id, author_id, operation_id);
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
