-- Apply after schema.sql in the shared office database. Source: document 11.
ALTER TABLE agent_placements ADD COLUMN placement_generation INTEGER NOT NULL DEFAULT 1 CHECK (placement_generation>=1);

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
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee','owner','system','unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee','owner','system','unknown')),
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
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee','owner','system','unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee','owner','system','unknown')),
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
  created_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (created_by_kind IN ('employee','owner','system','unknown')),
  created_by_id TEXT,
  updated_at TEXT,
  updated_by_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (updated_by_kind IN ('employee','owner','system','unknown')),
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
  PRIMARY KEY (organization_id,id),
  UNIQUE (organization_id,id,employee_id,office_id,placement_generation),
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
