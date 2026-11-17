# v1 implementation plan

Written 2026-11-17 by the lead, after the architecture review. It orders the work so that
the two engineers and their agents can work inside the product as early as possible. The
order and the split into increments are the lead's proposal and may be changed by the
engineer who takes an increment. The behaviour is not a proposal: it is what the linked
requirement documents say. Where they are silent, the engineer decides, inside the
[architecture non-negotiables](architecture-non-negotiables.md), and writes the decision
down.

How to read an increment:

- **Outcome** is what a person or agent can do when it is finished.
- **Build** lists what has to exist. It is a summary; the requirement documents are the
  detail.
- **Read** lists the requirement documents and the acceptance criteria to make pass.
- **Code today** says where to start and what state it is in.
- **Done when** is the finish line.

New here? Read [START-HERE](START-HERE.md) first.

## The path

| Milestone | A person can | Track |
| --- | --- | --- |
| M1 | Install the server, create an organization and sign in from a browser | A |
| M2 | Add people and teams, give the second engineer an account, and run the product in a Linux container | A and B |
| M3 | Create, assign and follow tasks, in the browser and with `agent-os` | B |
| M4 | Chat, and ask a question that waits for an answer | B |
| M5 | Hire an agent that runs a task, asks questions and pushes a branch | C |
| Later | Costs and budgets, a second office, Linux and containers, features and releases | |

Track A is the way in: host, setup, web, sign-in. Track B is what people do once in: the
office's storage and the domain on it. Track C is running agents. A and B start at the
same time and touch different crates. C starts when M3 is usable.

**M1 to M4 is the point at which the team can move its own work into the product.** M5 is
the point at which its agents work there too.

Both engineers work on macOS and use Docker to run the product on Linux, so Linux is
part of v1 and is scheduled, not optional: increment A6, straight after the second
person can sign in.

---

## M1 · Install, create an organization, sign in

### A1 · One launch contract, no database encryption

- **Outcome:** none a person can see. It removes two things every later increment would
  otherwise carry.
- **Build:**
  - The host starts an office worker only through the verified rendezvous. The older
    first-line form, the switch between the two, `app_token` and `db_key` are deleted in
    host and worker.
  - SQLCipher is removed. The office and host databases are plain SQLite, protected by
    file permissions.
  - The host holds one office secrets key per office and passes it over the rendezvous.
    The office wraps its private keys (the CA key today) with it; no private key is ever
    stored in the clear.
- **Read:** [host boundary design](design/office-host-boundary.md#what-the-code-does-today)
  (rows B-06, B-07, F-03), [ADR-0016](adr/0016-no-application-level-data-encryption.md)
  (ENC-1 to ENC-6), [ADR-0012](adr/0012-office-channel-lifecycle.md). Checklist rows
  FC-017, FC-162.
- **Code today:** `crates/org-host/src/supervisor/child.rs` implements both launch forms;
  `crates/org-office-launch/src/v3.rs` is the rendezvous form;
  `apps/agentic-org-office/src/launch.rs` receives a grant key and does not use it; the
  CA key sits in the encrypted `office.db`; `crates/org-pki` seals with the recovery code.
- **Done when:** the host starts and restarts an office with one launch form; the
  workspace has no SQLCipher dependency; ENC-1 to ENC-6 pass.

### A2 · Setup creates the headquarters

- **Outcome:** on a clean machine, `agentic-org setup` followed by `agentic-org serve`
  gives a running instance with one HQ office, and the administrator receives the
  recovery code once.
- **Build:** setup provisions the instance and its data root, registers the service,
  creates the HQ office through the existing creation provider, and delivers the recovery
  code. Organization and office IDs are UUIDv7, the only ID form.
- **Read:** [standalone server design](design/standalone-server.md) sections 3 and 4,
  [installation and access UX](specs/server-installation-and-access-ux.md) (SRV criteria),
  [ADR-0007](adr/0007-hq-creation-escrow.md), [ADR-0011](adr/0011-protected-recovery-delivery.md),
  [create organization design](design/create-organization.md). Rows FC-011, FC-012, FC-016.
- **Code today:** `apps/agentic-org/src/lib.rs` refuses setup;
  `crates/org-host/src/provisioning.rs` and the office's `hq_creation` implement
  protected creation and are covered by tests but not reachable from a command;
  `crates/org-host/src/hostdb` holds the account and registry storage and is unused
  outside tests.
- **Done when:** the SRV criteria for a fresh HQ install pass on macOS, and a second run
  of setup changes nothing.

### A3 · The web process and sign-in

- **Outcome:** the owner opens the address setup printed, signs in, and sees the
  application shell.
- **Build:**
  - `agentic-org-web` is started by the host with a listener handed to it, serves the
    static UI and `/api/v1`, and confines itself before handling input.
  - Sign-in and sign-out. Sessions are stored at the host as digests, cached in the web
    process, and survive a restart of either.
  - Every request made for a person reaches the office with that person's principal and
    a host-issued grant, which the office verifies. At most seven web channels per office.
- **Read:** [web API design](design/web-api-v1.md) sections 4 to 6,
  [static UI and web process design](design/rust-browser-bff-and-static-sveltekit.md),
  [ADR-0013](adr/0013-stored-browser-sessions.md) (SES-1 to SES-6),
  [ADR-0008](adr/0008-gateway-macos-sandbox.md), [ADR-0012](adr/0012-office-channel-lifecycle.md),
  [installation and access UX](specs/server-installation-and-access-ux.md) (SIGN criteria).
  Rows FC-013, FC-014, FC-015, FC-155, FC-159.
- **Code today:** `apps/agentic-org-web/src/main.rs` refuses to listen; its library has
  the HTTP and TLS server, the content security policy and the route table;
  `crates/org-host/src/service` answers only `hello` and `instance.status` and keeps
  sessions in memory.
- **Done when:** SES-1 to SES-6 and the SIGN criteria for first access, sign-in, sign-out
  and expiry pass against a real instance, in a browser.

### A4 · Create and open an organization

- **Outcome:** the signed-in administrator creates an organization and opens it.
- **Build:** the create request carries name, slug, the organization repository
  registration, the state branch and, when bootstrap employees are planned, the initial
  team. Creating the organization is the first accepted transaction of that
  organization's journal (see B1). The UI's create flow talks to the real API.
- **Read:** [web API, selected create payload](design/web-api-v1.md#selected-create-organization-payload),
  [create organization design](design/create-organization.md),
  [organization structure](design/organization-data/01-organization-structure.md),
  [offices and teams UX](specs/offices-and-teams-ux.md) (OPEN criteria). Row FC-018.
- **Code today:** the host command accepts a name only; the UI flow runs against fakes
  (`apps/agentic-org-ui/src/lib/create-flow.ts`).
- **Done when:** an organization created in the browser exists after a restart, and its
  files are under `durable/state/`.
- **Needs:** B1.

---

## M2 · People and teams

### B1 · The store foundation

- **Outcome:** none a person can see. Every later Track B increment writes through it.
- **Build:**
  - Files are the source. Organization files live under `durable/state/`; the database
    under `runtime/<office-id>/` is a projection that can be deleted and rebuilt.
  - One write path: a command is validated, committed to the acceptance journal with its
    event and its idempotency result, and then materialized to files and the projection.
  - Storage is reached only through contracts the domain crates define. SQL appears only
    in the store adapter. Each contract has a test suite that does not depend on the
    database engine.
  - Each operation declares whether it is acknowledged at the commit or after the drive
    flush.
- **Read:** the [data contract index](design/organization-data/README.md) (shared storage
  contract), [record contracts](design/organization-data/08-record-contracts.md),
  [recovery contract](design/organization-data/16-recovery-and-office-continuity.md)
  including its proposed resolution for the first directory generation,
  [ADR-0018](adr/0018-storage-engine-independence.md) (ENG-1 to ENG-5),
  [ADR-0014](adr/0014-acknowledgement-classes-and-agent-os-latency.md) decisions 2, 3, 6
  and 8, [ADR-0006](adr/0006-selected-task-acceptance.md). Rows FC-141, FC-146, FC-165.
- **Code today:** `crates/org-office/src/store` is about 14,700 lines against older
  tables and is the source of truth; `store/acceptance` is the journal the selected
  design keeps; `crates/org-docs` parses and edits the files; two files outside the store
  use SQL (`organization/bootstrap.rs`, `organization/admission.rs`).
- **Done when:** ENG-1 to ENG-5 pass; deleting `runtime/<office-id>/` and restarting
  rebuilds the projection with no loss.
- **This is the largest increment in the plan.** It starts with a written plan, recorded
  in the repository: which record families move first, and what is deleted.

### B2 · Organization, teams and people

- **Outcome:** through `agent-os` and the API, a person creates teams, adds people and
  agents, sets a team lead, and moves someone to another team.
- **Build:** the organization, teams, employees and roles of the data contract. One home
  team per employee. One status field. A team lead who is a direct member. HQ gives
  teams their numbers. The many-to-many membership model is deleted.
- **Read:** [organization structure](design/organization-data/01-organization-structure.md),
  [agent structure](design/organization-data/04-agent-structure.md),
  [identity and numbering](design/organization-data/06-identity-and-numbering.md),
  [teams design](design/teams-and-org-chart.md), [offices and teams UX](specs/offices-and-teams-ux.md)
  (TEAM, PEOPLE, REPORT criteria). Rows FC-025 to FC-029.
- **Code today:** `crates/org-office/src/organization` and `employees` implement members
  with several memberships; the rules in `crates/org-office/src/rules` carry tests worth
  keeping where the rule is unchanged.
- **Done when:** the TEAM, PEOPLE and REPORT criteria that need no agent runtime pass.

### A5 · A second person can sign in

- **Outcome:** the owner gives the second engineer an account, and that person signs in
  and sees the same organization.
- **Build:** account creation and removal by the instance administrator, and the link
  between an account and a person in the organization.
- **Read:** [installation and access UX](specs/server-installation-and-access-ux.md)
  (account administration), [index, what the host owns](design/organization-data/STATUS.md#outside-this-set-what-the-host-owns).
  Row FC-153.
- **Code today:** `crates/org-host/src/accounts` holds one owner account.
- **Done when:** two people are signed in at once from two browsers.

### A6 · Linux and containers

- **Outcome:** the same instance runs as the main process of a Docker container on an
  engineer's Mac, with its data and secrets on mounted volumes, and behaves as on macOS.
- **Build:** the Linux implementation of each platform contract: checking that a started
  worker is the intended program, where the host keeps its secrets, confinement of the
  web process, and running in the foreground under a container runtime. Nothing outside
  those adapters branches on the operating system. A container file in the repository
  builds the image. The workspace checks run on both systems in CI.
- **Read:** [ADR-0017](adr/0017-linux-and-container-hosting.md) (LNX-1 to LNX-6),
  [host boundary design](design/office-host-boundary.md), [ADR-0008](adr/0008-gateway-macos-sandbox.md).
  Row FC-164.
- **Code today:** `crates/org-host/src/spawn.rs` refuses to start a worker on anything
  but macOS; the web process's Linux confinement is a contract with no adapter and
  refuses to start; host secrets use the macOS keychain.
- **Done when:** LNX-1 to LNX-6 pass.
- **Needs:** A1 to A3. Each of the three Linux adapters starts with a short design,
  which the engineer writes and decides.

### U1 · The directory in the browser

- **Outcome:** the team directory, the organization chart and the add, edit and move
  dialogs work against the real API.
- **Build:** replace the fake-backed adapters for these screens; keep the screens.
- **Read:** [offices and teams UX](specs/offices-and-teams-ux.md),
  [our organization in the app](specs/our-org-in-the-app.md), [app shell UX](specs/app-shell-ux.md).
  Rows FC-003, FC-025 to FC-027, FC-032.
- **Code today:** `apps/agentic-org-ui/src/lib/components` has the dialogs and charts;
  `src/lib/platform/unavailable-api.ts` is what production uses today.
- **Done when:** the browser tests for these screens run against a real instance.
- **Needs:** A3, B2.

---

## M3 · Tasks

### B3 · Projects, the minimum

- **Outcome:** a person creates a project with an accountable team.
- **Build:** projects with their number and slug from HQ, the accountable team, and
  contributing teams. No features, releases or repositories yet.
- **Read:** [project structure](design/organization-data/02-project-structure.md). Rows
  FC-030, FC-038.
- **Done when:** a project exists after a restart and a task can name it.

### B4 · Typed tasks

- **Outcome:** a person or agent creates a task, bug or request, assigns it, changes its
  status, priority and effort, comments on it and ticks its checklist; every change is in
  its history.
- **Build:**
  - One task record with a type. The separate bug and request families and the area
    calls and tables are deleted.
  - Status from the closed list; priority from the closed list; optional effort in
    points.
  - The reference is minted by the creating office: HQ gives each office a letter code,
    and each office keeps its own sequence with a durable mark. Creation is acknowledged
    at the commit.
  - Accountable team, and a separate assigned team and assignee, changed together at an
    expected revision.
  - An immutable history event per change, starting with `created` at revision 1.
- **Read:** [features and work](design/organization-data/03-features-and-work.md),
  [operational data](design/organization-data/05-operational-data.md),
  [identity and numbering](design/organization-data/06-identity-and-numbering.md),
  [assignment and history](design/organization-data/12-task-assignment-and-history.md),
  [ADR-0019](adr/0019-office-coded-task-numbers.md), [work items UX](specs/work-items-ux.md)
  (TSK, BUG, CHK, WORK criteria). Rows FC-039 to FC-046, FC-050, FC-054, FC-056, FC-142.
- **Code today:** `crates/org-work` has tasks, bugs, checklists and history against the
  older model, with per-project counters; `selected_codec` and `selected_assignment` are
  the parts written for the selected model.
- **Done when:** the TSK and WORK criteria that need no agent runtime pass; a task
  created with the office unable to reach HQ has its final reference at once; the draft
  schema tests for tasks match the code's tables.

### B5 · Tasks in `agent-os` and in the browser

- **Outcome:** both engineers create, list and update tasks from the browser, and an
  agent run by hand does the same with `agent-os`.
- **Build:** the task commands of `agent-os`, with the project taken from the agent's
  context and required when there is none; the board, list and detail screens against
  the real API.
- **Read:** [work items UX](specs/work-items-ux.md),
  [identity and numbering, default project](design/organization-data/06-identity-and-numbering.md#stable-ids-readable-references-and-scopes).
  Rows FC-038, FC-039, FC-043.
- **Done when:** a task created in the browser is updated from `agent-os` and the
  browser shows the change without a reload.
- **Needs:** B4, A3.

---

## M4 · Chat and asks

### B6 · Conversations and messages

- **Outcome:** people and agents send direct messages, talk in channels, and see what
  they have not read.
- **Build:**
  - Conversations of kind DM, group DM and channel, with membership.
  - Every file has one writing office: a message is written once by the sender's office;
    each employee's read state, reactions and preferences are that employee's own files.
  - A send is acknowledged at the local commit and relayed afterwards.
  - `agent-os` talks to the office for every call and gains the long-running mode.
- **Read:** [messaging and chat](design/organization-data/15-messaging-and-chat.md),
  [ADR-0014](adr/0014-acknowledgement-classes-and-agent-os-latency.md) (ACK-1 to ACK-9),
  [chat UX](specs/chat-ux.md) (CHAT criteria). Rows FC-074 to FC-078, FC-160.
- **Code today:** `crates/org-mailbox` has directed messages, asks and triage with a
  conformance suite; `crates/org-office/src/store/messages.rs` stores them in older
  tables; `agent-os` uses a file queue by default and the office when one is named.
- **Done when:** the CHAT criteria for sending, reading and channels pass; ACK-1 is
  measured on a Mac and recorded.

### B7 · Asks and questions

- **Outcome:** an agent or person asks a question with choices, a yes/no or free text;
  the other answers in chat; the asker gets the answer, or a timeout after the wait.
- **Build:**
  - An ask carries the question schema, an optional default, `required`, and how long
    the asker waits (10 minutes unless stated).
  - The answer is the answerer's own record; the asker's office records the one close
    outcome.
  - The question components in the product's chat, built from shadcn-svelte and Bits UI:
    single choice, multiple choice, yes/no, free text. A default is preselected.
  - A required ask that is not answered in time returns "required answer pending". The
    suspension that follows is increment C4.
- **Read:** [REQ-04](specs/req-04-channel-messaging-and-question-components.md),
  [ADR-0020](adr/0020-required-asks-suspend-and-resume.md) decisions 1 to 5 and 14
  (ASK-1, ASK-2), [chat UX](specs/chat-ux.md) decision 6 and CHAT-6. Rows FC-079, FC-166.
- **Done when:** each question type is asked from `agent-os` and answered in the
  browser; ASK-1 and ASK-2 pass.

---

## M5 · Agents run inside the product

### C1 · The sign-in spike

- **Outcome:** a written result: for each vendor tool, how it is pointed at a private
  home, where its sign-in is stored, and whether it resumes a session from there.
- **Read:** [ADR-0015](adr/0015-agent-isolation-modes.md) decisions 7, 10 and 11,
  [agent runtimes research](research/agent-runtimes-control.md).
- **Done when:** the result is recorded and the first adapter to build is chosen.

### C2 · Launch one agent

- **Outcome:** the owner hires an agent, gives it a task, and the agent runs a turn in
  its own home and reports back.
- **Build:** an employee's execution configuration; the private agent home; the
  `same-user` isolation mode in the launch description; one vendor adapter behind the
  runtime contract; one open turn per agent, started only by an explicit instruction.
- **Read:** [model profiles and agent execution](design/organization-data/11-model-profiles-and-agent-execution.md),
  [providers and connection pools](design/organization-data/14-model-providers-and-connection-pools.md),
  [agent sessions design](design/agent-sessions.md), [ADR-0004](adr/0004-headless-agent-sessions.md),
  [ADR-0015](adr/0015-agent-isolation-modes.md) (ISO-3, ISO-5, ISO-8),
  [employee supervision UX](specs/employee-supervision-ux.md) (EMP criteria). Rows FC-096,
  FC-097, FC-098, FC-100, FC-101.
- **Code today:** `crates/org-runtime` is the contract and `crates/org-supervise` starts,
  stops and cleans up processes; neither is called to launch an agent.
- **Done when:** an agent completes one task from an instruction given in chat.

### C3 · The agent follows the organization's processes

- **Outcome:** an agent's instructions contain the processes and standards that apply to
  its work, with the non-overridable ones marked as must-follow.
- **Build:** the one-chain resolution (organization, the accountable team's chain, the
  project) and its delivery at launch.
- **Read:** [processes design](design/processes.md) decision 4, [processes UX](specs/processes-ux.md).
  Rows FC-110, FC-112.
- **Code today:** `crates/org-process` resolves an earlier, multi-chain rule and has no
  caller.

### C4 · A required ask suspends and resumes the agent

- **Outcome:** an agent that cannot continue without an answer is stopped after being
  told, and starts again in the same session when the answer arrives.
- **Read:** [ADR-0020](adr/0020-required-asks-suspend-and-resume.md) decisions 5 to 13
  (ASK-3 to ASK-9). Row FC-166.
- **Needs:** B7, C2.

### C5 · See what an agent did

- **Outcome:** a person opens an agent's session in the browser and reads it, with
  tool calls and file changes shown or hidden.
- **Build:** first a proof of concept in the UI against sample Claude and Codex
  conversation files; then the parsing and redaction in the office, served to authorized
  people.
- **Read:** [REQ-05](specs/req-05-virtual-console-conversation-log-viewer.md). Row FC-150.
- **The proof of concept needs nothing else and can start at any time.**

### C6 · Repositories and worktrees

- **Outcome:** an agent works in a worktree of a registered repository and the office
  pushes its task branch.
- **Read:** [Git design](design/projects-git-github.md) and its open questions, which are
  answered first. Rows FC-103, FC-139, FC-140.

---

## Later, in no fixed order

| Increment | What | Read |
| --- | --- | --- |
| L2 | Usage, prices and cost views | [document 10](design/organization-data/10-usage-and-cost-accounting.md), [costs UX](specs/costs-and-insights-ux.md), FC-084 to FC-088 |
| L3 | Budgets and the hold at the turn permit | [document 17](design/organization-data/17-budgets-and-controls.md), FC-089 to FC-093 |
| L4 | Subscription pools, quota permits, failover | [documents 11 and 14](design/organization-data/11-model-profiles-and-agent-execution.md), FC-098, FC-099 |
| L5 | A second office: enrolment, replication, relay between offices | [offices design](design/offices-hq-and-satellites.md), FC-020, FC-145 |
| L6 | The `dedicated-user` isolation mode | [ADR-0015](adr/0015-agent-isolation-modes.md), FC-161 |
| L7 | Features, knowledge, releases and deployments | documents [03](design/organization-data/03-features-and-work.md), [07](design/organization-data/07-release-deployments.md), [09](design/organization-data/09-release-content-and-flags.md), FC-058 to FC-070 |
| L8 | Private task notes | [document 05](design/organization-data/05-operational-data.md#private-notes-attached-to-tasks), FC-053 |
| L9 | Install from a signed release, update, uninstall | [standalone server design](design/standalone-server.md) section 2, FC-022, FC-154 |
| L10 | Rename `gateway` to `web` in code identifiers | mechanical; any time |

## For every increment

- Branch from `origin/v1-integration`. One writer per file at a time.
- Before code, write the short plan for the increment: what a person can do at the end,
  the files you will own, what you will delete, and the decisions you are taking where
  the documents are silent.
- Decide for yourself inside the [architecture non-negotiables](architecture-non-negotiables.md),
  and record each decision in the document that owns the topic.
- Keep the checks green: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`; for the UI, its type check, tests and build. When you change
  the data contract, `python3 docs/design/organization-data/test_schema.py`.
- Update the checklist row with evidence. Mark it Done only for the row's whole scope.
- Get an independent review, then merge into `v1-integration`.
