-- Separate access-controlled budgets projection database (runtime/<office-id>/protected/budgets.db);
-- never load into the shared plaintext office.db projection (ADR-0016, TASK-10 / H-05).
PRAGMA foreign_keys = ON;
PRAGMA recursive_triggers = ON;

CREATE TABLE budget_organizations (
  id TEXT PRIMARY KEY
) STRICT;

CREATE TABLE budget_tasks (
  organization_id TEXT NOT NULL REFERENCES budget_organizations(id),
  kind TEXT NOT NULL DEFAULT 'task' CHECK (kind='task'),
  id TEXT NOT NULL,
  is_deleted INTEGER NOT NULL DEFAULT 0 CHECK (is_deleted IN (0,1)),
  PRIMARY KEY (organization_id, kind, id)
) STRICT;

CREATE TRIGGER budget_tasks_no_undelete BEFORE UPDATE ON budget_tasks
WHEN OLD.is_deleted = 1 AND NEW.is_deleted = 0
BEGIN SELECT RAISE(ABORT, 'deleted budget task mirror cannot be undeleted'); END;

CREATE TABLE budget_employees (
  organization_id TEXT NOT NULL REFERENCES budget_organizations(id),
  id TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('person','agent')),
  PRIMARY KEY (organization_id, id)
) STRICT;

CREATE TABLE budget_releases (
  organization_id TEXT NOT NULL REFERENCES budget_organizations(id),
  id TEXT NOT NULL,
  PRIMARY KEY (organization_id, id)
) STRICT;

CREATE TABLE budgets (
  organization_id TEXT NOT NULL REFERENCES budget_organizations(id),
  id TEXT NOT NULL,
  schema_version INTEGER NOT NULL DEFAULT 1 CHECK (schema_version=1),
  current_revision INTEGER NOT NULL CHECK (current_revision>0),
  scope_kind TEXT NOT NULL CHECK (scope_kind IN ('task','agent','release')),
  task_id TEXT,
  task_kind TEXT NOT NULL DEFAULT 'task' CHECK (task_kind='task'),
  agent_id TEXT,
  release_id TEXT,
  period_kind TEXT NOT NULL CHECK (period_kind IN ('lifetime','day','month')),
  time_zone TEXT NOT NULL,
  time_zone_source TEXT NOT NULL CHECK (time_zone_source IN ('owner_person','hq')),
  time_zone_subject_ref TEXT,
  created_at TEXT NOT NULL,
  CHECK ((time_zone_source='owner_person')=(time_zone_subject_ref IS NOT NULL)),
  PRIMARY KEY (organization_id,id),
  CHECK ((scope_kind='task' AND task_id IS NOT NULL AND agent_id IS NULL AND release_id IS NULL AND period_kind='lifetime')
    OR (scope_kind='agent' AND task_id IS NULL AND agent_id IS NOT NULL AND release_id IS NULL AND period_kind IN ('day','month'))
    OR (scope_kind='release' AND task_id IS NULL AND agent_id IS NULL AND release_id IS NOT NULL AND period_kind='lifetime')),
  FOREIGN KEY (organization_id,task_kind,task_id) REFERENCES budget_tasks(organization_id,kind,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,agent_id) REFERENCES budget_employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,release_id) REFERENCES budget_releases(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,time_zone_subject_ref) REFERENCES budget_employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
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
  time_zone TEXT NOT NULL DEFAULT 'UTC',
  time_zone_source TEXT NOT NULL DEFAULT 'hq' CHECK (time_zone_source IN ('owner_person','hq')),
  time_zone_subject_ref TEXT,
  changed_by_id TEXT NOT NULL,
  changed_at TEXT NOT NULL,
  reason TEXT,
  PRIMARY KEY (organization_id,budget_id,revision),
  CHECK ((state='active' AND limit_amount IS NOT NULL) OR (state='cleared' AND limit_amount IS NULL)),
  CHECK ((time_zone_source='owner_person')=(time_zone_subject_ref IS NOT NULL)),
  FOREIGN KEY (organization_id,budget_id) REFERENCES budgets(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,time_zone_subject_ref) REFERENCES budget_employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,changed_by_id) REFERENCES budget_employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED
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
  FOREIGN KEY (organization_id,actor_id) REFERENCES budget_employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED
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
  CHECK (threshold=100 OR stage='base'),
  FOREIGN KEY (organization_id,budget_id,period_id) REFERENCES budget_periods(organization_id,budget_id,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,budget_id,policy_revision) REFERENCES budget_policy_versions(organization_id,budget_id,revision) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (organization_id,recipient_id) REFERENCES budget_employees(organization_id,id) DEFERRABLE INITIALLY DEFERRED
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
CREATE INDEX active_budget_holds ON budget_holds(organization_id, budget_id, state) WHERE state <> 'released';

CREATE TRIGGER budget_target_immutable BEFORE UPDATE ON budgets
WHEN NEW.organization_id<>OLD.organization_id OR NEW.id<>OLD.id OR NEW.scope_kind<>OLD.scope_kind OR NEW.task_id IS NOT OLD.task_id OR NEW.task_kind<>OLD.task_kind
 OR NEW.agent_id IS NOT OLD.agent_id OR NEW.release_id IS NOT OLD.release_id OR NEW.period_kind<>OLD.period_kind
 OR NEW.current_revision<OLD.current_revision
 OR ((NEW.time_zone<>OLD.time_zone OR NEW.time_zone_source<>OLD.time_zone_source OR NEW.time_zone_subject_ref IS NOT OLD.time_zone_subject_ref)
     AND NEW.current_revision<=OLD.current_revision)
BEGIN SELECT RAISE(ABORT,'budget scope or revision changed unsafely'); END;

CREATE TRIGGER budget_agent_insert BEFORE INSERT ON budgets
WHEN NEW.agent_id IS NOT NULL AND EXISTS (SELECT 1 FROM budget_employees WHERE organization_id=NEW.organization_id AND id=NEW.agent_id AND kind<>'agent')
BEGIN SELECT RAISE(ABORT,'agent budget requires an agent'); END;

CREATE TRIGGER employee_budget_kind_update BEFORE UPDATE OF kind ON budget_employees
WHEN NEW.kind<>'agent' AND EXISTS (SELECT 1 FROM budgets WHERE organization_id=NEW.organization_id AND agent_id=NEW.id)
BEGIN SELECT RAISE(ABORT,'budgeted employee must remain an agent'); END;

CREATE TRIGGER employee_budget_kind_insert BEFORE INSERT ON budget_employees
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
