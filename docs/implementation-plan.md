# v1 implementation plan

It orders the work so that the two engineers and their agents can work inside the
product as early as possible. The order and the split into increments are a proposal
and may be changed by the engineer who takes an increment. The behaviour is not a
proposal: it is what the linked requirement documents say. Where they are silent, the
engineer decides, inside the [architecture non-negotiables](architecture-non-negotiables.md),
and writes the decision down.

How to read an increment:

- **Outcome** is what a person or agent can do when it is finished.
- **Build** lists what has to exist. It is a summary; the requirement documents are the
  detail.
- **Read** lists the requirement documents and the acceptance criteria to make pass.
- **Code today** says where to start and what state it is in.
- **Done when** is the finish line.
- **Needs** names the increments that must be finished first.

New here? Read [START-HERE](START-HERE.md) first.

## The path

| Milestone | A person can | Increments |
| --- | --- | --- |
| M1 | Install the server, sign in, create the organization, and sign in from the command line | A1 to A5 |
| M2 | Add people and teams, give the second engineer an account, and run the product in a Linux container | B1, B2, A6, A7, U1 |
| M3 | Create, assign and follow tasks, in the browser and with `agent-os` | B3 to B5 |
| M4 | Chat, and ask a question that waits for an answer | B6, B7 |
| M5 | Hire an agent through HR that runs a task, asks questions and pushes a branch | C1 to C7 |
| Later | Costs and budgets, a second office, features and releases. All of it is v1 scope. | L1 to L10 |

Track A is the way in: host, setup, web, sign-in. Track B is what people do once in: the
office's storage and the domain on it. Track C is running agents. A and B start on the
same day. C starts when M3 is usable.

**M1 to M4 is the point at which the team can move its own work into the product.** M5 is
the point at which its agents work there too. An agent exists only as an employee of
the organization: it is onboarded through HR and started by the office. Until M5 the
product is used by people.

Two things to know about the order:

- **A1 and B1 both change `crates/org-office/src/store`.** A1 removes the database
  encryption from it; B1 rebuilds it. A1's part lands first. B1 begins with its written
  plan, which changes no code, so the two tracks still start together.
- **Both engineers work on macOS and use Docker to run the product on Linux.** Linux is
  therefore a scheduled increment, A7, and not an option.

---

## M1 · Install, sign in, create the organization

### A1 · One launch contract, no database encryption

- **Outcome:** none a person can see. It removes two things every later increment would
  otherwise carry.
- **Build:**
  - The host starts an office worker only through the verified rendezvous. The other
    launch form, the switch between the two, `app_token` and `db_key` are deleted in
    host and worker.
  - SQLCipher is removed. The office and host databases are plain SQLite, protected by
    file permissions.
  - The host holds one office secrets key per office and passes it over the rendezvous.
    The office wraps its private keys (the CA key today) with it; no private key is ever
    stored in the clear.
- **Read:** [host boundary design](design/office-host-boundary.md),
  [ADR-0016](adr/0016-no-application-level-data-encryption.md),
  [ADR-0012](adr/0012-office-channel-lifecycle.md). Checklist rows FC-017, FC-162.
- **Code today:** `crates/org-host/src/supervisor/child.rs` implements both launch forms;
  `crates/org-office-launch/src/v3.rs` is the rendezvous form;
  `apps/agentic-org-office/src/launch.rs` receives a grant key and does not use it; the
  CA key sits in the encrypted `office.db` (`crates/org-office/src/store/hq_creation.rs`);
  `crates/org-pki` seals with the recovery code; SQLCipher is a workspace dependency
  used by `crates/org-office/src/store` and `crates/org-host/src/hostdb`.
- **Done when:** the host starts and restarts an office with one launch form; the
  workspace has no SQLCipher dependency; ENC-1, ENC-5 and ENC-6 pass. ENC-3 is
  finished in A2 and ENC-2 in A4, where an organization exists. ENC-4 is finished in C2 for the
  `same-user` mode and in L6 for `dedicated-user`.

### A2 · Setup creates the instance and the first account

- **Outcome:** on a clean machine, `agentic-org setup` followed by `agentic-org serve`
  gives a running instance with one account, the instance administrator. There is no
  organization yet.
- **Build:**
  - Setup provisions the instance and its data root and registers the service.
  - Setup creates the first account, which is an instance administrator.
  - Setup reports the state of disk encryption and the file permissions it set.
  - Setup creates no organization and no office. An organization is created by a
    signed-in administrator (A4).
- **Read:** [standalone server design](design/standalone-server.md),
  [installation and access UX](specs/server-installation-and-access-ux.md) (SRV criteria),
  [ADR-0016](adr/0016-no-application-level-data-encryption.md) (ENC-3). Rows FC-011,
  FC-012.
- **Code today:**
  - `apps/agentic-org/src/lib.rs` refuses setup.
  - `crates/org-host/src/instance` (registry and the setup continuation) and
    `crates/org-host/src/bootstrap.rs` (the protected setup connection) exist and are
    reached only from tests.
  - `crates/org-host/src/hostdb` holds the account and registry storage and is unused
    outside tests.
- **Done when:** the SRV criteria for a fresh install pass on macOS; a second run of
  setup changes nothing; ENC-3 passes.
- **Needs:** A1.

### A3 · The web process and sign-in

- **Outcome:** the administrator opens the address setup printed, signs in, and sees
  the application shell. With no organization yet, it offers to create one.
- **Build:**
  - `agentic-org-web` is started by the host with a listener handed to it, serves the
    static UI and `/api/v1`, and confines itself before handling input.
  - Sign-in and sign-out. Sessions are stored at the host as digests, cached in the web
    process, and survive a restart of either.
  - Every request made for a person reaches the office with that person's principal and
    a host-issued grant, which the office verifies. At most seven web channels per office.
  - The change stream of the web API, so that a screen shows a change without a reload.
  - Host storage is reached through a contract with a test suite that does not depend on
    the database engine. This is the host's share of ADR-0018.
- **Read:** [web API design](design/web-api-v1.md),
  [static UI and web process design](design/rust-browser-bff-and-static-sveltekit.md),
  [ADR-0013](adr/0013-stored-browser-sessions.md) (SES-1 to SES-7),
  [ADR-0008](adr/0008-gateway-macos-sandbox.md), [ADR-0012](adr/0012-office-channel-lifecycle.md),
  [ADR-0018](adr/0018-storage-engine-independence.md) (ENG-1 and ENG-3 for the host),
  [installation and access UX](specs/server-installation-and-access-ux.md) (SIGN criteria).
  Rows FC-013, FC-014, FC-015, FC-155, FC-159.
- **Code today:**
  - `apps/agentic-org-web/src/main.rs` refuses to listen. Its library has the HTTP and
    TLS server, the content security policy and the routing contract; the only backend
    behind that contract refuses every route, so there are no API routes.
  - `crates/org-host/src/service` has handlers for verifying an account, for sessions
    (inspect, renew, end, grant), for opening an office channel and for provisioning.
    Sessions are digests kept in memory. These handlers answer only for a registered
    instance, and only the test entry registers one: the production entry serves
    `hello` and `instance.status`.
- **Done when:** SES-1 to SES-7 and the SIGN criteria for first access, sign-in, sign-out
  and expiry pass against a real instance, in a browser.
- **Needs:** A2.

### A4 · Create the organization

- **Outcome:** the signed-in administrator creates an organization in the browser and
  opens it. Whoever creates an organization is its owner.
- **Build:**
  - The create request carries the name, the slug, the remote organization repository,
    the state branch and the first team.
  - The host creates the organization together with its HQ office, through the
    existing protected creation path, and starts the office.
  - The creator becomes a person in the organization, in its first team, and their
    account owns the organization. Every account is tied to a person: from here on,
    what this account does in the organization carries that person's name.
  - The built-in HR lead is created as an employee in the first team. It is part of the
    product: it appears and is addressed as an agent, but it is not kept running. It
    runs only when called (C2), and the onboarding of a new agent is product code it
    invokes (C3).
  - The recovery code is released once, on the HQ machine.
  - Organization and office IDs are UUIDv7.
  - The UI's create flow talks to the real API.
- **Read:** [create organization design](design/create-organization.md),
  [web API design](design/web-api-v1.md) (selected create payload),
  [organization structure](design/organization-data/01-organization-structure.md),
  [offices and teams UX](specs/offices-and-teams-ux.md) (OPEN criteria),
  [installation and access UX](specs/server-installation-and-access-ux.md) (accounts,
  recovery), [ADR-0007](adr/0007-hq-creation-escrow.md),
  [ADR-0011](adr/0011-protected-recovery-delivery.md),
  [ADR-0016](adr/0016-no-application-level-data-encryption.md) (ENC-2). Rows FC-016,
  FC-018, FC-029, FC-031.
- **Code today:**
  - Two host commands create an organization. `organization.create` takes a name only.
    `organization.provision.v1` already carries the selected payload (name, slug,
    repository address, state branch, home team name: `crates/org-proto/src/selected_bootstrap.rs`)
    and is implemented by `crates/org-host/src/provisioning.rs` with the office's
    `hq_creation`. Keep the second, delete the first.
  - The UI flow runs against fakes (`apps/agentic-org-ui/src/lib/create-flow.ts`).
- **Done when:** an organization created in the browser exists after a restart with its
  owner as a person in the first team; its files are under `durable/state/`; the
  recovery code has been delivered once; ENC-2 passes.
- **Needs:** A3. A4 lands on the office store as it is. B1 then rebuilds that store
  underneath it and makes organization creation the first accepted transaction.

### A5 · A person on the command line

- **Outcome:** a person signs in from a terminal and runs commands against the running
  instance under their own name. This is how the commands of B2, B4 and B5 are used
  from a terminal before the office starts any agent.
- **Build:**
  - Sign-in on the command line through the host's local socket. The session ends when
    its connection closes.
  - A person-started command names the instance and the office, or uses a saved context
    bound to those two IDs. With neither it fails and says how to select one.
  - Each call reaches the office with the person's principal and a host-issued grant,
    exactly as a browser request does, and is recorded under that person. This replaces
    the application token that A1 deletes.
  - Nothing here gives an agent a way in. An agent is an employee the office starts,
    and it uses the binding of its run (C2).
- **Read:** [ADR-0003](adr/0003-agent-messaging-office-first.md) decisions 12 and 13 and
  "Person-started commands", [ADR-0013](adr/0013-stored-browser-sessions.md) (SES-8),
  [host boundary design](design/office-host-boundary.md),
  [`apps/agent-os/README.md`](../apps/agent-os/README.md). Row FC-015.
- **Code today:** a person or the owner binds to the office with `app_token`
  (`apps/agentic-org-office/src/serve.rs`), which A1 removes; `agent-os` talks to an
  office only when one is named and otherwise writes to a file queue, which B6 deletes.
- **Done when:** SES-8 passes; a signed-in person runs a read command and a write
  command against the office from a terminal; the same commands are refused without a
  session.
- **Needs:** A4.

---

## M2 · People and teams

### B1 · The store foundation

- **Outcome:** none a person can see. Every later Track B increment writes through it.
- **Build:**
  - Files are the source. Organization files live under `durable/state/`; the database
    under `runtime/<office-id>/` is a projection that can be deleted and rebuilt.
  - One write path: a command is validated, committed to the acceptance journal with its
    event and its idempotency result, and then materialized to files and the projection.
  - Creating an organization is the first accepted transaction of that organization's
    journal, and its first directory generation.
  - Storage is reached only through contracts the domain crates define. SQL appears only
    in the store adapter. Each contract has a test suite that does not depend on the
    database engine.
  - Each operation declares whether it is acknowledged at the commit or after the drive
    flush.
- **Read:** the [data contract index](design/organization-data/README.md) (shared storage
  contract), [record contracts](design/organization-data/08-record-contracts.md),
  [recovery contract](design/organization-data/16-recovery-and-office-continuity.md)
  including its resolution for the first directory generation,
  [ADR-0018](adr/0018-storage-engine-independence.md),
  [ADR-0014](adr/0014-acknowledgement-classes-and-agent-os-latency.md) decisions 2, 3, 6
  and 8, [ADR-0006](adr/0006-selected-task-acceptance.md). Rows FC-141, FC-146, FC-165.
- **Code today:** `crates/org-office/src/store` is about 14,700 lines against tables the
  data contract replaces, and is the source of truth; `store/acceptance` is the journal
  the selected design keeps; `crates/org-docs` parses and edits the files. Outside the
  store, two files use the database driver (`organization/bootstrap.rs`,
  `organization/admission.rs`) and two more hold SQL (`office.rs`, `office/hq_creation.rs`).
- **Done when:** deleting `runtime/<office-id>/` and restarting rebuilds the projection
  with no loss; in `org-office`, SQL and driver types are found only in the store
  adapter (ENG-1 for the office); the office starts with its store chosen through one
  seam and the organization suite passes against SQLite and against an in-memory
  implementation (ENG-2, and ENG-3 for organization records); ENG-4 and ENG-5 pass.
  The suites for work and messaging arrive with B4 and B6, for the host with A3, and for
  replication with L5.
- **This is the largest increment in the plan.** It starts with a written plan, recorded
  in the repository: which record families move first, and what is deleted.
- **Needs:** A1's change to the office store.

### B2 · Organization, teams and people

- **Outcome:** through the command line and the API, a person creates teams, adds people
  and agents, sets a team lead, and moves someone to another team.
- **Build:** the organization, teams, employees and roles of the data contract. One home
  team per employee. One status field. A team lead who is a direct member. HQ gives
  teams their numbers. The many-to-many membership model is deleted.
- **Read:** [organization structure](design/organization-data/01-organization-structure.md),
  [agent structure](design/organization-data/04-agent-structure.md),
  [identity and numbering](design/organization-data/06-identity-and-numbering.md),
  [teams design](design/teams-and-org-chart.md). Rows FC-025 to FC-029.
- **Code today:** `crates/org-office/src/organization` and `employees` implement members
  with several memberships (a `memberships` table in `store/org.rs`); the rules in
  `crates/org-office/src/rules` carry tests worth keeping where the rule is unchanged.
- **Done when:** every rule the three data documents state for teams, employees and
  roles is enforced by the office and covered by the engine-independent organization
  suite; each action in the outcome works from the command line and through `/api/v1`
  and is in the history. The screen criteria (TEAM, PEOPLE, REPORT) are finished in U1.
- **Needs:** B1, A5.

### A6 · A second person can sign in

- **Outcome:** the owner gives the second engineer an account, and that person signs in
  and works in the same organization under their own name.
- **Build:**
  - `agentic-org account create`, `link`, `unlink` and `revoke`, run on the HQ machine by
    an instance administrator. There is no browser screen for accounts.
  - Every account is tied to a person (owner decision, 2026-11-17). In each
    organization an account can open, it is tied to exactly one person record, and
    what the account does there carries that person's name. A person does not need an
    account. An account is never tied to an agent. `account create` and `account link`
    name the person, or create one.
- **Read:** [installation and access UX](specs/server-installation-and-access-ux.md)
  (accounts), [agent structure](design/organization-data/04-agent-structure.md),
  [index, what the host owns](design/organization-data/STATUS.md#outside-this-set-what-the-host-owns).
  Rows FC-029, FC-153.
- **Code today:** `crates/org-host/src/accounts` holds only the login value types.
  Account storage is `crates/org-host/src/hostdb/accounts.rs`; its schema allows many
  accounts and several linked organizations, but the only insert is the first
  administrator account. `account.create` is defined in `crates/org-host-control` and has
  no handler. Account IDs are still ULIDs and become UUIDv7.
- **Done when:** two people are signed in at once from two browsers, and a change each
  makes carries their own name.
- **Needs:** A3, B2.

### A7 · Linux and containers

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
- **Read:** [offices and teams UX](specs/offices-and-teams-ux.md) (TEAM, PEOPLE, REPORT
  criteria), [our organization in the app](specs/our-org-in-the-app.md),
  [app shell UX](specs/app-shell-ux.md). Rows FC-003, FC-025 to FC-027, FC-032.
- **Code today:** `apps/agentic-org-ui/src/lib/components` has the dialogs and charts;
  `src/lib/platform/unavailable-api.ts` is what production uses today.
- **Done when:** the TEAM, PEOPLE and REPORT criteria that need no agent runtime pass in
  browser tests against a real instance.
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
- **Needs:** B2.

### B4 · Typed tasks

- **Outcome:** a person or agent creates a task, bug or request, assigns it, changes its
  status, priority and effort, comments on it and ticks its checklist; every change is in
  its history.
- **Build:**
  - One task record with a type. The separate bug and request families and the area
    calls and tables are deleted.
  - Status from the closed list; priority from the closed list; optional effort in whole
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
  [ADR-0019](adr/0019-office-coded-task-numbers.md). Rows FC-039 to FC-046, FC-050,
  FC-054, FC-056, FC-142.
- **Code today:** `crates/org-work` has tasks, bugs, checklists and history against the
  model the data contract replaces, with per-project counters; `selected_codec`,
  `selected_assignment` and `selected_history` are the parts written for the selected
  model.
- **Done when:** every rule the data documents state for tasks is enforced by the office
  and covered by the engine-independent work suite; a task created with the office
  unable to reach HQ has its final reference at once; the code's tables match the draft
  schema for tasks. The screen criteria (TSK, BUG, CHK, WORK) are finished in B5.
- **Needs:** B3.

### B5 · Tasks in `agent-os` and in the browser

- **Outcome:** both engineers create, list and update tasks from the browser and from a
  terminal. The same commands are what an agent uses through `agent-os` from C2 on.
- **Build:** the task commands of `agent-os`, with the project taken from the agent's
  context and required when there is none; the board, list and detail screens against
  the real API, updated by the change stream.
- **Read:** [work items UX](specs/work-items-ux.md) (TSK, BUG, CHK, WORK criteria),
  [identity and numbering, default project](design/organization-data/06-identity-and-numbering.md#stable-ids-readable-references-and-scopes).
  Rows FC-038, FC-039, FC-043.
- **Done when:** the TSK, BUG, CHK and WORK criteria that need no agent runtime pass in
  browser tests against a real instance; a task created in the browser is updated from
  a terminal and the browser shows the change without a reload.
- **Needs:** B4, A3, A5.

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
  - `agent-os` talks to the office for every call and gains the long-running mode. The
    file queue, `crates/org-spool` and the route through it are deleted.
- **Read:** [messaging and chat](design/organization-data/15-messaging-and-chat.md),
  [ADR-0003](adr/0003-agent-messaging-office-first.md),
  [ADR-0014](adr/0014-acknowledgement-classes-and-agent-os-latency.md) (ACK-1 to ACK-9),
  [chat UX](specs/chat-ux.md) (CHAT criteria). Rows FC-074 to FC-078, FC-160.
- **Code today:** `crates/org-mailbox` has directed messages, asks and triage with a
  conformance suite; `crates/org-office/src/store/messages.rs` stores them in tables the
  messaging contract replaces; `agent-os` uses a file queue by default and the office
  when one is named.
- **Done when:** the CHAT criteria for sending, reading and channels pass; the
  engine-independent messaging suite passes; ACK-1 is measured on a Mac and recorded.
- **Needs:** B2, A5. The browser part needs A3.

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
    suspension that follows is increment C5.
- **Read:** [REQ-04](specs/req-04-channel-messaging-and-question-components.md),
  [ADR-0020](adr/0020-required-asks-suspend-and-resume.md) decisions 1 to 5 and 14
  (ASK-1, ASK-2), [chat UX](specs/chat-ux.md) (CHAT-6). Rows FC-079, FC-166.
- **Done when:** each question type is asked from a terminal and answered in the
  browser; ASK-1 and ASK-2 pass.
- **Needs:** B6.

---

## M5 · Agents run inside the product

### C1 · The sign-in spike

- **Outcome:** a written result: for each vendor tool, how it is pointed at a private
  home, where its sign-in is stored, and whether it resumes a session from there.
- **Read:** [ADR-0015](adr/0015-agent-isolation-modes.md) decisions 7, 10 and 11,
  [agent runtimes research](research/agent-runtimes-control.md).
- **Done when:** the result is recorded and the first adapter to build is chosen.
- **Needs nothing else and can start at any time.**

### C2 · Launch the first agent

- **Outcome:** the owner sends the built-in HR lead a message in chat; the office starts
  it for that call, it runs a turn in its own home, answers, and stops.
- **Build:** an employee's execution configuration; the private agent home; the
  `same-user` isolation mode in the launch description, with the product stating the
  reduced protection of that mode; one vendor adapter behind the runtime contract; one
  open turn per agent, started only by an explicit instruction; `agent-os` bound to the
  run the office started.
- **Read:** [model profiles and agent execution](design/organization-data/11-model-profiles-and-agent-execution.md),
  [providers and connection pools](design/organization-data/14-model-providers-and-connection-pools.md),
  [agent sessions design](design/agent-sessions.md), [ADR-0004](adr/0004-headless-agent-sessions.md),
  [ADR-0015](adr/0015-agent-isolation-modes.md) (ISO-3, ISO-5, ISO-8),
  [ADR-0016](adr/0016-no-application-level-data-encryption.md) (ENC-4),
  [ADR-0003](adr/0003-agent-messaging-office-first.md) decision 12,
  [employee supervision UX](specs/employee-supervision-ux.md) (EMP criteria). Rows FC-031,
  FC-096, FC-097, FC-098, FC-100, FC-101.
- **Code today:** `crates/org-runtime` is the contract and has no caller.
  `crates/org-supervise` starts, stops and cleans up processes; it is used only to
  recover processes the office owned before a restart, not to launch an agent.
- **Done when:** the HR lead answers a message, and creates and updates a task through
  `agent-os`.
- **Needs:** C1, B5, B6.

### C3 · Hire an agent through HR

- **Outcome:** the owner asks the HR lead for a new agent; HR proposes one; the owner
  approves; the new agent is onboarded, is given a task and completes it. This is the
  only way an agent comes to exist in an organization.
- **Build:** the hire proposal, its approval, and provisioning of the employee, its
  home and its execution configuration only after approval. HR drafts the proposal as
  an agent would; the provisioning is the product's own code, not something the model
  improvises.
- **Read:** [our organization in the app](specs/our-org-in-the-app.md) (the HR lead; the
  hire flow; who approves; STAFF and HRL criteria), [roles UX](specs/roles-ux.md),
  [agent structure](design/organization-data/04-agent-structure.md). Rows FC-031, FC-033.
- **Done when:** the STAFF and HRL criteria for a hire pass, and no employee, home or
  binding exists before approval.
- **Needs:** C2, B7.

### C4 · The agent follows the organization's processes

- **Outcome:** an agent's instructions contain the processes and standards that apply to
  its work, with the non-overridable ones marked as must-follow.
- **Build:** the one-chain resolution (organization, the accountable team's chain, the
  project) and its delivery at launch.
- **Read:** [processes design](design/processes.md), [processes UX](specs/processes-ux.md).
  Rows FC-110, FC-112.
- **Code today:** `crates/org-process` resolves several chains, which the design
  replaces with one, and has no caller.
- **Needs:** C2.

### C5 · A required ask suspends and resumes the agent

- **Outcome:** an agent that cannot continue without an answer is stopped after being
  told, and starts again in the same session when the answer arrives.
- **Read:** [ADR-0020](adr/0020-required-asks-suspend-and-resume.md) decisions 5 to 13.
  Row FC-166.
- **Done when:** ASK-3, ASK-5, ASK-6, ASK-7 and ASK-9 pass, and the presence part of
  ASK-4. The rest of ASK-4 (no turn permit and no subscription slot while suspended) is
  finished with L4, and ASK-8 (a budget hold keeps the agent suspended) with L3.
- **Needs:** B7, C2.

### C6 · See what an agent did

- **Outcome:** a person opens an agent's session in the browser and reads it, with
  tool calls and file changes shown or hidden.
- **Build:** first a proof of concept in the UI against sample Claude and Codex
  conversation files; then the parsing and redaction in the office, served to authorized
  people.
- **Read:** [REQ-05](specs/req-05-virtual-console-conversation-log-viewer.md). Row FC-150.
- **The proof of concept needs nothing else and can start at any time.**

### C7 · Repositories and worktrees

- **Outcome:** an agent works in a worktree of a registered repository and the office
  pushes its task branch.
- **Read:** [Git design](design/projects-git-github.md) and its open questions, which the
  engineer answers first. Rows FC-103, FC-139, FC-140.
- **Needs:** C2.

---

## Later, in no fixed order

All of this is v1 scope. It is "later" only because the team can already work in the
product without it.

| Increment | What | Read |
| --- | --- | --- |
| L1 | More than one organization on an instance, and switching between them | [offices and teams UX](specs/offices-and-teams-ux.md) (OPEN criteria), FC-015. Needs A4 |
| L2 | Usage, prices and cost views | [document 10](design/organization-data/10-usage-and-cost-accounting.md), [costs UX](specs/costs-and-insights-ux.md), FC-084 to FC-088 |
| L3 | Budgets and the hold at the turn permit | [document 17](design/organization-data/17-budgets-and-controls.md), FC-089 to FC-093; ASK-8 |
| L4 | Subscription pools, quota permits, failover | [documents 11 and 14](design/organization-data/11-model-profiles-and-agent-execution.md), FC-098, FC-099; the rest of ASK-4 |
| L5 | A second office: enrolment, replication, relay between offices | [offices design](design/offices-hq-and-satellites.md), [ADR-0003](adr/0003-agent-messaging-office-first.md) decisions 4 to 8, FC-020, FC-145; the replication suite of ENG-3 |
| L6 | The `dedicated-user` isolation mode | [ADR-0015](adr/0015-agent-isolation-modes.md), FC-161; ENC-4 for this mode |
| L7 | Features, knowledge, releases and deployments | documents [03](design/organization-data/03-features-and-work.md), [07](design/organization-data/07-release-deployments.md), [09](design/organization-data/09-release-content-and-flags.md), FC-058 to FC-070 |
| L8 | Private task notes | [document 05](design/organization-data/05-operational-data.md#private-notes-attached-to-tasks), FC-053 |
| L9 | Install from a signed release, update, uninstall | [standalone server design](design/standalone-server.md), FC-022, FC-154 |
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
