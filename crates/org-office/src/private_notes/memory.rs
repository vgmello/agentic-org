//! In-memory [`PrivateNotesStore`] adapter enforcing `private-notes-schema.sql` and `ADR-0031`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, MutexGuard};

use org_proto::private_notes::PRIVATE_NOTE_CHECKPOINT_TAG;
use org_proto::{ApiError, ErrorCode};

use crate::error::OfficeError;

use super::query::newest_first;
use super::{
    check_author_mutation, check_receipt_reuse, monotonic_ts, next_revision,
    parse_receipt_error_code, replayed_note, require_publishable, Accepted, BeginPublishCmd,
    CreateNoteCmd, DeleteNoteCmd, EditNoteCmd, NotePage, NoteQuery, NoteResult, PrivateNoteRecord,
    PrivateNotesStore, PublishAttempt,
};

type ReceiptKey = (String, String, String);
type NoteMap = Result<BTreeMap<String, PrivateNoteRecord>, OfficeError>;
type PubRes = Result<Result<PublishAttempt, ApiError>, OfficeError>;
type OptNote = Result<Option<PrivateNoteRecord>, OfficeError>;
type UnitRes = Result<(), OfficeError>;

fn err(code: ErrorCode, msg: impl Into<String>) -> ApiError {
    ApiError::new(code, msg)
}

fn rkey(org: &str, author: &str, op: &str) -> ReceiptKey {
    (org.to_owned(), author.to_owned(), op.to_owned())
}

#[derive(Debug, Clone)]
struct ReceiptRow {
    sha: String,
    cmd: &'static str,
    note_id: String,
    rev: u64,
    state: ReceiptState,
    comment_id: Option<String>,
    err_code: Option<String>,
    created_at: String,
    finished_at: Option<String>,
}

impl ReceiptRow {
    fn done(cmd: (&'static str, &str), note_id: &str, rev: u64, at: &str) -> Self {
        Self {
            sha: cmd.1.to_owned(),
            cmd: cmd.0,
            note_id: note_id.to_owned(),
            rev,
            state: ReceiptState::Completed,
            comment_id: None,
            err_code: None,
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

#[derive(Default)]
struct State {
    authors: BTreeSet<(String, String)>,
    tasks: BTreeMap<(String, String), bool>,
    notes: BTreeMap<(String, String), PrivateNoteRecord>,
    receipts: BTreeMap<ReceiptKey, ReceiptRow>,
}

impl State {
    fn note(&self, org: &str, id: &str) -> Option<&PrivateNoteRecord> {
        self.notes.get(&(org.to_owned(), id.to_owned()))
    }

    fn put(&mut self, note: PrivateNoteRecord) {
        self.notes
            .insert((note.organization_id.clone(), note.id.clone()), note);
    }

    fn has_pending_publish(&self, org: &str, id: &str) -> bool {
        self.receipts
            .iter()
            .any(|((o, _, _), r)| o == org && r.note_id == id && r.state == ReceiptState::Pending)
    }

    fn replay(
        &self,
        org: &str,
        ex: &ReceiptRow,
        cmd: (&str, &str),
        acc: Accepted<'_>,
    ) -> Result<PrivateNoteRecord, ApiError> {
        check_receipt_reuse(ex.cmd, &ex.sha, cmd.0, cmd.1)?;
        let cur = self
            .note(org, &ex.note_id)
            .ok_or_else(|| err(ErrorCode::StorageError, "replayed note missing"))?;
        Ok(replayed_note(cur, ex.rev, ex.time(), acc))
    }

    fn auth_mut(
        &self,
        org: &str,
        id: &str,
        caller: (&str, bool),
        rev: u64,
        msg: &str,
    ) -> NoteResult {
        check_author_mutation(
            self.note(org, id).cloned(),
            caller.0,
            caller.1,
            rev,
            || Ok(self.has_pending_publish(org, id)),
            msg,
        )
    }

    fn commit(
        &mut self,
        key: ReceiptKey,
        cmd: (&'static str, &str),
        rec: PrivateNoteRecord,
        at: &str,
    ) -> PrivateNoteRecord {
        let row = ReceiptRow::done(cmd, &rec.id, rec.revision, at);
        self.put(rec.clone());
        self.receipts.insert(key, row);
        rec
    }

    fn insert_new(
        &mut self,
        key: ReceiptKey,
        cmd: &CreateNoteCmd<'_>,
    ) -> Result<PrivateNoteRecord, ApiError> {
        if self.note(cmd.org_id, cmd.note_id).is_some() {
            let msg = format!("private note `{}` already exists", cmd.note_id);
            return Err(err(ErrorCode::AlreadyExists, msg));
        }
        self.authors
            .insert((cmd.org_id.to_owned(), cmd.author_id.to_owned()));
        if let Some(tid) = cmd.task_id {
            self.tasks
                .entry((cmd.org_id.to_owned(), tid.to_owned()))
                .or_insert(false);
        }
        let rec = PrivateNoteRecord::new_active(cmd);
        Ok(self.commit(key, ("create", cmd.request_sha256), rec, cmd.now))
    }

    fn mutate(
        &mut self,
        key: ReceiptKey,
        meta: (&'static str, &str, &str),
        ex: &PrivateNoteRecord,
        f: impl FnOnce(&PrivateNoteRecord, u64, &str) -> Result<PrivateNoteRecord, OfficeError>,
    ) -> NoteResult {
        let rev = match next_revision(ex.revision) {
            Ok(rev) => rev,
            Err(e) => return Ok(Err(e)),
        };
        let at = monotonic_ts(meta.2, &ex.updated_at);
        let rec = f(ex, rev, &at)?;
        Ok(Ok(self.commit(key, (meta.0, meta.1), rec, &at)))
    }

    fn find_checkpoint(&self, org: &str, author: &str, tid: &str) -> Option<PrivateNoteRecord> {
        self.notes
            .values()
            .filter(|n| {
                n.organization_id == org
                    && n.author_id == author
                    && n.task_id.as_deref() == Some(tid)
                    && n.deleted_at.is_none()
                    && n.tags.iter().any(|t| t == PRIVATE_NOTE_CHECKPOINT_TAG)
            })
            .min_by(|a, b| newest_first(a, b))
            .cloned()
    }

    fn replay_publish(&self, org: &str, ex: &ReceiptRow, sha: &str) -> PubRes {
        if let Err(e) = check_receipt_reuse(ex.cmd, &ex.sha, "publish", sha) {
            return Ok(Err(e));
        }
        let cid = ex.comment_id.clone().unwrap_or_default();
        let note = self
            .note(org, &ex.note_id)
            .cloned()
            .ok_or(OfficeError::Invalid("replayed publish note missing"))?;
        match ex.state {
            ReceiptState::Completed => Ok(Ok(PublishAttempt::AlreadyCompleted {
                note,
                result_revision: ex.rev,
                shared_comment_id: cid,
            })),
            ReceiptState::Pending => Ok(Ok(PublishAttempt::Pending {
                note,
                shared_comment_id: cid,
            })),
            ReceiptState::Rejected => Ok(Err(err(
                parse_receipt_error_code(ex.err_code.as_deref()),
                "private note publish was rejected",
            ))),
        }
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

    pub fn active_notes(&self, org_id: &str) -> Result<Vec<PrivateNoteRecord>, OfficeError> {
        let st = self.lock()?;
        Ok(st
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

impl PrivateNotesStore for MemoryPrivateNotesStore {
    fn create_note(&self, cmd: CreateNoteCmd<'_>) -> NoteResult {
        let mut st = self.lock()?;
        let key = rkey(cmd.org_id, cmd.author_id, cmd.operation_id);
        if let Some(ex) = st.receipts.get(&key) {
            let req = ("create", cmd.request_sha256);
            return Ok(st.replay(cmd.org_id, ex, req, Accepted::Created(cmd.content)));
        }
        Ok(st.insert_new(key, &cmd))
    }

    fn upsert_checkpoint_note(&self, cmd: CreateNoteCmd<'_>) -> NoteResult {
        let mut st = self.lock()?;
        let key = rkey(cmd.org_id, cmd.author_id, cmd.operation_id);
        if let Some(ex) = st.receipts.get(&key) {
            if !matches!(ex.cmd, "create" | "edit") {
                return Ok(Err(err(
                    ErrorCode::OperationReused,
                    "command_id reused with different private note request",
                )));
            }
            let req = (ex.cmd, cmd.request_sha256);
            return Ok(st.replay(cmd.org_id, ex, req, Accepted::Created(cmd.content)));
        }
        let Some(tid) = cmd.task_id else {
            return Ok(Err(err(
                ErrorCode::InvalidRequest,
                "a checkpoint note must belong to a task",
            )));
        };
        let Some(ex) = st.find_checkpoint(cmd.org_id, cmd.author_id, tid) else {
            return Ok(st.insert_new(key, &cmd));
        };
        if st.has_pending_publish(cmd.org_id, &ex.id) {
            return Ok(Err(err(
                ErrorCode::Conflict,
                "resolve pending publication before updating checkpoint note",
            )));
        }
        let meta = ("edit", cmd.request_sha256, cmd.now);
        st.mutate(key, meta, &ex, |n, rev, at| {
            Ok(n.with_content(cmd.content, rev, at))
        })
    }

    fn edit_note(&self, cmd: EditNoteCmd<'_>) -> NoteResult {
        let mut st = self.lock()?;
        let key = rkey(cmd.org_id, cmd.caller_id, cmd.operation_id);
        if let Some(ex) = st.receipts.get(&key) {
            let req = ("edit", cmd.request_sha256);
            return Ok(st.replay(cmd.org_id, ex, req, Accepted::Edited(cmd.patch)));
        }
        let who = (cmd.caller_id, cmd.caller_is_person);
        let msg = "resolve pending publication before editing private note";
        let ex = match st.auth_mut(cmd.org_id, cmd.note_id, who, cmd.expected_revision, msg)? {
            Ok(n) => n,
            Err(e) => return Ok(Err(e)),
        };
        let meta = ("edit", cmd.request_sha256, cmd.now);
        st.mutate(key, meta, &ex, |n, rev, at| {
            let body = cmd
                .patch
                .apply(n)
                .ok_or(OfficeError::Invalid("active private note without a body"))?;
            Ok(n.with_content(&body, rev, at))
        })
    }

    fn delete_note(&self, cmd: DeleteNoteCmd<'_>) -> NoteResult {
        let mut st = self.lock()?;
        let key = rkey(cmd.org_id, cmd.caller_id, cmd.operation_id);
        if let Some(ex) = st.receipts.get(&key) {
            let req = ("delete", cmd.request_sha256);
            return Ok(st.replay(cmd.org_id, ex, req, Accepted::Deleted));
        }
        let who = (cmd.caller_id, cmd.caller_is_person);
        let msg = "resolve pending publication before deleting private note";
        let ex = match st.auth_mut(cmd.org_id, cmd.note_id, who, cmd.expected_revision, msg)? {
            Ok(n) => n,
            Err(e) => return Ok(Err(e)),
        };
        let meta = ("delete", cmd.request_sha256, cmd.now);
        st.mutate(key, meta, &ex, |n, rev, at| Ok(n.tombstone(rev, at)))
    }

    fn get_note(&self, org: &str, caller: &str, is_person: bool, id: &str) -> OptNote {
        let st = self.lock()?;
        Ok(st
            .note(org, id)
            .filter(|n| n.deleted_at.is_none() && (is_person || n.author_id == caller))
            .cloned())
    }

    fn list_notes(&self, query: &NoteQuery<'_>) -> Result<NotePage, OfficeError> {
        let st = self.lock()?;
        let mut matched: Vec<_> = st.notes.values().filter(|n| query.matches(n)).collect();
        matched.sort_by(|a, b| newest_first(a, b));
        let total = matched.len() as u64;
        let items = matched
            .into_iter()
            .filter(|n| query.is_after_cursor(n))
            .take(query.limit.saturating_add(1))
            .cloned()
            .collect();
        Ok(NotePage::from_candidates(items, total, query.limit))
    }

    fn latest_author_notes_for_tasks(&self, org: &str, author: &str, tids: &[String]) -> NoteMap {
        let wanted: BTreeSet<&str> = tids.iter().map(String::as_str).collect();
        if wanted.is_empty() {
            return Ok(BTreeMap::new());
        }
        let st = self.lock()?;
        let mut latest: BTreeMap<String, &PrivateNoteRecord> = BTreeMap::new();
        for n in st.notes.values() {
            if n.organization_id != org || n.author_id != author || n.deleted_at.is_some() {
                continue;
            }
            if let Some(tid) = n.task_id.as_deref().filter(|t| wanted.contains(*t)) {
                latest
                    .entry(tid.to_owned())
                    .and_modify(|c| {
                        if newest_first(n, c).is_lt() {
                            *c = n;
                        }
                    })
                    .or_insert(n);
            }
        }
        Ok(latest.into_iter().map(|(k, v)| (k, v.clone())).collect())
    }

    fn begin_publish(&self, cmd: BeginPublishCmd<'_>) -> PubRes {
        let mut st = self.lock()?;
        let key = rkey(cmd.org_id, cmd.caller_id, cmd.operation_id);
        if let Some(ex) = st.receipts.get(&key).cloned() {
            return st.replay_publish(cmd.org_id, &ex, cmd.request_sha256);
        }
        let who = (cmd.caller_id, cmd.caller_is_person);
        let msg = "another publication of this private note is already pending";
        let note = match st.auth_mut(cmd.org_id, cmd.note_id, who, cmd.expected_revision, msg)? {
            Ok(n) => n,
            Err(e) => return Ok(Err(e)),
        };
        if let Err(e) = require_publishable(&note) {
            return Ok(Err(e));
        }
        let used = st.receipts.iter().any(|((o, _, _), r)| {
            o == cmd.org_id && r.comment_id.as_deref() == Some(cmd.comment_id)
        });
        if used {
            let m = format!("comment_id `{}` is already reserved", cmd.comment_id);
            return Ok(Err(err(ErrorCode::AlreadyExists, m)));
        }
        st.receipts.insert(
            key,
            ReceiptRow {
                sha: cmd.request_sha256.to_owned(),
                cmd: "publish",
                note_id: cmd.note_id.to_owned(),
                rev: note.revision,
                state: ReceiptState::Pending,
                comment_id: Some(cmd.comment_id.to_owned()),
                err_code: None,
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
        org: &str,
        author: &str,
        op: &str,
        code: Option<&str>,
        now: &str,
    ) -> UnitRes {
        let mut st = self.lock()?;
        if let Some(row) = st
            .receipts
            .get_mut(&rkey(org, author, op))
            .filter(|r| r.state == ReceiptState::Pending)
        {
            row.state = if code.is_some() {
                ReceiptState::Rejected
            } else {
                ReceiptState::Completed
            };
            row.err_code = code.map(str::to_owned);
            row.finished_at = Some(monotonic_ts(now, &row.created_at));
        }
        Ok(())
    }

    fn mark_task_deleted(&self, org_id: &str, task_id: &str) -> Result<(), OfficeError> {
        self.lock()?
            .tasks
            .insert((org_id.to_owned(), task_id.to_owned()), true);
        Ok(())
    }
}
