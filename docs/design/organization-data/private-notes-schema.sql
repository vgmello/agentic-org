-- Separate durable private database (`<durable>/private/task-notes.db`); never load into the
-- shared office projection. Private notes, plans and decisions (`TASK-50`, `TASK-111` ADEC-1).
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

-- A note may belong to a task (task_id), a project (project_id), both or neither. A tombstone
-- keeps only identity, scope, kind and timestamps: every private text field is cleared.
CREATE TABLE private_notes (
  organization_id TEXT NOT NULL,
  id TEXT NOT NULL,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
  task_id TEXT,
  project_id TEXT CHECK (project_id IS NULL OR length(project_id) > 0),
  author_id TEXT NOT NULL,
  kind TEXT NOT NULL DEFAULT 'note' CHECK (kind IN ('note', 'plan', 'decision')),
  title TEXT CHECK (title IS NULL OR (length(trim(title)) > 0 AND length(title) <= 200)),
  why TEXT CHECK (why IS NULL OR (length(trim(why)) > 0 AND length(why) <= 2000)),
  alternatives TEXT
    CHECK (alternatives IS NULL OR (length(trim(alternatives)) > 0 AND length(alternatives) <= 2000)),
  body_markdown TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL CHECK (updated_at >= created_at),
  deleted_at TEXT,
  PRIMARY KEY (organization_id, id),
  CHECK ((deleted_at IS NULL AND body_markdown IS NOT NULL AND length(trim(body_markdown)) > 0
      AND (kind <> 'decision' OR title IS NOT NULL))
    OR (deleted_at IS NOT NULL AND deleted_at = updated_at AND body_markdown IS NULL
      AND title IS NULL AND why IS NULL AND alternatives IS NULL)),
  FOREIGN KEY (organization_id, task_id)
    REFERENCES private_note_tasks(organization_id, task_id) MATCH SIMPLE,
  FOREIGN KEY (organization_id, author_id)
    REFERENCES private_note_authors(organization_id, author_id)
) STRICT;

CREATE INDEX private_notes_by_author_task
  ON private_notes(organization_id, author_id, task_id, updated_at)
  WHERE deleted_at IS NULL;

CREATE INDEX private_notes_by_task
  ON private_notes(organization_id, task_id, updated_at)
  WHERE deleted_at IS NULL;

CREATE INDEX private_notes_by_author_kind
  ON private_notes(organization_id, author_id, kind, updated_at)
  WHERE deleted_at IS NULL;

-- Normalized tags in the task-label grammar, at most 8 per active note.
CREATE TABLE private_note_tags (
  organization_id TEXT NOT NULL,
  note_id TEXT NOT NULL,
  tag TEXT NOT NULL CHECK (length(tag) BETWEEN 1 AND 64 AND tag NOT GLOB '*[^a-z0-9._:/-]*'
    AND substr(tag, 1, 1) GLOB '[a-z0-9]' AND substr(tag, -1, 1) GLOB '[a-z0-9]'),
  PRIMARY KEY (organization_id, note_id, tag),
  FOREIGN KEY (organization_id, note_id)
    REFERENCES private_notes(organization_id, id) ON DELETE CASCADE
) STRICT;

CREATE INDEX private_note_tags_by_tag
  ON private_note_tags(organization_id, tag, note_id);

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
  FOREIGN KEY (organization_id, note_id) REFERENCES private_notes(organization_id, id),
  FOREIGN KEY (organization_id, author_id) REFERENCES private_note_authors(organization_id, author_id)
) STRICT;

CREATE UNIQUE INDEX one_pending_private_publication
  ON private_note_command_receipts(organization_id,note_id) WHERE state='pending';

CREATE UNIQUE INDEX private_receipt_shared_comment
  ON private_note_command_receipts(organization_id, shared_comment_id)
  WHERE shared_comment_id IS NOT NULL;

CREATE INDEX private_receipts_by_note
  ON private_note_command_receipts(organization_id, note_id);

-- Integrity triggers. Everything below creates only triggers: the store creates them after
-- the tables, so an upgrade can copy existing rows in before they guard new writes.

CREATE TRIGGER private_note_tasks_no_undelete BEFORE UPDATE ON private_note_tasks
WHEN OLD.is_deleted = 1 AND NEW.is_deleted = 0
BEGIN SELECT RAISE(ABORT, 'deleted private note task mirror cannot be undeleted'); END;

CREATE TRIGGER private_notes_no_replace BEFORE INSERT ON private_notes
WHEN EXISTS (SELECT 1 FROM private_notes n WHERE n.organization_id=NEW.organization_id AND n.id=NEW.id)
BEGIN SELECT RAISE(ABORT, 'private note identity already exists'); END;

CREATE TRIGGER private_notes_initial_state BEFORE INSERT ON private_notes
WHEN NEW.revision <> 1 OR NEW.deleted_at IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'new private note must start active at revision 1'); END;

CREATE TRIGGER private_notes_update_contract BEFORE UPDATE ON private_notes
WHEN NEW.organization_id IS NOT OLD.organization_id OR NEW.id IS NOT OLD.id
  OR NEW.task_id IS NOT OLD.task_id OR NEW.project_id IS NOT OLD.project_id
  OR NEW.author_id IS NOT OLD.author_id
  OR NEW.created_at IS NOT OLD.created_at
  OR NEW.schema_version IS NOT OLD.schema_version OR NEW.revision <> OLD.revision + 1
  OR NEW.updated_at < OLD.updated_at OR OLD.deleted_at IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'private note identity, revision or deletion conflict'); END;

CREATE TRIGGER private_notes_keep_tombstone BEFORE DELETE ON private_notes
BEGIN SELECT RAISE(ABORT, 'delete private note content with a revisioned tombstone'); END;

CREATE TRIGGER private_note_tombstone_clears_tags AFTER UPDATE OF deleted_at ON private_notes
WHEN NEW.deleted_at IS NOT NULL
BEGIN DELETE FROM private_note_tags WHERE organization_id=NEW.organization_id AND note_id=NEW.id; END;

CREATE TRIGGER private_note_tags_on_active_note BEFORE INSERT ON private_note_tags
WHEN NOT EXISTS (SELECT 1 FROM private_notes n WHERE n.organization_id=NEW.organization_id
  AND n.id=NEW.note_id AND n.deleted_at IS NULL)
BEGIN SELECT RAISE(ABORT, 'tags belong only to an active private note'); END;

CREATE TRIGGER private_note_tags_limit BEFORE INSERT ON private_note_tags
WHEN (SELECT COUNT(*) FROM private_note_tags t WHERE t.organization_id=NEW.organization_id
  AND t.note_id=NEW.note_id) >= 8
BEGIN SELECT RAISE(ABORT, 'a private note carries at most 8 tags'); END;

CREATE TRIGGER private_receipts_no_replace BEFORE INSERT ON private_note_command_receipts
WHEN EXISTS (SELECT 1 FROM private_note_command_receipts r WHERE r.organization_id=NEW.organization_id
  AND r.author_id=NEW.author_id AND r.operation_id=NEW.operation_id)
BEGIN SELECT RAISE(ABORT, 'private command receipt already exists'); END;

CREATE TRIGGER private_receipt_matches_author BEFORE INSERT ON private_note_command_receipts
WHEN NOT EXISTS (SELECT 1 FROM private_notes n WHERE n.organization_id=NEW.organization_id
  AND n.id=NEW.note_id AND n.author_id=NEW.author_id
  AND n.revision=NEW.result_revision
  AND (NEW.command<>'create' OR NEW.result_revision=1)
  AND (NEW.command<>'delete' OR n.deleted_at IS NOT NULL)
  AND (NEW.command<>'publish' OR (n.deleted_at IS NULL AND n.task_id IS NOT NULL)))
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

CREATE TRIGGER private_note_pending_publication BEFORE UPDATE ON private_notes
WHEN EXISTS (SELECT 1 FROM private_note_command_receipts r WHERE r.organization_id=OLD.organization_id
  AND r.note_id=OLD.id AND r.state='pending')
BEGIN SELECT RAISE(ABORT, 'resolve pending publication before editing or deleting note'); END;
