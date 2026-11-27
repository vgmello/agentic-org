//! SQLite private notes adapter (`TASK-111` ADEC-1/2): schema creation, the one-time upgrade
//! of a `TASK-50` database, schema-level guards, and the store conformance run both engines
//! share.

use org_proto::private_notes::{PrivateNoteAddParams, PrivateNoteKind, PrivateNoteUpdateParams};
use org_proto::ErrorCode;
use rusqlite::{params, Connection};

use super::schema::{layout, Layout};
use super::SqlitePrivateNotesStore;
use crate::private_notes::memory::MemoryPrivateNotesStore;
use crate::private_notes::{
    BeginPublishCmd, CreateNoteCmd, DeleteNoteCmd, EditNoteCmd, NoteContent, NoteCursor, NotePatch,
    NoteQuery, PrivateNotesStore,
};
use crate::store::sqlite::private_notes_copy::{check_task_notes, snapshot_task_notes};

/// The `TASK-50` schema exactly as offices created it before `TASK-111`.
const TASK50_DDL: &str = r#"-- Separate durable private database; never load into the shared office projection.
PRAGMA foreign_keys = ON;
PRAGMA recursive_triggers = ON;

CREATE TABLE private_note_authors (
  organization_id TEXT NOT NULL,
  author_id TEXT NOT NULL CHECK (length(author_id) > 0),
  PRIMARY KEY (organization_id, author_id)
) STRICT;

CREATE TABLE private_note_tasks (
  organization_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  is_deleted INTEGER NOT NULL DEFAULT 0 CHECK (is_deleted IN (0, 1)),
  PRIMARY KEY (organization_id, task_id)
) STRICT;

CREATE TRIGGER private_note_tasks_no_undelete BEFORE UPDATE ON private_note_tasks
WHEN OLD.is_deleted = 1 AND NEW.is_deleted = 0
BEGIN SELECT RAISE(ABORT, 'deleted private note task mirror cannot be undeleted'); END;

CREATE TABLE private_task_notes (
  organization_id TEXT NOT NULL,
  id TEXT NOT NULL,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
  task_id TEXT NOT NULL,
  author_id TEXT NOT NULL,
  body_markdown TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL CHECK (updated_at >= created_at),
  deleted_at TEXT,
  PRIMARY KEY (organization_id, id),
  CHECK ((deleted_at IS NULL AND body_markdown IS NOT NULL AND length(trim(body_markdown)) > 0)
    OR (deleted_at IS NOT NULL AND body_markdown IS NULL AND deleted_at = updated_at)),
  FOREIGN KEY (organization_id, task_id) REFERENCES private_note_tasks(organization_id, task_id),
  FOREIGN KEY (organization_id, author_id)
    REFERENCES private_note_authors(organization_id, author_id)
) STRICT;

CREATE INDEX private_notes_by_author_task
  ON private_task_notes(organization_id, author_id, task_id, updated_at)
  WHERE deleted_at IS NULL;

CREATE INDEX private_notes_by_task
  ON private_task_notes(organization_id, task_id, updated_at)
  WHERE deleted_at IS NULL;

CREATE TRIGGER private_notes_no_replace BEFORE INSERT ON private_task_notes
WHEN EXISTS (SELECT 1 FROM private_task_notes n WHERE n.organization_id=NEW.organization_id AND n.id=NEW.id)
BEGIN SELECT RAISE(ABORT, 'private note identity already exists'); END;

CREATE TRIGGER private_notes_initial_state BEFORE INSERT ON private_task_notes
WHEN NEW.revision <> 1 OR NEW.deleted_at IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'new private note must start active at revision 1'); END;

CREATE TRIGGER private_notes_update_contract BEFORE UPDATE ON private_task_notes
WHEN NEW.organization_id IS NOT OLD.organization_id OR NEW.id IS NOT OLD.id
  OR NEW.task_id IS NOT OLD.task_id OR NEW.author_id IS NOT OLD.author_id
  OR NEW.created_at IS NOT OLD.created_at
  OR NEW.schema_version IS NOT OLD.schema_version OR NEW.revision <> OLD.revision + 1
  OR NEW.updated_at < OLD.updated_at OR OLD.deleted_at IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'private note identity, revision or deletion conflict'); END;

CREATE TRIGGER private_notes_keep_tombstone BEFORE DELETE ON private_task_notes
BEGIN SELECT RAISE(ABORT, 'delete private note content with a revisioned tombstone'); END;

CREATE TABLE private_note_command_receipts (
  organization_id TEXT NOT NULL,
  author_id TEXT NOT NULL,
  operation_id TEXT NOT NULL,
  request_sha256 TEXT NOT NULL CHECK (length(request_sha256)=64 AND request_sha256 NOT GLOB '*[^0-9a-f]*'),
  command TEXT NOT NULL CHECK (command IN ('create','edit','delete','publish')),
  note_id TEXT NOT NULL,
  result_revision INTEGER NOT NULL CHECK (result_revision>=1),
  state TEXT NOT NULL CHECK (state IN ('pending','completed','rejected')),
  shared_comment_id TEXT,
  error_code TEXT,
  created_at TEXT NOT NULL,
  finished_at TEXT,
  PRIMARY KEY (organization_id, author_id, operation_id),
  CHECK ((command='publish' AND shared_comment_id IS NOT NULL) OR (command<>'publish' AND shared_comment_id IS NULL AND state='completed')),
  CHECK ((state='pending' AND finished_at IS NULL) OR (state<>'pending' AND finished_at IS NOT NULL AND finished_at>=created_at)),
  CHECK ((state='rejected' AND error_code IS NOT NULL) OR (state<>'rejected' AND error_code IS NULL)),
  FOREIGN KEY (organization_id, note_id) REFERENCES private_task_notes(organization_id, id),
  FOREIGN KEY (organization_id, author_id) REFERENCES private_note_authors(organization_id, author_id)
) STRICT;

CREATE UNIQUE INDEX one_pending_private_publication
  ON private_note_command_receipts(organization_id,note_id) WHERE state='pending';

CREATE UNIQUE INDEX private_receipt_shared_comment
  ON private_note_command_receipts(organization_id, shared_comment_id)
  WHERE shared_comment_id IS NOT NULL;

CREATE INDEX private_receipts_by_note
  ON private_note_command_receipts(organization_id, note_id);

CREATE TRIGGER private_receipts_no_replace BEFORE INSERT ON private_note_command_receipts
WHEN EXISTS (SELECT 1 FROM private_note_command_receipts r WHERE r.organization_id=NEW.organization_id
  AND r.author_id=NEW.author_id AND r.operation_id=NEW.operation_id)
BEGIN SELECT RAISE(ABORT, 'private command receipt already exists'); END;

CREATE TRIGGER private_receipt_matches_author BEFORE INSERT ON private_note_command_receipts
WHEN NOT EXISTS (SELECT 1 FROM private_task_notes n WHERE n.organization_id=NEW.organization_id
  AND n.id=NEW.note_id AND n.author_id=NEW.author_id
  AND n.revision=NEW.result_revision
  AND (NEW.command<>'create' OR NEW.result_revision=1)
  AND (NEW.command<>'delete' OR n.deleted_at IS NOT NULL)
  AND (NEW.command<>'publish' OR n.deleted_at IS NULL))
BEGIN SELECT RAISE(ABORT, 'private receipt must match note author and accepted revision'); END;

CREATE TRIGGER private_receipts_no_delete BEFORE DELETE ON private_note_command_receipts
BEGIN SELECT RAISE(ABORT, 'private command receipts must be retained'); END;

CREATE TRIGGER private_receipts_finish_once BEFORE UPDATE ON private_note_command_receipts
WHEN OLD.state<>'pending' OR NEW.state NOT IN ('completed','rejected')
  OR NEW.organization_id IS NOT OLD.organization_id OR NEW.author_id IS NOT OLD.author_id
  OR NEW.operation_id IS NOT OLD.operation_id
  OR NEW.request_sha256 IS NOT OLD.request_sha256 OR NEW.command IS NOT OLD.command
  OR NEW.note_id IS NOT OLD.note_id OR NEW.result_revision IS NOT OLD.result_revision
  OR NEW.shared_comment_id IS NOT OLD.shared_comment_id OR NEW.created_at IS NOT OLD.created_at
BEGIN SELECT RAISE(ABORT, 'private receipt identity/result is immutable'); END;

CREATE TRIGGER private_note_pending_publication BEFORE UPDATE ON private_task_notes
WHEN EXISTS (SELECT 1 FROM private_note_command_receipts r WHERE r.organization_id=OLD.organization_id
  AND r.note_id=OLD.id AND r.state='pending')
BEGIN SELECT RAISE(ABORT, 'resolve pending publication before editing or deleting note'); END;
"#;

const ORG: &str = "org";
const AGENT: &str = "0193a5c0-0000-7000-8000-000000000001";
const LEAD: &str = "0193a5c0-0000-7000-8000-000000000002";
const TASK: &str = "0193a5c0-0000-7000-8000-000000000100";
const PROJECT: &str = "0193a5c0-0000-7000-8000-000000000200";

fn uuid(n: u64) -> String {
    format!("0193a5c0-0000-7000-8000-{n:012x}")
}

fn durable() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let durable = dir.path().canonicalize().unwrap().join("durable");
    std::fs::create_dir(&durable).unwrap();
    (dir, durable)
}

fn db_path(durable: &std::path::Path) -> std::path::PathBuf {
    durable.join("private/task-notes.db")
}

fn content(kind: PrivateNoteKind, body: &str, tags: &[&str]) -> NoteContent {
    NoteContent::from_add(&PrivateNoteAddParams {
        command_id: String::new(),
        note_id: String::new(),
        task: None,
        project_id: None,
        kind,
        title: None,
        why: Some(format!("why {body}")),
        alternatives: None,
        tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        body_markdown: body.to_owned(),
    })
    .unwrap()
}

struct Created {
    id: String,
}

fn create(
    store: &dyn PrivateNotesStore,
    n: u64,
    author: &str,
    scope: (Option<&str>, Option<&str>),
    content: &NoteContent,
    now: &str,
) -> Created {
    let id = uuid(n);
    let op = uuid(n + 1000);
    store
        .create_note(CreateNoteCmd {
            org_id: ORG,
            author_id: author,
            task_id: scope.0,
            project_id: scope.1,
            note_id: &id,
            content,
            operation_id: &op,
            request_sha256: &"a".repeat(64),
            now,
        })
        .unwrap()
        .unwrap();
    Created { id }
}

fn query<'a>(caller: &'a str, person: bool) -> NoteQuery<'a> {
    NoteQuery {
        org_id: ORG,
        caller_id: caller,
        caller_is_person: person,
        task_id: None,
        project_id: None,
        kind: None,
        tag: None,
        since: None,
        author_id: None,
        after: None,
        limit: 100,
    }
}

fn ids(store: &dyn PrivateNotesStore, q: &NoteQuery<'_>) -> (Vec<String>, u64) {
    let page = store.list_notes(q).unwrap();
    (page.notes.into_iter().map(|n| n.id).collect(), page.total)
}

/// The same scenario against any engine: every filter, page, patch, tombstone and refusal.
fn conformance(store: &dyn PrivateNotesStore) {
    let decision = content(
        PrivateNoteKind::Decision,
        "Use WAL\nbecause",
        &["DB:sqlite", "arch"],
    );
    let plan = content(PrivateNoteKind::Plan, "Plan the rollout", &["arch"]);
    let note = content(PrivateNoteKind::Note, "Loose thought", &[]);
    let d = create(
        store,
        1,
        AGENT,
        (Some(TASK), Some(PROJECT)),
        &decision,
        "2026-11-26T10:00:00.000Z",
    );
    let p = create(
        store,
        2,
        AGENT,
        (None, Some(PROJECT)),
        &plan,
        "2026-11-26T10:01:00.000Z",
    );
    let n = create(
        store,
        3,
        AGENT,
        (None, None),
        &note,
        "2026-11-26T10:02:00.000Z",
    );
    let l = create(
        store,
        4,
        LEAD,
        (Some(TASK), None),
        &decision,
        "2026-11-26T10:03:00.000Z",
    );

    // Newest first, with totals across pages.
    let (all, total) = ids(store, &query(AGENT, false));
    assert_eq!(all, [n.id.clone(), p.id.clone(), d.id.clone()]);
    assert_eq!(total, 3);
    let mut q = query(AGENT, false);
    q.limit = 2;
    let first = store.list_notes(&q).unwrap();
    assert_eq!(first.notes.len(), 2);
    assert_eq!(first.total, 3);
    let cursor = first.next_cursor.clone().expect("a second page");
    assert_eq!(NoteCursor::parse(&cursor.encode()).unwrap(), cursor);
    q.after = Some(&cursor);
    let second = store.list_notes(&q).unwrap();
    assert_eq!(
        second.notes.iter().map(|n| &n.id).collect::<Vec<_>>(),
        [&d.id]
    );
    assert!(second.next_cursor.is_none());
    assert_eq!(second.total, 3);

    // Filters.
    let mut q = query(AGENT, false);
    q.kind = Some(PrivateNoteKind::Decision);
    assert_eq!(ids(store, &q), (vec![d.id.clone()], 1));
    let mut q = query(AGENT, false);
    q.tag = Some("arch");
    assert_eq!(ids(store, &q), (vec![p.id.clone(), d.id.clone()], 2));
    let mut q = query(AGENT, false);
    q.tag = Some("db:sqlite");
    assert_eq!(ids(store, &q).0, std::slice::from_ref(&d.id));
    let mut q = query(AGENT, false);
    q.project_id = Some(PROJECT);
    assert_eq!(ids(store, &q).0, [p.id.clone(), d.id.clone()]);
    let mut q = query(AGENT, false);
    q.task_id = Some(TASK);
    assert_eq!(ids(store, &q).0, std::slice::from_ref(&d.id));
    let mut q = query(AGENT, false);
    q.since = Some("2026-11-26T10:01:00.000Z");
    assert_eq!(ids(store, &q).0, [n.id.clone(), p.id.clone()]);

    // ADR-0031: another agent (even the team lead holding the task) sees nothing of AGENT's.
    let mut q = query(LEAD, false);
    q.author_id = Some(AGENT);
    assert_eq!(ids(store, &q), (vec![], 0));
    let mut q = query(LEAD, false);
    q.task_id = Some(TASK);
    assert_eq!(ids(store, &q).0, std::slice::from_ref(&l.id));
    assert!(store.get_note(ORG, LEAD, false, &d.id).unwrap().is_none());
    let mut q = query("person", true);
    q.kind = Some(PrivateNoteKind::Decision);
    assert_eq!(ids(store, &q).0, [l.id.clone(), d.id.clone()]);
    q.author_id = Some(AGENT);
    assert_eq!(ids(store, &q).0, std::slice::from_ref(&d.id));
    let read = store.get_note(ORG, "person", true, &d.id).unwrap().unwrap();
    assert_eq!(read.tags, ["arch", "db:sqlite"]);
    assert_eq!(read.title.as_deref(), Some("Use WAL"));
    assert_eq!(read.why.as_deref(), Some("why Use WAL\nbecause"));

    // An author patch: kind, tags replaced, why cleared; others refused.
    let patch = NotePatch::from_update(&PrivateNoteUpdateParams {
        command_id: String::new(),
        note_id: String::new(),
        expected_revision: 1,
        kind: Some(PrivateNoteKind::Decision),
        title: None,
        why: Some(String::new()),
        alternatives: Some("Stay on JSON".into()),
        tags: Some(vec!["rollout".into()]),
        body_markdown: None,
    })
    .unwrap();
    let edit = |caller: &str, person: bool, op: u64| {
        let op = uuid(op);
        store
            .edit_note(EditNoteCmd {
                org_id: ORG,
                caller_id: caller,
                caller_is_person: person,
                note_id: &p.id,
                expected_revision: 1,
                patch: &patch,
                operation_id: &op,
                request_sha256: &"b".repeat(64),
                now: "2026-11-26T10:05:00.000Z",
            })
            .unwrap()
    };
    assert_eq!(edit(LEAD, false, 50).unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(
        edit("person", true, 51).unwrap_err().code,
        ErrorCode::NotAuthorized
    );
    let edited = edit(AGENT, false, 52).unwrap();
    assert_eq!(edited.revision, 2);
    assert_eq!(edited.kind, PrivateNoteKind::Decision);
    assert_eq!(edited.tags, ["rollout"]);
    assert_eq!(edited.why, None);
    assert_eq!(edited.alternatives.as_deref(), Some("Stay on JSON"));
    assert_eq!(edited.title.as_deref(), Some("Plan the rollout"));
    let replayed = edit(AGENT, false, 52).unwrap();
    assert_eq!(
        (replayed.revision, replayed.tags.clone()),
        (2, edited.tags.clone())
    );
    let mut q = query(AGENT, false);
    q.tag = Some("arch");
    assert_eq!(
        ids(store, &q).0,
        std::slice::from_ref(&d.id),
        "replaced tags no longer match"
    );

    // A task-less note cannot be published; a task note can start.
    let publish = |note: &str, op: u64, comment: u64| {
        let (op, comment) = (uuid(op), uuid(comment));
        store
            .begin_publish(BeginPublishCmd {
                org_id: ORG,
                caller_id: AGENT,
                caller_is_person: false,
                note_id: note,
                expected_revision: 1,
                comment_id: &comment,
                operation_id: &op,
                request_sha256: &"c".repeat(64),
                now: "2026-11-26T10:06:00.000Z",
            })
            .unwrap()
    };
    assert_eq!(
        publish(&n.id, 60, 61).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    assert!(publish(&d.id, 62, 63).is_ok());
    store
        .finish_publish(ORG, AGENT, &uuid(62), None, "2026-11-26T10:07:00.000Z")
        .unwrap();

    // The tombstone clears every private field and tag.
    let removed = store
        .delete_note(DeleteNoteCmd {
            org_id: ORG,
            caller_id: AGENT,
            caller_is_person: false,
            note_id: &d.id,
            expected_revision: 1,
            operation_id: &uuid(70),
            request_sha256: &"d".repeat(64),
            now: "2026-11-26T10:08:00.000Z",
        })
        .unwrap()
        .unwrap();
    assert_eq!(removed.revision, 2);
    assert!(removed.deleted_at.is_some());
    assert_eq!(
        (
            removed.title,
            removed.why,
            removed.alternatives,
            removed.body_markdown
        ),
        (None, None, None, None)
    );
    assert!(removed.tags.is_empty());
    assert_eq!(removed.kind, PrivateNoteKind::Decision, "metadata survives");
    assert!(store.get_note(ORG, AGENT, false, &d.id).unwrap().is_none());
    let mut q = query(AGENT, false);
    q.tag = Some("db:sqlite");
    assert_eq!(ids(store, &q), (vec![], 0));
}

#[test]
fn both_engines_pass_the_same_store_conformance() {
    conformance(&MemoryPrivateNotesStore::new());
    let (_dir, durable) = durable();
    let store = SqlitePrivateNotesStore::open(&durable).unwrap();
    conformance(&store);
    let raw = Connection::open(db_path(&durable)).unwrap();
    let tags: i64 = raw
        .query_row(
            "SELECT COUNT(*) FROM private_note_tags WHERE note_id = ?1",
            [uuid(1)],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tags, 0, "a tombstone leaves no tag rows");
    let text: (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = raw
        .query_row(
            "SELECT title, why, alternatives, body_markdown FROM private_notes WHERE id = ?1",
            [uuid(1)],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(text, (None, None, None, None));
}

#[test]
fn a_new_file_gets_the_schema_and_reopens_unchanged() {
    let (_dir, durable) = durable();
    drop(SqlitePrivateNotesStore::open(&durable).unwrap());
    drop(SqlitePrivateNotesStore::open(&durable).unwrap());
    let raw = Connection::open(db_path(&durable)).unwrap();
    assert_eq!(layout(&raw).unwrap(), Some(Layout::Current));
}

#[test]
fn the_schema_guards_tags_titles_and_tombstones() {
    let (_dir, durable) = durable();
    drop(SqlitePrivateNotesStore::open(&durable).unwrap());
    let raw = Connection::open(db_path(&durable)).unwrap();
    raw.execute_batch("PRAGMA foreign_keys = ON; PRAGMA recursive_triggers = ON;")
        .unwrap();
    raw.execute_batch(
        "INSERT INTO private_note_authors VALUES ('org', 'a');
         INSERT INTO private_notes (organization_id, id, schema_version, revision, author_id,
           kind, title, body_markdown, created_at, updated_at)
         VALUES ('org', 'n', 1, 1, 'a', 'decision', 'T', 'b', 't', 't');",
    )
    .unwrap();
    let refused = |sql: &str| raw.execute_batch(sql).unwrap_err().to_string();
    assert!(refused(
        "INSERT INTO private_notes (organization_id, id, schema_version, revision, author_id,
           kind, body_markdown, created_at, updated_at)
         VALUES ('org', 'untitled', 1, 1, 'a', 'decision', 'b', 't', 't')"
    )
    .contains("CHECK"));
    assert!(refused(
        "INSERT INTO private_notes (organization_id, id, schema_version, revision, task_id,
           author_id, body_markdown, created_at, updated_at)
         VALUES ('org', 'orphan', 1, 1, 'missing-task', 'a', 'b', 't', 't')"
    )
    .contains("FOREIGN KEY"));
    for tag in ["Upper", "-x", "x-", "a b"] {
        assert!(refused(&format!(
            "INSERT INTO private_note_tags VALUES ('org', 'n', '{tag}')"
        ))
        .contains("CHECK"));
    }
    for n in 0..8 {
        raw.execute(
            "INSERT INTO private_note_tags VALUES ('org', 'n', ?1)",
            [format!("t{n}")],
        )
        .unwrap();
    }
    assert!(
        refused("INSERT INTO private_note_tags VALUES ('org', 'n', 'ninth')")
            .contains("at most 8 tags")
    );
    assert!(refused(
        "UPDATE private_note_tags SET tag = 'renamed' WHERE note_id = 'n' AND tag = 't0'"
    )
    .contains("replaced, never updated"));
    raw.execute_batch(
        "UPDATE private_notes SET revision = 2, title = NULL, why = NULL, alternatives = NULL,
           body_markdown = NULL, updated_at = 'u', deleted_at = 'u' WHERE id = 'n'",
    )
    .unwrap();
    let tags: i64 = raw
        .query_row("SELECT COUNT(*) FROM private_note_tags", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(tags, 0);
    assert!(
        refused("INSERT INTO private_note_tags VALUES ('org', 'n', 'late')")
            .contains("only to an active private note")
    );
}

/// A `TASK-50` database with an edited note, a tombstone and a completed publication.
fn seed_task50(path: &std::path::Path) {
    let raw = Connection::open(path).unwrap();
    raw.execute_batch(TASK50_DDL).unwrap();
    raw.execute_batch(&format!(
        "INSERT INTO private_note_authors VALUES ('{ORG}', '{AGENT}');
         INSERT INTO private_note_tasks VALUES ('{ORG}', '{TASK}', 0);
         INSERT INTO private_task_notes VALUES ('{ORG}', 'kept', 1, 1, '{TASK}', '{AGENT}',
           'First draft', '2026-11-20T00:00:00.000Z', '2026-11-20T00:00:00.000Z', NULL);
         INSERT INTO private_note_command_receipts VALUES ('{ORG}', '{AGENT}', 'op-create',
           '{sha}', 'create', 'kept', 1, 'completed', NULL, NULL,
           '2026-11-20T00:00:00.000Z', '2026-11-20T00:00:00.000Z');
         UPDATE private_task_notes SET revision = 2, body_markdown = 'Kept plan',
           updated_at = '2026-11-21T00:00:00.000Z' WHERE id = 'kept';
         INSERT INTO private_note_command_receipts VALUES ('{ORG}', '{AGENT}', 'op-publish',
           '{sha}', 'publish', 'kept', 2, 'completed', 'comment-1', NULL,
           '2026-11-21T00:00:00.000Z', '2026-11-21T00:00:01.000Z');
         INSERT INTO private_task_notes VALUES ('{ORG}', 'gone', 1, 1, '{TASK}', '{AGENT}',
           'Doomed', '2026-11-20T00:00:00.000Z', '2026-11-20T00:00:00.000Z', NULL);
         UPDATE private_task_notes SET revision = 2, body_markdown = NULL,
           updated_at = '2026-11-22T00:00:00.000Z', deleted_at = '2026-11-22T00:00:00.000Z'
           WHERE id = 'gone';",
        sha = "e".repeat(64),
    ))
    .unwrap();
    assert_eq!(layout(&raw).unwrap(), Some(Layout::Task50));
}

#[test]
fn a_task50_database_is_upgraded_in_place_keeping_every_row() {
    let (dir, durable) = durable();
    std::fs::create_dir(durable.join("private")).unwrap();
    let path = db_path(&durable);
    seed_task50(&path);
    // A backup of the old layout still verifies: the store upgrades it when it opens it.
    let old_copy = dir.path().canonicalize().unwrap().join("old-copy.db");
    snapshot_task_notes(&path, &old_copy).unwrap();
    check_task_notes(&old_copy).unwrap();

    let store = SqlitePrivateNotesStore::open(&durable).unwrap();
    let kept = store.get_note(ORG, AGENT, false, "kept").unwrap().unwrap();
    assert_eq!(kept.revision, 2);
    assert_eq!(kept.body_markdown.as_deref(), Some("Kept plan"));
    assert_eq!(kept.kind, PrivateNoteKind::Note);
    assert_eq!(kept.task_id.as_deref(), Some(TASK));
    assert_eq!((kept.title.clone(), kept.project_id.clone()), (None, None));
    assert!(store
        .get_note(ORG, "person", true, "gone")
        .unwrap()
        .is_none());
    let page = store.list_notes(&query("person", true)).unwrap();
    assert_eq!(page.total, 1);

    // The carried-over note takes edits under the new contract; receipts survived.
    let patch = NotePatch {
        tags: Some(vec!["carried".into()]),
        ..NotePatch::default()
    };
    let edited = store
        .edit_note(EditNoteCmd {
            org_id: ORG,
            caller_id: AGENT,
            caller_is_person: false,
            note_id: "kept",
            expected_revision: 2,
            patch: &patch,
            operation_id: "op-tag",
            request_sha256: &"f".repeat(64),
            now: "2026-11-26T00:00:00.000Z",
        })
        .unwrap()
        .unwrap();
    assert_eq!(edited.revision, 3);
    assert_eq!(
        edited.title.as_deref(),
        Some("Kept plan"),
        "an edit derives the title"
    );
    assert_eq!(edited.tags, ["carried"]);
    drop(store);

    let raw = Connection::open(&path).unwrap();
    assert_eq!(layout(&raw).unwrap(), Some(Layout::Current));
    let receipts: i64 = raw
        .query_row(
            "SELECT COUNT(*) FROM private_note_command_receipts WHERE organization_id = ?1",
            params![ORG],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(receipts, 3);
    drop(raw);
    // Reopening a current database changes nothing.
    let reopened = SqlitePrivateNotesStore::open(&durable).unwrap();
    assert_eq!(reopened.list_notes(&query(AGENT, false)).unwrap().total, 1);
}

#[test]
fn a_foreign_or_partial_database_is_refused() {
    for foreign in [
        "CREATE TABLE private_note_authors (organization_id TEXT, author_id TEXT);",
        "CREATE VIEW foreign_view AS SELECT 1 AS x;",
    ] {
        let (_dir, durable) = durable();
        std::fs::create_dir(durable.join("private")).unwrap();
        let raw = Connection::open(db_path(&durable)).unwrap();
        raw.execute_batch(foreign).unwrap();
        drop(raw);
        assert!(
            SqlitePrivateNotesStore::open(&durable).is_err(),
            "{foreign}"
        );
    }
}

/// A current database, as the store creates it.
fn seed_current(path: &std::path::Path) {
    let durable = path.parent().and_then(std::path::Path::parent).unwrap();
    drop(SqlitePrivateNotesStore::open(durable).unwrap());
}

/// A notes file made by `seed` and then changed by `tamper` is neither opened by the store nor
/// accepted by a backup check, and the refusal leaves the file as it was.
fn assert_tampered_layout_refused(seed: fn(&std::path::Path), tamper: &str) {
    let (_dir, durable) = durable();
    std::fs::create_dir_all(durable.join("private")).unwrap();
    let path = db_path(&durable);
    seed(&path);
    let raw = Connection::open(&path).unwrap();
    raw.execute_batch(tamper).unwrap();
    assert_eq!(layout(&raw).unwrap(), None, "{tamper}");
    drop(raw);
    assert!(check_task_notes(&path).is_err(), "backup check: {tamper}");
    assert!(
        SqlitePrivateNotesStore::open(&durable).is_err(),
        "store: {tamper}"
    );
    let raw = Connection::open(&path).unwrap();
    assert_eq!(layout(&raw).unwrap(), None, "left unchanged: {tamper}");
}

#[test]
fn a_mixed_layout_or_foreign_object_is_refused_by_the_store_and_the_backup_check() {
    for tamper in [
        "CREATE TABLE private_note_tags (organization_id TEXT, note_id TEXT, tag TEXT);",
        "CREATE TABLE foreign_table (x TEXT);",
        "DROP TRIGGER private_notes_keep_tombstone;",
        "CREATE TRIGGER foreign_trigger BEFORE DELETE ON private_note_tasks
         BEGIN SELECT 1; END;",
    ] {
        assert_tampered_layout_refused(seed_task50, tamper);
    }
    for tamper in [
        "CREATE TABLE foreign_table (x TEXT);",
        "CREATE TABLE private_task_notes (id TEXT);",
        "DROP TRIGGER private_note_tags_no_update;",
        "CREATE VIEW foreign_view AS SELECT id FROM private_notes;",
        "CREATE TRIGGER foreign_trigger BEFORE DELETE ON private_note_tags
         BEGIN SELECT 1; END;",
    ] {
        assert_tampered_layout_refused(seed_current, tamper);
    }
}

#[test]
fn both_engines_derive_the_same_title_around_control_characters() {
    let (_dir, durable) = durable();
    let sqlite = SqlitePrivateNotesStore::open(&durable).unwrap();
    let memory = MemoryPrivateNotesStore::new();
    for store in [&memory as &dyn PrivateNotesStore, &sqlite] {
        for (n, body, title) in [
            (1, "\u{0007}", "Untitled"),
            (2, "# \x01\nReal heading", "Real heading"),
        ] {
            let decision = content(PrivateNoteKind::Decision, body, &[]);
            assert_eq!(decision.title.as_deref(), Some(title), "{body:?}");
            let note = create(
                store,
                n,
                AGENT,
                (None, None),
                &decision,
                "2026-11-26T10:00:00.000Z",
            );
            let stored = store.get_note(ORG, AGENT, false, &note.id).unwrap();
            assert_eq!(stored.unwrap().title.as_deref(), Some(title), "{body:?}");
        }
        // A cleared title is derived again from the edited body the same way.
        let patch = NotePatch {
            title: Some(None),
            body_markdown: Some("\x02\n## \x03 Heading \x04".into()),
            ..NotePatch::default()
        };
        let edited = store
            .edit_note(EditNoteCmd {
                org_id: ORG,
                caller_id: AGENT,
                caller_is_person: false,
                note_id: &uuid(2),
                expected_revision: 1,
                patch: &patch,
                operation_id: &uuid(3000),
                request_sha256: &"b".repeat(64),
                now: "2026-11-26T11:00:00.000Z",
            })
            .unwrap()
            .unwrap();
        assert_eq!(edited.title.as_deref(), Some("Heading"));
    }
}

#[test]
fn the_store_keeps_temporary_data_in_memory() {
    let (_dir, durable) = durable();
    let store = SqlitePrivateNotesStore::open(&durable).unwrap();
    let conn = store.lock().unwrap();
    let temp_store: i64 = conn
        .query_row("PRAGMA temp_store", [], |row| row.get(0))
        .unwrap();
    assert_eq!(temp_store, 2, "MEMORY");
}

#[test]
fn a_write_changes_only_the_authors_active_note_at_the_revision_read() {
    use super::rows::{tombstone, update_content, Expected};
    use crate::error::OfficeError;

    let (_dir, durable) = durable();
    let store = SqlitePrivateNotesStore::open(&durable).unwrap();
    let body = content(PrivateNoteKind::Note, "Body", &[]);
    let note = create(
        &store,
        1,
        AGENT,
        (None, None),
        &body,
        "2026-11-26T10:00:00.000Z",
    );
    let current = store.get_note(ORG, AGENT, false, &note.id).unwrap();
    let later = "2026-11-26T11:00:00.000Z";
    let edited =
        current
            .unwrap()
            .with_content(&content(PrivateNoteKind::Plan, "Edited", &[]), 2, later);
    let conn = store.lock().unwrap();
    let key = (ORG, note.id.as_str());
    let guarded = |result: Result<(), OfficeError>| matches!(result, Err(OfficeError::Invalid(msg)) if msg.contains("underneath its writer"));
    for expected in [
        Expected {
            author_id: LEAD,
            revision: 1,
        },
        Expected {
            author_id: AGENT,
            revision: 2,
        },
    ] {
        assert!(guarded(update_content(&conn, expected, &edited)));
        assert!(guarded(tombstone(&conn, key, expected, 2, later)));
    }
    let read = Expected {
        author_id: AGENT,
        revision: 1,
    };
    tombstone(&conn, key, read, 2, later).unwrap();
    // A tombstone takes no further write, even at its own revision.
    let at_tombstone = Expected {
        author_id: AGENT,
        revision: 2,
    };
    assert!(guarded(update_content(&conn, at_tombstone, &edited)));
    assert!(guarded(tombstone(&conn, key, at_tombstone, 3, later)));
}
