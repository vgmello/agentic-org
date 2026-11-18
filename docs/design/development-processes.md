# Design: development processes and project documentation

How a project is given a way of building software without the product knowing any
methodology: a development process, the roles it starts a project with, the tabs it
gives the project, how its guidance reaches each agent, how agents hire when it says
it is time, and where a project's documents live. It implements the owner's
[REQ-07](../specs/req-07-composable-process-packages-and-team-blueprints.md). The
records are owned by
[document 18](organization-data/18-development-processes-and-project-documentation.md);
[projects](organization-data/02-project-structure.md),
[roles](organization-data/01-organization-structure.md) and
[employees](organization-data/04-agent-structure.md) own what it builds on. The
decision and what was rejected are in
[ADR-0021](../adr/0021-development-processes-are-content.md). The first one, for
desired state specifications, is in the
[desired state design](desired-state-specifications.md).

## Two kinds of process

The owner's rule (2026-11-18): there are two primitives, and they stay apart.

- An **organization process**, with its standards, says how the organization works
  whatever is being built: "before you commit, run these checks". It is written at
  the organization, a team or a project, inherited downward, and can be must-follow.
  It is the [process design](processes.md).
- A **development process** says how a project builds software: which roles start
  it, what each role does, which documents it produces, which tabs the project has.
  The owner's name for the kind is an agentic software development process (ASDP).
  The desired state specification (DSS) is one; feature delivery is another. A
  project follows at most one. This design is about these.
- A **skill** is one piece of a development process's guidance, written for one or
  more roles.

Both kinds are text an agent reads and applies. Neither is code the product runs.
Neither inherits from, overrides or contains the other. Where they disagree, the
organization's process or standard is followed.

## Decisions

1. **The product holds primitives; a development process holds the method.**
   Projects, teams, releases, milestones, tasks, documents, roles, hiring, messages
   and asks are the product's. What to do with them, in what order and by whom is a
   development process's content. No process name, role name, document kind or tab
   title appears in product code, and adding a methodology adds no code.
2. **No engine.** A development process's stages describe the order of work. The
   product shows them to people and to agents. Nothing in the product acts on a
   stage, starts work because a document changed, or blocks an action. An agent acts
   because a skill told it to and it has a turn.
3. **A development process is content.** Some ship with the product. The others are
   created by the organization's people: in the editor, by copying one, or by
   importing a process file. Any person may create and edit one.
4. **A development process grants nothing.** It names no tool, model, credential,
   budget or approver. Authority still comes from the caller's place in the
   organization, and every call is still authorized by the office.
5. **Skills are delivered beside organization processes, not through them.** An agent
   working in a project receives two separate sections: the organization processes
   and standards that apply, and its project's development process with the skills
   for its role. A skill has no level, no override key and no lock.
6. **Skills are bound to roles by key.** An agent receives the skills written for its
   role and the ones written for everyone. An employee still has one role.
7. **A project starts lean.** A development process marks a few roles as its seed.
   Choosing it shows them; a person fills each by a hire or with an employee who
   already has the role. Everyone else is hired when a skill says the work calls for
   it.
8. **Hiring stays the product's.** An agent asks for a hire; HR drafts it; a lead
   approves it; product code provisions it
   ([document 04](organization-data/04-agent-structure.md#who-approves-a-hire),
   [ADR-0022](../adr/0022-a-lead-approves-a-hire.md)). A lead that is an agent
   approves only when a person has turned its May hire permission on. A development
   process decides when a lead asks and for which role; it gives no one the right to
   approve.
9. **Releases and milestones are first-class in every project.** Whatever the
   process, a change goes into a release, and a milestone may group work inside it.
   Milestones are optional; releases are not.
10. **A development process declares the project's tabs.** The product provides a
    small set of generic views. A process lists the things it has, each as a tab
    built from one of those views. Features are one such thing: a project has them
    when its process says so.
11. **A project's documents live in one tree whose layout the project owns.** The
    process declares the sections it starts with; agents and people grow it. The
    product has no document type for any methodology.
12. **One index, kept by the office from what agents do.** Every project has
    `docs/index.yaml`, the table of contents of its tree. One read answers where
    anything is.
13. **A project's documentation is its own.** An agent reads and writes the tree of
    its own projects only. What it needs from another project it asks that project's
    agents and people for.
14. **Agents hand work to each other.** An ask addressed to an agent, or a task
    assigned to it by its leads or its project team, starts it
    ([ADR-0023](../adr/0023-an-ask-or-an-assignment-starts-an-agent.md)).
15. **Changing a project's development process removes nothing.**

## 1. Where each thing lives

| Thing | Source |
| --- | --- |
| Organization's development process | Organization Git `development-processes/<process-id>/` |
| Built-in development process | The running product; projected with its fixed ID and no file |
| A project's development process | `development_process_id` in `projects/<project-id>/project.yaml` |
| Role an employee holds | Organization role record, named by the employee's `role_id` |
| Project documents, diagrams, images | Organization Git `projects/<project-id>/docs/` |
| The index | `projects/<project-id>/docs/index.yaml` |
| Issued versions | `projects/<project-id>/docs-history/<document-id>/<version>.md` |
| A project's tabs, seed roster, an agent's skills | Computed; never authored |

## 2. The development process

The manifest, the role, skill and template files, and their rules are in
[document 18](organization-data/18-development-processes-and-project-documentation.md#development-processes).
In short:

```text
development-processes/<process-id>/
├── process.yaml      # name, summary, roles (some seed), kinds, folders, tabs, stages
├── roles/<key>.md    # what each role is for
├── skills/<key>.md   # what a role does and how; front matter says which roles
└── templates/<key>.md
```

- **Catalogue.** The list shows every `current` development process, built-in ones
  first, each with its summary, its seed roles, its tabs and the sections it starts a
  project with. A draft is visible to people and cannot be chosen for a project.
- **Creating one.** Any person of the organization creates a development process in
  the editor, copies an existing one, or imports a process file. Every write names
  the expected revision and is attributed. An agent cannot create or change one.
- **Importing.** A process file is an archive of the folder above. Importing it
  validates it like any write, gives it a new ID, and adds it as a draft; if its key
  is taken the person chooses another. Any development process can be downloaded as
  the same file. That is all "portable" means: there is no link, sync or sharing
  between organizations.
- **Validation at every write.** The manifest, role, skill, tab and stage fields are a
  closed list
  ([document 18](organization-data/18-development-processes-and-project-documentation.md#durable-process-files));
  any other field is refused, naming it, so that a process cannot smuggle a setting.
  A skill names only roles of its own process. A kind is not named like a built-in
  kind. Folder paths and tab keys are valid and do not repeat. A `current` process
  has an owner, at least one role and at least one skill.
- **In use.** A development process that a project follows cannot be deleted. It can
  be deprecated: its projects keep working and a notice on each says so.
- **Built in.** Built-in development processes are part of the product and change
  with it. They cannot be edited; a copy can. The two are `dss`
  ([desired state design](desired-state-specifications.md)) and `feature-delivery`
  (section 5).
- **Adjusting one project.** There is no per-project override of a skill. A project
  that needs a different skill follows a copy of the process with that skill changed.

## 3. Choosing a project's development process

**Choosing.** The project form has a **Development process** select listing the
catalogue and "None". It can be set when the project is created and changed later, by
the organization's owner or a person in the lead chain of the project's owning team.

**What choosing does,** in one accepted change at HQ:

1. Sets `development_process_id` on the project.
2. Creates an organization role, from the process's definition, for every role of the
   process whose key the organization does not have yet. Existing roles are not
   touched. Before confirming, the person sees which roles will be created and which
   existing roles the process will use, so that a role that merely shares a key is
   not matched by surprise.
3. Adds to the project's index every section the process declares and the project
   lacks. The sections are there, with their titles and summaries, before any
   document exists.

The project's tabs change at once, because they are computed from the process
(section 4). Nothing else happens. No agent is hired, started or told anything by the
choice itself.

**The seed roster.** The project's Overview then shows one row per seed role:

```
 Development process · Desired State Specification                        [Change…]
 ───────────────────────────────────────────────────────────────────────────────────
 Seed team
 Product Owner / Principal Designer        Open                    [Hire]  [Assign…]
 System Architect / Senior Staff Engineer  Hire requested · waiting for you
```

- A seed role is **filled** when an active employee whose home team is the project's
  owning team, or a team beneath it, has that role. The roster is derived; nothing
  stores it.
- **Hire** opens the [hire flow](../specs/our-org-in-the-app.md) with the role and the
  project's team preset. Approval and provisioning are unchanged.
- **Assign…** lists active employees who have the role, or lets a person give the role
  to one; either way the employee is moved to the project's team by the ordinary move.
  Creating the project with an existing team as its owner fills at once every seed
  role that team's members already hold.
- An open seed role blocks nothing. The project says "2 seed roles open" until they
  are filled.
- **Who leads.** A project always has a team, and the team has one lead, appointed by
  the owner like any team lead: a person, or one of the seed agents. An agent follows
  instructions from the leads above it and treats anyone else's as a request
  ([document 04](organization-data/04-agent-structure.md#who-an-agent-takes-instructions-from)),
  so with two seed leads the one that is not the team's lead directs by request.

**Changing it.** The new process's missing roles and sections are added as above. No
file, document kind, role or employee changes. The tabs follow the new process, and
agents receive its skills at their next start.

## 4. The project's tabs

A project page is a row of tabs. Some are the same for every project. The rest are
the things its development process says it has.

```
 Overview | Work | Product | Technical | Docs | Releases | Team | Activity | Conversations
           fixed   └── from the process ──┘        fixed for every project
```

**Fixed tabs,** in every project: Overview, Work (all tasks), Docs (the whole
documentation tree), Releases (with their milestones), Team, Activity and
Conversations. No process removes or renames one.

**Process tabs** come from `tabs[]` in the manifest, in the order written, between
Work and Docs. Each names one of the product's generic views and says what it shows:

| View | What the tab shows | What the process sets |
| --- | --- | --- |
| `documents` | The documents, diagrams and images of one section of the tree, with status, version, unissued changes and progress. **New** starts a document there | `section` (one of the process's folders), optional `kinds` |
| `work` | The project's tasks of the given types, as a list or as a board with one column per status. **New** creates a task of the first type | `types` (task types; a process may use its own, such as `story`), `layout: list \| board` |
| `features` | The feature map of the [features UX](../specs/features-ux.md). Listing this view is how a process says it uses features | Nothing |

```yaml
tabs:
  - key: product
    title: Product
    view: documents
    section: product
    kinds: [product-dss]
  - key: technical
    title: Technical
    view: documents
    section: technical
```

**How the tab bar is worked out,** every time the project page is opened:

1. Start with Overview and Work.
2. Add the tabs of the project's current development process, in order.
3. If the project has features and no `features` tab came from step 2, add Features,
   so that nothing a project holds becomes unreachable.
4. Add Docs, Releases, Team, Activity and Conversations.

A project with no development process gets Features in step 2, as today.

- Nothing about tabs is stored on the project. Changing the process changes the tabs
  at once and touches no data; changing back brings them back.
- A tab with nothing in it shows its empty state and its **New** action. It is not
  hidden, because the process says the project has that thing.
- A tab is a view of records that exist anyway. Every document is also in Docs and
  every task is also in Work, whatever the tabs are.
- The address is `/projects/<slug>/<tab key>`. A process tab's key cannot be one of
  the fixed tabs' keys, and a process has at most eight tabs.
- The set of views is closed and belongs to the product. A new kind of view is a
  product change; a new tab is not.
- Tabs are for people. Agents find the same things through the index, task lists and
  their skills.

## 5. Two built-in development processes

| | Desired State Specification (`dss`) | Feature delivery (`feature-delivery`) |
| --- | --- | --- |
| What it is | Specify the product and the system as desired state, then keep the code in line | Software delivery by features: requests are scoped into features, features are built by tasks and shipped in releases |
| The specification is | Versioned Product DSS and Technical DSS documents | Each feature's definition, `feature.md` |
| A task points at | The document versions it realizes | Its feature |
| Tabs it adds | Product, Technical (`documents`) | Features (`features`), Requests and Bugs (`work`, by type), Board (`work`, as a board) |
| Kinds it adds | `product-dss`, `technical-dss` | None; it uses decisions, designs and reports |
| Seed roles | Product lead, technical lead | Product lead, engineering lead |
| Starting sections | `product`, `technical` | `decisions`, `designs`, `reports` |
| Delivered through | A release; milestones optional | A release; milestones optional |

`feature-delivery` covers capturing and scoping a request
([request UX](../specs/feature-requests-ux.md)), writing a feature definition, breaking
a feature into tasks, planning a release and staffing. Its skills are written when it
is built. Iterations, boards and the other habits of an agile team are skills, stages
and tabs of such a process, not parts of the product. Both are content only, which is
the proof of PKG-1.

## 6. How skills reach an agent

An agent working in a project that follows a development process has two separate
sections in its instructions:

1. **Organization processes and standards,** resolved and delivered as the
   [process design](processes.md#3-resolution-and-delivery) says. Unchanged.
2. **The project's development process:** its name and summary, its stages, the
   agent's role in it, and for each of the agent's skills the title and the
   when-to-use line. The body of a skill is retrieved with
   `agent-os skill show <key>`. The section says that where it and an organization
   process or standard disagree, the organization's document is followed.

- **Which skills.** Every skill of the process whose `roles` is empty or names the key
  of the agent's role. An agent with no role, or a role the process does not name,
  receives the skills written for everyone, so a process puts what every member must
  know in those.
- **Size.** The section is at most 8 KiB. If the list of skills does not fit, the ones
  left out are named with how to retrieve them. A skill body is never cut.
- **Fresh contexts** get the section again, like the organization's processes.
- **Not the vendor tool's skills.** An agent's runtime loads no skill folder of its own
  ([agent sessions](agent-sessions.md), Claude's isolation switches). The office
  composes the section for every runtime the same way.
- **Work without a project** has no development process section.
- **`agent-os dev-process show`** prints the project's process, its stages and the
  agent's role; **`agent-os skill list`** and **`skill show`** print its skills.

## 7. Hiring when the development process says so

A skill tells a lead when to staff the project and how to decide which roles it needs.
The lead then uses the product's hiring, as any agent may:

1. **Inspect the role.** `agent-os role list` and `agent-os role show <key>` print the
   role's definition and what a hire proposal needs: the required home team, the
   optional title, focus and reporting line, and the execution profiles available, with
   which are ready on which office.
2. **Ask.** `agent-os hire request --role <key> --team <project team> --focus … --why …`
   sends the request to HR. HR drafts the proposal; when HR cannot run, the command
   says so and a person fills the form
   ([hire flow](../specs/our-org-in-the-app.md), APPR-8).
3. **Approval.** A lead approves. The proposal goes to the nearest lead, going up from
   the project's team, who may approve: a person, or an agent whose May hire
   permission is on; else to the owner. When the requesting lead is itself that lead,
   its request is its approval. A seed lead without the permission waits for the lead
   above it.
4. **Onboarding.** On approval the office provisions the agent in the project's team,
   so it is under that team's lead. The proposal suggests the requesting lead as the
   new agent's reporting line, which the approver confirms or changes. From its first
   start the agent receives the process's skills for its role.

A lead may ask for a role the process does not define; HR drafts it like any hire.

## 8. The project documentation tree

The files, the index format, issued versions, code links and the read and write rule
are in
[document 18](organization-data/18-development-processes-and-project-documentation.md#project-documentation-tree).
What the design adds is how they are used.

- **Finding.** `agent-os doc index` returns the project's index: one office-local read,
  with every section, document and media file, its path, kind, title, status and a
  line on what it is for. It is the first thing a skill tells an agent to read.
  `agent-os doc ls [path]` lists one folder and exists as a fallback;
  `agent-os doc list` filters by kind, status or scope across the project. Both are the
  `doc.list` call.
- **Reading.** `agent-os doc show <id | path>` prints a document. For a versioned
  document it prints the latest issued version and says how many unissued changes
  follow it; `--working` prints the working text and `--version <n>` an earlier one.
  `agent-os doc get <id | path> --to <file>` copies a media file into the agent's
  temporary folder.
- **Writing.** `agent-os doc new --kind … --path … --title … --summary …` starts a
  document from the kind's template. `doc edit`, `doc move`, `doc put <path> <file>`
  for media, and `doc section add | edit | move` change the tree. Each is one accepted
  change that also writes the index. A document is never written into a code worktree.
- **Skills name kinds and sections by what they are for, not by path.** A skill says
  "save it as a Product DSS in the section for its product domain". The agent finds the
  section in the index, or adds it. A project may rearrange its tree without any skill
  changing.
- **Another project.** A call for a project that is not the agent's own is refused with
  `other_project` and the name of that project's lead. The agent asks, with a directed
  ask, and the other project's agent answers from its own index.
- **Limits.** A document is at most 512 KiB, so that a write fits one frame
  ([task design](tasks-projects-docs.md#4-the-contract)); a read pages with an offset.
  A media file is at most 5 MiB. One larger than 512 KiB is first uploaded in numbered
  parts to the office's temporary area (`doc.upload`), and the write that stores it
  names the upload and its SHA-256. Nothing enters the tree before that write is
  accepted, and an upload that no write claims is discarded.
- **Offices.** A project's tree has one writing office at a time, handed over like a
  task's writer. An agent in that office is acknowledged locally. From another office
  a read is local and a write waits for the writer, as a cross-office edit of a task
  does ([document 18](organization-data/18-development-processes-and-project-documentation.md#the-index)).
- **The browser.** The Docs tab shows the whole tree from the index. A process's
  `documents` tabs show one section each. People create, edit, move and issue from
  either with the same rules.

## 9. The contract

Calls go to the caller's own office on a bound connection; every write carries an
operation ID and an expected revision ([task design](tasks-projects-docs.md#4-the-contract)).

| Call | Params | Result | Errors |
| --- | --- | --- | --- |
| `dev_process.list` | `status?` | `{ processes }` | `unavailable` |
| `dev_process.get` | `process_id`, `part?: manifest \| role { key } \| skill { key } \| template { key }` | The part, with the process revision | `not_found` |
| `dev_process.write` | `process_id?`, the part and its content, `expected_revision` | `{ process_id, revision }` | `invalid_request`, `conflict`, `not_authorized` |
| `dev_process.copy` | `process_id`, `key`, `name` | `{ process_id }` | `already_exists`, `not_authorized` |
| `dev_process.import` | An uploaded process file, `key?` | `{ process_id }`, as a draft | `invalid_request`, `already_exists`, `too_large` |
| `dev_process.export` | `process_id` | The process file | `not_found` |
| `project.set_development_process` | `project_id`, `development_process_id?`, `expected_revision` | `{ project, roles_created, roles_matched, sections_added }` | `invalid_request` (`not_current`), `conflict`, `not_authorized` |
| `project.tabs` | `project_id` | `{ tabs: [{ key, title, view, fixed, … }] }`, worked out as in section 4 | `not_found` |
| `project.seed_roster` | `project_id` | `{ rows: [{ role_key, name, hats, state, employee_id?, hire_request_id? }] }` | `not_found` |
| `skill.list`, `skill.get` | `employee_id?`, `key?` | The agent's skills, or one body, with the process and its revision | `not_found`, `not_authorized` |
| `role.list`, `role.get` | `key?` | The role, and for `get` what a hire proposal needs | `not_found` |
| `doc.index` | `project_id?`, `section?` | `{ index, revision, stale_since? }` | `not_authorized` (`other_project`), `unavailable` |
| `doc.list` | `project_id?`, `path?`, `kind?`, `status?`, `scope?` | `{ entries }` | as above |
| `doc.read` | `document_id \| media_id \| path`, `text?: issued \| working \| version { n }`, `offset?` | `{ document, content, unissued_changes, next_offset? }`; for a media file its entry and its bytes | as above, `not_found` |
| `doc.upload` | `upload_id`, `seq`, `bytes`; then `finish { sha256 }` | `{ upload_id, received_bytes }` | `too_large`, `invalid_request`, `not_authorized` |
| `doc.write` | Create or change a document, a section or a media file; move one | `{ id, revision, index_revision }` | `invalid_request`, `conflict`, `too_large`, `not_authorized`, `unavailable` |

`project_id` defaults to the project of the agent's current work. `dev_process.write`,
`copy`, `import` and `project.set_development_process` are people's calls; the others
are open to agents within the rules above. The calls for issuing, differences, code
links and realizing tasks are in the
[desired state design](desired-state-specifications.md#6-the-contract). A hire request
([hire flow](../specs/our-org-in-the-app.md), APPR-8) may name a role key and gains
nothing else. The browser routes are added to the [web API](web-api-v1.md) with each
increment.

## 10. Where the code lives

- **`org-process`:** organization processes, as today, and in a module of its own the
  development process formats and their validation, the role filter for skills, the
  tab bar rule and rendering of the section. Pure.
- **`org-docs`:** tree paths, the index format, composing the index from a tree, and
  the comparison that finds unissued changes. Pure.
- **`org-office`:** the development process projection, import and export,
  `project.set_development_process`, the derived seed roster and tabs, the tree writes
  with the index in one change, the project boundary for agents, and the calls.
- **`agent-os`:** `dev-process`, `skill`, `role`, `doc` and the role on `hire request`.
- **`apps/agentic-org-ui`:** the Development process select and seed roster on a
  project, the catalogue and editor, the three generic tab views, and the Docs tab.

Built-in development processes are files in the product's source, compiled into the
office.

## 11. Acceptance criteria

| ID | Passing evidence |
| --- | --- |
| PKG-1 | No source file outside a built-in development process's own content names a development process, one of its roles, a process-defined document kind or a tab title. The second built-in is added with content only. |
| PKG-2 | A write with an unknown field, a skill for an undeclared role, a kind named like a built-in kind, a repeated folder path, an unknown view or a tab key that is a fixed tab's is refused, naming the problem. |
| PKG-3 | A built-in development process cannot be edited; its copy can, has a new ID and records its origin. |
| PKG-4 | The project form lists every current development process and "None". A draft or deprecated one cannot be chosen. |
| PKG-5 | Choosing one creates the missing roles and adds the declared sections in the same accepted change, and starts no agent. |
| PKG-6 | The seed roster shows each seed role as open, hire requested or filled, derived from the project team's members; an existing owner team fills the roles its members hold. |
| PKG-7 | An agent's instructions carry the development process as its own section, separate from organization processes: the skills for its role and the ones for everyone, with retrievable bodies; another role's skills are not in it. |
| PKG-8 | No skill appears in `process list`, `process why` or the Processes page, and no organization, team or project process document replaces or is replaced by a skill. |
| PKG-9 | With a skill and a non-overridable organization standard that disagree, the standard is delivered in full as must-follow and the development process section says the organization's document is followed. |
| PKG-10 | A lead agent's `hire request` for a role of the process produces a proposal for the project's team; nothing is provisioned before a lead who may approve does: a person, or an agent lead with May hire on, whose own request is then enough (HIRE-1 to HIRE-3); the new agent's first instructions carry its role's skills. |
| PKG-11 | `doc index` returns the whole index in one call while it is at most 256 KiB, and the section list with each section readable by path beyond that. After any accepted write it lists exactly what the tree holds. |
| PKG-12 | An agent of another project is refused `doc index`, `doc ls`, `doc show` and every write, with `other_project` and the lead to ask; a person is not. |
| PKG-13 | Changing a project's development process removes no file, section, document or role; the tabs follow the new process at once; the next start of each agent delivers its skills. |
| PKG-14 | With no development process, a project's tree starts with `decisions`, `designs` and `reports`, and its tabs are the fixed ones and Features. |
| PKG-15 | The tab bar is exactly Overview, Work, the process's tabs in order, Features if the project has features and the process has no `features` tab, then Docs, Releases, Team, Activity, Conversations. Nothing about tabs is stored on the project. |
| PKG-16 | Any person of the organization creates, copies, imports and edits a development process; an agent's write of one is refused. An imported file is validated, gets a new ID and arrives as a draft; a downloaded one imports again unchanged. |
| PKG-17 | Releases and milestones are in every project whatever its development process, and no manifest field hides or removes a fixed tab. |
| PKG-18 | A `documents` tab lists exactly the documents and media of its section, limited to its kinds; a `work` tab lists exactly the tasks of its types, as a list or a board; an empty tab shows its empty state and New. |

## 12. Open points

- **One role per employee.** A person or agent working in two projects with different
  development processes has one role key. Whether a project may map an organization
  role onto a role of its process is not decided.
- **Declared handoffs.** If agents prove unreliable at handing work on, a process
  could declare "when a document of this kind is issued, create this task for that
  role", run by the office. It is not selected: it is the engine decision 2 excludes.
- **More views.** A calendar, a timeline or a table of custom fields would each be a
  new generic view. None is selected.
- **Native presentation of skills** in a vendor tool, where its isolation allows.

## 13. Delivery order

Each step is usable on its own. Increments are in the
[implementation plan](../implementation-plan.md) (L11 to L14).

1. **The tree and the index** for every project, with `agent-os doc` and the Docs tab
   (FC-173). It needs no development process.
2. **Development processes and a project's choice of one:** records, catalogue,
   editor and import, the select, the seed roster, the tabs, skills in an agent's
   instructions, and process and standard writes by agents of the project's team
   (FC-168, FC-169, FC-170, FC-172, FC-177, FC-180). Needs the agent launch path (C3).
3. **Hiring from a skill:** `role show` and the role on `hire request` (FC-171). Needs
   the hire flow (C2).
4. **Issued versions, code links and realizing tasks** (FC-174, FC-175), then **the
   DSS process** (FC-176) ([desired state design](desired-state-specifications.md))
   and **feature delivery** (FC-178).

## What the code does today

Nothing of this exists. `crates/org-process` parses an earlier organization process
format and has no caller. `crates/org-docs` parses and edits Markdown front matter and
knows no tree, index or kind. The hire code is a direct profile write that the HR flow
replaces (FC-033). `agent-os` has no `doc`, `role`, `skill` or `process` command. The
UI's project page has fixed tabs and no Development process select.
