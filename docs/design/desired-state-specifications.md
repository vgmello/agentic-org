# Design: desired state specifications (the DSS package)

How a person specifies software by talking to an agent, and how the specification
stays the thing the code is held to. It implements the owner's
[REQ-06](../specs/req-06-desired-state-spec-authoring-and-reconciliation.md) as a
[process package](process-packages.md): the built-in package `dss`. The general
records it uses (the documentation tree, issued versions, code links and realizing
tasks) are owned by
[document 18](organization-data/18-process-packages-and-project-documentation.md) and
are available to every package.

## In one paragraph

A project that follows the `dss` package starts with two agents: a product lead and a
technical lead. The person describes what they want in chat. The product lead writes
it down as Product DSS documents, organized by product domain, and issues a version
when the person agrees. The technical lead derives the Technical DSS from it: from the
standards and the specification that already exist where they answer the question,
and by asking the person where they do not. With both issued, the leads plan the work
as tasks that name the versions they realize, hire the specialists the plan needs, and
answer the engineers' questions while they build. When a specification changes, a new
version is issued and the same steps bring the code back in line with it. A version is
converged when the tasks that realize it are finished.

## Decisions

1. **DSS is a package, not a feature of the product.** Everything specific to it is in
   the package's manifest, roles, skills and templates. The product parts it relies on
   are general: versioned documents, code links, tasks that realize a version, asks,
   hiring.
2. **The desired state is the latest issued version of a document.** A document is
   edited freely; issuing fixes a text that work is done against. Nobody builds from
   unissued changes.
3. **The Product DSS is the root.** Every Technical DSS version names the Product DSS
   versions it was derived from, every task names the versions it realizes, and code
   links tie documents to code. From any of them the product domain is one step away.
4. **Standards first, then the person.** The technical lead answers a technical
   question from the standards that apply and the existing Technical DSS before asking
   anyone. It asks the person only what neither answers, and writes the answer down so
   that it is not asked again.
5. **Standards grow from decisions.** An answer that will be needed again becomes a
   project standard, an ordinary record of the [process design](processes.md). It is
   never a precondition for starting.
6. **Staffing follows the specification.** The two leads are the whole team until both
   layers are issued. Only then do they ask for specialists.
7. **Questions about the architecture go to the technical lead,** not to the person.
8. **Convergence is derived.** A version is converged when the tasks that realize it
   are finished. No one declares it.
9. **The leads act because their skills tell them to.** The product starts nothing when
   a version is issued ([process package design](process-packages.md), decision 2).

## 1. The package

```yaml
key: dss
name: Desired State Specification
roles:
  - key: product-lead
    name: Product Owner / Principal Designer
    hats: [product-owner, principal-designer]
    seed: true
  - key: technical-lead
    name: System Architect / Senior Staff Engineer
    hats: [system-architect, staff-engineer]
    seed: true
  - key: backend-engineer
    name: REST Backend Engineer
    seed: false
  - key: frontend-engineer
    name: Frontend Engineer
    seed: false
kinds:
  - { key: product-dss, title: Product DSS, versioned: true }
  - { key: technical-dss, title: Technical DSS, versioned: true }
folders:
  - { path: product, title: Product documentation, kinds: [product-dss] }
  - { path: technical, title: Technical documentation, kinds: [technical-dss] }
stages:
  - { key: specify, title: Specify the product, lead_role: product-lead }
  - { key: derive, title: Derive the technical specification, lead_role: technical-lead }
  - { key: plan, title: Plan and staff, lead_role: technical-lead }
  - { key: build, title: Build and answer questions }
  - { key: reconcile, title: Keep the code in line with the specification }
```

| Skill | For | What it tells the agent |
| --- | --- | --- |
| `how-dss-works` | Everyone | The two layers, what an issued version is, where documents live (read the index first), and whom to ask what. |
| `write-product-dss` | Product lead | Talk with the person until the behaviour is clear; write it under its product domain; confirm; issue. |
| `derive-technical-dss` | Technical lead | From an issued Product DSS version, update the Technical DSS: standards first, then the person. |
| `record-a-standard` | Technical lead | When a decision will be needed again, write or update a project standard. |
| `plan-and-staff` | Both leads | Break both layers into tasks; put them in a release, with milestones if useful; decide the roles needed; ask HR. |
| `answer-architecture-questions` | Technical lead | Answer engineers from the Technical DSS and standards; change the specification when the answer changes it. |
| `build-from-the-specification` | Everyone | Read the versions a task realizes; ask the technical lead; keep code links current. Written for everyone so that an engineer hired with any role receives it. |
| `reconcile-a-new-version` | Both leads | When a version is issued over an earlier one, find what it changes and plan the work. |

The two specialist roles are examples; a lead may ask HR for any role. The manifest
above is shortened: stage summaries are left out.

## 2. The two layers

**Product DSS.** One document per product area, in a section per product domain:
`product/identity-and-access/user-authentication.md` holds login flows and password
expiry rules. The template's sections are Purpose, Behaviour, Rules, Out of scope and
Open questions. The product lead adds a section for a new domain when the first
document for it is written. A person finds an area by its domain in the Docs tab; an
agent finds it in the index.

**Technical DSS.** One document per part of the system the specification governs,
under `technical/`: architecture, protocols, data schemas and their migrations,
external providers. The template's sections are Scope, Design, Interfaces, Data,
Decisions and Open questions. Diagrams and images sit beside it. The "system-wide
change on record" for a product change is the set of Technical DSS versions issued
because of it, each naming the Product DSS version it follows.

Both are ordinary tree documents with `versioned: true`. The product knows neither
name.

The package works with releases and milestones, as every process does, and does not
use feature records: the Product DSS is the definition, and a
task points at the versions it realizes. Software delivery by features is another
package ([process package design](process-packages.md#3a-features-and-a-second-package)).

## 3. The flow

### 3.1 Specify

1. The person opens a chat with the product lead and describes what they want, or a
   change to what exists.
2. The product lead reads the index and the documents the request touches, and asks
   what is unclear: choices and yes/no questions as structured asks
   ([REQ-04](../specs/req-04-channel-messaging-and-question-components.md)), the rest
   as conversation.
3. It writes or edits the Product DSS documents. Edits are unissued changes; nothing
   downstream reads them.
4. When the text is complete it asks the person to confirm, with a required yes/no ask
   that links the documents. On yes it issues each changed document with a note saying
   what changed.

### 3.2 Derive

5. The product lead hands over: it creates a task for the technical lead, "Derive the
   Technical DSS", naming the Product DSS versions just issued.
6. The technical lead reads those versions and what changed since the versions the
   Technical DSS was last derived from (`agent-os doc diff`). For each technical
   question the change raises it looks, in order, at the standards that apply
   (`agent-os process list --kind standard`), then at the existing Technical DSS.
   - **Answered there:** it applies the answer and records where it came from in the
     document's Decisions section. The person is not asked. This is the autonomous
     mode.
   - **Not answered:** it asks the person, marking the ask required when it cannot
     continue without it ([ADR-0020](../adr/0020-required-asks-suspend-and-resume.md)).
     The answer goes into the Decisions section with who decided and when. This is the
     interactive mode. A person can also be asked by the product lead to join when the
     question is about behaviour rather than technology.
7. Where a decision will apply beyond this document, the lead writes or updates a
   project standard (section 4).
8. It issues the changed Technical DSS documents, each with `derived_from` naming the
   Product DSS versions.

### 3.3 Plan and staff

9. The leads break the work into tasks. Each task's `realizes` names the versions,
   product or technical or both, that it brings the code in line with. Every change goes
   into a release: the leads put the tasks in the project's next release, creating it
   if there is none, and may group them in milestones of that release. Milestones are
   optional; the release is not (owner, 2026-11-18). A task says which code links it is
   expected to touch.
10. They decide which roles the tasks need. For each, the technical lead reads the role
    (`agent-os role show`) and asks HR for the hire with a reason
    ([process package design](process-packages.md#5-hiring-when-the-package-says-so)).
    A lead approves: a person, or the technical lead itself when it leads the
    project's team and a person has turned its May hire permission on
    ([ADR-0022](../adr/0022-a-lead-approves-a-hire.md)). Tasks are assigned as the
    hires complete.

### 3.4 Build

11. An engineer reads the versions its task realizes and the code links, and works in
    its task's worktree.
12. When the specification does not answer a question, the engineer asks the technical
    lead with a directed ask. The lead answers in its own session, which holds the
    context in which it wrote the specification. If the answer changes the
    specification, the lead edits and issues it; an answer is never the only place a
    decision lives.
13. Before closing the task the engineer adds or corrects the code links of the
    documents it realized.

### 3.5 Reconcile

14. To change behaviour the person talks to the product lead again. Steps 1 to 8 repeat
    and produce new versions.
15. For each new version the leads read the difference from the version before it and
    the document's code links, check which links are stale, and create the tasks that
    realize the new version. A version that needs no code change gets one task closed
    with that resolution, so that the record says someone looked.
16. The Docs tab and `agent-os doc status` show, per document, the latest version, how
    many tasks realize it and how many are finished. When all are, the version is
    converged. An earlier version that never converged is shown as superseded. A
    version whose tasks were all cancelled is converged by the rule, and its page says
    that they were cancelled.

## 4. Standards as a byproduct

An architecture standard is a `kind: standard` document at the project's scope,
`projects/<project-id>/standards/<document-id>.md`
([process design](processes.md)). It is versioned as every record is: each change
advances its revision, is attributed and is kept in organization Git. It reaches every
agent that works on the project as guidance, and the technical lead reads it first the
next time the same question comes up.

- The technical lead writes project standards with `agent-os process new` and
  `process edit`, which the process design opens to agents of the project's own team
  ([process design](processes.md), decision 8).
- An agent adds standards and changes the project's own. It cannot replace one
  inherited from the organization, a team or the package, cannot mark a document not
  overridable and cannot write at team or organization scope. A standard that should bind more than one project is proposed to
  a person, who writes it at the level it belongs to.
- A standard the organization or a team marked not overridable is a rule the Technical
  DSS must satisfy. The lead cites it and does not ask the person to decide against it.

## 5. What people see

- **Chat** with the product lead and, when a technical decision is needed, the
  technical lead. Questions arrive as cards.
- **The project's Docs tab:** the product and technical sections, each document with
  its latest version, "2 unissued changes" when it has them, and its convergence:
  "v3 · 4 of 5 tasks done".
- **A document:** the issued text by default, the working text with the differences
  marked, the list of versions with who issued each and what it was derived from, the
  tasks that realize each version, and the code links with any that are stale.
- **A task:** the versions it realizes, each a link to that text.
- **Needs you:** a confirmation before a version is issued, a technical decision the
  lead cannot take from the standards, and a hire to approve when the project's lead
  is a person or an agent without May hire. Nothing else asks for the person.

## 6. The contract

These calls are general and belong to the documentation tree
([process package design](process-packages.md#7-the-contract)).

| Call | Params | Result | Errors |
| --- | --- | --- | --- |
| `doc.issue` | `document_id`, `expected_revision`, `note?`, `derived_from?: [{ document_id, version }]` | `{ version, issued_at }` | `invalid_request` (`not_versioned`, `no_changes`, `no_owner`), `conflict`, `not_authorized` |
| `doc.diff` | `document_id`, `from: version \| working`, `to: version \| working` | `{ title_changed, hunks }` | `not_found`, `not_authorized` |
| `doc.versions` | `document_id` | `{ versions: [{ version, issued_at, issued_by, note, derived_from, task_count, done_task_count, is_converged }] }` | as above |
| `doc.link_code` | `document_id`, add or remove `{ repository_id, path, symbol?, note? }`, `expected_revision` | `{ revision }` | `invalid_request` (`not_project_repository`), `conflict` |
| `doc.code_links` | `document_id?` or `repository_id` with `path`; `check?` | `{ links: [{ …, stale? }] }` | `unavailable` when the repository cache cannot be read |
| `doc.status` | `project_id?` | Per versioned document: latest version, unissued changes, progress | `not_authorized` |

- **Issuing** is refused for a document whose kind is not versioned, whose text equals
  its latest version, or that has no owner. Any employee who may write the document may
  issue it; the result is attributed to them.
- **`realizes`** is a field of the task and is set through the task calls
  ([task design](tasks-projects-docs.md)). The service checks that each entry is an
  issued version of a document of the task's project.
- **`agent-os`:** `doc issue`, `doc diff`, `doc versions`, `doc code-link add | remove`,
  `doc code-links [--path <file>] [--check]`, `doc status`, and `task create | edit
  --realizes <document>@<version>`. `doc link` stays the command for links between
  records ([knowledge UX](../specs/decisions-and-tech-debt-ux.md), AUTH-5).

## 7. Acceptance criteria

| ID | Passing evidence |
| --- | --- |
| DSS-1 | Choosing `dss` for a project shows two seed roles and adds the product and technical sections; nothing else is created. |
| DSS-2 | A Product DSS document is created from its template under a product domain section and appears in the index with its summary. |
| DSS-3 | Edits to an issued document do not change what `doc show` returns by default; issuing does, and adds one version with its note and issuer. |
| DSS-4 | A Technical DSS version records the Product DSS versions it was derived from, and a document's page shows them in both directions. |
| DSS-5 | In a project whose standards answer every question a change raises, the technical lead issues the Technical DSS without sending the person an ask. |
| DSS-6 | In a project with no standards, the technical lead asks; the answer appears in the document's Decisions section with who decided; a standard written from it is delivered to the next agent started on the project. |
| DSS-7 | A task shows the versions it realizes; a version shows its tasks and is converged only when all are finished. |
| DSS-8 | Issuing a new version of a document that has code links lists those links, with stale ones marked, to the lead planning the change. |
| DSS-9 | An engineer's question to the technical lead is answered in the lead's existing session, and an answer that changes the specification results in a new issued version. |
| DSS-10 | No specialist is hired before both layers have an issued version; each hire has the approval of a lead who may approve. |
| DSS-11 | The whole flow runs with no code in the product that names DSS, a DSS role or a DSS kind (PKG-1). |
| DSS-12 | The tasks that realize an issued version are all in a release of the project, with or without a milestone, and the version's page names the release; a realizing task in no release is marked. |

DSS-5, DSS-6, DSS-9 and DSS-10 are checked by running the package with real agents:
they test the skills' text as much as the product.

## 8. Open points

- **Who starts the next agent.** Steps 5, 10 and 12 put work in front of another agent.
  Today that agent acts when a person presses Run or, for an agent suspended on a
  required ask, when its answer arrives. Whether an ask or an assignment starts an
  agent is with the owner
  ([REQ-07](../specs/req-07-composable-process-packages-and-team-blueprints.md#how-this-fits-the-selected-architecture),
  point 1).
- **Cost.** A factory spends without a person watching each step. Task and agent
  budgets ([document 17](organization-data/17-budgets-and-controls.md)) are the limit;
  the package sets none.
- **Granularity of a Product DSS document.** One document per product area is the
  package's guidance. Whether very large areas need issuing by section is not decided.
- **Reading code to propose links.** An engineer keeps links current by hand. Deriving
  candidate links from a task's integrated commits is possible and not selected.

## What the code does today

Nothing of this exists. It needs the documentation tree and issued versions, process
packages, the hire flow and agent sessions, none of which is built
([process package design](process-packages.md#what-the-code-does-today)), and asks,
which exist in part (FC-079).
