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

**Milestone one: one office on a Mac, with people and agents on board and talking.**
This is the owner's first milestone (2026-11-17), and everything in it runs in a
single office on macOS. Its steps, in order:

| Step | A person can | Increments |
| --- | --- | --- |
| 1 · Getting in | Install the server, register from the link setup prints, and create the organization | A2, A3, A4, then A5; A1 alongside |
| 2 · The organization | Set up the office, projects, teams and people; invite the second engineer; open the organization again from its repository after losing the server's database | B1, B2, B3, O1, A6, R1, U1 |
| 3 · Agents | Hire an agent through HR, start it with an instruction, and see what it did | C1 to C5 |
| 4 · Messages | Chat with people and agents, and ask a question that waits for an answer | B6, B7, C6 |
| 5 · Tasks | Create, assign and follow tasks, in the browser and with `agent-os`; an agent works in a repository | B4, B5, C7 |

**Milestone two: Linux, in parallel (Track X).** The same server runs on Linux, including inside a
container. A third engineer builds it now, beside milestone one (X1a to X6); its first
increment puts every platform difference behind one crate, after which no other
change may add a platform branch.

**Milestone three: a second office on another machine.** It is a separate thread:
enrolment of the office, replication between offices, and messages relayed between
them (S1).

**Later:** costs and budgets, features and releases, updates. All of it is v1 scope.

Most of step 2 and the hire flow are create, read, update and delete on records the
data contract already defines. The parts that are not: the web process and sessions
working end to end for the first time (step 1), the store rebuilt so that files are
the source (B1), and starting and supervising an agent process (C3).

Track A is the way in: host, setup, web, sign-in. Track B is the organization's data:
the office's storage and the domain on it. Track C is agents. Track X is Linux and containers, built alongside the other three. A and B start on the same
day. C starts when step 2 is usable; its spike (C1) and the proof of concept of the
session view (C4) can start at once.

An agent exists only as an employee of the organization: it is onboarded through HR
and started by the office. People use the browser and the terminal.

Three things to know about the order:

- **Getting in comes first and waits for nothing.** The Track A engineer starts at A2
  and goes straight through A3 and A4: the link, registration, and creating the
  organization.
- **A1 is a clean-up that the Track B engineer takes first,** because B1 rebuilds the
  office store that A1 removes the encryption from. B1's written plan is prepared
  meanwhile. A1 also removes the encryption of the host database, which A2 and A3
  write to: the two leads agree the order of those commits.
- **macOS and Linux in parallel.** The Track A and B engineers work on macOS. Track X
  brings Linux, including a container, at the same time (X1a to X6). X1a lands on the first day and
  X1b to X1d follow in small merges, so that later Track A and B changes write against the platform
  crate instead of adding platform branches.

---

## Step 1 · Getting in: install, register, create the organization

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
  finished in A2 and ENC-2 in A4, where an organization exists. ENC-4 is finished in C3 for the
  `same-user` mode and in L6 for `dedicated-user`.

### A2 · Setup creates the instance and a one-time registration link

- **Outcome:** on a clean machine, `agentic-org setup` gives a running instance and
  prints a link. There is no account and no organization yet.
- **Build:**
  - Setup provisions the instance and its data root, registers the service and starts
    it.
  - Setup asks for no username and no password. It prints one registration link to
    the instance's own address. The link works once, expires, and is stored only as a
    digest. `agentic-org admin reset` on the server machine issues a new link, which
    cancels the previous one.
  - Setup reports the state of disk encryption and the file permissions it set.
  - Setup creates no account, no organization and no office (A3, A4).
- **Read:** [standalone server design](design/standalone-server.md),
  [installation and access UX](specs/server-installation-and-access-ux.md) (the SRV
  criteria for a single machine; the satellite ones belong to S1),
  [ADR-0016](adr/0016-no-application-level-data-encryption.md) (ENC-3). Rows FC-011,
  FC-012.
- **Code today:**
  - `apps/agentic-org/src/lib.rs` refuses setup.
  - `crates/org-host/src/instance` (registry and the setup continuation) and
    `crates/org-host/src/bootstrap.rs` (the protected setup connection) exist and are
    reached only from tests.
  - `crates/org-host/src/hostdb` holds the account and registry storage and is unused
    outside tests. `hostdb/first_account.rs` creates the first account from a typed
    password, which the link replaces.
- **Done when:** the single-machine SRV criteria for a fresh install pass on macOS; a second run of
  setup changes nothing; a used or replaced link is refused; ENC-3 passes.
- **Needs:** nothing. It is where Track A starts.

### A3 · The web process, registration and sign-in

- **Outcome:** the person who ran setup opens the link, chooses a username and a
  password, and is signed in as the instance administrator. With no organization yet,
  the application offers to create one.
- **Build:**
  - `agentic-org-web` is started by the host through the same launch contract as an
    office worker, with its own identifier and a listener handed to it, serves the
    static UI and `/api/v1`, and confines itself before handling input. The separate
    web rendezvous lines are deleted.
  - Registration through a one-time link: the page asks for a username and a
    password, and the host creates the account. The account created from setup's link
    is the server's administrator. Exactly one account is the administrator at any
    time; it can hand the role to another account from the browser. No password is
    ever typed in a terminal.
  - `agentic-org admin reset` always concerns the administrator account: it prints a
    one-time link that registers the account if none exists, and otherwise sets a new
    password. It is the only account command.
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
  - The web process has its own rendezvous lines (`GatewayFirstLine`,
    `GatewayReadyForSecrets` in `crates/org-host-control/src/gateway.rs`), separate from
    the office worker's in `crates/org-office-launch/src/v3.rs`.
  - To delete with the stored sessions of ADR-0013: the absolute session lifetime
    (`ABSOLUTE`, `absolute_expires_at_ms` in `crates/org-host/src/service/authentication.rs`
    and `crates/org-host-control/src/ops.rs`), the `absolute_expires_at` field of the
    wire session and its test vectors (`crates/org-proto/src/gateway/session.rs`), the
    field the UI decoder requires (`apps/agentic-org-ui/src/lib/platform/http/wire.ts`),
    and the per-account session cap.
- **Done when:** registration from the link works once and only once; SES-1 to SES-7
  and the SIGN criteria for first access, sign-in, sign-out and expiry pass against a real instance, in a browser.
- **Needs:** A2.

### A4 · Create the organization

- **Outcome:** the signed-in administrator creates an organization in the browser and
  opens it. Whoever creates an organization is its owner.
- **Build:**
  - The create request carries the name, the slug, the remote organization repository
    and the state branch. It asks for no team.
  - The host creates the organization together with its HQ office, through the
    existing protected creation path, and starts the office.
  - Every employee sits in exactly one team, so creation makes one top-level team
    named after the organization, without asking. The creator becomes a person in it,
    and its lead, and their account owns the organization. Every account is tied to a person: from here on,
    what this account does in the organization carries that person's name.
  - The built-in HR lead is created as an employee in the top-level team. It is part of the
    product: it appears and is addressed as an agent, but it is not kept running. It
    runs only when called, and the onboarding of a new agent is product code it
    invokes (C2, C3).
  - The recovery code is released once, on the HQ machine.
  - Organization and office IDs are UUIDv7.
  - The UI's create flow talks to the real API.
- **Read:** [create organization design](design/create-organization.md),
  [web API design](design/web-api-v1.md) (selected create payload),
  [organization structure](design/organization-data/01-organization-structure.md),
  [offices and teams UX](specs/offices-and-teams-ux.md) (create organization; OFC-15),
  [installation and access UX](specs/server-installation-and-access-ux.md) (SRV-24,
  accounts, recovery), [ADR-0007](adr/0007-hq-creation-escrow.md),
  [ADR-0011](adr/0011-protected-recovery-delivery.md),
  [ADR-0016](adr/0016-no-application-level-data-encryption.md) (ENC-2). Rows FC-016,
  FC-018, FC-029, FC-031.
- **Code today:**
  - Two host commands create an organization. `organization.create` takes a name only.
    `organization.provision.v1` already carries the selected payload (name, slug,
    repository address, state branch, home team name: `crates/org-proto/src/selected_bootstrap.rs`)
    and is implemented by `crates/org-host/src/provisioning.rs` with the office's
    `hq_creation`. One command remains, named `organization.create` as the web API
    design has it, with the second one's payload and implementation; the name-only
    handler and the versioned name are deleted.
  - The UI flow runs against fakes (`apps/agentic-org-ui/src/lib/create-flow.ts`).
- **Done when:** an organization created in the browser exists after a restart with its
  owner as a person in the top-level team; its files are under `durable/state/`; the
  recovery code has been delivered once; ENC-2 passes.
- **Needs:** A3. A4 lands on the office store as it is. B1 then rebuilds that store
  underneath it and makes organization creation the first accepted transaction.

### A5 · A person on the command line

- **Outcome:** a person signs in from a terminal and runs commands against the running
  instance under their own name. This is how the commands of B2, B4 and B5 are used
  from a terminal before the office starts any agent.
- **Build:**
  - Sign-in on the command line through the host's local socket, approved in the
    browser: the terminal prints a one-time link, and the person opens it where they
    are signed in and approves. No password is typed in a terminal. The session ends
    when its connection closes.
  - The person's executable is `agentic-org`. After `agentic-org sign-in` it offers the
    same office commands that `agent-os` offers an agent (teams, people, tasks,
    messages), from one shared library, under the person's name. `agent-os` is for
    agents only and refuses to run without the binding of a run the office started.
  - A person-started command names the instance and the office, or uses a saved context
    bound to those two IDs. With neither it fails and says how to select one.
  - Each call reaches the office with the person's principal and a host-issued grant,
    exactly as a browser request does, and is recorded under that person. This replaces
    the application token that A1 deletes.
  - Nothing here gives an agent a way in. An agent is an employee the office starts,
    and it uses the binding of its run (C3).
- **Read:** [ADR-0003](adr/0003-agent-messaging-office-first.md) decisions 12 and 13 and
  "Person-started commands", [ADR-0013](adr/0013-stored-browser-sessions.md) (SES-8),
  [host boundary design](design/office-host-boundary.md),
  [`apps/agent-os/README.md`](../apps/agent-os/README.md). Row FC-014.
- **Code today:** a person or the owner binds to the office with `app_token`
  (`apps/agentic-org-office/src/serve.rs`), which A1 removes; `agent-os` talks to an
  office only when one is named and otherwise writes to a file queue, which B6 deletes.
- **Done when:** SES-8 passes; a signed-in person runs a read command and a write
  command against the office from a terminal; the same commands are refused without a
  session.
- **Needs:** A4.

---

## Step 2 · The organization: office, projects, teams and people

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
  replication with S1.
- **This is the largest increment in the plan.** It starts with a written plan, recorded
  in the repository: which record families move first, and what is deleted.
- **Needs:** A1's change to the office store.

### B2 · Organization, teams and people

- **Outcome:** through the command line and the API, the owner creates teams, adds
  people, sets a team lead, and moves someone to another team. Agents are not added
  here: an agent comes to exist only through a hire (C2).
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

### B3 · Projects, the minimum

- **Outcome:** a person creates a project, and the project has its team.
- **Build:** projects with their number and slug from HQ, the accountable team, and
  contributing teams. Creating a project creates its team by default: a new team
  named after the project, under the top-level team, accountable for it. Naming an
  existing team instead stays possible. No features, releases or repositories yet.
- **Read:** [project structure](design/organization-data/02-project-structure.md). Rows
  FC-030, FC-038.
- **Done when:** a project and its team exist after a restart, created from a terminal
  and through `/api/v1`.
- **Needs:** B2.

### O1 · The office in the product

- **Outcome:** a person sees the organization's offices with their readiness, and an
  administrator or the organization's owner restarts one.
- **Build:** the office list and each office's state as the host and the office report
  them; restart of an office through the host. The HQ office is the only office until a
  second one is enrolled (milestone three, S1).
- **Read:** [offices design](design/offices-hq-and-satellites.md),
  [offices and teams UX](specs/offices-and-teams-ux.md) (OFC criteria that need one
  office), [host boundary design](design/office-host-boundary.md). Rows FC-019, FC-021.
- **Code today:** the host supervises and restarts an office worker
  (`crates/org-host/src/supervisor`); the browser's office status screens run against
  fakes.
- **Done when:** the office list shows the real HQ office and its state from a terminal
  and through `/api/v1`, and a restart from there brings it back ready.
- **Needs:** A4.

### A6 · A second person can sign in

- **Outcome:** the owner invites the second engineer from the browser; that person
  creates their account from the link, signs in, and works in the same organization
  under their own name.
- **Build:**
  - In People, the administrator or the organization's owner invites a person to sign
    in. That creates a one-time registration link, shown once to pass on. The person
    chooses their own username and password, as in A3. The product sends no email.
  - The same people issue a new link for someone who forgot their password, and remove
    a person's access, which signs them out everywhere. A signed-in person changes
    their own password in the browser.
  - There is no account command besides `agentic-org admin reset` (A3).
  - Every account is tied to a person (owner decision, 2026-11-17). In each
    organization an account can open, it is tied to exactly one person record, and
    what the account does there carries that person's name. A person does not need an
    account. An account is never tied to an agent.
  - An owner hands ownership of the organization to another person who has an account.
    An organization has exactly one owner.
  - A person is whatever place the administrator or owner gives them in the hierarchy:
    a member of a team that an agent leads, the lead of a team of agents, or a lead of
    leads. There are no access roles. Any person can interact with any agent; position
    only decides whose instructions an agent follows
    ([supervision design](design/employee-supervision.md), decision 6). The few rights
    that are not everyone's (changing the structure, inviting and removing access,
    the two hand-overs, approving a hire, creating organizations) and who holds each
    are one table in the [agent structure contract](design/organization-data/04-agent-structure.md#accounts-people-and-the-organization-owner).
- **Read:** [installation and access UX](specs/server-installation-and-access-ux.md)
  (accounts), [agent structure](design/organization-data/04-agent-structure.md),
  [index, what the host owns](design/organization-data/STATUS.md#outside-this-set-what-the-host-owns).
  Rows FC-029, FC-153.
- **Code today:** `crates/org-host/src/accounts` holds only the login value types.
  Account storage is `crates/org-host/src/hostdb/accounts.rs`; its schema allows many
  accounts and several linked organizations, but the only insert is the first
  administrator account. Nothing issues or redeems a link. The account operations
  defined for the command line in `crates/org-host-control` (`account.create` among
  them, which has no handler) are removed. Account IDs are still ULIDs and become UUIDv7.
- **Done when:** two people are signed in at once from two browsers, and a change each
  makes carries their own name.
- **Needs:** A3, B2.

### R1 · Open an existing organization

- **Outcome:** on a server that has lost its database, or on a new server, the
  administrator registers again, opens the organization from its repository, and finds
  its teams, people, projects and records as they were.
- **Build:** the "open an existing organization" choice beside "create"; validation of
  the files against the data contract; registration at the host under the existing ID;
  the office's projection rebuilt from the files; the administrator choosing their
  person record and becoming the owner; people invited again; a plain statement of
  what did not come back.
- **Read:** [recovery contract, opening an existing organization](design/organization-data/16-recovery-and-office-continuity.md#opening-an-existing-organization)
  and its loss boundaries, [identity contract](design/organization-data/06-identity-and-numbering.md),
  [ADR-0007](adr/0007-hq-creation-escrow.md). Row FC-167.
- **Code today:** nothing opens an organization from files; the office's store is the
  source of truth until B1.
- **Done when:** with `runtime/` and the host's database deleted, the organization is
  opened again from its repository and every record published before the loss is
  there; an invalid organization record refuses the open and names the file.
- **Needs:** B1, A4, A6.

### U1 · Offices, teams, people and projects in the browser

- **Outcome:** the office list, the team directory, the organization chart, the project
  list and the add, edit and move dialogs work against the real API.
- **Build:** replace the fake-backed adapters for these screens; keep the screens.
- **Read:** [offices and teams UX](specs/offices-and-teams-ux.md) (TEAM, PEOPLE, REPORT
  criteria), [our organization in the app](specs/our-org-in-the-app.md),
  [app shell UX](specs/app-shell-ux.md). Rows FC-003, FC-019, FC-025 to FC-027,
  FC-030, FC-032, FC-038.
- **Code today:** `apps/agentic-org-ui/src/lib/components` has the dialogs and charts;
  `src/lib/platform/unavailable-api.ts` is what production uses today.
- **Done when:** the TEAM, PEOPLE and REPORT criteria that need no agent runtime pass in
  browser tests against a real instance.
- **Needs:** A3, B2, B3, O1.

---

## Step 3 · Agents

### C1 · The sign-in spike

- **Outcome:** a written result: for each vendor tool, how it is pointed at a private
  home, where its sign-in is stored, and whether it resumes a session from there.
- **Read:** [ADR-0015](adr/0015-agent-isolation-modes.md) decisions 7, 10 and 11,
  [agent runtimes research](research/agent-runtimes-control.md).
- **Done when:** the result is recorded and the first adapter to build is chosen.
- **Needs nothing else and can start at any time.**

### C2 · Hire an agent through HR

- **Outcome:** the owner asks for a new agent, reviews the proposal, approves it, and
  the agent exists in the organization with its team, its home and how it runs. This is
  the only way an agent comes to exist.
- **Build:**
  - The hire flow: describe what is needed, or fill in the details yourself; review;
    approve. Nothing is provisioned before approval.
  - Provisioning, as product code: the employee record, its private home, and its
    execution configuration (model profile and connection).
  - The built-in HR lead drafts the proposal once agents can run (C3). Until then, and
    whenever HR cannot run, the same flow works with the form.
  - Edit, move, pause, resume and offboard an agent.
- **Read:** [our organization in the app](specs/our-org-in-the-app.md) (the HR lead; the
  hire flow; who approves; STAFF and HRL criteria), [roles UX](specs/roles-ux.md),
  [agent structure](design/organization-data/04-agent-structure.md),
  [model profiles and agent execution](design/organization-data/11-model-profiles-and-agent-execution.md),
  [providers and connection pools](design/organization-data/14-model-providers-and-connection-pools.md).
  Rows FC-031, FC-033, FC-034, FC-096, FC-097.
- **Done when:** the STAFF and HRL criteria for a hire pass with the form, and no
  employee, home or binding exists before approval.
- **Needs:** B2, U1.

### C3 · Start an agent

- **Outcome:** from an agent's page the owner gives it an instruction; the office starts
  it, it runs a turn in its own home, and the result is shown. The built-in HR lead
  runs the same way when called, and now drafts hire proposals.
- **Build:** the private agent home; the `same-user` isolation mode in the launch
  description, with the product stating the reduced protection of that mode; one vendor
  adapter behind the runtime contract; one open turn per agent, started only by an
  explicit instruction; `agent-os` bound to the run the office started.
- **Read:** [agent sessions design](design/agent-sessions.md),
  [how agents run](specs/agent-run-modes-ux.md), [ADR-0004](adr/0004-headless-agent-sessions.md),
  [ADR-0015](adr/0015-agent-isolation-modes.md) (ISO-3, ISO-5, ISO-8),
  [ADR-0016](adr/0016-no-application-level-data-encryption.md) (ENC-4),
  [ADR-0003](adr/0003-agent-messaging-office-first.md) decision 12,
  [employee supervision UX](specs/employee-supervision-ux.md) (EMP criteria). Rows FC-031,
  FC-098, FC-100, FC-101.
- **Code today:** `crates/org-runtime` is the contract; only `crates/org-supervise`
  uses its types, and nothing launches an agent.
  `crates/org-supervise` starts, stops and cleans up processes; it is used only to
  recover processes the office owned before a restart, not to launch an agent.
- **Done when:** a hired agent completes an instruction given from its page, and the HR
  lead drafts a hire proposal when asked.
- **Needs:** C1, C2.

### C4 · See what an agent did

- **Outcome:** a person opens an agent's session in the browser and reads it, with
  tool calls and file changes shown or hidden.
- **Build:** first a proof of concept in the UI against sample Claude and Codex
  conversation files; then the parsing and redaction in the office, served to authorized
  people.
- **Read:** [REQ-05](specs/req-05-virtual-console-conversation-log-viewer.md). Row FC-150.
- **The proof of concept needs nothing else and can start at any time.** The rest
  needs C3.

### C5 · The agent follows the organization's processes

- **Outcome:** an agent's instructions contain the processes and standards that apply to
  its work, with the non-overridable ones marked as must-follow.
- **Build:** the one-chain resolution (organization, the accountable team's chain, the
  project) and its delivery at launch.
- **Read:** [processes design](design/processes.md), [processes UX](specs/processes-ux.md).
  Rows FC-110, FC-112.
- **Code today:** `crates/org-process` resolves several chains, which the design
  replaces with one, and has no caller.
- **Needs:** C3.

---

## Step 4 · Messages: chat and asks

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
    suspension that follows is increment C6.
- **Read:** [REQ-04](specs/req-04-channel-messaging-and-question-components.md),
  [ADR-0020](adr/0020-required-asks-suspend-and-resume.md) decisions 1 to 5 and 14
  (ASK-1, ASK-2), [chat UX](specs/chat-ux.md) (CHAT-6). Rows FC-079, FC-166.
- **Done when:** each question type is asked from a terminal and answered in the
  browser; ASK-1 and ASK-2 pass.
- **Needs:** B6.

### C6 · A required ask suspends and resumes the agent

- **Outcome:** an agent that cannot continue without an answer is stopped after being
  told, and starts again in the same session when the answer arrives.
- **Read:** [ADR-0020](adr/0020-required-asks-suspend-and-resume.md) decisions 5 to 13.
  Row FC-166.
- **Done when:** ASK-3, ASK-5, ASK-6, ASK-7 and ASK-9 pass, and the presence part of
  ASK-4. The rest of ASK-4 (no turn permit and no subscription slot while suspended) is
  finished with L4, and ASK-8 (a budget hold keeps the agent suspended) with L3.
- **Needs:** B7, C3.

---

## Step 5 · Tasks

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
  terminal. An agent uses the same commands through `agent-os`.
- **Build:** the task commands of `agent-os`, with the project taken from the agent's
  context and required when there is none; the board, list and detail screens against
  the real API, updated by the change stream.
- **Read:** [work items UX](specs/work-items-ux.md) (TSK, BUG, CHK, WORK criteria),
  [identity and numbering, default project](design/organization-data/06-identity-and-numbering.md#stable-ids-readable-references-and-scopes).
  Rows FC-038, FC-039, FC-043.
- **Done when:** the TSK, BUG, CHK and WORK criteria that need no agent runtime pass in
  browser tests against a real instance; a task created in the browser is updated from
  a terminal and the browser shows the change without a reload.
- **Needs:** B4, A3, A5. The agent part needs C3.

### C7 · Repositories and worktrees

- **Outcome:** an agent works in a worktree of a registered repository and the office
  pushes its task branch.
- **Read:** [Git design](design/projects-git-github.md) and its open questions, which the
  engineer answers first. Rows FC-103, FC-139, FC-140.
- **Needs:** C3, B4.

---

## Milestone two · Linux, in parallel (Track X)

Linux runs as its own track, in parallel with milestone one (owner decision
2026-11-17). Its design is [Linux host](design/linux-host.md); its acceptance criteria are
LNX-1 to LNX-6 of [ADR-0017](adr/0017-linux-and-container-hosting.md); its checklist row
is FC-164. The Track X engineer works on Linux; the increments touch the macOS
increments' files only where [the design](design/linux-host.md#7-working-in-parallel-with-the-macos-track)
says, and agree those commits with the Track A lead.

Which increment finishes each criterion: LNX-1 in X2, LNX-2 in X5, LNX-3 in X4,
LNX-4 in X3, LNX-5 and LNX-6 in X6 (X1a starts the LNX-5 check; X2 makes the Linux
half of LNX-6 run).

### X1a · The platform crate and the check

- **Outcome:** none a person can see. From the first day no change can add a
  platform branch outside one crate, and A2 has a contract to write service
  registration behind.
- **Build:**
  - The crate `crates/org-platform`, with its module layout and the `Refused` type,
    and the `service` contract with a refusing adapter, agreed with the A2 lead.
  - The CI check: a script that parses every `.rs` file outside
    `crates/org-platform/` with `syn` and fails on a `cfg`, `cfg_attr` or `cfg!`
    predicate naming `target_os`, `target_vendor`, `target_env` or `target_family`,
    with an allowlist of today's files that may only shrink.
- **Read:** [Linux host design](design/linux-host.md) sections 1, 3.4 and 7,
  [architecture rules](architecture-rules.md#planned-variation-goes-behind-an-adapter-from-day-one).
- **Code today:** 44 source files and 35 test files outside any adapter name a
  platform target. The source files are 29 in `org-host` (among them `spawn.rs`,
  `server_check.rs`, `code_signing.rs`, `peer.rs`, `app_auth.rs`, `bootstrap.rs`,
  `human.rs`, `offices.rs`, `recovery.rs`, `release.rs`, `writer.rs`, `hostdb.rs`,
  `lib.rs`, `keys/mod.rs`, `host_secrets.rs`, `supervisor/mod.rs`,
  `supervisor/child.rs`, `supervisor/channels.rs`, `instance/directory.rs`,
  `instance/mod.rs`, `instance/registry.rs`, `recovery/presence.rs`,
  `release/platform.rs`), 6 in `org-socket`, 6 in `org-supervise` (among them the
  `compile_error!` in `lib.rs`), `org-office`'s `store/acceptance/durable.rs`,
  `org-client`'s `agent.rs` and `org-spool`'s `safe_read.rs`.
  `org-proto/src/location.rs` hard-codes macOS values with no conditional. The
  [design](design/linux-host.md#2-what-each-executable-needs-from-the-platform) maps
  each to its contract. Milestone-one work in those files adds no new conditional.
  CI lints `org-host` and `agentic-org` on Linux with `dead_code` and
  `unused_imports` allowed and runs none of their tests there.
- **Code today (2026-11-18):** A2 B0 created `crates/org-platform` with the `Refused`
  type and the `service`, `disk` (disk-encryption state for ENC-3, a new capability)
  and `dirs` (the application base only) modules, each with its macOS and refusing
  adapters and one non-conditional constructor, so that A2 never added a platform
  predicate outside the crate. X1a adds the CI check, the allowlist and the remaining
  module layout, and reviews the `service` contract as built (section 3.4 of the
  design).
- **Done when:** the crate builds on macOS and Linux; the check runs in CI on both and
  fails on a new predicate; the A2 lead has accepted the `service` contract. Merged on
  the first day of Track X.
- **Needs:** nothing.

### X1b · Files, locations, processes and sockets behind the crate

- **Outcome:** none a person can see. Durable flushes, exclusive renames, filesystem
  checks, the default locations, process supervision and socket peer checks of all
  four executables come from `org-platform`.
- **Build:** the `fs`, `dirs`, `process` and `socket` modules with today's macOS code
  and today's Linux code moved in unchanged and refusing adapters elsewhere;
  `org-proto/src/location.rs` keeps only path composition; `org-supervise`'s
  `compile_error!` goes; the allowlist shrinks by every moved file.
- **Read:** [Linux host design](design/linux-host.md) section 4.
- **Code today:** see X1a.
- **Done when:** none of the moved files has a platform predicate; the macOS and Linux
  CI runs pass unchanged; the macOS CI run is the macOS verification.
- **Needs:** X1a. Ordered with A2 for `instance/`, `home.rs` and `hostdb`.

### X1c · Admission, confinement and presence behind the crate

- **Outcome:** none a person can see. Starting and admitting a worker or the web
  process, confining the web process and the recovery presence check go through
  `org-platform` on macOS, with refusing adapters elsewhere.
- **Build:** the `admission`, `confinement` and `presence` modules with today's macOS
  code (suspended spawn, audit tokens, Developer ID checks, Seatbelt, the presence
  bridge and its build script); `ServerCheck` becomes the policy that builds
  `ExpectedImage`; `ChildEnv` refuses loader variables; the supervisor's call sites
  use `Admission::spawn` and `verify_reader`.
- **Read:** [Linux host design](design/linux-host.md) sections 3.1, 3.3 and 4.
- **Code today:** see X1a; `reader_is_child` in `supervisor/channels.rs` refuses on
  every system but macOS.
- **Done when:** none of the moved files has a platform predicate; on Linux
  `cargo clippy --workspace --all-targets -- -D warnings` passes with no allowed lint
  and no excluded crate; the Track A engineer has run the signed-build admission,
  Seatbelt and presence-dialog checks on a Mac and recorded the result in the merge.
- **Needs:** X1b. Ordered with A1 (`supervisor/`); merged before A3 starts the web
  process.

### X1d · Credentials behind the crate

- **Outcome:** none a person can see. Both host stores sit on one `Credentials`
  contract.
- **Build:** the `credentials` module with the Keychain adapter and one directory
  engine that replaces the two development stores; `KeyStore` and `HostSecrets` become
  typed views.
- **Read:** [Linux host design](design/linux-host.md) section 3.2.
- **Code today:** `org-host/src/keys/` and `host_secrets` each have a Keychain store
  and a separate development store.
- **Done when:** no file in `keys/` or `host_secrets` has a platform predicate; the
  Track A engineer has run the signed Keychain checks on a Mac and recorded the
  result in the merge.
- **Needs:** X1c, and A1's change of the database key to the office secrets key.

### X2 · The first office worker on Linux

- **Outcome:** on Linux, the host starts an office worker, the worker serves its local
  socket, and `agent-os` sends a message and reads it back.
- **Build:**
  - The Linux admission adapter: the verified image copied into a sealed, execute-only
    memory file (retrying without `MFD_EXEC` before Linux 6.3) and executed from it,
    the child held by a pidfd, the rendezvous peer checked with `SO_PEERCRED`; the
    release digest from the signed manifest, and the development build's own output.
  - The Linux `fs`, `dirs`, `process` and `socket` adapters the start path needs.
  - `org_platform::testing`: the path-admission test adapter, onto which the
    shell-script worker tests (`script()` in `org-host/src/supervisor/mod.rs`,
    `wrapper()` in `apps/agentic-org/tests/support/mod.rs`) move, and the expected
    operating-system values that replace `cfg` in tests.
  - The root refusal at the `serve` composition; the non-dumpable test drops its child
    to an unprivileged user when run as root.
  - The host's integration tests and `org-host`'s tests run on Linux.
- **Read:** [Linux host design](design/linux-host.md) sections 3.1 and 4,
  [host boundary design](design/office-host-boundary.md) (admission and the verified
  rendezvous). LNX-1.
- **Code today:** `crates/org-host/src/spawn.rs` returns "office servers run only on
  macOS"; six tests in `supervisor/channels/tests.rs` fail on Linux.
- **Done when:** LNX-1 passes in a test that starts the real host composition on Linux
  and uses `agent-os` through the same authenticated route the macOS tests use when X2
  lands; a worker image one byte different from the expected digest, and a child
  environment with `LD_PRELOAD`, each receive no secret; the Linux CI job runs
  `cargo test --workspace` with no excluded crate; no test file stays allowlisted
  because Linux could not start a worker.
- **Needs:** X1c.

### X3 · Host secrets on Linux

- **Outcome:** a Linux host keeps its office secrets keys and its own secrets in an
  owner-only store outside the data root, and refuses a store set up wrongly or found
  empty.
- **Build:** the Linux directory credential adapter of the design, with its owner,
  mode, private-group, filesystem, `(device, inode)` disjointness, mount-point,
  empty-store and lock checks; the `credentials` property of non-interactive setup.
- **Read:** [Linux host design](design/linux-host.md) section 3.2,
  [ADR-0016](adr/0016-no-application-level-data-encryption.md) decision 3. LNX-4.
- **Code today:** a Linux release build has no store and refuses.
- **Done when:** each wrong owner, mode, group, filesystem, store inside the data
  root, and an empty store for an instance with keys refuses the start (temporary
  roots); LNX-4 passes: a scan of an instance provisioned by setup finds every secret
  only in the store, readable only by the service account.
- **Needs:** X1d, X2, and A2 (setup provisions the instance).

### X4 · The web process confined on Linux

- **Outcome:** on Linux the web process confines itself with Landlock and seccomp
  before it handles input, and refuses to start where it cannot.
- **Build:**
  - First, on the first day of Track X: a one-hour probe of Docker Desktop's kernel on
    a Mac (Landlock ABI, seccomp, `memfd_create` and `execveat`). If it has no
    Landlock, the web process cannot run in a container on a Mac, and the Track X lead
    brings that to the owner with a recommendation before this adapter is built.
  - The Linux confinement adapter (Landlock ABI 3 or higher handling every filesystem
    right, a seccomp allowlist that kills on anything else, `prctl` and `ioctl`
    filtered by argument) and the host's check of the child's seccomp and
    no-new-privileges state.
- **Read:** [Linux host design](design/linux-host.md) section 3.3,
  [ADR-0008](adr/0008-gateway-macos-sandbox.md). LNX-3.
- **Code today:** `org-socket/src/confinement.rs` has a refusing adapter on every
  system but macOS; the host does not start the web process yet (A3).
- **Done when:** LNX-3 passes: with Landlock unavailable, below ABI 3 or blocked by a
  seccomp profile, the web process refuses and reports which (proved first with the
  confinement example process); confined, it cannot open a file, create a socket or
  start a program, and its whole HTTP and TLS path works under the filter on the
  development system and in the container image.
- **Needs:** X1c; the HTTP and TLS proof needs A3.

### X5 · Service and container

- **Outcome:** an instance runs as a systemd user service on a Linux machine, and the
  whole instance (host, web process and office) runs as the main process of a
  container built from the repository's container file, with its data and its secrets
  on two named volumes, on Linux and in Docker on an engineer's Mac, reachable from
  that Mac's browser.
- **Build:** the systemd user-unit adapter; the foreground mode and `serve --setup`;
  the `service` setup property; stopping within 8 seconds on `SIGTERM`; the container
  file (Debian base, UID 61000, `tini`, root-owned release, `/data` and `/secrets`),
  its seccomp and AppArmor profiles, and the AppArmor profile for vendor sandboxes on
  Ubuntu hosts; the container's HTTPS listener with an operator-supplied certificate
  pair and the host's verifier for it, unless A3 has built it; the documented run
  command and the `docker exec -u 0 … admin reset --user` step.
- **Read:** [Linux host design](design/linux-host.md) sections 3.4 and 3.5,
  [standalone server design](design/standalone-server.md) (service, non-interactive
  setup). LNX-2.
- **Code today:** `org_platform::service` holds the contract and the macOS launchd
  adapter (A2 B0); Linux and every other system get the refusing adapter, so setup
  there refuses before it changes anything (A2 decision D7). SRV-34's "a missing
  service manager reports foreground instructions" waits for this increment.
  `org-host/src/signals.rs` stops on `SIGTERM` and `SIGINT`; a supplied TLS pair is refused until the host
  certificate verifier exists.
- **Done when:** LNX-2 passes, including after the container is replaced with the same
  volumes; in the container on a Mac, the administrator registers from the link
  `admin reset` prints and signs in at `https://localhost:8443`; replacing the
  container with a new, empty secrets volume is refused, not silently re-keyed; a
  systemd user service survives a restart of the machine when lingering is on, and
  setup reports when it is off.
- **Needs:** X2, X3, X4, A2 (setup) and A3 (the web process and sign-in).

### X6 · Both systems in CI

- **Outcome:** none a person can see. Every change is checked on both systems and in
  the container.
- **Build:** the CI matrix runs the full workspace checks on macOS and Linux with no
  exclusion; the platform check runs with an empty allowlist; a job builds the
  container image and runs the LNX-2 send and read in it.
- **Read:** ADR-0017. LNX-5, LNX-6.
- **Code today:** `.github/workflows/ci.yml` excludes `org-host` and `agentic-org` from
  the Linux clippy and test runs.
- **Done when:** LNX-5 and LNX-6 pass; FC-164 is Done.
- **Needs:** X2 to X5.

---

## Milestone three · A second office

### S1 · A second office on another machine

- **Outcome:** an owner adds an office on another machine; agents placed there work
  when HQ is unreachable, and messages reach people and agents in the other office.
- **Build:** enrolment of the office from HQ, replication of the organization's data
  between HQ and the office, and relay of messages between offices through HQ.
- **Read:** [offices design](design/offices-hq-and-satellites.md),
  [ADR-0001](adr/0001-app-is-a-client-office-is-a-server.md),
  [ADR-0003](adr/0003-agent-messaging-office-first.md) decisions 4 to 8,
  [offices and teams UX](specs/offices-and-teams-ux.md) (OFC criteria for a second
  office), [recovery contract](design/organization-data/16-recovery-and-office-continuity.md).
  Rows FC-020, FC-145; the replication suite of ADR-0018 (ENG-3).
- **Code today:** `crates/org-sync` is the replication core; nothing enrols an office.
- **Needs:** milestone one. It starts with its own short plan.

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
| L6 | The `dedicated-user` isolation mode | [ADR-0015](adr/0015-agent-isolation-modes.md), FC-161; ENC-4 for this mode |
| L7 | Features, knowledge, releases and deployments | documents [03](design/organization-data/03-features-and-work.md), [07](design/organization-data/07-release-deployments.md), [09](design/organization-data/09-release-content-and-flags.md), FC-058 to FC-070 |
| L8 | Private task notes | [document 05](design/organization-data/05-operational-data.md#private-notes-attached-to-tasks), FC-053 |
| L9 | Install from a signed release, update, uninstall | [standalone server design](design/standalone-server.md), FC-022, FC-154 |
| L10 | Rename `gateway` to `web` in code identifiers | Mechanical, any time, with one exception: the signing identifier constant, already renamed (`WEB_SIGNING_ID` in `crates/org-host/src/server_check.rs` and `release/platform.rs`), is checked at release admission |

## Checklist rows that ride with an increment

Rows of the [feature checklist](FEATURES.md) that no increment above names, and where
each is finished.

| Rows | What | Finished with |
| --- | --- | --- |
| FC-004, FC-005 | Responsive layout, keyboard access, shared forms and feedback | Every browser increment, first U1 |
| FC-108 | Role records and assigning a role | C2 |
| FC-036, FC-102 | Live activity, presence, progress and health of a run | C3 |
| FC-111 | Process list and editor in the browser | C5 |
| FC-080, FC-081 | Task handoff notices; delivery that never interrupts, pause and waiting | B6, C6 |
| FC-035 | Home and the inbox of things that need you | B7 |
| FC-047, FC-048, FC-049 | Requests: capture, routing, scoping and approval | B4, B5 |
| FC-051, FC-052, FC-055 | Task output, work and waiting time, needs-me views | B5 |
| FC-024 | Release acceptance run with a second office | S1 |
| FC-006, FC-007, FC-008 | Search and New, device preferences, account time zone | Later |
| FC-023 | Logs, doctor and support diagnostics | Later, with L9 |
| FC-123, FC-124 | Drafting text and a bug from a sentence | Later |
| FC-147, FC-148 | Restore of protected data; credential rebind and cache rebuild | Later |
| FC-151 | Taking over an agent's session from a terminal and handing it back | Later |

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
