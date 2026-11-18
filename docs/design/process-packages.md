# Design: process packages and project documentation

How a project is given a way of working without the product knowing any methodology:
a process package, the roles it starts a project with, how its guidance reaches each
agent, how agents hire when the package says it is time, and where a project's
documents live. It implements the owner's
[REQ-07](../specs/req-07-composable-process-packages-and-team-blueprints.md). The
records are owned by
[document 18](organization-data/18-process-packages-and-project-documentation.md);
[projects](organization-data/02-project-structure.md),
[roles](organization-data/01-organization-structure.md) and
[employees](organization-data/04-agent-structure.md) own what it builds on. Resolution
and delivery of guidance are owned by the [process design](processes.md). The decision
and what was rejected are in [ADR-0021](../adr/0021-process-packages-are-content.md). The first
package, for desired state specifications, is in the
[desired state design](desired-state-specifications.md).

## Three words

- A **process package** is a bundle that describes one way of working: roles, skills,
  document kinds and a documentation layout. A project follows at most one. In a
  project's settings it is shown as the project's **Process**.
- A **skill** is one piece of a package's guidance, written for one or more roles.
- A **process document** and a **standard** are what the [process design](processes.md)
  already defines: guidance written at the organization, a team or a project.

All three are text that an agent reads and applies. None of them is code the product
runs.

## Decisions

1. **The product holds primitives; a package holds the method.** Projects, releases,
   milestones, tasks, documents, roles, hiring, messages and asks are the product's.
   What to do with them, in what order and by whom is a package's content. No package
   name, role name or document kind appears in product code, and adding a methodology
   adds no code.
2. **No engine.** A package's stages describe the order of work. Nothing in the
   product reads a stage, starts work because a document changed, or blocks an action.
   An agent acts because a skill told it to and it has a turn
   ([process design](processes.md), decision 7).
3. **A package is content in organization Git,** written through the product, with a
   stable ID, a key, a revision, a lifecycle and an owner. Packages that ship with the
   product are built in and are changed by copying them.
4. **A package grants nothing.** It names no tool, model, credential, budget or
   approver. Authority still comes from the caller's place in the organization, and
   every call is still authorized by the office.
5. **Skills are a layer of the existing resolution.** The guidance for a piece of work
   is the organization's documents, then the accountable team's chain, then the
   project's package, then the project's own documents. A skill is overridable
   guidance, never must-follow text, and it cannot displace a document the organization
   or a team marked not overridable.
6. **Skills are bound to roles by key.** An agent receives the skills written for its
   role and the ones written for everyone. An employee still has one role.
7. **A project starts lean.** A package marks a few roles as its seed. Choosing the
   package shows them; a person fills each by a hire or with an employee who already
   has the role. Everyone else is hired when a skill says the work calls for it.
8. **Hiring stays the product's.** An agent asks for a hire; HR drafts it; a person
   approves it; product code provisions it
   ([document 04](organization-data/04-agent-structure.md#the-hr-lead-and-how-agents-are-onboarded)).
   A package decides when a lead asks and for which role, never whether the hire happens.
9. **A project's documents live in one tree whose layout the project owns.** The
   package declares the sections it starts with; agents and people grow it. The product
   has no document type for any methodology: a tree holds Markdown documents, diagrams
   and images.
10. **One index, kept by the office from what agents do.** Every project has
    `docs/index.yaml`, the table of contents of its tree. One read answers where
    anything is. Agents shape it through their writes; the office writes the file in
    the same accepted change.
11. **A project's documentation is its own.** An agent reads and writes the tree of its
    own projects only. What it needs from another project it asks that project's
    agents and people for.
12. **Changing a project's package removes nothing.**

## 1. Where each thing lives

| Thing | Source |
| --- | --- |
| Organization package | Organization Git `process-packages/<package-id>/` |
| Built-in package | The running product; projected with its fixed ID and no file |
| A project's package | `process_package_id` in `projects/<project-id>/project.yaml` |
| Role an employee holds | Organization role record, named by the employee's `role_id` |
| Project documents, diagrams, images | Organization Git `projects/<project-id>/docs/` |
| The index | `projects/<project-id>/docs/index.yaml` |
| Issued versions | `projects/<project-id>/docs-history/<document-id>/<version>.md` |
| Seed roster, effective skills | Computed; never authored |

## 2. The package

The manifest, the role, skill and template files, and their rules are in
[document 18](organization-data/18-process-packages-and-project-documentation.md#process-packages).
In short:

```text
process-packages/<package-id>/
├── package.yaml      # name, summary, roles (some seed), kinds, folders, stages
├── roles/<key>.md    # what each role is for
├── skills/<key>.md   # what a role does and how; front matter says which roles
└── templates/<key>.md
```

- **Catalogue.** The package list shows every `current` package, built-in ones first,
  each with its summary, its seed roles and the sections it starts a project with.
  A draft is visible to people and cannot be chosen for a project.
- **Authoring.** A person creates a package, or copies one, and edits its manifest,
  roles, skills and templates in the product. A copy has a new ID and remembers what it
  was copied from. Every write names the expected revision and is attributed.
- **Validation at every write.** Unknown manifest fields are refused, naming the field,
  so that a package cannot smuggle a setting. A skill names only roles of its own
  package. A kind is not named like a built-in kind. Folder paths are valid and do not
  repeat. A `current` package has an owner, at least one role and at least one skill.
- **A package in use.** A package that a project follows cannot be deleted. It can be
  deprecated: its projects keep working and a notice on each says so.
- **Built in.** Built-in packages are part of the product and change with it. They are
  listed with the product's version and cannot be edited. The first is `dss`
  ([desired state design](desired-state-specifications.md)).
- **Moving a package between organizations** is an archive of the package folder. It
  is the last increment (section 11).

## 3. A project's process

**Choosing.** The project form has a **Process** select listing the catalogue and
"No process". It can be set when the project is created and changed later, by the
organization's owner or a person in the lead chain of the project's owning team.

**What choosing does,** in one accepted change at HQ:

1. Sets `process_package_id` on the project.
2. Creates an organization role, from the package's definition, for every package role
   whose key the organization does not have yet. Existing roles are not touched.
3. Adds to the project's index every section the package declares and the project
   lacks. This is the eager layout: the sections are there, with their titles and
   summaries, before any document exists.

Nothing else happens. No agent is hired, started or told anything by the choice itself.

**The seed roster.** The project's page then shows one row per seed role:

```
 Process · Desired State Specification                                   [Change…]
 ───────────────────────────────────────────────────────────────────────────────────
 Seed team
 Product Owner / Principal Designer      Open                      [Hire]  [Assign…]
 System Architect / Senior Staff Engineer  Hire requested · waiting for you
```

- A seed role is **filled** when an active employee whose home team is the project's
  owning team, or a team beneath it, has that role. The roster is derived; nothing
  stores it.
- **Hire** opens the [hire flow](../specs/our-org-in-the-app.md) with the role and the
  project's team preset. Approval and provisioning are unchanged.
- **Assign…** lists active employees who have the role, or lets a person give the role
  to one; either way the employee is moved to the project's team by the ordinary move.
  Creating the project with an existing team as its owner fills at once every seed role
  that team's members already hold.
- An open seed role blocks nothing. The project says "2 seed roles open" until they are
  filled.

**Changing the package.** The new package's missing roles and sections are added as
above. No file, document kind, role or employee changes. Agents receive the new
package's skills at their next start.

## 4. How skills reach an agent

A project's package adds one layer to the resolution of the
[process design](processes.md#3-resolution-and-delivery):

```
organization  →  accountable team's chain  →  project's package  →  project
```

- **Which skills.** For an agent working in the project: every skill of the package
  whose `roles` is empty or names the key of the agent's role. An agent with no role,
  or a role the package does not name, receives the skills written for everyone.
- **Keys.** A skill takes part under its key like any document. A project document with
  the same key replaces it for that project, which is how one project adjusts one
  skill. A skill replaces an organization or team document with the same key only if
  that document is overridable; otherwise the skill is left out with a visible problem
  naming the level that forbids it.
- **Delivery.** Skills are overridable guidance: the agent's instructions carry each
  skill's title, source ("Desired State Specification, rev 4") and when-to-use line,
  and the body is retrieved with `agent-os process show <key>`. The cap, the order and
  the rules for fresh contexts are those of the process design. The instructions also
  carry a short block naming the project's process, its stages and the agent's role in
  it.
- **Not the vendor tool's skills.** An agent's runtime loads no skill folder of its own
  ([agent sessions](agent-sessions.md), Claude's isolation switches). The office composes the
  guidance for every runtime the same way. A runtime adapter may later also present
  them in that tool's native form; the package stays the one source.
- **Projectless work** has no package layer.
- **`agent-os process list`, `show` and `why`** include the package layer and name it.

The fingerprint under which effective sets are cached gains the package's ID and
revision and the agent's role key.

## 5. Hiring when the package says so

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
3. **Approval.** The proposal goes to the nearest person in the lead chain of the
   project's team, else to the owner. An agent never approves a hire.
4. **Onboarding.** On approval the office provisions the agent in the project's team.
   From its first start it receives the package's skills for its role. The requesting
   lead is told when the hire completes or is rejected.

A lead may ask for a role the package does not define; HR drafts it like any hire.

## 6. The project documentation tree

The files, the index format, issued versions, code links and the read and write rule
are in
[document 18](organization-data/18-process-packages-and-project-documentation.md#project-documentation-tree).
What the design adds is how they are used.

- **Finding.** `agent-os doc index` returns the project's index: one office-local read,
  with every section, document and media file, its path, kind, title, status and a
  line on what it is for. It is the first thing a skill tells an agent to read.
  `agent-os doc ls [path]` lists a folder and exists as a fallback.
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
- **Limits.** A document body is at most 8 MiB and a media file 5 MiB. A media file
  larger than one frame is sent in parts under one operation ID
  ([task design](tasks-projects-docs.md#4-the-contract)).
- **The browser.** A project's Docs tab shows the tree from the index: sections,
  documents and media, with status, version and unissued changes. People create, edit,
  move and issue from there with the same rules.

## 7. The contract

Calls go to the caller's own office on a bound connection; every write carries an
operation ID and an expected revision ([task design](tasks-projects-docs.md#4-the-contract)).

| Call | Params | Result | Errors |
| --- | --- | --- | --- |
| `package.list` | `status?` | `{ packages }` | `unavailable` |
| `package.get` | `package_id`, `part?: manifest \| role { key } \| skill { key } \| template { key }` | The part, with the package revision | `not_found` |
| `package.write` | `package_id?`, the part and its content, `expected_revision` | `{ package_id, revision }` | `invalid_request`, `conflict`, `not_authorized` |
| `package.copy` | `package_id`, `key`, `name` | `{ package_id }` | `already_exists`, `not_authorized` |
| `project.set_process` | `project_id`, `process_package_id?`, `expected_revision` | `{ project, roles_created, sections_added }` | `invalid_request` (`not_current`), `conflict`, `not_authorized` |
| `project.seed_roster` | `project_id` | `{ rows: [{ role_key, name, hats, state, employee_id?, hire_request_id? }] }` | `not_found` |
| `role.list`, `role.get` | `key?` | The role, and for `get` what a hire proposal needs | `not_found` |
| `doc.index` | `project_id?`, `section?` | `{ index, revision, stale_since? }` | `not_authorized` (`other_project`), `unavailable` |
| `doc.list` | `project_id?`, `path?` | `{ entries }` | as above |
| `doc.read` | `document_id \| path`, `text?: issued \| working \| version { n }`, `offset?` | `{ document, content, unissued_changes, next_offset? }` | as above, `not_found` |
| `doc.write` | Create or change a document, a section or a media file; move one | `{ id, revision, index_revision }` | `invalid_request`, `conflict`, `too_large`, `not_authorized`, `unavailable` |

`project_id` defaults to the project of the agent's current work. `package.write`,
`package.copy` and `project.set_process` are people's calls; the others are open to
agents within the rules above. The calls for issuing, differences, code links and
realizing tasks are in the [desired state design](desired-state-specifications.md#6-the-contract).
A hire request ([hire flow](../specs/our-org-in-the-app.md), APPR-8) may name a role
key and gains nothing else. The browser routes are added to the [web API](web-api-v1.md) with each increment.

`process.effective`, `process.why` and `process.show` of the
[process design](processes.md#6-the-contract) return package skills with
`layer: package`.

## 8. Where the code lives

- **`org-process`:** the manifest and skill formats and their validation; the package
  layer and the role filter in `resolve`; rendering. Pure.
- **`org-docs`:** tree paths, the index format, composing the index from a tree, and
  the comparison that finds unissued changes. Pure.
- **`org-office`:** the package projection, `project.set_process`, the derived seed
  roster, the tree writes with the index in one change, the project boundary for
  agents, and the calls.
- **`agent-os`:** `role`, `doc` and the extended `process` and `hire request` commands.
- **`apps/agentic-org-ui`:** the Process select and seed roster on a project, the
  package catalogue and editor, and the Docs tab.

Built-in packages are files in the product's source, compiled into the office.

## 9. Acceptance criteria

| ID | Passing evidence |
| --- | --- |
| PKG-1 | No source file outside a built-in package's own content names a package, a package role or a package-defined document kind. A second built-in package is added with content only. |
| PKG-2 | A package write with an unknown field, a skill for an undeclared role, a kind named like a built-in kind or a repeated folder path is refused, naming the problem. |
| PKG-3 | A built-in package cannot be edited; its copy can, has a new ID and records its origin. |
| PKG-4 | The project form lists every current package and "No process". A draft or deprecated package cannot be chosen. |
| PKG-5 | Choosing a package creates the missing roles and adds the declared sections in the same accepted change, and starts no agent. |
| PKG-6 | The seed roster shows each seed role as open, hire requested or filled, derived from the project team's members; an existing owner team fills the roles its members hold. |
| PKG-7 | An agent receives the skills for its role and the ones for everyone, named with the package and its revision, as summaries with retrievable bodies; another role's skills are not in its set. |
| PKG-8 | A project document with a skill's key replaces that skill in that project only; `process why` shows the package layer and the replacement. |
| PKG-9 | A skill whose key the organization marked not overridable is left out, with a problem naming the organization's document; that document is still delivered in full. |
| PKG-10 | A lead agent's `hire request` for a package role produces a proposal for the project's team; nothing is provisioned before a person approves; the new agent's first instructions carry its role's skills. |
| PKG-11 | `doc index` returns the whole tree in one call. After any accepted write it lists exactly what the tree holds. |
| PKG-12 | An agent of another project is refused `doc index`, `doc ls`, `doc show` and every write, with `other_project` and the lead to ask; a person is not. |
| PKG-13 | Changing a project's package removes no file, section, document or role, and the next start of each agent delivers the new package's skills. |
| PKG-14 | With no package, a project's tree starts with `decisions`, `designs` and `reports`, and resolution is exactly the process design's. |

## 10. Open points

- **Agents starting other agents.** A lead that is asked a question, or an engineer
  that is assigned a task, acts only when it has a turn, and today only a person's Run
  or the answer to a required ask starts one
  ([run modes](../specs/agent-run-modes-ux.md), RUN-3). This is with the owner
  ([REQ-07](../specs/req-07-composable-process-packages-and-team-blueprints.md#how-this-fits-the-selected-architecture),
  point 1). Nothing in this design changes that rule.
- **One role per employee.** A person or agent working in two projects with different
  packages has one role key. Whether a project may map an organization role onto a
  package role is not decided.
- **Declared handoffs.** If agents prove unreliable at handing work on, a package could
  declare "when a document of this kind is issued, create this task for that role", run
  by the office. It is not selected: it is the trigger engine decision 2 excludes.
- **Native presentation of skills** in a vendor tool, where its isolation allows.

## 11. Delivery order

Each step is usable on its own. Increments are in the
[implementation plan](../implementation-plan.md) (L11 to L14).

1. **The tree and the index** for every project, with `agent-os doc` and the Docs tab
   (FC-173). It needs no package.
2. **Packages and a project's process:** records, catalogue, select, seed roster, the
   skill layer in resolution (FC-168, FC-169, FC-170). Needs the agent launch path
   and the one-chain resolution (C3, C5).
3. **Hiring from a skill:** `role show` and the role on `hire request` (FC-171). Needs
   the hire flow (C2).
4. **The DSS package** ([desired state design](desired-state-specifications.md)).
5. **Moving a package between organizations** (FC-172).

## What the code does today

Nothing of this exists. `crates/org-process` parses an earlier process document format
and has no caller. `crates/org-docs` parses and edits Markdown front matter and knows
no tree, index or kind. The hire code is a direct profile write that the HR flow
replaces (FC-033). `agent-os` has no `doc`, `role` or `process` command. The UI's
project form has no Process select.
