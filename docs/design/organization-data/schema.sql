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
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'owner', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'owner', 'system', 'unknown')),
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
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'owner', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'owner', 'system', 'unknown')),
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
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'owner', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'owner', 'system', 'unknown')),
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
  number INTEGER CHECK (number > 0),
  key TEXT GENERATED ALWAYS AS (CASE WHEN number IS NOT NULL THEN 'TEAM-' || number END) VIRTUAL,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
name TEXT NOT NULL,
  parent_team_id TEXT,
  lead_employee_id TEXT,
  status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'archived')),
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'owner', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'owner', 'system', 'unknown')),
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
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'owner', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'owner', 'system', 'unknown')),
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
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'owner', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'owner', 'system', 'unknown')),
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
  number INTEGER CHECK (number > 0),
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
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'owner', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'owner', 'system', 'unknown')),
  updated_by_id TEXT,
  PRIMARY KEY (organization_id, id),
UNIQUE (organization_id, number),
  UNIQUE (organization_id, slug),
  UNIQUE (organization_id, id, owner_team_id),
  FOREIGN KEY (organization_id, owner_team_id) REFERENCES teams(organization_id, id)
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
  number INTEGER CHECK (number > 0),
  key TEXT GENERATED ALWAYS AS (CASE WHEN number IS NOT NULL THEN 'R-' || number END) VIRTUAL,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
project_id TEXT NOT NULL,
  title TEXT NOT NULL,
  version_label TEXT,
  status TEXT NOT NULL,
  body_markdown TEXT NOT NULL DEFAULT '',
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'owner', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'owner', 'system', 'unknown')),
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
  number INTEGER CHECK (number > 0),
  key TEXT GENERATED ALWAYS AS (CASE WHEN number IS NOT NULL THEN 'M-' || number END) VIRTUAL,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
project_id TEXT NOT NULL,
  release_id TEXT NOT NULL,
  title TEXT NOT NULL,
  status TEXT NOT NULL,
  body_markdown TEXT NOT NULL DEFAULT '',
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'owner', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'owner', 'system', 'unknown')),
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
  kind TEXT NOT NULL CHECK (kind IN ('brief', 'decision', 'design', 'report', 'changelog')),
  owner_employee_id TEXT,
  title TEXT NOT NULL,
  body_markdown TEXT NOT NULL DEFAULT '',
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'owner', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'owner', 'system', 'unknown')),
  updated_by_id TEXT,
  PRIMARY KEY (organization_id, id),
  FOREIGN KEY (organization_id, owner_employee_id) REFERENCES employees(organization_id, id)
    DEFERRABLE INITIALLY DEFERRED,
  CHECK (status <> 'current' OR owner_employee_id IS NOT NULL),
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
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'owner', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'owner', 'system', 'unknown')),
  updated_by_id TEXT,
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
  number INTEGER CHECK (number > 0),
  key TEXT GENERATED ALWAYS AS (CASE WHEN number IS NOT NULL THEN 'T-' || number END) VIRTUAL,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  revision INTEGER NOT NULL CHECK (revision >= 1),
  team_id TEXT NOT NULL,
  assigned_team_id TEXT,
  project_id TEXT,
  type TEXT NOT NULL DEFAULT 'task' CHECK (length(type) > 0),
  work_type TEXT CHECK (work_type IN ('build', 'run')),
  title TEXT NOT NULL,
  status TEXT NOT NULL,
  assignee_id TEXT,
  body_markdown TEXT NOT NULL DEFAULT '',
  feature_id TEXT,
  milestone_id TEXT,
  priority TEXT NOT NULL DEFAULT 'normal',
  severity TEXT,
  resolution TEXT,
  blocked_reason TEXT,
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'owner', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'owner', 'system', 'unknown')),
  updated_by_id TEXT,
  PRIMARY KEY (organization_id, id),
  CHECK (project_id IS NOT NULL OR (feature_id IS NULL AND milestone_id IS NULL)),
  UNIQUE (organization_id, project_id, number),
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
  author_kind TEXT NOT NULL CHECK (author_kind IN ('employee', 'owner')),
  author_id TEXT,
  created_at TEXT NOT NULL,
  body_markdown TEXT NOT NULL,
  PRIMARY KEY (organization_id, target_kind, target_id, id),
  UNIQUE (organization_id, target_kind, target_id, position),
  CHECK ((author_kind = 'employee' AND author_id IS NOT NULL) OR (author_kind = 'owner' AND author_id IS NULL)),
  FOREIGN KEY (organization_id, target_kind, target_id) REFERENCES record_refs(organization_id, kind, id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE links (
  organization_id TEXT NOT NULL,
  source_kind TEXT NOT NULL CHECK (source_kind IN ('project', 'feature', 'task', 'document')),
  source_id TEXT NOT NULL,
  id TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('depends_on', 'relates_to')),
  target_kind TEXT NOT NULL,
  target_id TEXT NOT NULL,
  PRIMARY KEY (organization_id, source_kind, source_id, id),
  UNIQUE (organization_id, source_kind, source_id, kind, target_kind, target_id),
  CHECK (source_kind <> target_kind OR source_id <> target_id),
  CHECK (kind <> 'depends_on' OR (source_kind = 'task' AND target_kind = 'task')),
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
  status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'inactive')),
  created_at TEXT,
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'owner', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'owner', 'system', 'unknown')),
  updated_by_id TEXT,
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
    OR (scope_kind IN ('team', 'project') AND entity_kind = 'task')
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
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'owner', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'owner', 'system', 'unknown')),
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
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee', 'owner', 'system', 'unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee', 'owner', 'system', 'unknown')),
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
  created_by_kind TEXT NOT NULL CHECK (created_by_kind IN ('employee', 'owner', 'system', 'unknown')),
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
  recorded_by_kind TEXT NOT NULL CHECK (recorded_by_kind IN ('employee', 'owner', 'system', 'unknown')),
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
  attribution_basis TEXT NOT NULL CHECK (attribution_basis IN ('work_for', 'current_task', 'explicit_context', 'unattributed', 'manual_correction', 'legacy')),
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
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  occurred_at TEXT NOT NULL,
  actor_kind TEXT NOT NULL CHECK (actor_kind IN ('employee', 'owner', 'system', 'unknown')),
  actor_id TEXT,
  operation_id TEXT,
  change_json TEXT NOT NULL CHECK (
    json_valid(change_json) AND json_type(change_json) = 'object'
    AND json_type(change_json, '$.type') IS 'text'
    AND length(json_extract(change_json, '$.type')) > 0),
  source_metadata_json TEXT NOT NULL DEFAULT '{}' CHECK (
    json_valid(source_metadata_json) AND json_type(source_metadata_json) = 'object'),
  payload_sha256 TEXT NOT NULL CHECK (length(payload_sha256) = 64 AND payload_sha256 NOT GLOB '*[^0-9a-f]*'),
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
CREATE UNIQUE INDEX team_task_numbers ON tasks(organization_id, team_id, number) WHERE project_id IS NULL;
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
