-- Separate durable private database; never load into the shared office projection.
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
