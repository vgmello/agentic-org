-- HQ-only human-private file space (TASK-98.1, ADR-0037): <durable-root>/private/user-space.db.
-- One person's own files: only that person (never an agent, never another person) reads or
-- writes them. Never loaded into the shared office.db projection, never in durable/state/,
-- the journal or Git; bytes live in <durable-root>/private/blobs/sha256/<aa>/<bb>/<sha256>.
-- A satellite office never creates this database (ARCH-02).
PRAGMA foreign_keys = ON;

CREATE TABLE user_space_files (
  organization_id TEXT NOT NULL,
  id TEXT NOT NULL,
  owner_employee_id TEXT NOT NULL,
  -- The command that created the row: a retry is answered with the first row.
  operation_id TEXT NOT NULL,
  content_sha256 TEXT NOT NULL CHECK (length(content_sha256) = 64 AND content_sha256 NOT GLOB '*[^0-9a-f]*'),
  mime_type TEXT NOT NULL CHECK (mime_type IN (
    'image/png', 'image/jpeg', 'image/webp', 'image/gif', 'application/pdf',
    'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
    'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
    'text/markdown', 'text/plain', 'text/csv', 'application/json')),
  byte_count INTEGER NOT NULL CHECK (byte_count >= 0),
  filename TEXT NOT NULL CHECK (length(filename) BETWEEN 1 AND 255),
  title TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 160),
  description TEXT CHECK (description IS NULL OR length(description) <= 280),
  artifact_status TEXT NOT NULL DEFAULT 'final' CHECK (artifact_status IN ('draft', 'review', 'final', 'superseded', 'archived')),
  width INTEGER CHECK (width IS NULL OR width > 0),
  height INTEGER CHECK (height IS NULL OR height > 0),
  page_count INTEGER CHECK (page_count IS NULL OR page_count > 0),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (organization_id, id),
  UNIQUE (organization_id, operation_id)
) STRICT;
CREATE INDEX user_space_files_by_owner ON user_space_files(organization_id, owner_employee_id, id);
