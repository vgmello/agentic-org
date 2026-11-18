PRAGMA foreign_keys = ON;
-- Required on every write connection: REPLACE must run immutable-row DELETE guards.
PRAGMA recursive_triggers = ON;

CREATE TABLE organizations (
  id TEXT PRIMARY KEY,
  slug TEXT NOT NULL UNIQUE CHECK (length(slug) > 0),
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
  name TEXT NOT NULL,
  organization_repository_id TEXT NOT NULL,
  state_branch TEXT NOT NULL,
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'system', 'unknown')),
  updated_by_id TEXT,
  FOREIGN KEY (id, organization_repository_id) REFERENCES repositories(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED,
  CHECK ((created_by_kind = 'employee' AND created_by_id IS NOT NULL) OR (created_by_kind <> 'employee' AND created_by_id IS NULL)),
  CHECK ((updated_by_kind = 'employee' AND updated_by_id IS NOT NULL) OR (updated_by_kind <> 'employee' AND updated_by_id IS NULL)),
  FOREIGN KEY (id, created_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (id, updated_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE repositories (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  id TEXT NOT NULL,
  key TEXT,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
kind TEXT NOT NULL CHECK (kind IN ('remote', 'local')),
  remote_url TEXT,
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'system', 'unknown')),
  updated_by_id TEXT,
  PRIMARY KEY (organization_id, id),
UNIQUE (organization_id, key),
  CHECK ((kind = 'remote' AND remote_url IS NOT NULL) OR (kind = 'local' AND remote_url IS NULL)),
  CHECK ((created_by_kind = 'employee' AND created_by_id IS NOT NULL) OR (created_by_kind <> 'employee' AND created_by_id IS NULL)),
  CHECK ((updated_by_kind = 'employee' AND updated_by_id IS NOT NULL) OR (updated_by_kind <> 'employee' AND updated_by_id IS NULL)),
  FOREIGN KEY (organization_id, created_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, updated_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE roles (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  id TEXT NOT NULL,
  key TEXT,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
name TEXT NOT NULL,
  body_markdown TEXT NOT NULL DEFAULT '',
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'system', 'unknown')),
  updated_by_id TEXT,
  PRIMARY KEY (organization_id, id),
UNIQUE (organization_id, key),
  CHECK ((created_by_kind = 'employee' AND created_by_id IS NOT NULL) OR (created_by_kind <> 'employee' AND created_by_id IS NULL)),
  CHECK ((updated_by_kind = 'employee' AND updated_by_id IS NOT NULL) OR (updated_by_kind <> 'employee' AND updated_by_id IS NULL)),
  FOREIGN KEY (organization_id, created_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, updated_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE teams (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  id TEXT NOT NULL,
  number INTEGER NOT NULL CHECK (number > 0),
  key TEXT GENERATED ALWAYS AS (CASE WHEN number IS NOT NULL THEN 'TEAM-' || number END) VIRTUAL,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
name TEXT NOT NULL,
  parent_team_id TEXT,
  lead_employee_id TEXT,
  status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'archived')),
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'system', 'unknown')),
  updated_by_id TEXT,
  PRIMARY KEY (organization_id, id),
UNIQUE (organization_id, number),
  CHECK (parent_team_id IS NULL OR parent_team_id <> id),
  FOREIGN KEY (organization_id, parent_team_id) REFERENCES teams(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, lead_employee_id, id) REFERENCES employees(organization_id, id, home_team_id)
    DEFERRABLE INITIALLY DEFERRED,
  CHECK ((created_by_kind = 'employee' AND created_by_id IS NOT NULL) OR (created_by_kind <> 'employee' AND created_by_id IS NULL)),
  CHECK ((updated_by_kind = 'employee' AND updated_by_id IS NOT NULL) OR (updated_by_kind <> 'employee' AND updated_by_id IS NULL)),
  FOREIGN KEY (organization_id, created_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, updated_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE offices (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  id TEXT NOT NULL,
  slug TEXT NOT NULL CHECK (length(slug) > 0),
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
name TEXT NOT NULL,
  role TEXT NOT NULL CHECK (role IN ('hq', 'satellite')),
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'system', 'unknown')),
  updated_by_id TEXT,
  PRIMARY KEY (organization_id, id),
UNIQUE (organization_id, slug),
  CHECK ((created_by_kind = 'employee' AND created_by_id IS NOT NULL) OR (created_by_kind <> 'employee' AND created_by_id IS NULL)),
  CHECK ((updated_by_kind = 'employee' AND updated_by_id IS NOT NULL) OR (updated_by_kind <> 'employee' AND updated_by_id IS NULL)),
  FOREIGN KEY (organization_id, created_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, updated_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE structure_documents (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  id TEXT NOT NULL,
  key TEXT,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
kind TEXT NOT NULL CHECK (kind IN ('process', 'standard')),
  owner_employee_id TEXT,
  status TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'current', 'deprecated')),
  title TEXT NOT NULL,
  team_id TEXT,
  project_id TEXT,
  overridable INTEGER NOT NULL DEFAULT 1 CHECK (overridable IN (0, 1)),
  body_markdown TEXT NOT NULL DEFAULT '',
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'system', 'unknown')),
  updated_by_id TEXT,
  PRIMARY KEY (organization_id, id),
  CHECK (status <> 'current' OR owner_employee_id IS NOT NULL),
  FOREIGN KEY (organization_id, owner_employee_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
CHECK (team_id IS NULL OR project_id IS NULL),
  FOREIGN KEY (organization_id, team_id) REFERENCES teams(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, project_id) REFERENCES projects(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED,
  CHECK ((created_by_kind = 'employee' AND created_by_id IS NOT NULL) OR (created_by_kind <> 'employee' AND created_by_id IS NULL)),
  CHECK ((updated_by_kind = 'employee' AND updated_by_id IS NOT NULL) OR (updated_by_kind <> 'employee' AND updated_by_id IS NULL)),
  FOREIGN KEY (organization_id, created_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, updated_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE local_repository_locations (
  organization_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  repository_id TEXT NOT NULL,
  origin_path TEXT NOT NULL,
  PRIMARY KEY (organization_id, office_id, repository_id),
  FOREIGN KEY (organization_id, office_id) REFERENCES offices(organization_id, id),
  FOREIGN KEY (organization_id, repository_id) REFERENCES repositories(organization_id, id)
) STRICT;

CREATE TABLE repository_caches (
  organization_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  repository_id TEXT NOT NULL,
  bare_path TEXT NOT NULL,
  PRIMARY KEY (organization_id, office_id, repository_id),
  FOREIGN KEY (organization_id, office_id) REFERENCES offices(organization_id, id),
  FOREIGN KEY (organization_id, repository_id) REFERENCES repositories(organization_id, id)
) STRICT;

CREATE TABLE projects (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  id TEXT NOT NULL,
  number INTEGER NOT NULL CHECK (number > 0),
  key TEXT GENERATED ALWAYS AS (CASE WHEN number IS NOT NULL THEN 'P-' || number END) VIRTUAL,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
slug TEXT NOT NULL CHECK (
    length(slug) > 0 AND slug NOT GLOB '*[^a-z0-9-]*'
    AND substr(slug, 1, 1) GLOB '[a-z0-9]'
    AND substr(slug, -1, 1) GLOB '[a-z0-9]'
    AND instr(slug, '--') = 0),
  name TEXT NOT NULL,
  owner_team_id TEXT NOT NULL,
  archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'system', 'unknown')),
  updated_by_id TEXT,
  development_process_id TEXT,
  PRIMARY KEY (organization_id, id),
UNIQUE (organization_id, number),
  UNIQUE (organization_id, slug),
  UNIQUE (organization_id, id, owner_team_id),
  FOREIGN KEY (organization_id, owner_team_id) REFERENCES teams(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, development_process_id) REFERENCES development_processes(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED,
  CHECK ((created_by_kind = 'employee' AND created_by_id IS NOT NULL) OR (created_by_kind <> 'employee' AND created_by_id IS NULL)),
  CHECK ((updated_by_kind = 'employee' AND updated_by_id IS NOT NULL) OR (updated_by_kind <> 'employee' AND updated_by_id IS NULL)),
  FOREIGN KEY (organization_id, created_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, updated_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE project_contributing_teams (
  organization_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  team_id TEXT NOT NULL,
  PRIMARY KEY (organization_id, project_id, team_id),
  FOREIGN KEY (organization_id, project_id) REFERENCES projects(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, team_id) REFERENCES teams(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE project_repositories (
  organization_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  repository_id TEXT NOT NULL,
  integration_branch TEXT NOT NULL,
  PRIMARY KEY (organization_id, project_id, repository_id),
  FOREIGN KEY (organization_id, project_id) REFERENCES projects(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, repository_id) REFERENCES repositories(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE releases (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  id TEXT NOT NULL,
  number INTEGER NOT NULL CHECK (number > 0),
  key TEXT GENERATED ALWAYS AS (CASE WHEN number IS NOT NULL THEN 'R-' || number END) VIRTUAL,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
project_id TEXT NOT NULL,
  title TEXT NOT NULL,
  version_label TEXT,
  body_markdown TEXT NOT NULL DEFAULT '',
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'system', 'unknown')),
  updated_by_id TEXT,
  PRIMARY KEY (organization_id, id),
UNIQUE (organization_id, project_id, number),
  UNIQUE (organization_id, id, project_id),
  FOREIGN KEY (organization_id, project_id) REFERENCES projects(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED,
  CHECK ((created_by_kind = 'employee' AND created_by_id IS NOT NULL) OR (created_by_kind <> 'employee' AND created_by_id IS NULL)),
  CHECK ((updated_by_kind = 'employee' AND updated_by_id IS NOT NULL) OR (updated_by_kind <> 'employee' AND updated_by_id IS NULL)),
  FOREIGN KEY (organization_id, created_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, updated_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE release_repository_commits (
  organization_id TEXT NOT NULL,
  release_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  repository_id TEXT NOT NULL,
  commit_sha TEXT NOT NULL,
  PRIMARY KEY (organization_id, release_id, repository_id),
  FOREIGN KEY (organization_id, release_id, project_id) REFERENCES releases(organization_id, id, project_id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, repository_id) REFERENCES repositories(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE milestones (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  id TEXT NOT NULL,
  number INTEGER NOT NULL CHECK (number > 0),
  key TEXT GENERATED ALWAYS AS (CASE WHEN number IS NOT NULL THEN 'M-' || number END) VIRTUAL,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
project_id TEXT NOT NULL,
  release_id TEXT NOT NULL,
  title TEXT NOT NULL,
  body_markdown TEXT NOT NULL DEFAULT '',
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'system', 'unknown')),
  updated_by_id TEXT,
  PRIMARY KEY (organization_id, id),
UNIQUE (organization_id, release_id, number),
  UNIQUE (organization_id, id, project_id),
  FOREIGN KEY (organization_id, release_id, project_id) REFERENCES releases(organization_id, id, project_id)
    DEFERRABLE INITIALLY DEFERRED,
  CHECK ((created_by_kind = 'employee' AND created_by_id IS NOT NULL) OR (created_by_kind <> 'employee' AND created_by_id IS NULL)),
  CHECK ((updated_by_kind = 'employee' AND updated_by_id IS NOT NULL) OR (updated_by_kind <> 'employee' AND updated_by_id IS NULL)),
  FOREIGN KEY (organization_id, created_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, updated_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE documents (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  id TEXT NOT NULL,
  key TEXT,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
  project_id TEXT NOT NULL,
  feature_id TEXT,
  release_id TEXT,
  deployment_id TEXT,
  status TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'current', 'deprecated')),
  kind TEXT NOT NULL CHECK (length(kind) BETWEEN 1 AND 64 AND kind NOT GLOB '*[^a-z0-9-]*'),
  owner_employee_id TEXT,
  title TEXT NOT NULL,
  body_markdown TEXT NOT NULL DEFAULT '',
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'system', 'unknown')),
  updated_by_id TEXT,
  summary TEXT,
  path TEXT CHECK (path IS NULL OR (
    length(path) BETWEEN 4 AND 512 AND path NOT GLOB '*[^a-z0-9/._-]*'
    AND substr(path, 1, 1) GLOB '[a-z0-9]' AND path GLOB '*.md'
    AND instr(path, '//') = 0 AND instr(path, '..') = 0 AND instr(path, '/.') = 0)),
  version INTEGER CHECK (version >= 1),
  issued_revision INTEGER CHECK (issued_revision >= 1),
  PRIMARY KEY (organization_id, id),
  UNIQUE (organization_id, project_id, path),
  FOREIGN KEY (organization_id, owner_employee_id) REFERENCES employees(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED,
  CHECK (status <> 'current' OR owner_employee_id IS NOT NULL),
  CHECK ((path IS NOT NULL) = (feature_id IS NULL AND release_id IS NULL AND deployment_id IS NULL AND kind <> 'brief')),
  CHECK (path IS NOT NULL OR kind IN ('brief', 'decision', 'design', 'report', 'changelog')),
  CHECK (summary IS NULL OR path IS NOT NULL),
  CHECK ((version IS NULL) = (issued_revision IS NULL)),
  CHECK (version IS NULL OR (path IS NOT NULL AND issued_revision <= revision AND status <> 'draft')),
  CHECK ((feature_id IS NOT NULL) + (release_id IS NOT NULL) + (deployment_id IS NOT NULL) <= 1),
  CHECK (feature_id IS NULL OR kind IN ('design', 'decision', 'report')),
  CHECK (kind <> 'brief' OR (feature_id IS NULL AND release_id IS NULL AND deployment_id IS NULL)),
  CHECK (kind <> 'changelog' OR release_id IS NOT NULL OR deployment_id IS NOT NULL),
  FOREIGN KEY (organization_id, release_id, project_id) REFERENCES releases(organization_id, id, project_id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, deployment_id, project_id) REFERENCES deployments(organization_id, id, project_id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, project_id) REFERENCES projects(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, feature_id, project_id) REFERENCES features(organization_id, id, project_id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, id, version) REFERENCES document_versions(organization_id, document_id, version)
    DEFERRABLE INITIALLY DEFERRED,
  CHECK ((created_by_kind = 'employee' AND created_by_id IS NOT NULL) OR (created_by_kind <> 'employee' AND created_by_id IS NULL)),
  CHECK ((updated_by_kind = 'employee' AND updated_by_id IS NOT NULL) OR (updated_by_kind <> 'employee' AND updated_by_id IS NULL)),
  FOREIGN KEY (organization_id, created_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, updated_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE features (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  id TEXT NOT NULL,
  number INTEGER CHECK (number > 0),
  key TEXT GENERATED ALWAYS AS (CASE WHEN number IS NOT NULL THEN 'F-' || number END) VIRTUAL,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
project_id TEXT NOT NULL,
  local_slug TEXT NOT NULL CHECK (
    length(local_slug) > 0 AND local_slug NOT GLOB '*[^a-z0-9-]*'
    AND substr(local_slug, 1, 1) GLOB '[a-z0-9]'
    AND substr(local_slug, -1, 1) GLOB '[a-z0-9]'
    AND instr(local_slug, '--') = 0),
  owner_employee_id TEXT,
  work_type TEXT CHECK (work_type IN ('build', 'run')),
  title TEXT NOT NULL,
  status TEXT NOT NULL,
  body_markdown TEXT NOT NULL DEFAULT '',
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'system', 'unknown')),
  updated_by_id TEXT,
  delivered_at TEXT,
  retired_reason TEXT,
  replaced_by_feature_id TEXT,
  CHECK (replaced_by_feature_id IS NULL OR replaced_by_feature_id<>id),
  FOREIGN KEY (organization_id,replaced_by_feature_id,project_id)
    REFERENCES features(organization_id,id,project_id) DEFERRABLE INITIALLY DEFERRED,
  PRIMARY KEY (organization_id, id),
  CHECK (status NOT IN ('in_progress', 'live') OR owner_employee_id IS NOT NULL),
  FOREIGN KEY (organization_id, owner_employee_id) REFERENCES employees(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED,
UNIQUE (organization_id, project_id, number),
  UNIQUE (organization_id, project_id, local_slug),
  UNIQUE (organization_id, id, project_id),
  FOREIGN KEY (organization_id, project_id) REFERENCES projects(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED,
  CHECK ((created_by_kind = 'employee' AND created_by_id IS NOT NULL) OR (created_by_kind <> 'employee' AND created_by_id IS NULL)),
  CHECK ((updated_by_kind = 'employee' AND updated_by_id IS NOT NULL) OR (updated_by_kind <> 'employee' AND updated_by_id IS NULL)),
  FOREIGN KEY (organization_id, created_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, updated_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE tasks (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  id TEXT NOT NULL,
  office_code TEXT NOT NULL,
  number INTEGER NOT NULL CHECK (number > 0),
  key TEXT GENERATED ALWAYS AS ('T-' || office_code || number) VIRTUAL,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
  team_id TEXT NOT NULL,
  assigned_team_id TEXT,
  project_id TEXT,
  type TEXT NOT NULL DEFAULT 'task' CHECK (length(type) > 0),
  work_type TEXT CHECK (work_type IN ('build', 'run')),
  title TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('todo', 'in_progress', 'blocked', 'cancelled', 'done')),
  assignee_id TEXT,
  body_markdown TEXT NOT NULL DEFAULT '',
  feature_id TEXT,
  milestone_id TEXT,
  priority TEXT NOT NULL DEFAULT 'normal' CHECK (priority IN ('urgent', 'high', 'normal', 'low')),
  effort INTEGER CHECK (effort >= 0),
  severity TEXT,
  resolution TEXT,
  blocked_reason TEXT,
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'system', 'unknown')),
  updated_by_id TEXT,
  PRIMARY KEY (organization_id, id),
  CHECK (project_id IS NOT NULL OR (feature_id IS NULL AND milestone_id IS NULL)),
  UNIQUE (organization_id, office_code, number),
  FOREIGN KEY (organization_id, office_code) REFERENCES office_codes(organization_id, code) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, team_id) REFERENCES teams(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, assigned_team_id) REFERENCES teams(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, project_id, team_id) REFERENCES projects(organization_id, id, owner_team_id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, assignee_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, feature_id, project_id) REFERENCES features(organization_id, id, project_id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, milestone_id, project_id) REFERENCES milestones(organization_id, id, project_id) DEFERRABLE INITIALLY DEFERRED,
  CHECK ((created_by_kind = 'employee' AND created_by_id IS NOT NULL) OR (created_by_kind <> 'employee' AND created_by_id IS NULL)),
  CHECK ((updated_by_kind = 'employee' AND updated_by_id IS NOT NULL) OR (updated_by_kind <> 'employee' AND updated_by_id IS NULL)),
  FOREIGN KEY (organization_id, created_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, updated_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE task_repositories (
  organization_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  repository_id TEXT NOT NULL,
  branch TEXT,
  integrated_commit TEXT,
  PRIMARY KEY (organization_id, task_id, repository_id),
  FOREIGN KEY (organization_id, task_id) REFERENCES tasks(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, repository_id) REFERENCES repositories(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE record_refs (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  kind TEXT NOT NULL,
  id TEXT NOT NULL,
  is_deleted INTEGER NOT NULL CHECK (is_deleted IN (0, 1)),
  PRIMARY KEY (organization_id, kind, id)
) STRICT;

CREATE TABLE checklist_steps (
  organization_id TEXT NOT NULL,
  target_kind TEXT NOT NULL CHECK (target_kind = 'task'),
  target_id TEXT NOT NULL,
  id TEXT NOT NULL,
  position INTEGER NOT NULL CHECK (position >= 0),
  text TEXT NOT NULL,
  required INTEGER NOT NULL CHECK (required IN (0, 1)),
  done INTEGER NOT NULL CHECK (done IN (0, 1)),
  linked_task_id TEXT,
  checked_by_employee_id TEXT,
  checked_at TEXT,
  revision INTEGER NOT NULL DEFAULT 1 CHECK (revision>=1),
  FOREIGN KEY (organization_id,linked_task_id) REFERENCES tasks(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,checked_by_employee_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  PRIMARY KEY (organization_id, target_kind, target_id, id),
  UNIQUE (organization_id, target_kind, target_id, position),
  FOREIGN KEY (organization_id, target_kind, target_id) REFERENCES record_refs(organization_id, kind, id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE comments (
  organization_id TEXT NOT NULL,
  target_kind TEXT NOT NULL CHECK (target_kind = 'task'),
  target_id TEXT NOT NULL,
  id TEXT NOT NULL,
  position INTEGER NOT NULL CHECK (position >= 0),
  author_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  body_markdown TEXT NOT NULL,
  PRIMARY KEY (organization_id, target_kind, target_id, id),
  UNIQUE (organization_id, target_kind, target_id, position),
  FOREIGN KEY (organization_id, target_kind, target_id) REFERENCES record_refs(organization_id, kind, id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, author_id) REFERENCES employees(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE links (
  organization_id TEXT NOT NULL,
  source_kind TEXT NOT NULL CHECK (source_kind IN ('project', 'feature', 'task', 'document')),
  source_id TEXT NOT NULL,
  id TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('depends_on', 'relates_to', 'fixes')),
  target_kind TEXT NOT NULL,
  target_id TEXT NOT NULL,
  PRIMARY KEY (organization_id, source_kind, source_id, id),
  UNIQUE (organization_id, source_kind, source_id, kind, target_kind, target_id),
  CHECK (source_kind <> target_kind OR source_id <> target_id),
  CHECK (kind NOT IN ('depends_on','fixes') OR (source_kind = 'task' AND target_kind = 'task')),
  FOREIGN KEY (organization_id, source_kind, source_id) REFERENCES record_refs(organization_id, kind, id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, target_kind, target_id) REFERENCES record_refs(organization_id, kind, id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE tombstones (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  kind TEXT NOT NULL,
  id TEXT NOT NULL,
  key TEXT,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
  team_id TEXT,
  project_id TEXT,
  number INTEGER CHECK (number > 0),
  type TEXT,
  feature_id TEXT,
  milestone_id TEXT,
  terminal_state TEXT NOT NULL,
  resolution TEXT,
  archived_commit TEXT,
  record_metadata_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(record_metadata_json) AND json_type(record_metadata_json) = 'object'),
  PRIMARY KEY (organization_id, kind, id)
) STRICT;

CREATE TABLE employees (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  id TEXT NOT NULL,
  handle TEXT NOT NULL CHECK (length(handle) > 0),
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
kind TEXT NOT NULL CHECK (kind IN ('agent', 'person')),
  name TEXT NOT NULL,
  home_team_id TEXT NOT NULL,
  role_id TEXT,
  reports_to_employee_id TEXT,
  body_markdown TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'paused', 'inactive')),
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'system', 'unknown')),
  updated_by_id TEXT,
  contact TEXT,
  title TEXT,
  focus TEXT,
  role_text TEXT,
  title_in_team TEXT,
  offboarded_at TEXT,
  may_hire INTEGER NOT NULL DEFAULT 0 CHECK (may_hire IN (0, 1)),
  CHECK (kind = 'agent' OR status <> 'paused'),
  CHECK (offboarded_at IS NULL OR status = 'inactive'),
  PRIMARY KEY (organization_id, id),
UNIQUE (organization_id, handle),
  UNIQUE (organization_id, id, home_team_id),
  CHECK (reports_to_employee_id IS NULL OR reports_to_employee_id <> id),
  FOREIGN KEY (organization_id, home_team_id) REFERENCES teams(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, role_id) REFERENCES roles(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, reports_to_employee_id) REFERENCES employees(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED,
  CHECK ((created_by_kind = 'employee' AND created_by_id IS NOT NULL) OR (created_by_kind <> 'employee' AND created_by_id IS NULL)),
  CHECK ((updated_by_kind = 'employee' AND updated_by_id IS NOT NULL) OR (updated_by_kind <> 'employee' AND updated_by_id IS NULL)),
  FOREIGN KEY (organization_id, created_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, updated_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE agent_placements (
  organization_id TEXT NOT NULL,
  employee_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  placement_generation INTEGER NOT NULL DEFAULT 1 CHECK (placement_generation>=1),
  PRIMARY KEY (organization_id, employee_id),
  FOREIGN KEY (organization_id, employee_id) REFERENCES employees(organization_id, id),
  FOREIGN KEY (organization_id, office_id) REFERENCES offices(organization_id, id)
) STRICT;

CREATE TABLE task_workspaces (
  organization_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  employee_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  repository_id TEXT NOT NULL,
  worktree_path TEXT NOT NULL,
  PRIMARY KEY (organization_id, task_id, repository_id),
  FOREIGN KEY (organization_id, employee_id) REFERENCES employees(organization_id, id),
  FOREIGN KEY (organization_id, office_id) REFERENCES offices(organization_id, id),
  FOREIGN KEY (organization_id, task_id, repository_id) REFERENCES task_repositories(organization_id, task_id, repository_id)
) STRICT;

CREATE TABLE hire_requests (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  id TEXT NOT NULL,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
  state TEXT NOT NULL CHECK (state IN ('drafting', 'waiting', 'approved', 'hiring', 'hired', 'rejected', 'withdrawn')),
  requested_by_employee_id TEXT NOT NULL,
  home_team_id TEXT NOT NULL,
  role_id TEXT,
  proposal_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(proposal_json) AND json_type(proposal_json) = 'object'),
  reason TEXT NOT NULL DEFAULT '',
  approver_employee_id TEXT,
  escalations_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(escalations_json) AND json_type(escalations_json) = 'array'),
  decided_by_employee_id TEXT,
  decided_at TEXT,
  decision_reason TEXT,
  employee_id TEXT,
  failed_step TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (organization_id, id),
  UNIQUE (organization_id, employee_id),
  CHECK ((state IN ('approved', 'hiring', 'hired', 'rejected')) = (decided_by_employee_id IS NOT NULL)),
  CHECK ((decided_by_employee_id IS NULL) = (decided_at IS NULL)),
  CHECK (state <> 'rejected' OR (decision_reason IS NOT NULL AND length(decision_reason) > 0)),
  CHECK ((state = 'hired') = (employee_id IS NOT NULL)),
  CHECK (state IN ('approved', 'hiring') OR failed_step IS NULL),
  FOREIGN KEY (organization_id, requested_by_employee_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, home_team_id) REFERENCES teams(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, role_id) REFERENCES roles(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, approver_employee_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, decided_by_employee_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, employee_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE watchers (
  organization_id TEXT NOT NULL,
  target_kind TEXT NOT NULL CHECK (target_kind = 'task'),
  target_id TEXT NOT NULL,
  id TEXT NOT NULL,
  watcher_kind TEXT NOT NULL CHECK (watcher_kind IN ('employee', 'owner')),
  watcher_id TEXT,
  PRIMARY KEY (organization_id, target_kind, target_id, id),
  CHECK ((watcher_kind = 'employee' AND watcher_id IS NOT NULL) OR (watcher_kind = 'owner' AND watcher_id IS NULL)),
  FOREIGN KEY (organization_id, target_kind, target_id) REFERENCES record_refs(organization_id, kind, id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, watcher_id) REFERENCES employees(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE owner_questions (
  organization_id TEXT NOT NULL,
  target_kind TEXT NOT NULL CHECK (target_kind = 'task'),
  target_id TEXT NOT NULL,
  id TEXT NOT NULL,
  question_comment_id TEXT NOT NULL,
  asked_by_id TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('open', 'answered', 'cancelled')),
  answer_comment_id TEXT,
  created_at TEXT NOT NULL,
  answered_at TEXT,
  PRIMARY KEY (organization_id, target_kind, target_id, id),
  CHECK ((state = 'answered' AND answer_comment_id IS NOT NULL AND answered_at IS NOT NULL) OR
    (state <> 'answered' AND answer_comment_id IS NULL AND answered_at IS NULL)),
  FOREIGN KEY (organization_id, target_kind, target_id, question_comment_id) REFERENCES comments(organization_id, target_kind, target_id, id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, target_kind, target_id, answer_comment_id) REFERENCES comments(organization_id, target_kind, target_id, id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, asked_by_id) REFERENCES employees(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE waiting_spans (
  organization_id TEXT NOT NULL,
  target_kind TEXT NOT NULL CHECK (target_kind = 'task'),
  target_id TEXT NOT NULL,
  id TEXT NOT NULL,
  reason TEXT NOT NULL,
  waiting_on_kind TEXT NOT NULL CHECK (waiting_on_kind IN ('owner', 'employee')),
  waiting_on_id TEXT,
  question_id TEXT,
  started_at TEXT NOT NULL,
  ended_at TEXT,
  PRIMARY KEY (organization_id, target_kind, target_id, id),
  CHECK ((waiting_on_kind = 'employee' AND waiting_on_id IS NOT NULL) OR (waiting_on_kind = 'owner' AND waiting_on_id IS NULL)),
  FOREIGN KEY (organization_id, target_kind, target_id) REFERENCES record_refs(organization_id, kind, id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, waiting_on_id) REFERENCES employees(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, target_kind, target_id, question_id) REFERENCES owner_questions(organization_id, target_kind, target_id, id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE reference_counters (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  scope_kind TEXT NOT NULL,
  scope_id TEXT NOT NULL,
  entity_kind TEXT NOT NULL,
  last_number INTEGER NOT NULL CHECK (last_number >= 0),
  PRIMARY KEY (organization_id, scope_kind, scope_id, entity_kind),
  CHECK ((scope_kind = 'organization' AND scope_id = organization_id AND entity_kind IN ('team', 'project'))
    OR (scope_kind = 'project' AND entity_kind IN ('feature', 'release'))
    OR (scope_kind = 'release' AND entity_kind = 'milestone')),
  FOREIGN KEY (organization_id, scope_kind, scope_id) REFERENCES record_refs(organization_id, kind, id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE reference_allocations (
  organization_id TEXT NOT NULL,
  scope_kind TEXT NOT NULL,
  scope_id TEXT NOT NULL,
  entity_kind TEXT NOT NULL,
  number INTEGER NOT NULL CHECK (number > 0),
  entity_id TEXT NOT NULL,
  allocated_at TEXT NOT NULL,
  PRIMARY KEY (organization_id, scope_kind, scope_id, entity_kind, number),
  UNIQUE (organization_id, scope_kind, scope_id, entity_kind, entity_id),
  FOREIGN KEY (organization_id, scope_kind, scope_id, entity_kind)
    REFERENCES reference_counters(organization_id, scope_kind, scope_id, entity_kind)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, entity_kind, entity_id) REFERENCES record_refs(organization_id, kind, id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE office_codes (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  code TEXT NOT NULL CHECK (code GLOB '[A-HJ-NP-Z]' OR code GLOB '[A-HJ-NP-Z][A-HJ-NP-Z]'),
  office_id TEXT NOT NULL,
  issued_at TEXT NOT NULL,
  retired_at TEXT,
  PRIMARY KEY (organization_id, code),
  FOREIGN KEY (organization_id, office_id) REFERENCES offices(organization_id, id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE UNIQUE INDEX office_current_code ON office_codes(organization_id, office_id) WHERE retired_at IS NULL;

CREATE TABLE office_task_counters (
  organization_id TEXT NOT NULL,
  office_code TEXT NOT NULL,
  last_number INTEGER NOT NULL CHECK (last_number >= 0),
  reserved_through INTEGER NOT NULL,
  PRIMARY KEY (organization_id, office_code),
  CHECK (reserved_through >= last_number),
  FOREIGN KEY (organization_id, office_code) REFERENCES office_codes(organization_id, code) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE deployments (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  id TEXT NOT NULL,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
  project_id TEXT NOT NULL,
  release_id TEXT NOT NULL,
  environment TEXT NOT NULL CHECK (length(environment) > 0),
  target TEXT NOT NULL DEFAULT 'primary' CHECK (length(target) > 0),
  operation TEXT NOT NULL DEFAULT 'deploy' CHECK (operation IN ('deploy', 'rollback')),
  rollback_of_id TEXT,
  baseline_deployment_id TEXT,
  status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
  started_at TEXT,
  finished_at TEXT,
  external_url TEXT,
  body_markdown TEXT NOT NULL DEFAULT '',
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'system', 'unknown')),
  updated_by_id TEXT,
  PRIMARY KEY (organization_id, id),
  UNIQUE (organization_id, id, project_id),
  UNIQUE (organization_id, id, project_id, environment, target),
  CHECK (rollback_of_id IS NULL OR rollback_of_id <> id),
  CHECK (baseline_deployment_id IS NULL OR baseline_deployment_id <> id),
  FOREIGN KEY (organization_id, baseline_deployment_id, project_id, environment, target)
    REFERENCES deployments(organization_id, id, project_id, environment, target)
    DEFERRABLE INITIALLY DEFERRED,
  CHECK ((operation = 'deploy' AND rollback_of_id IS NULL) OR (operation = 'rollback' AND rollback_of_id IS NOT NULL)),
  CHECK ((status = 'queued' AND started_at IS NULL AND finished_at IS NULL)
    OR (status = 'running' AND started_at IS NOT NULL AND finished_at IS NULL)
    OR (status IN ('succeeded', 'failed') AND started_at IS NOT NULL AND finished_at IS NOT NULL)
    OR (status = 'cancelled' AND finished_at IS NOT NULL)),
  CHECK (finished_at IS NULL OR started_at IS NULL OR finished_at >= started_at),
  FOREIGN KEY (organization_id, release_id, project_id) REFERENCES releases(organization_id, id, project_id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, rollback_of_id, project_id, environment, target)
    REFERENCES deployments(organization_id, id, project_id, environment, target)
    DEFERRABLE INITIALLY DEFERRED,
  CHECK ((created_by_kind = 'employee' AND created_by_id IS NOT NULL) OR (created_by_kind <> 'employee' AND created_by_id IS NULL)),
  CHECK ((updated_by_kind = 'employee' AND updated_by_id IS NOT NULL) OR (updated_by_kind <> 'employee' AND updated_by_id IS NULL)),
  FOREIGN KEY (organization_id, created_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, updated_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE deployment_repository_commits (
  organization_id TEXT NOT NULL,
  deployment_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  repository_id TEXT NOT NULL,
  commit_sha TEXT NOT NULL CHECK (length(commit_sha) > 0),
  PRIMARY KEY (organization_id, deployment_id, repository_id),
  FOREIGN KEY (organization_id, deployment_id, project_id) REFERENCES deployments(organization_id, id, project_id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, repository_id) REFERENCES repositories(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE release_features (
  organization_id TEXT NOT NULL,
  release_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  feature_id TEXT NOT NULL,
  PRIMARY KEY (organization_id, release_id, feature_id),
  FOREIGN KEY (organization_id, release_id, project_id) REFERENCES releases(organization_id, id, project_id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, feature_id, project_id) REFERENCES features(organization_id, id, project_id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE release_tasks (
  organization_id TEXT NOT NULL,
  release_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  task_kind TEXT NOT NULL DEFAULT 'task' CHECK (task_kind = 'task'),
  PRIMARY KEY (organization_id, release_id, task_id),
  FOREIGN KEY (organization_id, release_id, project_id) REFERENCES releases(organization_id, id, project_id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, project_id) REFERENCES projects(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, task_kind, task_id) REFERENCES record_refs(organization_id, kind, id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE feature_flags (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  id TEXT NOT NULL,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
  project_id TEXT NOT NULL,
  feature_id TEXT,
  key TEXT NOT NULL CHECK (length(key) > 0),
  owner_employee_id TEXT,
  status TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'active', 'retired')),
  value_type TEXT NOT NULL CHECK (value_type IN ('boolean', 'string', 'number', 'json')),
  default_value_json TEXT NOT NULL CHECK (json_valid(default_value_json)),
  body_markdown TEXT NOT NULL DEFAULT '',
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'system', 'unknown')),
  updated_by_id TEXT,
  PRIMARY KEY (organization_id, id),
  UNIQUE (organization_id, project_id, key),
  UNIQUE (organization_id, id, project_id),
  CHECK (status <> 'active' OR owner_employee_id IS NOT NULL),
  CHECK ((value_type = 'boolean' AND json_type(default_value_json) IN ('true', 'false'))
    OR (value_type = 'string' AND json_type(default_value_json) = 'text')
    OR (value_type = 'number' AND json_type(default_value_json) IN ('integer', 'real'))
    OR value_type = 'json'),
  FOREIGN KEY (organization_id, project_id) REFERENCES projects(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, feature_id, project_id) REFERENCES features(organization_id, id, project_id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, owner_employee_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  CHECK ((created_by_kind = 'employee' AND created_by_id IS NOT NULL) OR (created_by_kind <> 'employee' AND created_by_id IS NULL)),
  CHECK ((updated_by_kind = 'employee' AND updated_by_id IS NOT NULL) OR (updated_by_kind <> 'employee' AND updated_by_id IS NULL)),
  FOREIGN KEY (organization_id, created_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, updated_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE release_flag_settings (
  organization_id TEXT NOT NULL,
  release_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  flag_id TEXT NOT NULL,
  environment TEXT NOT NULL CHECK (length(environment) > 0),
  target TEXT NOT NULL DEFAULT 'primary' CHECK (length(target) > 0),
  value_json TEXT NOT NULL CHECK (json_valid(value_json)),
  rollout_rules_json TEXT CHECK (rollout_rules_json IS NULL OR (json_valid(rollout_rules_json) AND json_type(rollout_rules_json) = 'object')),
  PRIMARY KEY (organization_id, release_id, environment, target, flag_id),
  FOREIGN KEY (organization_id, release_id, project_id) REFERENCES releases(organization_id, id, project_id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, flag_id, project_id) REFERENCES feature_flags(organization_id, id, project_id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE deployment_flag_settings (
  organization_id TEXT NOT NULL,
  deployment_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  flag_id TEXT NOT NULL,
  value_json TEXT NOT NULL CHECK (json_valid(value_json)),
  rollout_rules_json TEXT CHECK (rollout_rules_json IS NULL OR (json_valid(rollout_rules_json) AND json_type(rollout_rules_json) = 'object')),
  applied_at TEXT,
  PRIMARY KEY (organization_id, deployment_id, flag_id),
  FOREIGN KEY (organization_id, deployment_id, project_id) REFERENCES deployments(organization_id, id, project_id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, flag_id, project_id) REFERENCES feature_flags(organization_id, id, project_id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE model_prices (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  id TEXT NOT NULL,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  effective_from TEXT NOT NULL,
  currency TEXT NOT NULL DEFAULT 'USD' CHECK (currency = 'USD'),
  input_micro_usd_per_million INTEGER CHECK (input_micro_usd_per_million >= 0),
  output_micro_usd_per_million INTEGER CHECK (output_micro_usd_per_million >= 0),
  cache_read_micro_usd_per_million INTEGER CHECK (cache_read_micro_usd_per_million >= 0),
  cache_write_micro_usd_per_million INTEGER CHECK (cache_write_micro_usd_per_million >= 0),
  created_at TEXT NOT NULL,
  created_by_kind TEXT NOT NULL CHECK (created_by_kind IN ('employee', 'system', 'unknown')),
  created_by_id TEXT,
  PRIMARY KEY (organization_id, id),
  UNIQUE (organization_id, provider, model, effective_from),
  UNIQUE (organization_id, id, provider, model),
  CHECK ((created_by_kind = 'employee' AND created_by_id IS NOT NULL) OR (created_by_kind <> 'employee' AND created_by_id IS NULL)),
  FOREIGN KEY (organization_id, created_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE usage_records (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  id TEXT NOT NULL,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  origin_office_id TEXT NOT NULL,
  producer_id TEXT NOT NULL,
  source_event_id TEXT NOT NULL,
  source_sequence INTEGER NOT NULL CHECK (source_sequence >= 1),
  execution_office_id TEXT,
  run_id TEXT,
  actor_kind TEXT NOT NULL CHECK (actor_kind IN ('agent', 'one_off')),
  employee_id TEXT,
  runtime TEXT NOT NULL,
  provider TEXT,
  model TEXT,
  occurred_at TEXT NOT NULL,
  recorded_at TEXT NOT NULL,
  recorded_by_kind TEXT NOT NULL CHECK (recorded_by_kind IN ('employee', 'system', 'unknown')),
  recorded_by_id TEXT,
  normalization_version INTEGER NOT NULL CHECK (normalization_version >= 1),
  source_metadata_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(source_metadata_json) AND json_type(source_metadata_json) = 'object'),
  payload_sha256 TEXT NOT NULL CHECK (length(payload_sha256) = 64 AND payload_sha256 NOT GLOB '*[^0-9a-f]*'),
  input_tokens INTEGER CHECK (input_tokens >= 0),
  output_tokens INTEGER CHECK (output_tokens >= 0),
  cache_read_tokens INTEGER CHECK (cache_read_tokens >= 0),
  cache_write_tokens INTEGER CHECK (cache_write_tokens >= 0),
  reasoning_tokens INTEGER CHECK (reasoning_tokens >= 0),
  token_status TEXT NOT NULL CHECK (token_status IN ('complete', 'partial', 'unavailable')),
  token_issue TEXT,
  price_id TEXT,
  cost_micro_usd INTEGER CHECK (cost_micro_usd >= 0),
  cost_basis TEXT NOT NULL CHECK (cost_basis IN ('runtime_estimate', 'token_estimate', 'unpriced', 'unknown')),
  cost_status TEXT NOT NULL CHECK (cost_status IN ('complete', 'partial', 'unavailable')),
  cost_issue TEXT,
  scope_kind TEXT NOT NULL CHECK (scope_kind IN ('task', 'project', 'team', 'organization')),
  task_id TEXT,
  task_kind TEXT NOT NULL DEFAULT 'task' CHECK (task_kind = 'task'),
  project_id TEXT,
  team_id TEXT,
  feature_id TEXT,
  milestone_id TEXT,
  release_id TEXT,
  task_type_snapshot TEXT,
  work_type TEXT CHECK (work_type IN ('build', 'run')),
  attribution_basis TEXT NOT NULL CHECK (attribution_basis IN ('work_for', 'current_task', 'explicit_context', 'unattributed', 'manual_correction')),
  supersedes_id TEXT,
  correction_kind TEXT CHECK (correction_kind IN ('usage', 'attribution')),
  correction_reason TEXT,
  PRIMARY KEY (organization_id, id),
  UNIQUE (organization_id, origin_office_id, producer_id, source_event_id),
  UNIQUE (organization_id, origin_office_id, producer_id, source_sequence),
  UNIQUE (organization_id, supersedes_id),
  CHECK ((actor_kind = 'agent' AND employee_id IS NOT NULL) OR (actor_kind = 'one_off' AND employee_id IS NULL)),
  CHECK ((recorded_by_kind = 'employee' AND recorded_by_id IS NOT NULL) OR (recorded_by_kind <> 'employee' AND recorded_by_id IS NULL)),
  CHECK (reasoning_tokens IS NULL OR output_tokens IS NULL OR reasoning_tokens <= output_tokens),
  CHECK (token_status <> 'complete' OR (input_tokens IS NOT NULL AND output_tokens IS NOT NULL AND cache_read_tokens IS NOT NULL AND cache_write_tokens IS NOT NULL)),
  CHECK (token_status <> 'unavailable' OR (input_tokens IS NULL AND output_tokens IS NULL AND cache_read_tokens IS NULL AND cache_write_tokens IS NULL AND reasoning_tokens IS NULL)),
  CHECK (token_status = 'complete' OR (token_issue IS NOT NULL AND length(token_issue) > 0)),
  CHECK ((cost_basis IN ('runtime_estimate', 'token_estimate') AND cost_micro_usd IS NOT NULL AND cost_status IN ('complete', 'partial'))
    OR (cost_basis IN ('unpriced', 'unknown') AND cost_micro_usd IS NULL AND cost_status = 'unavailable')),
  CHECK (cost_status = 'complete' OR (cost_issue IS NOT NULL AND length(cost_issue) > 0)),
  CHECK (cost_basis <> 'token_estimate' OR (price_id IS NOT NULL AND token_status <> 'unavailable')),
  CHECK (price_id IS NULL OR (provider IS NOT NULL AND model IS NOT NULL)),
  CHECK ((scope_kind = 'task' AND task_id IS NOT NULL)
    OR (scope_kind = 'project' AND task_id IS NULL AND project_id IS NOT NULL)
    OR (scope_kind = 'team' AND task_id IS NULL AND project_id IS NULL AND team_id IS NOT NULL)
    OR (scope_kind = 'organization' AND task_id IS NULL AND project_id IS NULL AND team_id IS NULL)),
  CHECK (project_id IS NOT NULL OR (feature_id IS NULL AND milestone_id IS NULL AND release_id IS NULL)),
  CHECK ((supersedes_id IS NULL AND correction_kind IS NULL AND correction_reason IS NULL)
    OR (supersedes_id IS NOT NULL AND supersedes_id <> id AND correction_kind IS NOT NULL AND correction_reason IS NOT NULL AND length(correction_reason) > 0)),
  FOREIGN KEY (organization_id, origin_office_id) REFERENCES offices(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, execution_office_id) REFERENCES offices(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, employee_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, recorded_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, price_id, provider, model) REFERENCES model_prices(organization_id, id, provider, model) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, task_kind, task_id) REFERENCES record_refs(organization_id, kind, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, project_id) REFERENCES projects(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, team_id) REFERENCES teams(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, feature_id) REFERENCES features(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, milestone_id) REFERENCES milestones(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, release_id) REFERENCES releases(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, supersedes_id) REFERENCES usage_records(organization_id, id)
) STRICT;

CREATE TABLE usage_stream_checkpoints (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  origin_office_id TEXT NOT NULL,
  producer_id TEXT NOT NULL,
  high_water_sequence INTEGER NOT NULL CHECK (high_water_sequence >= 0),
  head_sha256 TEXT,
  state TEXT NOT NULL CHECK (state IN ('open', 'sealed')),
  checkpointed_at TEXT NOT NULL,
  PRIMARY KEY (organization_id, origin_office_id, producer_id),
  CHECK ((high_water_sequence = 0 AND head_sha256 IS NULL)
    OR (high_water_sequence > 0 AND head_sha256 IS NOT NULL AND length(head_sha256) = 64 AND head_sha256 NOT GLOB '*[^0-9a-f]*')),
  FOREIGN KEY (organization_id, origin_office_id) REFERENCES offices(organization_id, id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE VIEW feature_slugs AS
SELECT f.organization_id, f.id AS feature_id, f.project_id,
       p.slug || '/' || f.local_slug AS slug
FROM features AS f
JOIN projects AS p ON p.organization_id = f.organization_id AND p.id = f.project_id;

CREATE VIEW entity_relationships AS
SELECT organization_id, source_kind, source_id, kind, target_kind, target_id FROM links
UNION ALL
SELECT organization_id, target_kind, target_id, 'blocks', source_kind, source_id
FROM links WHERE kind = 'depends_on'
UNION ALL
SELECT organization_id, 'task', id, 'implements', 'feature', feature_id
FROM tasks WHERE feature_id IS NOT NULL
UNION ALL
SELECT organization_id, 'document', id, 'documents',
       CASE WHEN feature_id IS NOT NULL THEN 'feature' WHEN release_id IS NOT NULL THEN 'release' WHEN deployment_id IS NOT NULL THEN 'deployment' ELSE 'project' END,
       CAST(COALESCE(feature_id, release_id, deployment_id, project_id) AS TEXT)
FROM documents;

CREATE VIEW usage_stream_coverage AS
SELECT k.organization_id, k.origin_office_id, k.producer_id,
       c.high_water_sequence, c.checkpointed_at, c.state,
       CASE WHEN c.producer_id IS NULL THEN 1 ELSE 0 END AS checkpoint_missing,
       SUM(CASE WHEN u.source_sequence <= c.high_water_sequence THEN 1 ELSE 0 END) AS received_events,
       CASE WHEN c.producer_id IS NULL THEN NULL ELSE c.high_water_sequence - SUM(CASE WHEN u.source_sequence <= c.high_water_sequence THEN 1 ELSE 0 END) END AS missing_events,
       CASE WHEN c.high_water_sequence = 0 THEN 1
         ELSE MAX(CASE WHEN u.source_sequence = c.high_water_sequence AND u.payload_sha256 = c.head_sha256 THEN 1 ELSE 0 END) END AS head_matches,
       CASE WHEN c.producer_id IS NOT NULL
         AND c.high_water_sequence = SUM(CASE WHEN u.source_sequence <= c.high_water_sequence THEN 1 ELSE 0 END)
         AND SUM(CASE WHEN u.source_sequence > c.high_water_sequence THEN 1 ELSE 0 END) = 0
         AND (c.high_water_sequence = 0 OR MAX(CASE WHEN u.source_sequence = c.high_water_sequence AND u.payload_sha256 = c.head_sha256 THEN 1 ELSE 0 END) = 1)
         THEN 1 ELSE 0 END AS coverage_complete
FROM (SELECT organization_id, origin_office_id, producer_id FROM usage_records
      UNION SELECT organization_id, origin_office_id, producer_id FROM usage_stream_checkpoints) k
LEFT JOIN usage_stream_checkpoints c
  ON c.organization_id=k.organization_id AND c.origin_office_id=k.origin_office_id AND c.producer_id=k.producer_id
LEFT JOIN usage_records u
  ON u.organization_id=k.organization_id AND u.origin_office_id=k.origin_office_id AND u.producer_id=k.producer_id
GROUP BY k.organization_id, k.origin_office_id, k.producer_id;

CREATE VIEW effective_usage_records AS
SELECT u.* FROM usage_records u
WHERE NOT EXISTS (SELECT 1 FROM usage_records replacement
  WHERE replacement.organization_id = u.organization_id AND replacement.supersedes_id = u.id);

CREATE VIEW usage_task_totals AS
SELECT organization_id, task_id, COUNT(*) AS usage_count,
       SUM(input_tokens) AS input_tokens, SUM(output_tokens) AS output_tokens,
       SUM(cache_read_tokens) AS cache_read_tokens, SUM(cache_write_tokens) AS cache_write_tokens,
       SUM(reasoning_tokens) AS reasoning_tokens,
       SUM(CASE WHEN input_tokens IS NULL AND output_tokens IS NULL AND cache_read_tokens IS NULL AND cache_write_tokens IS NULL THEN NULL
         ELSE COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0) + COALESCE(cache_read_tokens, 0) + COALESCE(cache_write_tokens, 0) END) AS known_tokens,
       SUM(CASE WHEN token_status <> 'complete' THEN 1 ELSE 0 END) AS token_incomplete_records,
       SUM(cost_micro_usd) AS known_cost_micro_usd,
       SUM(CASE WHEN cost_status <> 'complete' THEN 1 ELSE 0 END) AS cost_incomplete_records,
       SUM(CASE WHEN cost_micro_usd IS NULL THEN 1 ELSE 0 END) AS unpriced_records,
       (SELECT COUNT(*) FROM usage_stream_coverage c WHERE c.organization_id=e.organization_id AND c.coverage_complete=0) AS ledger_incomplete_streams
FROM effective_usage_records e WHERE task_id IS NOT NULL
GROUP BY organization_id, task_id;

CREATE VIEW usage_agent_totals AS
SELECT organization_id, employee_id, COUNT(*) AS usage_count,
       SUM(input_tokens) AS input_tokens, SUM(output_tokens) AS output_tokens,
       SUM(cache_read_tokens) AS cache_read_tokens, SUM(cache_write_tokens) AS cache_write_tokens,
       SUM(reasoning_tokens) AS reasoning_tokens,
       SUM(CASE WHEN input_tokens IS NULL AND output_tokens IS NULL AND cache_read_tokens IS NULL AND cache_write_tokens IS NULL THEN NULL
         ELSE COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0) + COALESCE(cache_read_tokens, 0) + COALESCE(cache_write_tokens, 0) END) AS known_tokens,
       SUM(CASE WHEN token_status <> 'complete' THEN 1 ELSE 0 END) AS token_incomplete_records,
       SUM(cost_micro_usd) AS known_cost_micro_usd,
       SUM(CASE WHEN cost_status <> 'complete' THEN 1 ELSE 0 END) AS cost_incomplete_records,
       SUM(CASE WHEN cost_micro_usd IS NULL THEN 1 ELSE 0 END) AS unpriced_records,
       (SELECT COUNT(*) FROM usage_stream_coverage c WHERE c.organization_id=e.organization_id AND c.coverage_complete=0) AS ledger_incomplete_streams
FROM effective_usage_records e WHERE employee_id IS NOT NULL
GROUP BY organization_id, employee_id;

CREATE VIEW usage_project_totals AS
SELECT organization_id, project_id, COUNT(*) AS usage_count,
       SUM(input_tokens) AS input_tokens, SUM(output_tokens) AS output_tokens,
       SUM(cache_read_tokens) AS cache_read_tokens, SUM(cache_write_tokens) AS cache_write_tokens,
       SUM(reasoning_tokens) AS reasoning_tokens,
       SUM(CASE WHEN input_tokens IS NULL AND output_tokens IS NULL AND cache_read_tokens IS NULL AND cache_write_tokens IS NULL THEN NULL
         ELSE COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0) + COALESCE(cache_read_tokens, 0) + COALESCE(cache_write_tokens, 0) END) AS known_tokens,
       SUM(CASE WHEN token_status <> 'complete' THEN 1 ELSE 0 END) AS token_incomplete_records,
       SUM(cost_micro_usd) AS known_cost_micro_usd,
       SUM(CASE WHEN cost_status <> 'complete' THEN 1 ELSE 0 END) AS cost_incomplete_records,
       SUM(CASE WHEN cost_micro_usd IS NULL THEN 1 ELSE 0 END) AS unpriced_records,
       (SELECT COUNT(*) FROM usage_stream_coverage c WHERE c.organization_id=e.organization_id AND c.coverage_complete=0) AS ledger_incomplete_streams
FROM effective_usage_records e WHERE project_id IS NOT NULL
GROUP BY organization_id, project_id;

CREATE TABLE task_events (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  task_kind TEXT NOT NULL DEFAULT 'task' CHECK (task_kind = 'task'),
  task_id TEXT NOT NULL,
  revision INTEGER NOT NULL CHECK (revision >= 1),
  schema_version INTEGER NOT NULL CHECK (schema_version = 2),
  occurred_at TEXT,
  actor_kind TEXT NOT NULL CHECK (actor_kind IN ('employee', 'system', 'unknown')),
  actor_id TEXT,
  operation_id TEXT,
  change_json TEXT NOT NULL CHECK (
    json_valid(change_json) AND json_type(change_json) = 'object'
    AND json_type(change_json, '$.type') IS 'text'
    AND length(json_extract(change_json, '$.type')) > 0),
  source_metadata_json TEXT NOT NULL DEFAULT '{}' CHECK (
    json_valid(source_metadata_json) AND json_type(source_metadata_json) = 'object'),
  payload_sha256 TEXT NOT NULL CHECK (length(payload_sha256) = 64 AND payload_sha256 NOT GLOB '*[^0-9a-f]*'),
  resource_profile TEXT NOT NULL CHECK (resource_profile = 'selected-task-resources/1'),
  event_bytes BLOB NOT NULL,
  PRIMARY KEY (organization_id, task_id, revision),
  CHECK ((actor_kind = 'employee' AND actor_id IS NOT NULL) OR (actor_kind <> 'employee' AND actor_id IS NULL)),
  CHECK (json_extract(change_json, '$.type') <> 'assignment_changed' OR (
    json_type(change_json, '$.from') IS 'object' AND json_type(change_json, '$.to') IS 'object'
    AND COALESCE(json_type(change_json, '$.from.assigned_team_id') IN ('text', 'null'), 0)
    AND COALESCE(json_type(change_json, '$.from.assignee_id') IN ('text', 'null'), 0)
    AND COALESCE(json_type(change_json, '$.to.assigned_team_id') IN ('text', 'null'), 0)
    AND COALESCE(json_type(change_json, '$.to.assignee_id') IN ('text', 'null'), 0))),
  FOREIGN KEY (organization_id, task_kind, task_id) REFERENCES record_refs(organization_id, kind, id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, actor_id) REFERENCES employees(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TRIGGER task_events_no_replace BEFORE INSERT ON task_events
WHEN EXISTS (SELECT 1 FROM task_events e WHERE e.organization_id = NEW.organization_id
  AND e.task_id = NEW.task_id AND e.revision = NEW.revision)
BEGIN SELECT RAISE(ABORT, 'task events are immutable; compare identical retries before insertion'); END;
CREATE TRIGGER task_events_no_update BEFORE UPDATE ON task_events
BEGIN SELECT RAISE(ABORT, 'task events are immutable; append a new event'); END;
CREATE TRIGGER task_events_no_delete BEFORE DELETE ON task_events
BEGIN SELECT RAISE(ABORT, 'task history must be retained'); END;

CREATE INDEX documents_by_release ON documents(organization_id, release_id, kind);
CREATE INDEX documents_by_deployment ON documents(organization_id, deployment_id, kind);
CREATE UNIQUE INDEX one_release_changelog ON documents(organization_id, release_id) WHERE kind = 'changelog' AND release_id IS NOT NULL;
CREATE UNIQUE INDEX one_deployment_changelog ON documents(organization_id, deployment_id) WHERE kind = 'changelog' AND deployment_id IS NOT NULL;
CREATE INDEX release_features_by_feature ON release_features(organization_id, feature_id);
CREATE INDEX release_tasks_by_task ON release_tasks(organization_id, task_id);
CREATE INDEX feature_flags_by_feature ON feature_flags(organization_id, feature_id);
CREATE INDEX features_by_owner ON features(organization_id, owner_employee_id, status);
CREATE INDEX documents_by_owner ON documents(organization_id, owner_employee_id, status);
CREATE INDEX links_by_target ON links(organization_id, target_kind, target_id, kind);
CREATE INDEX deployments_by_release ON deployments(organization_id, release_id, created_at);
CREATE INDEX deployments_by_target ON deployments(organization_id, project_id, environment, target, started_at);
CREATE INDEX tasks_by_team_status ON tasks(organization_id, team_id, status);
CREATE UNIQUE INDEX one_hq_per_org ON offices(organization_id) WHERE role = 'hq';
CREATE INDEX teams_by_parent ON teams(organization_id, parent_team_id);
CREATE INDEX employees_by_team ON employees(organization_id, home_team_id);
CREATE INDEX projects_by_owner ON projects(organization_id, owner_team_id);
CREATE INDEX tasks_by_project_status ON tasks(organization_id, project_id, status);
CREATE INDEX tasks_by_assignee ON tasks(organization_id, assignee_id, status);
CREATE INDEX tasks_by_assigned_team ON tasks(organization_id, assigned_team_id, status);
CREATE INDEX tasks_by_milestone ON tasks(organization_id, milestone_id);
CREATE INDEX documents_by_scope ON documents(organization_id, project_id, feature_id, kind);
CREATE INDEX milestones_by_release ON milestones(organization_id, release_id);
CREATE INDEX project_repositories_by_repo ON project_repositories(organization_id, repository_id);
CREATE UNIQUE INDEX unique_employee_watcher ON watchers(organization_id, target_kind, target_id, watcher_id) WHERE watcher_kind = 'employee';
CREATE UNIQUE INDEX unique_owner_watcher ON watchers(organization_id, target_kind, target_id) WHERE watcher_kind = 'owner';
CREATE INDEX open_questions ON owner_questions(organization_id, target_kind, target_id) WHERE state = 'open';
CREATE INDEX open_waits ON waiting_spans(organization_id, target_kind, target_id) WHERE ended_at IS NULL;
CREATE TRIGGER organization_repository_must_be_remote_insert BEFORE INSERT ON repositories
WHEN NEW.kind <> 'remote'
BEGIN
  SELECT RAISE(ABORT, 'organization repository must be remote') WHERE EXISTS (
    SELECT 1 FROM organizations WHERE id = NEW.organization_id AND organization_repository_id = NEW.id
  );
END;
CREATE TRIGGER organization_repository_must_be_remote_update BEFORE UPDATE OF kind ON repositories
WHEN NEW.kind <> 'remote'
BEGIN
  SELECT RAISE(ABORT, 'organization repository must be remote') WHERE EXISTS (
    SELECT 1 FROM organizations WHERE id = NEW.organization_id AND organization_repository_id = NEW.id
  );
END;
CREATE TRIGGER organization_remote_reference_insert BEFORE INSERT ON organizations
BEGIN
  SELECT RAISE(ABORT, 'organization repository must be remote') WHERE EXISTS (
    SELECT 1 FROM repositories WHERE organization_id = NEW.id AND id = NEW.organization_repository_id AND kind <> 'remote'
  );
END;
CREATE TRIGGER organization_remote_reference_update BEFORE UPDATE OF organization_repository_id ON organizations
BEGIN
  SELECT RAISE(ABORT, 'organization repository must be remote') WHERE EXISTS (
    SELECT 1 FROM repositories WHERE organization_id = NEW.id AND id = NEW.organization_repository_id AND kind <> 'remote'
  );
END;
CREATE TRIGGER teams_no_cycle_insert BEFORE INSERT ON teams
WHEN NEW.parent_team_id IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'team cycle') WHERE EXISTS (
    WITH RECURSIVE ancestors(id) AS (
      SELECT NEW.parent_team_id UNION
      SELECT t.parent_team_id FROM teams t JOIN ancestors a ON t.id = a.id
      WHERE t.organization_id = NEW.organization_id AND t.parent_team_id IS NOT NULL
    ) SELECT 1 FROM ancestors WHERE id = NEW.id
  );
END;
CREATE TRIGGER teams_no_cycle_update BEFORE UPDATE OF parent_team_id ON teams
WHEN NEW.parent_team_id IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'team cycle') WHERE EXISTS (
    WITH RECURSIVE ancestors(id) AS (
      SELECT NEW.parent_team_id UNION
      SELECT t.parent_team_id FROM teams t JOIN ancestors a ON t.id = a.id
      WHERE t.organization_id = NEW.organization_id AND t.parent_team_id IS NOT NULL
    ) SELECT 1 FROM ancestors WHERE id = NEW.id
  );
END;
CREATE TRIGGER links_no_dependency_cycle_insert BEFORE INSERT ON links
WHEN NEW.kind = 'depends_on'
BEGIN
  SELECT RAISE(ABORT, 'task dependency cycle') WHERE EXISTS (
    WITH RECURSIVE prerequisites(id) AS (
      SELECT NEW.target_id UNION
      SELECT l.target_id FROM links l JOIN prerequisites p ON l.source_id = p.id
      WHERE l.organization_id = NEW.organization_id AND l.kind = 'depends_on'
        AND l.source_kind = 'task' AND l.target_kind = 'task'
    ) SELECT 1 FROM prerequisites WHERE id = NEW.source_id
  );
END;
CREATE TRIGGER links_no_dependency_cycle_update BEFORE UPDATE ON links
WHEN NEW.kind = 'depends_on'
BEGIN
  SELECT RAISE(ABORT, 'task dependency cycle') WHERE EXISTS (
    WITH RECURSIVE prerequisites(id) AS (
      SELECT NEW.target_id UNION
      SELECT l.target_id FROM links l JOIN prerequisites p ON l.source_id = p.id
      WHERE l.organization_id = NEW.organization_id AND l.kind = 'depends_on'
        AND l.source_kind = 'task' AND l.target_kind = 'task' AND NOT (l.source_kind = OLD.source_kind AND l.source_id = OLD.source_id AND l.id = OLD.id)
    ) SELECT 1 FROM prerequisites WHERE id = NEW.source_id
  );
END;
CREATE TRIGGER release_flag_settings_value_type_insert BEFORE INSERT ON release_flag_settings
BEGIN
  SELECT RAISE(ABORT, 'flag value type mismatch') WHERE EXISTS (
    SELECT 1 FROM feature_flags f WHERE f.organization_id = NEW.organization_id AND f.id = NEW.flag_id
      AND NOT ((f.value_type = 'boolean' AND json_type(NEW.value_json) IN ('true', 'false'))
    OR (f.value_type = 'string' AND json_type(NEW.value_json) = 'text')
    OR (f.value_type = 'number' AND json_type(NEW.value_json) IN ('integer', 'real'))
    OR f.value_type = 'json')
  );
END;
CREATE TRIGGER release_flag_settings_value_type_update BEFORE UPDATE ON release_flag_settings
BEGIN
  SELECT RAISE(ABORT, 'flag value type mismatch') WHERE EXISTS (
    SELECT 1 FROM feature_flags f WHERE f.organization_id = NEW.organization_id AND f.id = NEW.flag_id
      AND NOT ((f.value_type = 'boolean' AND json_type(NEW.value_json) IN ('true', 'false'))
    OR (f.value_type = 'string' AND json_type(NEW.value_json) = 'text')
    OR (f.value_type = 'number' AND json_type(NEW.value_json) IN ('integer', 'real'))
    OR f.value_type = 'json')
  );
END;
CREATE TRIGGER deployment_flag_settings_value_type_insert BEFORE INSERT ON deployment_flag_settings
BEGIN
  SELECT RAISE(ABORT, 'flag value type mismatch') WHERE EXISTS (
    SELECT 1 FROM feature_flags f WHERE f.organization_id = NEW.organization_id AND f.id = NEW.flag_id
      AND NOT ((f.value_type = 'boolean' AND json_type(NEW.value_json) IN ('true', 'false'))
    OR (f.value_type = 'string' AND json_type(NEW.value_json) = 'text')
    OR (f.value_type = 'number' AND json_type(NEW.value_json) IN ('integer', 'real'))
    OR f.value_type = 'json')
  );
END;
CREATE TRIGGER deployment_flag_settings_value_type_update BEFORE UPDATE ON deployment_flag_settings
BEGIN
  SELECT RAISE(ABORT, 'flag value type mismatch') WHERE EXISTS (
    SELECT 1 FROM feature_flags f WHERE f.organization_id = NEW.organization_id AND f.id = NEW.flag_id
      AND NOT ((f.value_type = 'boolean' AND json_type(NEW.value_json) IN ('true', 'false'))
    OR (f.value_type = 'string' AND json_type(NEW.value_json) = 'text')
    OR (f.value_type = 'number' AND json_type(NEW.value_json) IN ('integer', 'real'))
    OR f.value_type = 'json')
  );
END;
CREATE TRIGGER used_flag_identity_immutable BEFORE UPDATE OF organization_id, id, key, value_type, project_id ON feature_flags
WHEN NEW.organization_id <> OLD.organization_id OR NEW.id <> OLD.id OR NEW.key <> OLD.key OR NEW.value_type <> OLD.value_type OR NEW.project_id <> OLD.project_id
BEGIN
  SELECT RAISE(ABORT, 'used flag identity is immutable') WHERE
    EXISTS (SELECT 1 FROM release_flag_settings s WHERE s.organization_id = OLD.organization_id AND s.flag_id = OLD.id)
    OR EXISTS (SELECT 1 FROM deployment_flag_settings s WHERE s.organization_id = OLD.organization_id AND s.flag_id = OLD.id);
END;

CREATE INDEX usage_by_task_time ON usage_records(organization_id, task_id, occurred_at);
CREATE INDEX usage_by_agent_time ON usage_records(organization_id, employee_id, occurred_at);
CREATE INDEX usage_by_project_time ON usage_records(organization_id, project_id, occurred_at);
CREATE TRIGGER usage_records_immutable_update BEFORE UPDATE ON usage_records
BEGIN SELECT RAISE(ABORT, 'usage records are immutable; append a correction'); END;
CREATE TRIGGER usage_records_immutable_delete BEFORE DELETE ON usage_records
BEGIN SELECT RAISE(ABORT, 'usage history must be retained'); END;
CREATE TRIGGER used_model_price_immutable_update BEFORE UPDATE ON model_prices
WHEN EXISTS (SELECT 1 FROM usage_records u WHERE u.organization_id = OLD.organization_id AND u.price_id = OLD.id)
BEGIN SELECT RAISE(ABORT, 'used price versions are immutable'); END;
CREATE TRIGGER usage_actor_must_be_agent BEFORE INSERT ON usage_records
WHEN NEW.actor_kind = 'agent'
BEGIN
  SELECT RAISE(ABORT, 'usage actor must be an agent') WHERE EXISTS (
    SELECT 1 FROM employees e WHERE e.organization_id = NEW.organization_id AND e.id = NEW.employee_id AND e.kind <> 'agent'
  );
END;
CREATE TRIGGER usage_correction_parent_exists BEFORE INSERT ON usage_records
WHEN NEW.supersedes_id IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'correction parent must already exist') WHERE NOT EXISTS (
    SELECT 1 FROM usage_records p WHERE p.organization_id = NEW.organization_id AND p.id = NEW.supersedes_id
  );
END;
CREATE TRIGGER attribution_correction_preserves_facts BEFORE INSERT ON usage_records
WHEN NEW.correction_kind = 'attribution'
BEGIN
  SELECT RAISE(ABORT, 'attribution correction cannot change consumption or valuation') WHERE EXISTS (
    SELECT 1 FROM usage_records p WHERE p.organization_id = NEW.organization_id AND p.id = NEW.supersedes_id AND (
      NEW.execution_office_id IS NOT p.execution_office_id OR
      NEW.run_id IS NOT p.run_id OR
      NEW.actor_kind IS NOT p.actor_kind OR
      NEW.employee_id IS NOT p.employee_id OR
      NEW.runtime IS NOT p.runtime OR
      NEW.provider IS NOT p.provider OR
      NEW.model IS NOT p.model OR
      NEW.occurred_at IS NOT p.occurred_at OR
      NEW.normalization_version IS NOT p.normalization_version OR
      NEW.source_metadata_json IS NOT p.source_metadata_json OR
      NEW.input_tokens IS NOT p.input_tokens OR
      NEW.output_tokens IS NOT p.output_tokens OR
      NEW.cache_read_tokens IS NOT p.cache_read_tokens OR
      NEW.cache_write_tokens IS NOT p.cache_write_tokens OR
      NEW.reasoning_tokens IS NOT p.reasoning_tokens OR
      NEW.token_status IS NOT p.token_status OR
      NEW.token_issue IS NOT p.token_issue OR
      NEW.price_id IS NOT p.price_id OR
      NEW.cost_micro_usd IS NOT p.cost_micro_usd OR
      NEW.cost_basis IS NOT p.cost_basis OR
      NEW.cost_status IS NOT p.cost_status OR
      NEW.cost_issue IS NOT p.cost_issue
    )
  );
END;

CREATE TRIGGER usage_checkpoint_monotonic BEFORE UPDATE ON usage_stream_checkpoints
WHEN NEW.organization_id <> OLD.organization_id OR NEW.origin_office_id <> OLD.origin_office_id
  OR NEW.producer_id <> OLD.producer_id OR NEW.high_water_sequence < OLD.high_water_sequence
  OR (NEW.high_water_sequence = OLD.high_water_sequence AND NEW.head_sha256 IS NOT OLD.head_sha256)
  OR (OLD.state = 'sealed' AND (NEW.state <> 'sealed' OR NEW.high_water_sequence <> OLD.high_water_sequence))
BEGIN SELECT RAISE(ABORT, 'usage checkpoint regressed, forked or reopened'); END;

CREATE TRIGGER usage_checkpoint_no_delete BEFORE DELETE ON usage_stream_checkpoints
BEGIN SELECT RAISE(ABORT, 'usage checkpoint identity and coverage must be retained'); END;

CREATE TRIGGER used_model_price_no_delete BEFORE DELETE ON model_prices
WHEN EXISTS (SELECT 1 FROM usage_records u WHERE u.organization_id=OLD.organization_id AND u.price_id=OLD.id)
BEGIN SELECT RAISE(ABORT, 'used price versions must be retained'); END;

CREATE TRIGGER used_flag_no_delete BEFORE DELETE ON feature_flags
WHEN EXISTS (SELECT 1 FROM release_flag_settings s WHERE s.organization_id=OLD.organization_id AND s.flag_id=OLD.id)
  OR EXISTS (SELECT 1 FROM deployment_flag_settings s WHERE s.organization_id=OLD.organization_id AND s.flag_id=OLD.id)
BEGIN SELECT RAISE(ABORT, 'used flag identity must be retained'); END;

-- Validate the parent as well as the setting, so deferred-FK load order is safe.
CREATE TRIGGER inserted_flag_checks_existing_settings AFTER INSERT ON feature_flags
BEGIN
  SELECT RAISE(ABORT, 'flag value type mismatch') WHERE EXISTS (
    SELECT 1 FROM (
      SELECT value_json FROM release_flag_settings WHERE organization_id=NEW.organization_id AND flag_id=NEW.id
      UNION ALL
      SELECT value_json FROM deployment_flag_settings WHERE organization_id=NEW.organization_id AND flag_id=NEW.id
    ) s WHERE NOT (
      (NEW.value_type='boolean' AND json_type(s.value_json) IN ('true','false'))
      OR (NEW.value_type='string' AND json_type(s.value_json)='text')
      OR (NEW.value_type='number' AND json_type(s.value_json) IN ('integer','real'))
      OR NEW.value_type='json')
  );
END;

CREATE TRIGGER employees_no_reporting_cycle_insert AFTER INSERT ON employees
WHEN NEW.reports_to_employee_id IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'employee reporting cycle') WHERE EXISTS (
    WITH RECURSIVE managers(id) AS (
      SELECT NEW.reports_to_employee_id UNION
      SELECT e.reports_to_employee_id FROM employees e JOIN managers m ON e.id=m.id
      WHERE e.organization_id=NEW.organization_id AND e.reports_to_employee_id IS NOT NULL
    ) SELECT 1 FROM managers WHERE id=NEW.id
  );
END;
CREATE TRIGGER employees_no_reporting_cycle_update AFTER UPDATE ON employees
WHEN NEW.reports_to_employee_id IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'employee reporting cycle') WHERE EXISTS (
    WITH RECURSIVE managers(id) AS (
      SELECT NEW.reports_to_employee_id UNION
      SELECT e.reports_to_employee_id FROM employees e JOIN managers m ON e.id=m.id
      WHERE e.organization_id=NEW.organization_id AND e.reports_to_employee_id IS NOT NULL
    ) SELECT 1 FROM managers WHERE id=NEW.id
  );
END;

CREATE TRIGGER employee_usage_kind_insert AFTER INSERT ON employees
WHEN NEW.kind <> 'agent' AND EXISTS (
  SELECT 1 FROM usage_records u WHERE u.organization_id=NEW.organization_id AND u.employee_id=NEW.id AND u.actor_kind='agent')
BEGIN SELECT RAISE(ABORT, 'historical usage consumer must remain an agent'); END;
CREATE TRIGGER employee_usage_kind_update BEFORE UPDATE OF kind ON employees
WHEN NEW.kind <> 'agent' AND EXISTS (
  SELECT 1 FROM usage_records u WHERE u.organization_id=NEW.organization_id AND u.employee_id=NEW.id AND u.actor_kind='agent')
BEGIN SELECT RAISE(ABORT, 'historical usage consumer must remain an agent'); END;

-- Model profiles, provider connections and agent execution (document 11).
CREATE TABLE provider_connections (
organization_id TEXT NOT NULL REFERENCES organizations(id),
  id TEXT NOT NULL,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
key TEXT NOT NULL CHECK (length(key)>0),
  name TEXT NOT NULL,
  provider_type TEXT NOT NULL CHECK (length(provider_type)>0),
  auth_mode TEXT NOT NULL CHECK (auth_mode IN ('subscription_cli','api_key','cloud_iam')),
  subscription_identity TEXT CHECK (subscription_identity IS NULL OR length(trim(subscription_identity))>0),
  runtime_driver TEXT NOT NULL,
  endpoint_url TEXT,
  enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
  archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0,1)),
  max_concurrent_turns INTEGER NOT NULL DEFAULT 1 CHECK (max_concurrent_turns>=1),
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee','system','unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee','system','unknown')),
  updated_by_id TEXT,
  PRIMARY KEY (organization_id,id),
UNIQUE (organization_id,key),
  UNIQUE (organization_id,provider_type,subscription_identity),
  CHECK (auth_mode<>'subscription_cli' OR subscription_identity IS NOT NULL),
  CHECK ((created_by_kind = 'employee' AND created_by_id IS NOT NULL) OR (created_by_kind <> 'employee' AND created_by_id IS NULL)),
  CHECK ((updated_by_kind = 'employee' AND updated_by_id IS NOT NULL) OR (updated_by_kind <> 'employee' AND updated_by_id IS NULL)),
  FOREIGN KEY (organization_id,created_by_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,updated_by_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TRIGGER registered_subscription_cannot_be_replaced
BEFORE INSERT ON provider_connections
WHEN EXISTS (
  SELECT 1 FROM provider_connections p
  WHERE p.organization_id=NEW.organization_id AND p.subscription_identity IS NOT NULL
    AND (p.id=NEW.id OR p.key=NEW.key OR
      (p.provider_type=NEW.provider_type AND p.subscription_identity=NEW.subscription_identity)))
BEGIN SELECT RAISE(ABORT,'registered subscription cannot be replaced; resolve existing connection'); END;

CREATE TRIGGER registered_subscription_cannot_be_displaced
BEFORE UPDATE ON provider_connections
WHEN EXISTS (
  SELECT 1 FROM provider_connections p
  WHERE p.organization_id=NEW.organization_id AND p.subscription_identity IS NOT NULL
    AND NOT (p.organization_id=OLD.organization_id AND p.id=OLD.id)
    AND (p.id=NEW.id OR p.key=NEW.key OR
      (p.provider_type=NEW.provider_type AND p.subscription_identity=NEW.subscription_identity)))
BEGIN SELECT RAISE(ABORT,'registered subscription cannot be displaced by another connection'); END;

CREATE TRIGGER registered_subscription_identity_is_immutable
BEFORE UPDATE OF provider_type,subscription_identity ON provider_connections
WHEN OLD.subscription_identity IS NOT NULL AND
  (NEW.provider_type IS NOT OLD.provider_type OR NEW.subscription_identity IS NOT OLD.subscription_identity)
BEGIN SELECT RAISE(ABORT,'registered subscription identity is immutable'); END;

CREATE TABLE model_profiles (
organization_id TEXT NOT NULL REFERENCES organizations(id),
  id TEXT NOT NULL,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
key TEXT NOT NULL CHECK (length(key)>0),
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  is_auto_generated INTEGER NOT NULL DEFAULT 0 CHECK (is_auto_generated IN (0,1)),
  generated_for_connection_id TEXT,
  archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0,1)),
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee','system','unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee','system','unknown')),
  updated_by_id TEXT,
  PRIMARY KEY (organization_id,id),
UNIQUE (organization_id,key),
  UNIQUE (organization_id,generated_for_connection_id),
  CHECK ((is_auto_generated=1 AND generated_for_connection_id IS NOT NULL) OR (is_auto_generated=0 AND generated_for_connection_id IS NULL)),
  FOREIGN KEY (organization_id,generated_for_connection_id) REFERENCES provider_connections(organization_id,id) ON DELETE RESTRICT,
  CHECK ((created_by_kind = 'employee' AND created_by_id IS NOT NULL) OR (created_by_kind <> 'employee' AND created_by_id IS NULL)),
  CHECK ((updated_by_kind = 'employee' AND updated_by_id IS NOT NULL) OR (updated_by_kind <> 'employee' AND updated_by_id IS NULL)),
  FOREIGN KEY (organization_id,created_by_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,updated_by_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE model_profile_connections (
organization_id TEXT NOT NULL,
  model_profile_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  priority INTEGER NOT NULL CHECK (priority BETWEEN 1 AND 64),
  PRIMARY KEY (organization_id,model_profile_id,connection_id),
  UNIQUE (organization_id,model_profile_id,priority),
  FOREIGN KEY (organization_id,model_profile_id) REFERENCES model_profiles(organization_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,connection_id) REFERENCES provider_connections(organization_id,id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE agent_execution_configs (
organization_id TEXT NOT NULL REFERENCES organizations(id),
  employee_id TEXT NOT NULL,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
model_profile_id TEXT NOT NULL,
  model_id TEXT NOT NULL CHECK (length(model_id)>0),
  system_prompt_markdown TEXT NOT NULL CHECK (length(system_prompt_markdown)>0),
  tools_config_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(tools_config_json) AND json_type(tools_config_json)='array'),
  enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee','system','unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee','system','unknown')),
  updated_by_id TEXT,
  PRIMARY KEY (organization_id,employee_id),
FOREIGN KEY (organization_id,employee_id) REFERENCES employees(organization_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,model_profile_id) REFERENCES model_profiles(organization_id,id) ON DELETE RESTRICT,
  CHECK ((created_by_kind = 'employee' AND created_by_id IS NOT NULL) OR (created_by_kind <> 'employee' AND created_by_id IS NULL)),
  CHECK ((updated_by_kind = 'employee' AND updated_by_id IS NOT NULL) OR (updated_by_kind <> 'employee' AND updated_by_id IS NULL)),
  FOREIGN KEY (organization_id,created_by_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,updated_by_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE office_connection_bindings (
organization_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  revision INTEGER NOT NULL CHECK (revision>=1),
  auth_context_ref TEXT NOT NULL CHECK (length(auth_context_ref)>0),
  runtime_binary_path TEXT,
  account_identity_ref TEXT,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (organization_id,office_id,connection_id),
  FOREIGN KEY (organization_id,office_id) REFERENCES offices(organization_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,connection_id) REFERENCES provider_connections(organization_id,id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE office_connection_state (
organization_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('unknown','ready','auth_expired','unavailable')),
  binding_revision INTEGER NOT NULL CHECK (binding_revision>=1),
  driver_version TEXT,
  observed_at TEXT,
  valid_until TEXT,
  issue_code TEXT,
  PRIMARY KEY (organization_id,office_id,connection_id),
  CHECK (status<>'ready' OR (driver_version IS NOT NULL AND observed_at IS NOT NULL AND valid_until IS NOT NULL)),
  FOREIGN KEY (organization_id,office_id,connection_id) REFERENCES office_connection_bindings(organization_id,office_id,connection_id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE office_connection_models (
organization_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  model_id TEXT NOT NULL,
  driver_model_id TEXT NOT NULL,
  entitlement TEXT NOT NULL CHECK (entitlement IN ('confirmed','denied','unknown')),
  capabilities_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(capabilities_json) AND json_type(capabilities_json)='object'),
  observed_at TEXT NOT NULL,
  valid_until TEXT NOT NULL,
  PRIMARY KEY (organization_id,office_id,connection_id,model_id),
  FOREIGN KEY (organization_id,office_id,connection_id) REFERENCES office_connection_bindings(organization_id,office_id,connection_id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE connection_quota_state (
organization_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  scope_key TEXT NOT NULL,
  model_id TEXT,
  status TEXT NOT NULL CHECK (status IN ('unknown','open','cooldown')),
  cooldown_until TEXT,
  observed_at TEXT NOT NULL,
  observed_by_office_id TEXT NOT NULL,
  coordinator_generation INTEGER NOT NULL CHECK (coordinator_generation>=1),
  PRIMARY KEY (organization_id,connection_id,scope_key),
  CHECK ((model_id IS NULL AND scope_key='connection') OR (model_id IS NOT NULL AND scope_key='model:'||model_id)),
  CHECK (status<>'cooldown' OR cooldown_until IS NOT NULL),
  FOREIGN KEY (organization_id,connection_id) REFERENCES provider_connections(organization_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,observed_by_office_id) REFERENCES offices(organization_id,id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE connection_turn_permits (
organization_id TEXT NOT NULL,
  id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  employee_id TEXT NOT NULL,
  placement_generation INTEGER NOT NULL CHECK (placement_generation>=1),
  model_id TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('active','release_pending','released')),
  issued_at TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  coordinator_generation INTEGER NOT NULL CHECK (coordinator_generation>=1),
  PRIMARY KEY (organization_id,id),
  UNIQUE (organization_id,id,connection_id,employee_id,office_id,placement_generation,model_id),
  FOREIGN KEY (organization_id,connection_id) REFERENCES provider_connections(organization_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,office_id) REFERENCES offices(organization_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,employee_id) REFERENCES employees(organization_id,id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE agent_execution_turns (
organization_id TEXT NOT NULL,
  id TEXT NOT NULL,
  employee_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  placement_generation INTEGER NOT NULL CHECK (placement_generation>=1),
  revision INTEGER NOT NULL CHECK (revision>=1),
  task_id TEXT,
  task_kind TEXT NOT NULL DEFAULT 'task' CHECK (task_kind='task'),
  workspace_key TEXT NOT NULL,
  input_ref TEXT NOT NULL,
  input_sha256 TEXT NOT NULL CHECK (length(input_sha256)=64),
  agent_config_revision INTEGER NOT NULL CHECK (agent_config_revision>=1),
  model_profile_id TEXT NOT NULL,
  profile_revision INTEGER NOT NULL CHECK (profile_revision>=1),
  model_id TEXT NOT NULL,
  pool_snapshot_json TEXT NOT NULL CHECK (json_valid(pool_snapshot_json) AND json_type(pool_snapshot_json)='array'),
  status TEXT NOT NULL CHECK (status IN ('accepted','running','switching','blocked','recovery_required','succeeded','failed','cancelled')),
  created_at TEXT NOT NULL,
  ended_at TEXT,
  error_code TEXT,
  started_by_kind TEXT NOT NULL DEFAULT 'run' CHECK (started_by_kind IN ('run','message','mention','ask','handoff','assignment','answer','recovery')),
  started_by_employee_id TEXT,
  started_by_ref TEXT,
  CHECK (started_by_kind<>'recovery' OR started_by_employee_id IS NULL),
  PRIMARY KEY (organization_id,id),
  UNIQUE (organization_id,id,employee_id,office_id,placement_generation),
  FOREIGN KEY (organization_id,started_by_employee_id) REFERENCES employees(organization_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,employee_id) REFERENCES agent_execution_configs(organization_id,employee_id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,office_id) REFERENCES offices(organization_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,model_profile_id) REFERENCES model_profiles(organization_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,task_kind,task_id) REFERENCES record_refs(organization_id,kind,id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE agent_process_instances (
organization_id TEXT NOT NULL,
  id TEXT NOT NULL,
  employee_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  placement_generation INTEGER NOT NULL CHECK (placement_generation>=1),
  connection_id TEXT NOT NULL,
  runtime_driver TEXT NOT NULL,
  driver_version TEXT NOT NULL,
  model_id TEXT NOT NULL,
  resolved_model_id TEXT NOT NULL,
  connection_revision INTEGER NOT NULL CHECK (connection_revision>=1),
  workspace_key TEXT NOT NULL,
  launch_digest TEXT NOT NULL CHECK (length(launch_digest)=64),
  binding_revision INTEGER NOT NULL CHECK (binding_revision>=1),
  session_mark TEXT NOT NULL,
  pid INTEGER,
  pgid INTEGER,
  process_start_identity TEXT,
  boot_id TEXT,
  state TEXT NOT NULL CHECK (state IN ('planned','starting','idle','busy','stopping','cleanup_required','gone')),
  updated_at TEXT NOT NULL,
  PRIMARY KEY (organization_id,id),
  UNIQUE (organization_id,session_mark),
  UNIQUE (organization_id,id,employee_id,office_id,placement_generation),
  CHECK ((pid IS NULL AND pgid IS NULL AND process_start_identity IS NULL AND boot_id IS NULL)
    OR (pid IS NOT NULL AND pgid IS NOT NULL AND pid>0 AND pgid>0 AND process_start_identity IS NOT NULL AND boot_id IS NOT NULL)),
  CHECK (state NOT IN ('idle','busy') OR (pid IS NOT NULL AND process_start_identity IS NOT NULL AND boot_id IS NOT NULL)),
  FOREIGN KEY (organization_id,employee_id) REFERENCES agent_execution_configs(organization_id,employee_id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,office_id) REFERENCES offices(organization_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,connection_id) REFERENCES provider_connections(organization_id,id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE agent_execution_attempts (
organization_id TEXT NOT NULL,
  id TEXT NOT NULL,
  turn_id TEXT NOT NULL,
  attempt_no INTEGER NOT NULL CHECK (attempt_no>=1),
  employee_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  placement_generation INTEGER NOT NULL CHECK (placement_generation>=1),
  connection_id TEXT NOT NULL,
  permit_id TEXT NOT NULL,
  process_instance_id TEXT,
  runtime_driver TEXT NOT NULL,
  driver_version TEXT NOT NULL,
  requested_model_id TEXT NOT NULL,
  resolved_model_id TEXT NOT NULL,
  connection_revision INTEGER NOT NULL CHECK (connection_revision>=1),
  binding_revision INTEGER NOT NULL CHECK (binding_revision>=1),
  state TEXT NOT NULL CHECK (state IN ('selected','starting','running','switching','succeeded','failed','cancelled','recovery_required')),
  effect_state TEXT NOT NULL CHECK (effect_state IN ('none','recorded','unknown')),
  checkpoint_ref TEXT,
  predecessor_attempt_id TEXT,
  started_at TEXT,
  ended_at TEXT,
  error_code TEXT,
  PRIMARY KEY (organization_id,id),
  UNIQUE (organization_id,turn_id,attempt_no),
  UNIQUE (organization_id,id,turn_id),
  CHECK (predecessor_attempt_id IS NULL OR predecessor_attempt_id<>id),
  FOREIGN KEY (organization_id,turn_id,employee_id,office_id,placement_generation) REFERENCES agent_execution_turns(organization_id,id,employee_id,office_id,placement_generation) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,process_instance_id,employee_id,office_id,placement_generation) REFERENCES agent_process_instances(organization_id,id,employee_id,office_id,placement_generation) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,connection_id) REFERENCES provider_connections(organization_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,permit_id,connection_id,employee_id,office_id,placement_generation,requested_model_id) REFERENCES connection_turn_permits(organization_id,id,connection_id,employee_id,office_id,placement_generation,model_id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,predecessor_attempt_id,turn_id) REFERENCES agent_execution_attempts(organization_id,id,turn_id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE supervisor_command_receipts (
organization_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  operation_id TEXT NOT NULL,
  command TEXT NOT NULL CHECK (command IN ('RunTurn','ResumeTurn','CancelTurn','StopAgent','BindConnection','RefreshConnection')),
  employee_id TEXT,
  payload_sha256 TEXT NOT NULL CHECK (length(payload_sha256)=64),
  outcome TEXT NOT NULL CHECK (outcome IN ('accepted','rejected')),
  response_json TEXT NOT NULL CHECK (json_valid(response_json) AND json_type(response_json)='object'),
  recorded_at TEXT NOT NULL,
  PRIMARY KEY (organization_id,office_id,operation_id),
  FOREIGN KEY (organization_id,office_id) REFERENCES offices(organization_id,id) ON DELETE RESTRICT,
  FOREIGN KEY (organization_id,employee_id) REFERENCES employees(organization_id,id) ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER placement_generation_must_advance BEFORE UPDATE ON agent_placements
WHEN NEW.placement_generation<OLD.placement_generation OR (NEW.office_id<>OLD.office_id AND NEW.placement_generation<=OLD.placement_generation)
BEGIN SELECT RAISE(ABORT,'office changes require a newer placement generation'); END;
CREATE INDEX agent_configs_by_profile ON agent_execution_configs(organization_id,model_profile_id);
CREATE INDEX profiles_by_connection ON model_profile_connections(organization_id,connection_id);
CREATE INDEX attempts_by_turn ON agent_execution_attempts(organization_id,turn_id,state);
CREATE INDEX permits_by_connection ON connection_turn_permits(organization_id,connection_id,state);
CREATE UNIQUE INDEX one_open_turn_per_agent ON agent_execution_turns(organization_id,employee_id)
WHERE status IN ('accepted','running','switching','blocked','recovery_required');
CREATE UNIQUE INDEX one_owned_main_process ON agent_process_instances(organization_id,employee_id)
WHERE state<>'gone';
CREATE TRIGGER agent_config_requires_agent BEFORE INSERT ON agent_execution_configs
BEGIN SELECT RAISE(ABORT,'execution config requires an agent employee') WHERE EXISTS (
 SELECT 1 FROM employees e WHERE e.organization_id=NEW.organization_id AND e.id=NEW.employee_id AND e.kind<>'agent'); END;
CREATE TRIGGER agent_config_update_requires_agent BEFORE UPDATE OF employee_id,organization_id ON agent_execution_configs
BEGIN SELECT RAISE(ABORT,'execution config requires an agent employee') WHERE EXISTS (
 SELECT 1 FROM employees e WHERE e.organization_id=NEW.organization_id AND e.id=NEW.employee_id AND e.kind<>'agent'); END;
CREATE TRIGGER configured_employee_stays_agent BEFORE UPDATE OF kind ON employees
WHEN NEW.kind<>'agent' AND EXISTS (SELECT 1 FROM agent_execution_configs c WHERE c.organization_id=OLD.organization_id AND c.employee_id=OLD.id)
BEGIN SELECT RAISE(ABORT,'configured employee must remain an agent'); END;

CREATE VIEW model_profile_model_union AS
SELECT m.organization_id,m.office_id,p.model_profile_id,m.model_id,
 COUNT(*) AS supporting_connections,MIN(p.priority) AS first_priority
FROM model_profile_connections p JOIN office_connection_models m
 ON m.organization_id=p.organization_id AND m.connection_id=p.connection_id
WHERE m.entitlement='confirmed'
GROUP BY m.organization_id,m.office_id,p.model_profile_id,m.model_id;

CREATE TABLE task_details (
  organization_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  definition_of_done TEXT,
  branch TEXT,
  shortcut_json TEXT CHECK (shortcut_json IS NULL OR (json_valid(shortcut_json) AND json_type(shortcut_json)='object')),
  done_anyway_json TEXT CHECK (done_anyway_json IS NULL OR (json_valid(done_anyway_json) AND json_type(done_anyway_json)='object')),
  what_happened TEXT,
  expected TEXT,
  steps TEXT,
  environment TEXT,
  severity_confirmed INTEGER NOT NULL DEFAULT 0 CHECK (severity_confirmed IN (0,1)),
  reopen_count INTEGER NOT NULL DEFAULT 0 CHECK (reopen_count>=0),
  duplicate_of_task_id TEXT,
  closed_reason TEXT,
  affects_feature_id TEXT,
  request_kind TEXT,
  request_text TEXT,
  route_reason TEXT,
  routing_source TEXT CHECK (routing_source IN ('person','rule','copilot')),
  routed_by_employee_id TEXT,
  request_outcome_json TEXT CHECK (request_outcome_json IS NULL OR (json_valid(request_outcome_json) AND json_type(request_outcome_json)='object')),
  merged_into_task_id TEXT,
  decline_reason TEXT,
  suggested_triager_id TEXT,
  suggested_verifier_id TEXT,
  PRIMARY KEY (organization_id,task_id),
  CHECK (duplicate_of_task_id IS NULL OR duplicate_of_task_id<>task_id),
  CHECK (merged_into_task_id IS NULL OR merged_into_task_id<>task_id),
  FOREIGN KEY (organization_id,task_id) REFERENCES tasks(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,duplicate_of_task_id) REFERENCES tasks(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,merged_into_task_id) REFERENCES tasks(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,affects_feature_id) REFERENCES features(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,routed_by_employee_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,suggested_triager_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,suggested_verifier_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE conversations (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  id TEXT NOT NULL,
  schema_version INTEGER NOT NULL DEFAULT 1 CHECK (schema_version=1),
  revision INTEGER NOT NULL DEFAULT 1 CHECK (revision>=1),
  kind TEXT NOT NULL CHECK (kind IN ('dm','group_dm','channel')),
  name TEXT,
  auto_scope TEXT CHECK (auto_scope IN ('organization','team','project')),
  team_id TEXT,
  project_id TEXT,
  archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0,1)),
  created_at TEXT NOT NULL,
  PRIMARY KEY (organization_id,id),
  CHECK (auto_scope IS NULL OR (kind='channel' AND
    ((auto_scope='organization' AND team_id IS NULL AND project_id IS NULL)
    OR (auto_scope='team' AND team_id IS NOT NULL AND project_id IS NULL)
    OR (auto_scope='project' AND project_id IS NOT NULL AND team_id IS NULL)))),
  FOREIGN KEY (organization_id,team_id) REFERENCES teams(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,project_id) REFERENCES projects(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE UNIQUE INDEX conversation_auto_org ON conversations(organization_id) WHERE auto_scope='organization';
CREATE UNIQUE INDEX conversation_auto_team ON conversations(organization_id,team_id) WHERE auto_scope='team';
CREATE UNIQUE INDEX conversation_auto_project ON conversations(organization_id,project_id) WHERE auto_scope='project';
CREATE TABLE conversation_memberships (
  organization_id TEXT NOT NULL,
  conversation_id TEXT NOT NULL,
  employee_id TEXT NOT NULL,
  joined_at TEXT NOT NULL,
  left_at TEXT,
  PRIMARY KEY (organization_id,conversation_id,employee_id),
  FOREIGN KEY (organization_id,conversation_id) REFERENCES conversations(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,employee_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE messages (
  organization_id TEXT NOT NULL,
  id TEXT NOT NULL,
  conversation_id TEXT NOT NULL,
  sender_kind TEXT NOT NULL CHECK (sender_kind IN ('employee','office')),
  sender_employee_id TEXT,
  sender_office_id TEXT,
  sender_display_name TEXT NOT NULL,
  kind TEXT NOT NULL,
  priority TEXT NOT NULL DEFAULT 'normal' CHECK (priority IN ('normal','high','urgent')),
  subject TEXT,
  body_markdown TEXT NOT NULL,
  metadata_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(metadata_json) AND json_type(metadata_json)='object'),
  thread_root_id TEXT,
  reply_to_message_id TEXT,
  also_in_channel INTEGER NOT NULL DEFAULT 0 CHECK (also_in_channel IN (0,1)),
  created_at TEXT NOT NULL,
  expires_at TEXT,
  ttl_ms INTEGER CHECK (ttl_ms>=0),
  origin_office_id TEXT NOT NULL,
  origin_incarnation TEXT NOT NULL,
  origin_sequence INTEGER NOT NULL CHECK (origin_sequence>0),
  org_epoch INTEGER NOT NULL CHECK (org_epoch>=0),
  org_sequence INTEGER NOT NULL CHECK (org_sequence>=0),
  content_sha256 TEXT NOT NULL CHECK (length(content_sha256)=64 AND content_sha256 NOT GLOB '*[^0-9a-f]*'),
  PRIMARY KEY (organization_id,id),
  UNIQUE (organization_id,id,conversation_id),
  UNIQUE (organization_id,origin_office_id,origin_incarnation,origin_sequence),
  CHECK ((sender_kind='employee' AND sender_employee_id IS NOT NULL AND sender_office_id IS NULL)
    OR (sender_kind='office' AND sender_employee_id IS NULL AND sender_office_id IS NOT NULL)),
  FOREIGN KEY (organization_id,conversation_id) REFERENCES conversations(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,sender_employee_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,sender_office_id) REFERENCES offices(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,origin_office_id) REFERENCES offices(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,thread_root_id,conversation_id) REFERENCES messages(organization_id,id,conversation_id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,reply_to_message_id,conversation_id) REFERENCES messages(organization_id,id,conversation_id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE INDEX message_conversation_time ON messages(organization_id,conversation_id,created_at,id);
CREATE TABLE message_recipient_states (
  organization_id TEXT NOT NULL,
  message_id TEXT NOT NULL,
  conversation_id TEXT NOT NULL,
  employee_id TEXT NOT NULL,
  recipient_display_name TEXT,
  state TEXT NOT NULL CHECK (state IN ('unread','snoozed','read','done','expired','dead_letter')),
  triage_generation INTEGER NOT NULL DEFAULT 0 CHECK (triage_generation>=0),
  snoozed_until TEXT,
  snooze_after_task INTEGER NOT NULL DEFAULT 0 CHECK (snooze_after_task IN (0,1)),
  snooze_note TEXT,
  triaged_at TEXT,
  read_at TEXT,
  updated_at TEXT NOT NULL,
  reason TEXT,
  delivered_because TEXT NOT NULL DEFAULT 'addressed' CHECK (delivered_because IN ('addressed','mention','thread_reply','answer_candidate')),
  related_message_ids_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(related_message_ids_json) AND json_type(related_message_ids_json)='array'),
  expired_waiting_person INTEGER NOT NULL DEFAULT 0 CHECK (expired_waiting_person IN (0,1)),
  owner_office_id TEXT NOT NULL,
  origin_incarnation TEXT NOT NULL,
  origin_sequence INTEGER NOT NULL CHECK (origin_sequence>0),
  org_epoch INTEGER NOT NULL CHECK (org_epoch>=0),
  org_sequence INTEGER NOT NULL CHECK (org_sequence>=0),
  PRIMARY KEY (organization_id,message_id,employee_id),
  FOREIGN KEY (organization_id,message_id,conversation_id) REFERENCES messages(organization_id,id,conversation_id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,conversation_id,employee_id) REFERENCES conversation_memberships(organization_id,conversation_id,employee_id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,owner_office_id) REFERENCES offices(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE INDEX message_recipient_inbox ON message_recipient_states(organization_id,employee_id,state,snoozed_until);
CREATE TABLE conversation_preferences (
  organization_id TEXT NOT NULL,
  conversation_id TEXT NOT NULL,
  viewer_employee_id TEXT NOT NULL,
  muted_employee_id TEXT NOT NULL,
  mute_mode TEXT NOT NULL DEFAULT 'none' CHECK (mute_mode IN ('none','muted','digest')),
  PRIMARY KEY (organization_id,conversation_id,viewer_employee_id,muted_employee_id),
  FOREIGN KEY (organization_id,conversation_id) REFERENCES conversations(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,viewer_employee_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,muted_employee_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE conversation_read_markers (
  organization_id TEXT NOT NULL,
  conversation_id TEXT NOT NULL,
  viewer_employee_id TEXT NOT NULL,
  read_at TEXT,
  last_read_message_id TEXT,
  PRIMARY KEY (organization_id,conversation_id,viewer_employee_id),
  FOREIGN KEY (organization_id,conversation_id) REFERENCES conversations(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,viewer_employee_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,last_read_message_id,conversation_id) REFERENCES messages(organization_id,id,conversation_id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE message_attachments (
  organization_id TEXT NOT NULL,
  message_id TEXT NOT NULL,
  id TEXT NOT NULL,
  content_ref TEXT NOT NULL,
  media_type TEXT NOT NULL,
  byte_count INTEGER NOT NULL CHECK (byte_count>=0),
  content_sha256 TEXT NOT NULL CHECK (length(content_sha256)=64 AND content_sha256 NOT GLOB '*[^0-9a-f]*'),
  PRIMARY KEY (organization_id,message_id,id),
  FOREIGN KEY (organization_id,message_id) REFERENCES messages(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE message_reactions (
  organization_id TEXT NOT NULL,
  message_id TEXT NOT NULL,
  employee_id TEXT NOT NULL,
  reaction TEXT NOT NULL CHECK (reaction IN ('seen','looking')),
  created_at TEXT NOT NULL,
  PRIMARY KEY (organization_id,message_id,employee_id,reaction),
  FOREIGN KEY (organization_id,message_id) REFERENCES messages(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,employee_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE mutation_receipts (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  command_id TEXT NOT NULL,
  origin_office_id TEXT NOT NULL,
  actor_kind TEXT NOT NULL CHECK (actor_kind IN ('employee','system')),
  actor_id TEXT,
  command_kind TEXT NOT NULL,
  request_sha256 TEXT NOT NULL CHECK (length(request_sha256)=64 AND request_sha256 NOT GLOB '*[^0-9a-f]*'),
  state TEXT NOT NULL CHECK (state IN ('reserved','linked','completed','rejected','abandoned','outcome_unknown')),
  result_ref TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  expires_at TEXT,
  PRIMARY KEY (organization_id,command_id),
  CHECK ((actor_kind='employee')=(actor_id IS NOT NULL)),
  FOREIGN KEY (organization_id,actor_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,origin_office_id) REFERENCES offices(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE office_origin_streams (
  organization_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  stream_kind TEXT NOT NULL CHECK (stream_kind IN ('organization','work','message_content','message_state')),
  incarnation TEXT NOT NULL,
  high_water_sequence INTEGER NOT NULL CHECK (high_water_sequence>=0),
  journal_ref TEXT NOT NULL,
  head_sha256 TEXT CHECK (head_sha256 IS NULL OR (length(head_sha256)=64 AND head_sha256 NOT GLOB '*[^0-9a-f]*')),
  state TEXT NOT NULL CHECK (state IN ('open','sealed')),
  PRIMARY KEY (organization_id,office_id,stream_kind,incarnation),
  FOREIGN KEY (organization_id,office_id) REFERENCES offices(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE office_recovery_checkpoints (
  organization_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  hq_office_id TEXT NOT NULL,
  authority_epoch INTEGER NOT NULL CHECK (authority_epoch>0),
  highest_seen_epoch INTEGER NOT NULL CHECK (highest_seen_epoch>=authority_epoch),
  applied_sequence INTEGER NOT NULL CHECK (applied_sequence>=0),
  digest_sha256 TEXT NOT NULL CHECK (length(digest_sha256)=64 AND digest_sha256 NOT GLOB '*[^0-9a-f]*'),
  base_sequence INTEGER NOT NULL CHECK (base_sequence>=0 AND base_sequence<=applied_sequence),
  base_digest_sha256 TEXT NOT NULL CHECK (length(base_digest_sha256)=64 AND base_digest_sha256 NOT GLOB '*[^0-9a-f]*'),
  protocol_level INTEGER NOT NULL CHECK (protocol_level>0),
  restore_pending INTEGER NOT NULL CHECK (restore_pending IN (0,1)),
  awaiting_snapshot INTEGER NOT NULL CHECK (awaiting_snapshot IN (0,1)),
  native_sequence INTEGER NOT NULL CHECK (native_sequence>=0),
  pending_base_sha256 TEXT CHECK (pending_base_sha256 IS NULL OR (length(pending_base_sha256)=64 AND pending_base_sha256 NOT GLOB '*[^0-9a-f]*')),
  journal_ref TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (organization_id,office_id),
  CHECK ((authority_epoch<highest_seen_epoch)=(pending_base_sha256 IS NOT NULL)),
  CHECK (authority_epoch=highest_seen_epoch OR restore_pending=1),
  FOREIGN KEY (organization_id,office_id) REFERENCES offices(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,hq_office_id) REFERENCES offices(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE recovery_snapshot_transfers (
  organization_id TEXT NOT NULL,
  id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  authority_epoch INTEGER NOT NULL CHECK (authority_epoch>0),
  base_sequence INTEGER NOT NULL CHECK (base_sequence>=0),
  content_ref TEXT NOT NULL,
  content_sha256 TEXT NOT NULL CHECK (length(content_sha256)=64 AND content_sha256 NOT GLOB '*[^0-9a-f]*'),
  state TEXT NOT NULL CHECK (state IN ('staging','verified','installing','installed','rejected')),
  byte_count INTEGER NOT NULL CHECK (byte_count>=0),
  updated_at TEXT NOT NULL,
  PRIMARY KEY (organization_id,id),
  FOREIGN KEY (organization_id,office_id) REFERENCES offices(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE process_cleanup_records (
  organization_id TEXT NOT NULL,
  id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  execution_process_id TEXT,
  generation INTEGER NOT NULL CHECK (generation>0),
  ownership_mark TEXT NOT NULL CHECK (length(ownership_mark)>0),
  machine_id TEXT NOT NULL,
  boot_id TEXT,
  leader_pid INTEGER,
  leader_pgid INTEGER,
  leader_started TEXT,
  inventory_json TEXT NOT NULL CHECK (json_valid(inventory_json) AND json_type(inventory_json)='object'),
  state TEXT NOT NULL CHECK (state IN ('owned','cleanup_pending','cleaned','unverified')),
  updated_at TEXT NOT NULL,
  PRIMARY KEY (organization_id,id),
  UNIQUE (organization_id,office_id,ownership_mark),
  CHECK ((leader_pid IS NULL)=(leader_pgid IS NULL) AND (leader_pid IS NULL)=(leader_started IS NULL) AND (leader_pid IS NULL)=(boot_id IS NULL)),
  FOREIGN KEY (organization_id,office_id) REFERENCES offices(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,execution_process_id) REFERENCES agent_process_instances(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TRIGGER receipt_identity_immutable BEFORE UPDATE ON mutation_receipts
WHEN NEW.organization_id<>OLD.organization_id OR NEW.command_id<>OLD.command_id OR NEW.origin_office_id<>OLD.origin_office_id
 OR NEW.actor_kind<>OLD.actor_kind OR NEW.actor_id IS NOT OLD.actor_id OR NEW.command_kind<>OLD.command_kind OR NEW.request_sha256<>OLD.request_sha256
 OR (OLD.state IN ('completed','rejected','abandoned') AND (NEW.state<>OLD.state OR NEW.result_ref IS NOT OLD.result_ref))
BEGIN SELECT RAISE(ABORT,'receipt identity or terminal outcome changed'); END;
CREATE TRIGGER receipt_no_replace BEFORE INSERT ON mutation_receipts
WHEN EXISTS (SELECT 1 FROM mutation_receipts WHERE organization_id=NEW.organization_id AND command_id=NEW.command_id)
BEGIN SELECT RAISE(ABORT,'receipt command already registered'); END;
CREATE TRIGGER origin_stream_no_regression BEFORE UPDATE ON office_origin_streams
WHEN NEW.organization_id<>OLD.organization_id OR NEW.office_id<>OLD.office_id OR NEW.stream_kind<>OLD.stream_kind OR NEW.incarnation<>OLD.incarnation
 OR NEW.high_water_sequence<OLD.high_water_sequence OR (NEW.high_water_sequence=OLD.high_water_sequence AND NEW.head_sha256 IS NOT OLD.head_sha256)
 OR (OLD.state='sealed' AND (NEW.state<>OLD.state OR NEW.high_water_sequence<>OLD.high_water_sequence))
BEGIN SELECT RAISE(ABORT,'origin stream regressed or forked'); END;
CREATE TRIGGER origin_stream_no_replace BEFORE INSERT ON office_origin_streams
WHEN EXISTS (SELECT 1 FROM office_origin_streams WHERE organization_id=NEW.organization_id AND office_id=NEW.office_id AND stream_kind=NEW.stream_kind AND incarnation=NEW.incarnation)
BEGIN SELECT RAISE(ABORT,'origin identity already registered'); END;
CREATE TRIGGER checkpoint_no_regression BEFORE UPDATE ON office_recovery_checkpoints
WHEN NEW.organization_id<>OLD.organization_id OR NEW.office_id<>OLD.office_id OR NEW.highest_seen_epoch<OLD.highest_seen_epoch
 OR NEW.authority_epoch<OLD.authority_epoch OR (NEW.authority_epoch=OLD.authority_epoch AND NEW.applied_sequence<OLD.applied_sequence)
 OR (NEW.authority_epoch=OLD.authority_epoch AND NEW.applied_sequence=OLD.applied_sequence AND NEW.digest_sha256<>OLD.digest_sha256)
BEGIN SELECT RAISE(ABORT,'recovery checkpoint regressed or forked'); END;
CREATE TRIGGER checkpoint_no_replace BEFORE INSERT ON office_recovery_checkpoints
WHEN EXISTS (SELECT 1 FROM office_recovery_checkpoints WHERE organization_id=NEW.organization_id AND office_id=NEW.office_id)
BEGIN SELECT RAISE(ABORT,'checkpoint identity already registered'); END;
CREATE TRIGGER receipt_no_delete BEFORE DELETE ON mutation_receipts
BEGIN SELECT RAISE(ABORT,'receipt retention requires reviewed expiry/archive procedure'); END;
CREATE TRIGGER origin_stream_no_delete BEFORE DELETE ON office_origin_streams
BEGIN SELECT RAISE(ABORT,'origin watermark must be retained'); END;
CREATE TRIGGER checkpoint_no_delete BEFORE DELETE ON office_recovery_checkpoints
BEGIN SELECT RAISE(ABORT,'authority checkpoint must be retained'); END;

CREATE TABLE budgets (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  id TEXT NOT NULL,
  schema_version INTEGER NOT NULL DEFAULT 1 CHECK (schema_version=1),
  current_revision INTEGER NOT NULL CHECK (current_revision>0),
  scope_kind TEXT NOT NULL CHECK (scope_kind IN ('task','agent','release')),
  task_id TEXT,
  agent_id TEXT,
  release_id TEXT,
  period_kind TEXT NOT NULL CHECK (period_kind IN ('lifetime','day','month')),
  time_zone TEXT NOT NULL,
  time_zone_source TEXT NOT NULL CHECK (time_zone_source IN ('owner_account','hq')),
  time_zone_subject_ref TEXT,
  created_at TEXT NOT NULL,
  CHECK ((time_zone_source='owner_account')=(time_zone_subject_ref IS NOT NULL)),
  PRIMARY KEY (organization_id,id),
  CHECK ((scope_kind='task' AND task_id IS NOT NULL AND agent_id IS NULL AND release_id IS NULL AND period_kind='lifetime')
    OR (scope_kind='agent' AND task_id IS NULL AND agent_id IS NOT NULL AND release_id IS NULL AND period_kind IN ('day','month'))
    OR (scope_kind='release' AND task_id IS NULL AND agent_id IS NULL AND release_id IS NOT NULL AND period_kind='lifetime')),
  FOREIGN KEY (organization_id,task_id) REFERENCES tasks(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,agent_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,release_id) REFERENCES releases(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,id,current_revision) REFERENCES budget_policy_versions(organization_id,budget_id,revision) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE UNIQUE INDEX budget_task_identity ON budgets(organization_id,task_id) WHERE scope_kind='task';
CREATE UNIQUE INDEX budget_agent_identity ON budgets(organization_id,agent_id,period_kind) WHERE scope_kind='agent';
CREATE UNIQUE INDEX budget_release_identity ON budgets(organization_id,release_id) WHERE scope_kind='release';
CREATE TABLE budget_policy_versions (
  organization_id TEXT NOT NULL,
  budget_id TEXT NOT NULL,
  revision INTEGER NOT NULL CHECK (revision>0),
  state TEXT NOT NULL CHECK (state IN ('active','cleared')),
  unit TEXT NOT NULL CHECK (unit IN ('micro_usd','tokens')),
  limit_amount INTEGER CHECK (limit_amount>0),
  changed_by_id TEXT NOT NULL,
  changed_at TEXT NOT NULL,
  reason TEXT,
  PRIMARY KEY (organization_id,budget_id,revision),
  CHECK ((state='active' AND limit_amount IS NOT NULL) OR (state='cleared' AND limit_amount IS NULL)),
  FOREIGN KEY (organization_id,budget_id) REFERENCES budgets(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,changed_by_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE budget_periods (
  organization_id TEXT NOT NULL,
  budget_id TEXT NOT NULL,
  id TEXT NOT NULL,
  policy_revision INTEGER NOT NULL,
  period_key TEXT NOT NULL,
  starts_at TEXT,
  ends_at TEXT,
  used_amount INTEGER NOT NULL DEFAULT 0 CHECK (used_amount>=0),
  usage_status TEXT NOT NULL DEFAULT 'unavailable' CHECK (usage_status IN ('complete','partial','unavailable')),
  includes_estimates INTEGER NOT NULL DEFAULT 0 CHECK (includes_estimates IN (0,1)),
  includes_api_equivalent INTEGER NOT NULL DEFAULT 0 CHECK (includes_api_equivalent IN (0,1)),
  unpriced_records INTEGER NOT NULL DEFAULT 0 CHECK (unpriced_records>=0),
  incomplete_records INTEGER NOT NULL DEFAULT 0 CHECK (incomplete_records>=0),
  extension_used INTEGER NOT NULL DEFAULT 0 CHECK (extension_used IN (0,1)),
  kept_paused INTEGER NOT NULL DEFAULT 0 CHECK (kept_paused IN (0,1)),
  state TEXT NOT NULL CHECK (state IN ('open','closed')),
  evaluated_at TEXT,
  ledger_checkpoint_ref TEXT,
  PRIMARY KEY (organization_id,id),
  UNIQUE (organization_id,budget_id,id),
  UNIQUE (organization_id,budget_id,period_key),
  FOREIGN KEY (organization_id,budget_id,policy_revision) REFERENCES budget_policy_versions(organization_id,budget_id,revision) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE budget_events (
  organization_id TEXT NOT NULL,
  id TEXT NOT NULL,
  budget_id TEXT NOT NULL,
  period_id TEXT NOT NULL,
  operation_id TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('evaluated','warning','reached','extended','policy_changed','cleared','stop_requested','hold_released','period_reset')),
  policy_revision INTEGER NOT NULL,
  actor_kind TEXT NOT NULL CHECK (actor_kind IN ('employee','system')),
  actor_id TEXT,
  occurred_at TEXT NOT NULL,
  payload_json TEXT NOT NULL CHECK (json_valid(payload_json) AND json_type(payload_json)='object'),
  PRIMARY KEY (organization_id,id),
  UNIQUE (organization_id,budget_id,operation_id),
  CHECK ((actor_kind='employee')=(actor_id IS NOT NULL)),
  FOREIGN KEY (organization_id,budget_id,period_id) REFERENCES budget_periods(organization_id,budget_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,budget_id,policy_revision) REFERENCES budget_policy_versions(organization_id,budget_id,revision) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,actor_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE budget_notices (
  organization_id TEXT NOT NULL,
  id TEXT NOT NULL,
  budget_id TEXT NOT NULL,
  period_id TEXT NOT NULL,
  policy_revision INTEGER NOT NULL,
  stage TEXT NOT NULL CHECK (stage IN ('base','extended')),
  threshold INTEGER NOT NULL CHECK (threshold IN (80,100)),
  recipient_kind TEXT NOT NULL CHECK (recipient_kind IN ('employee','owner')),
  recipient_id TEXT,
  kind TEXT NOT NULL CHECK (kind IN ('neutral','budget_reached')),
  delivery_state TEXT NOT NULL CHECK (delivery_state IN ('pending','delivered','resolved')),
  created_at TEXT NOT NULL,
  resolved_at TEXT,
  PRIMARY KEY (organization_id,id),
  UNIQUE (organization_id,budget_id,period_id,policy_revision,stage,threshold),
  CHECK ((recipient_kind='employee')=(recipient_id IS NOT NULL)),
  CHECK (threshold=100 OR kind='neutral'),
  FOREIGN KEY (organization_id,budget_id,period_id) REFERENCES budget_periods(organization_id,budget_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,budget_id,policy_revision) REFERENCES budget_policy_versions(organization_id,budget_id,revision) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,recipient_id) REFERENCES employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE budget_holds (
  organization_id TEXT NOT NULL,
  budget_id TEXT NOT NULL,
  period_id TEXT NOT NULL,
  policy_revision INTEGER NOT NULL,
  stage TEXT NOT NULL CHECK (stage IN ('base','extended')),
  state TEXT NOT NULL CHECK (state IN ('stop_pending','held','released')),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  release_reason TEXT,
  PRIMARY KEY (organization_id,budget_id,period_id,policy_revision,stage),
  FOREIGN KEY (organization_id,budget_id,period_id) REFERENCES budget_periods(organization_id,budget_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,budget_id,policy_revision) REFERENCES budget_policy_versions(organization_id,budget_id,revision) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TRIGGER budget_target_immutable BEFORE UPDATE ON budgets
WHEN NEW.organization_id<>OLD.organization_id OR NEW.id<>OLD.id OR NEW.scope_kind<>OLD.scope_kind OR NEW.task_id IS NOT OLD.task_id
 OR NEW.agent_id IS NOT OLD.agent_id OR NEW.release_id IS NOT OLD.release_id OR NEW.period_kind<>OLD.period_kind OR NEW.time_zone<>OLD.time_zone OR NEW.time_zone_source<>OLD.time_zone_source OR NEW.time_zone_subject_ref IS NOT OLD.time_zone_subject_ref
 OR NEW.current_revision<OLD.current_revision
BEGIN SELECT RAISE(ABORT,'budget scope or revision changed unsafely'); END;
CREATE TRIGGER budget_agent_insert BEFORE INSERT ON budgets
WHEN NEW.agent_id IS NOT NULL AND EXISTS (SELECT 1 FROM employees WHERE organization_id=NEW.organization_id AND id=NEW.agent_id AND kind<>'agent')
BEGIN SELECT RAISE(ABORT,'agent budget requires an agent'); END;
CREATE TRIGGER employee_budget_kind_update BEFORE UPDATE OF kind ON employees
WHEN NEW.kind<>'agent' AND EXISTS (SELECT 1 FROM budgets WHERE organization_id=NEW.organization_id AND agent_id=NEW.id)
BEGIN SELECT RAISE(ABORT,'budgeted employee must remain an agent'); END;
CREATE TRIGGER employee_budget_kind_insert BEFORE INSERT ON employees
WHEN NEW.kind<>'agent' AND EXISTS (SELECT 1 FROM budgets WHERE organization_id=NEW.organization_id AND agent_id=NEW.id)
BEGIN SELECT RAISE(ABORT,'budgeted employee must be an agent'); END;
CREATE TRIGGER budgets_no_delete BEFORE DELETE ON budgets
BEGIN SELECT RAISE(ABORT,'budget identity/history must be retained'); END;
CREATE TRIGGER budget_policy_versions_no_update BEFORE UPDATE ON budget_policy_versions
BEGIN SELECT RAISE(ABORT,'budget history is immutable'); END;
CREATE TRIGGER budget_policy_versions_no_delete BEFORE DELETE ON budget_policy_versions
BEGIN SELECT RAISE(ABORT,'budget identity/history must be retained'); END;
CREATE TRIGGER budget_periods_no_delete BEFORE DELETE ON budget_periods
BEGIN SELECT RAISE(ABORT,'budget identity/history must be retained'); END;
CREATE TRIGGER budget_events_no_update BEFORE UPDATE ON budget_events
BEGIN SELECT RAISE(ABORT,'budget history is immutable'); END;
CREATE TRIGGER budget_events_no_delete BEFORE DELETE ON budget_events
BEGIN SELECT RAISE(ABORT,'budget identity/history must be retained'); END;
CREATE TRIGGER budget_notices_no_delete BEFORE DELETE ON budget_notices
BEGIN SELECT RAISE(ABORT,'budget identity/history must be retained'); END;
CREATE TRIGGER budget_holds_no_delete BEFORE DELETE ON budget_holds
BEGIN SELECT RAISE(ABORT,'budget identity/history must be retained'); END;

CREATE TRIGGER budget_period_identity_update BEFORE UPDATE ON budget_periods
WHEN NEW.organization_id<>OLD.organization_id OR NEW.id<>OLD.id OR NEW.budget_id<>OLD.budget_id OR NEW.period_key<>OLD.period_key
 OR NEW.starts_at IS NOT OLD.starts_at OR NEW.ends_at IS NOT OLD.ends_at
 OR NEW.policy_revision<OLD.policy_revision OR (NEW.policy_revision=OLD.policy_revision AND NEW.extension_used<OLD.extension_used)
BEGIN SELECT RAISE(ABORT,'budget period identity/extension regressed'); END;
CREATE TRIGGER budget_release_no_hold BEFORE INSERT ON budget_holds
WHEN EXISTS (SELECT 1 FROM budgets WHERE organization_id=NEW.organization_id AND id=NEW.budget_id AND scope_kind='release')
BEGIN SELECT RAISE(ABORT,'release caps do not stop work'); END;
CREATE TRIGGER budget_release_no_hold_update BEFORE UPDATE ON budget_holds
WHEN EXISTS (SELECT 1 FROM budgets WHERE organization_id=NEW.organization_id AND id=NEW.budget_id AND scope_kind='release')
BEGIN SELECT RAISE(ABORT,'release caps do not stop work'); END;

CREATE VIEW milestone_progress AS
SELECT m.organization_id, m.id AS milestone_id, m.release_id,
  COUNT(t.id) AS task_count,
  COALESCE(SUM(t.status IN ('done', 'cancelled')), 0) AS done_task_count,
  (COUNT(t.id) > 0 AND COUNT(t.id) = COALESCE(SUM(t.status IN ('done', 'cancelled')), 0)) AS is_done
FROM milestones m
LEFT JOIN tasks t ON t.organization_id = m.organization_id AND t.milestone_id = m.id
GROUP BY m.organization_id, m.id;

CREATE VIEW release_progress AS
SELECT r.organization_id, r.id AS release_id,
  (SELECT COUNT(*) FROM milestones m WHERE m.organization_id = r.organization_id AND m.release_id = r.id) AS milestone_count,
  CASE
    WHEN EXISTS (SELECT 1 FROM milestones m WHERE m.organization_id = r.organization_id AND m.release_id = r.id)
      THEN (SELECT COALESCE(SUM(p.task_count), 0) FROM milestone_progress p
            WHERE p.organization_id = r.organization_id AND p.release_id = r.id)
    ELSE (SELECT COUNT(*) FROM release_tasks rt
            JOIN tasks t ON t.organization_id = rt.organization_id AND t.id = rt.task_id
          WHERE rt.organization_id = r.organization_id AND rt.release_id = r.id)
  END AS task_count,
  CASE
    WHEN EXISTS (SELECT 1 FROM milestones m WHERE m.organization_id = r.organization_id AND m.release_id = r.id)
      THEN (SELECT COALESCE(SUM(p.done_task_count), 0) FROM milestone_progress p
            WHERE p.organization_id = r.organization_id AND p.release_id = r.id)
    ELSE (SELECT COUNT(*) FROM release_tasks rt
            JOIN tasks t ON t.organization_id = rt.organization_id AND t.id = rt.task_id
          WHERE rt.organization_id = r.organization_id AND rt.release_id = r.id AND t.status IN ('done', 'cancelled'))
  END AS done_task_count,
  CASE
    WHEN EXISTS (SELECT 1 FROM milestones m WHERE m.organization_id = r.organization_id AND m.release_id = r.id)
      THEN NOT EXISTS (SELECT 1 FROM milestone_progress p
                       WHERE p.organization_id = r.organization_id AND p.release_id = r.id AND NOT p.is_done)
    ELSE EXISTS (SELECT 1 FROM release_tasks rt
                 JOIN tasks t ON t.organization_id = rt.organization_id AND t.id = rt.task_id
                 WHERE rt.organization_id = r.organization_id AND rt.release_id = r.id)
      AND NOT EXISTS (SELECT 1 FROM release_tasks rt
                      JOIN tasks t ON t.organization_id = rt.organization_id AND t.id = rt.task_id
                      WHERE rt.organization_id = r.organization_id AND rt.release_id = r.id AND t.status NOT IN ('done', 'cancelled'))
  END AS is_done
FROM releases r;

-- Document 18: development processes and project documentation
CREATE TABLE development_processes (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  id TEXT NOT NULL,
  key TEXT NOT NULL CHECK (
    length(key) BETWEEN 1 AND 64 AND key NOT GLOB '*[^a-z0-9-]*'
    AND substr(key, 1, 1) GLOB '[a-z0-9]'),
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
  origin TEXT NOT NULL CHECK (origin IN ('built_in', 'organization')),
  name TEXT NOT NULL,
  summary TEXT NOT NULL DEFAULT '',
  version_label TEXT,
  status TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'current', 'deprecated')),
  owner_employee_id TEXT,
  copied_from_process_id TEXT,
  stages_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(stages_json) AND json_type(stages_json) = 'array'),
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'system', 'unknown')),
  updated_by_id TEXT,
  PRIMARY KEY (organization_id, id),
  UNIQUE (organization_id, key),
  CHECK (origin = 'built_in' OR status <> 'current' OR owner_employee_id IS NOT NULL),
  CHECK (copied_from_process_id IS NULL OR copied_from_process_id <> id),
  FOREIGN KEY (organization_id, owner_employee_id) REFERENCES employees(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED,
  CHECK ((created_by_kind = 'employee' AND created_by_id IS NOT NULL) OR (created_by_kind <> 'employee' AND created_by_id IS NULL)),
  CHECK ((updated_by_kind = 'employee' AND updated_by_id IS NOT NULL) OR (updated_by_kind <> 'employee' AND updated_by_id IS NULL)),
  FOREIGN KEY (organization_id, created_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, updated_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE development_process_roles (
  organization_id TEXT NOT NULL,
  process_id TEXT NOT NULL,
  key TEXT NOT NULL CHECK (
    length(key) BETWEEN 1 AND 64 AND key NOT GLOB '*[^a-z0-9-]*'
    AND substr(key, 1, 1) GLOB '[a-z0-9]'),
  position INTEGER NOT NULL CHECK (position >= 0),
  name TEXT NOT NULL,
  seed INTEGER NOT NULL DEFAULT 0 CHECK (seed IN (0, 1)),
  hats_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(hats_json) AND json_type(hats_json) = 'array'),
  body_markdown TEXT NOT NULL DEFAULT '',
  PRIMARY KEY (organization_id, process_id, key),
  FOREIGN KEY (organization_id, process_id) REFERENCES development_processes(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE development_process_skills (
  organization_id TEXT NOT NULL,
  process_id TEXT NOT NULL,
  key TEXT NOT NULL CHECK (
    length(key) BETWEEN 1 AND 64 AND key NOT GLOB '*[^a-z0-9-]*'
    AND substr(key, 1, 1) GLOB '[a-z0-9]'),
  position INTEGER NOT NULL CHECK (position >= 0),
  title TEXT NOT NULL,
  description TEXT NOT NULL CHECK (length(description) > 0),
  body_markdown TEXT NOT NULL DEFAULT '',
  PRIMARY KEY (organization_id, process_id, key),
  FOREIGN KEY (organization_id, process_id) REFERENCES development_processes(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE development_process_skill_roles (
  organization_id TEXT NOT NULL,
  process_id TEXT NOT NULL,
  skill_key TEXT NOT NULL,
  role_key TEXT NOT NULL,
  PRIMARY KEY (organization_id, process_id, skill_key, role_key),
  FOREIGN KEY (organization_id, process_id, skill_key) REFERENCES development_process_skills(organization_id, process_id, key)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, process_id, role_key) REFERENCES development_process_roles(organization_id, process_id, key)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE development_process_kinds (
  organization_id TEXT NOT NULL,
  process_id TEXT NOT NULL,
  key TEXT NOT NULL CHECK (
    length(key) BETWEEN 1 AND 64 AND key NOT GLOB '*[^a-z0-9-]*'
    AND substr(key, 1, 1) GLOB '[a-z0-9]'
    AND key NOT IN ('brief', 'decision', 'design', 'report', 'changelog')),
  title TEXT NOT NULL,
  versioned INTEGER NOT NULL DEFAULT 0 CHECK (versioned IN (0, 1)),
  template_markdown TEXT NOT NULL DEFAULT '',
  PRIMARY KEY (organization_id, process_id, key),
  FOREIGN KEY (organization_id, process_id) REFERENCES development_processes(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE development_process_folders (
  organization_id TEXT NOT NULL,
  process_id TEXT NOT NULL,
  path TEXT NOT NULL CHECK (
    length(path) BETWEEN 1 AND 256 AND path NOT GLOB '*[^a-z0-9/_-]*'
    AND substr(path, 1, 1) GLOB '[a-z0-9]' AND substr(path, -1, 1) <> '/'
    AND instr(path, '//') = 0),
  position INTEGER NOT NULL CHECK (position >= 0),
  title TEXT NOT NULL,
  summary TEXT NOT NULL DEFAULT '',
  kinds_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(kinds_json) AND json_type(kinds_json) = 'array'),
  PRIMARY KEY (organization_id, process_id, path),
  FOREIGN KEY (organization_id, process_id) REFERENCES development_processes(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE development_process_tabs (
  organization_id TEXT NOT NULL,
  process_id TEXT NOT NULL,
  key TEXT NOT NULL CHECK (
    length(key) BETWEEN 1 AND 64 AND key NOT GLOB '*[^a-z0-9-]*'
    AND substr(key, 1, 1) GLOB '[a-z0-9]'
    AND key NOT IN ('overview', 'work', 'docs', 'team', 'activity', 'conversations')),
  position INTEGER NOT NULL CHECK (position BETWEEN 0 AND 7),
  title TEXT NOT NULL CHECK (length(title) > 0),
  view TEXT NOT NULL CHECK (view IN ('documents', 'work', 'features', 'releases')),
  config_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(config_json) AND json_type(config_json) = 'object'),
  PRIMARY KEY (organization_id, process_id, key),
  UNIQUE (organization_id, process_id, position),
  FOREIGN KEY (organization_id, process_id) REFERENCES development_processes(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE documentation_sections (
  organization_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  path TEXT NOT NULL CHECK (
    length(path) BETWEEN 1 AND 256 AND path NOT GLOB '*[^a-z0-9/_-]*'
    AND substr(path, 1, 1) GLOB '[a-z0-9]' AND substr(path, -1, 1) <> '/'
    AND instr(path, '//') = 0),
  position INTEGER NOT NULL CHECK (position >= 0),
  title TEXT NOT NULL,
  summary TEXT NOT NULL DEFAULT '',
  kinds_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(kinds_json) AND json_type(kinds_json) = 'array'),
  PRIMARY KEY (organization_id, project_id, path),
  FOREIGN KEY (organization_id, project_id) REFERENCES projects(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE documentation_media (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  id TEXT NOT NULL,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
  project_id TEXT NOT NULL,
  path TEXT NOT NULL CHECK (
    length(path) BETWEEN 3 AND 512 AND path NOT GLOB '*[^a-z0-9/._-]*'
    AND substr(path, 1, 1) GLOB '[a-z0-9]' AND path NOT GLOB '*.md'
    AND instr(path, '//') = 0 AND instr(path, '..') = 0 AND instr(path, '/.') = 0),
  type TEXT NOT NULL CHECK (type IN ('image', 'diagram')),
  media_type TEXT NOT NULL CHECK (length(media_type) > 0),
  bytes INTEGER NOT NULL CHECK (bytes BETWEEN 1 AND 5242880),
  sha256 TEXT NOT NULL CHECK (length(sha256) = 64 AND sha256 NOT GLOB '*[^0-9a-f]*'),
  title TEXT NOT NULL,
  summary TEXT NOT NULL DEFAULT '',
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'system', 'unknown')),
  updated_by_id TEXT,
  PRIMARY KEY (organization_id, id),
  UNIQUE (organization_id, project_id, path),
  FOREIGN KEY (organization_id, project_id) REFERENCES projects(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED,
  CHECK ((created_by_kind = 'employee' AND created_by_id IS NOT NULL) OR (created_by_kind <> 'employee' AND created_by_id IS NULL)),
  CHECK ((updated_by_kind = 'employee' AND updated_by_id IS NOT NULL) OR (updated_by_kind <> 'employee' AND updated_by_id IS NULL)),
  FOREIGN KEY (organization_id, created_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, updated_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE document_versions (
  organization_id TEXT NOT NULL REFERENCES organizations(id),
  document_id TEXT NOT NULL,
  version INTEGER NOT NULL CHECK (version >= 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
  title TEXT NOT NULL,
  body_sha256 TEXT NOT NULL CHECK (length(body_sha256) = 64 AND body_sha256 NOT GLOB '*[^0-9a-f]*'),
  body_markdown TEXT NOT NULL,
  note TEXT NOT NULL DEFAULT '',
  issued_at TEXT NOT NULL,
  issued_by_kind TEXT NOT NULL CHECK (issued_by_kind IN ('employee', 'system')),
  issued_by_id TEXT,
  PRIMARY KEY (organization_id, document_id, version),
  CHECK ((issued_by_kind = 'employee' AND issued_by_id IS NOT NULL) OR (issued_by_kind <> 'employee' AND issued_by_id IS NULL)),
  FOREIGN KEY (organization_id, issued_by_id) REFERENCES employees(organization_id, id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TRIGGER document_versions_are_written_once
BEFORE UPDATE ON document_versions
BEGIN
  SELECT RAISE(ABORT, 'an issued document version is never changed');
END;

CREATE TRIGGER document_versions_are_kept
BEFORE DELETE ON document_versions
BEGIN
  SELECT RAISE(ABORT, 'an issued document version is never removed');
END;

CREATE TABLE document_version_sources (
  organization_id TEXT NOT NULL,
  document_id TEXT NOT NULL,
  version INTEGER NOT NULL,
  source_document_id TEXT NOT NULL,
  source_version INTEGER NOT NULL,
  PRIMARY KEY (organization_id, document_id, version, source_document_id, source_version),
  CHECK (source_document_id <> document_id),
  FOREIGN KEY (organization_id, document_id, version) REFERENCES document_versions(organization_id, document_id, version)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, source_document_id, source_version) REFERENCES document_versions(organization_id, document_id, version)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE document_code_links (
  organization_id TEXT NOT NULL,
  id TEXT NOT NULL,
  document_id TEXT NOT NULL,
  repository_id TEXT NOT NULL,
  path TEXT NOT NULL CHECK (
    length(path) > 0 AND substr(path, 1, 1) <> '/' AND path <> '..'
    AND path NOT GLOB '../*' AND path NOT GLOB '*/../*' AND path NOT GLOB '*/..'),
  symbol TEXT,
  note TEXT NOT NULL DEFAULT '',
  PRIMARY KEY (organization_id, id),
  FOREIGN KEY (organization_id, document_id) REFERENCES documents(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, repository_id) REFERENCES repositories(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE INDEX document_code_links_by_path ON document_code_links (organization_id, repository_id, path);

CREATE TABLE task_document_versions (
  organization_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  document_id TEXT NOT NULL,
  version INTEGER NOT NULL,
  PRIMARY KEY (organization_id, task_id, document_id, version),
  FOREIGN KEY (organization_id, task_id) REFERENCES tasks(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id, document_id, version) REFERENCES document_versions(organization_id, document_id, version)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE VIEW task_releases AS
SELECT t.organization_id, t.id AS task_id, m.release_id
FROM tasks t
JOIN milestones m ON m.organization_id = t.organization_id AND m.id = t.milestone_id
UNION
SELECT rt.organization_id, rt.task_id, rt.release_id
FROM release_tasks rt
JOIN tasks t ON t.organization_id = rt.organization_id AND t.id = rt.task_id;

CREATE VIEW document_version_progress AS
SELECT v.organization_id, v.document_id, v.version,
  COUNT(t.id) AS task_count,
  COALESCE(SUM(t.status IN ('done', 'cancelled')), 0) AS done_task_count,
  (COUNT(t.id) > 0 AND COUNT(t.id) = COALESCE(SUM(t.status IN ('done', 'cancelled')), 0)) AS is_converged
FROM document_versions v
LEFT JOIN task_document_versions r
  ON r.organization_id = v.organization_id AND r.document_id = v.document_id AND r.version = v.version
LEFT JOIN tasks t ON t.organization_id = r.organization_id AND t.id = r.task_id
GROUP BY v.organization_id, v.document_id, v.version;
