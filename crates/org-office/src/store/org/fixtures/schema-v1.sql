-- Frozen legacy org.db DDL from v1 44d895657c323c0be79a3f72908bf083ef0434c6 before 2a.6a.
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;
             CREATE TABLE events (
                 epoch INTEGER NOT NULL, seq INTEGER NOT NULL,
                 kind TEXT NOT NULL, command_id TEXT NOT NULL, at TEXT NOT NULL,
                 body TEXT NOT NULL,
                 PRIMARY KEY (epoch, seq)
             ) STRICT;
             CREATE TABLE commands (
                 command_id TEXT PRIMARY KEY, epoch INTEGER NOT NULL, seq INTEGER NOT NULL,
                 expires_at TEXT NOT NULL, result TEXT
             ) STRICT;
             CREATE TABLE teams (
                 id TEXT PRIMARY KEY, owner_team_id TEXT, name_key TEXT NOT NULL,
                 lead_member_id TEXT, body TEXT NOT NULL
             ) STRICT;
             CREATE UNIQUE INDEX teams_sibling ON teams (COALESCE(owner_team_id, ''), name_key);
             CREATE INDEX teams_owner ON teams (owner_team_id);
             CREATE INDEX teams_lead ON teams (lead_member_id);
             CREATE TABLE members (
                 id TEXT PRIMARY KEY, handle_key TEXT, reports_to_member_id TEXT,
                 body TEXT NOT NULL
             ) STRICT;
             CREATE UNIQUE INDEX members_handle ON members (handle_key);
             CREATE INDEX members_reports_to ON members (reports_to_member_id);
             CREATE TABLE memberships (
                 id TEXT PRIMARY KEY, team_id TEXT NOT NULL, member_id TEXT NOT NULL,
                 body TEXT NOT NULL,
                 UNIQUE (team_id, member_id)
             ) STRICT;
             CREATE INDEX memberships_member ON memberships (member_id);
             CREATE TABLE project_teams (
                 id TEXT PRIMARY KEY, project_id TEXT NOT NULL, team_id TEXT NOT NULL,
                 body TEXT NOT NULL,
                 UNIQUE (project_id, team_id)
             ) STRICT;
             CREATE INDEX project_teams_team ON project_teams (team_id);
             CREATE TABLE projects (id TEXT PRIMARY KEY, body TEXT NOT NULL) STRICT;
             CREATE TABLE areas (
                 id TEXT PRIMARY KEY, project_id TEXT NOT NULL, body TEXT NOT NULL
             ) STRICT;
