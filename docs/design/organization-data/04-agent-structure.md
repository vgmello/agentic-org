# Agents and employees

Structural draft for v1. Document revision 2.8. This document fixes employee identity relationships. [Model profiles and agent execution](11-model-profiles-and-agent-execution.md) owns execution configuration, routing/lifecycle behavior and durable placement/fencing authority in the same integrated schema. Memory consolidation and scheduling remain separate pending work. [Document 05](05-operational-data.md#private-notes-attached-to-tasks) checks private task notes against v1.

## Confirmed relationships

```mermaid
erDiagram
    teams ||--o{ employees : home_team_id
    teams o|--o| employees : lead_employee_id
    roles o|--o{ employees : role_id
    employees o|--o{ employees : reports_to_employee_id
    employees ||--o| agent_execution_configs : execution_settings
    employees ||--o| agent_placements : current_placement
    offices ||--o{ agent_placements : hosts
    employees ||--o{ task_workspaces : assigned_work
    offices ||--o{ task_workspaces : contains
    task_repositories ||--o| task_workspaces : checked_out_as
    employees {
        TEXT organization_id PK, FK "required"
        TEXT id PK "required"
        TEXT handle "required"
        INTEGER schema_version "required"
        INTEGER revision "required"
        TEXT kind "required"
        TEXT name "required"
        TEXT home_team_id FK "required"
        TEXT role_id FK "optional"
        TEXT reports_to_employee_id FK "optional"
        TEXT body_markdown "required"
        TEXT status "required"
        TEXT created_at "optional"
        TEXT created_by_kind "required"
        TEXT created_by_id FK "optional"
        TEXT updated_at "optional"
        TEXT updated_by_kind "required"
        TEXT updated_by_id FK "optional"
        TEXT contact "optional"
        TEXT title "optional"
        TEXT focus "optional"
        TEXT role_text "optional"
        TEXT title_in_team "optional"
        INTEGER is_owner "required"
        TEXT agent_status "optional"
        TEXT offboarded_at "optional"
    }
    agent_placements {
        TEXT organization_id PK, FK "required"
        TEXT employee_id PK, FK "required"
        TEXT office_id FK "required"
        INTEGER placement_generation "required"
    }
    task_workspaces {
        TEXT organization_id PK, FK "required"
        TEXT office_id FK "required"
        TEXT employee_id FK "required"
        TEXT task_id PK, FK "required"
        TEXT repository_id PK, FK "required"
        TEXT worktree_path "required"
    }
```

Employees are agents or people, with `status: active | inactive` and common creator/update attribution. Inactive profiles retain their handles and stable IDs for historical references; new active task assignment or ownership must resolve to an active employee. [Record contracts](08-record-contracts.md) specifies lifecycle and attribution. Each has exactly one home team. A team can contain both kinds directly while owning child teams. Leadership is an explicit team appointment; ordinary members gain no managerial authority simply by being in an ancestor team. Explicit direct reporting references do not remove ancestor-lead management authority.

Office placement is separate from home-team membership. An agent can move between offices without changing its home team or stable identity. People have profiles and reporting relationships but no agent execution process or worktree. The table is a placement projection; reconstruction requires the durable authority generation and verified cleanup/fencing before any replacement starts. Cache reconstruction alone cannot recreate launch authority.

Agent token/cost totals are derived from [usage records](10-usage-and-cost-accounting.md) for that actual consumer, including task work and overhead across projects. The ledger references stable employee identity, not a live process; actual route/attempt context is supplied by document 11, while accounting remains the ledger authority.

## Durable identity files

```text
teams/<team-id>/
├── agents/<employee-id>/
│   ├── profile.md
│   └── execution.md      # Optional configuration; required before the agent is runnable
├── people/<employee-id>/profile.md
└── teams/<child-team-id>/...
```

```markdown
---
schema_version: 1
id: 01J00000000000000000000030
handle: ana
revision: 1
kind: agent
status: active
name: Ana
home_team_id: 01J00000000000000000000020
role_id: null
reports_to_employee_id: null
---
Reservations team employee identity. Execution configuration is stored separately in execution.md, as defined in document 11.
```

Agents and people share an organization-wide unique `handle`, stored without `@` and displayed as `@ana`. Handles are readable identity references; stable IDs remain the foreign keys. Preserve existing v1 handles on import where valid and resolve collisions explicitly. Agent provisioning and messaging transport remain pending specifications. Employee identity does not embed a model or credentials. An agent requires the separate execution.md profile/model/tool configuration to become runnable; people have no execution configuration. Memory and proactive scheduling remain outside this specification.

## Runtime location

```text
runtime/<office-id>/
├── repositories/<repository-id>.git/
└── teams/<root-team-id>/teams/<child-team-id>/.../
    └── agents/<employee-id>/
        ├── projects/<project-id>/tasks/<task-id>/repositories/<repository-id>/
        └── tasks/<task-id>/repositories/<repository-id>/  # Team tasks
```

The runtime team tree is the local office's projection of the organization-wide team hierarchy, containing only its placed agents and their necessary ancestors. One office's directories cannot define or replace organization-wide membership.

## File to SQL mapping

| Table | Source |
| --- | --- |
| `employees` | `profile.md`: stable identity/revision fields, required `handle`, `kind`, `name`, `home_team_id`, optional `role_id`, optional `reports_to_employee_id`, and body. File location and `kind` must agree. |
| `agent_placements` | Current placement projection, rebuilt from HQ's durable execution placement/generation journal. Only agents are eligible. No office_id is silently added to an employee profile. |
| `agent_execution_configs` | execution.md front matter/body; full table and routing contract are owned by document 11. |
| `task_workspaces` | Current workspace-controller state; runtime-only office/employee/task/repository IDs and `worktree_path`. |

### Execution and remaining boundaries

[Document 11](11-model-profiles-and-agent-execution.md) specifies the agent's model profile, target model, prompt/tools configuration, local credential bindings, supervision and turn/attempt lifecycle. This document retains employee/home-team/reporting/placement ownership; there is no competing agents identity. Provisioning workflows, private memory, proactive scheduling and a fully wired execution backend remain implementation or separately scoped work. [Task assignment](12-task-assignment-and-history.md) owns assignment/reporter semantics; [operational data](05-operational-data.md) owns task events/history and private notes. Execution owns processes.

## SQLite tables

```sql
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
  contact TEXT,
  title TEXT,
  focus TEXT,
  role_text TEXT,
  title_in_team TEXT,
  is_owner INTEGER NOT NULL DEFAULT 0 CHECK (is_owner IN (0,1)),
  agent_status TEXT CHECK (agent_status IN ('active','paused','offboarded')),
  offboarded_at TEXT,
  CHECK (kind='agent' OR (agent_status IS NULL AND offboarded_at IS NULL)),
  CHECK (offboarded_at IS NULL OR agent_status='offboarded'),
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
```

Direct reporting must be acyclic within an organization, independently of the team tree. The consolidated schema rejects cycles on insertion and update, including deferred-reference imports.

## Employee field reconciliation with v1

The same-named scalar fields below live in `profile.md` front matter and project into `employees`: `contact`, `title`, `focus`, `role_text`, `title_in_team`, `is_owner`, `agent_status`, `offboarded_at`. Nullable text remains NULL if not known. `is_owner` defaults to false; `agent_status` is optional legacy operating state (`active`, `paused`, `offboarded`), allowed only for agents. `offboarded_at` requires offboarded state; legacy records without the timestamp remain distinguishable from fabricated dates. `status` still describes active/inactive employment: import maps offboarded to inactive and active/paused to active. Commands/imports reject contradictory new records. Pause preserves employment and history; it is not removal. An offboarded profile keeps its handle.

`role_text` preserves v1's descriptive role independently of the normalized `role_id`; import must not invent a role ID from arbitrary text. `title_in_team` describes the home-team appointment, not a second membership. `is_owner` preserves the v1 organization-owner marker; it does not create a login, grant administrator privileges or replace the host's authenticated ownership record. Import requires at most one owner and reconciles it with that record. Team leadership remains a separate appointment.

V1 members can have several memberships. Import must require an explicit chosen home team; preserve unselected associations in migration staging rather than picking the first or copying all membership titles into one field. Runtime/model move to document 11's execution configuration; machine-specific home remains managed local state. Run modes, scheduling and memory remain deferred; no schedule engine or schedule table is introduced here.

```yaml
contact: "product@example.invalid"
title: "Product lead"
focus: "Guest booking"
role_text: "Product management"
title_in_team: "Lead"
is_owner: false
agent_status: paused
```

[Messaging and chat](15-messaging-and-chat.md) defines employee participation and conversation data independently of team membership and office placement.

[Recovery and office continuity](16-recovery-and-office-continuity.md) defines durable receipt/checkpoint/cleanup evidence separately from disposable office projections.

[Budgets and controls](17-budgets-and-controls.md) now selects the budget data model. Policy/extension/hold records must survive office cache loss; automatic pausing/evaluation still requires service implementation. Other deferred features remain unchanged.
