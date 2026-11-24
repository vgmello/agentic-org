-- Separate access-controlled messaging projection database (runtime/<office-id>/protected/messaging.db);
-- never load into the shared plaintext office.db projection (ADR-0016, TASK-10 / H-05).
PRAGMA foreign_keys = ON;
PRAGMA recursive_triggers = ON;

CREATE TABLE messaging_organizations (
  id TEXT PRIMARY KEY
) STRICT;

CREATE TABLE messaging_teams (
  organization_id TEXT NOT NULL REFERENCES messaging_organizations(id),
  id TEXT NOT NULL,
  PRIMARY KEY (organization_id, id)
) STRICT;

CREATE TABLE messaging_projects (
  organization_id TEXT NOT NULL REFERENCES messaging_organizations(id),
  id TEXT NOT NULL,
  PRIMARY KEY (organization_id, id)
) STRICT;

CREATE TABLE messaging_employees (
  organization_id TEXT NOT NULL REFERENCES messaging_organizations(id),
  id TEXT NOT NULL,
  PRIMARY KEY (organization_id, id)
) STRICT;

CREATE TABLE messaging_offices (
  organization_id TEXT NOT NULL REFERENCES messaging_organizations(id),
  id TEXT NOT NULL,
  PRIMARY KEY (organization_id, id)
) STRICT;

CREATE TABLE conversations (
  organization_id TEXT NOT NULL REFERENCES messaging_organizations(id),
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
  FOREIGN KEY (organization_id,team_id) REFERENCES messaging_teams(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,project_id) REFERENCES messaging_projects(organization_id,id) DEFERRABLE INITIALLY DEFERRED
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
  FOREIGN KEY (organization_id,employee_id) REFERENCES messaging_employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE INDEX conversation_memberships_by_employee ON conversation_memberships(organization_id, employee_id);

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
  org_epoch INTEGER NOT NULL CHECK (org_epoch>0),
  org_sequence INTEGER NOT NULL CHECK (org_sequence>=0),
  content_sha256 TEXT NOT NULL CHECK (length(content_sha256)=64 AND content_sha256 NOT GLOB '*[^0-9a-f]*'),
  PRIMARY KEY (organization_id,id),
  UNIQUE (organization_id,id,conversation_id),
  UNIQUE (organization_id,origin_office_id,origin_incarnation,origin_sequence),
  CHECK ((sender_kind='employee' AND sender_employee_id IS NOT NULL AND sender_office_id IS NULL)
    OR (sender_kind='office' AND sender_employee_id IS NULL AND sender_office_id IS NOT NULL)),
  FOREIGN KEY (organization_id,conversation_id) REFERENCES conversations(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,sender_employee_id) REFERENCES messaging_employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,sender_office_id) REFERENCES messaging_offices(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,origin_office_id) REFERENCES messaging_offices(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,thread_root_id,conversation_id) REFERENCES messages(organization_id,id,conversation_id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,reply_to_message_id,conversation_id) REFERENCES messages(organization_id,id,conversation_id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE INDEX message_conversation_time ON messages(organization_id,conversation_id,created_at,id);
CREATE INDEX messages_by_thread ON messages(organization_id, thread_root_id, created_at);
CREATE INDEX messages_by_sender ON messages(organization_id, sender_employee_id);

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
  org_epoch INTEGER NOT NULL CHECK (org_epoch>0),
  org_sequence INTEGER NOT NULL CHECK (org_sequence>=0),
  PRIMARY KEY (organization_id,message_id,employee_id),
  FOREIGN KEY (organization_id,message_id,conversation_id) REFERENCES messages(organization_id,id,conversation_id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,conversation_id,employee_id) REFERENCES conversation_memberships(organization_id,conversation_id,employee_id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,owner_office_id) REFERENCES messaging_offices(organization_id,id) DEFERRABLE INITIALLY DEFERRED
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
  FOREIGN KEY (organization_id,viewer_employee_id) REFERENCES messaging_employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,muted_employee_id) REFERENCES messaging_employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE conversation_read_markers (
  organization_id TEXT NOT NULL,
  conversation_id TEXT NOT NULL,
  viewer_employee_id TEXT NOT NULL,
  read_at TEXT,
  last_read_message_id TEXT,
  PRIMARY KEY (organization_id,conversation_id,viewer_employee_id),
  FOREIGN KEY (organization_id,conversation_id) REFERENCES conversations(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,viewer_employee_id) REFERENCES messaging_employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
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
  FOREIGN KEY (organization_id,employee_id) REFERENCES messaging_employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE INDEX message_reactions_by_employee ON message_reactions(organization_id, employee_id);
