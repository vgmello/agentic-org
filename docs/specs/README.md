# Project specifications and product knowledge

This directory brings the project requirements and design knowledge into the product repository.
Start with the product requirements and UX specs below; supporting research and historical records
are preserved under the original authors’ names.

## Product requirements

Emery’s PRDs retain their original decisions, acceptance-criterion IDs and release boundaries.
Importing them does not move deferred features into v1.0. In particular, private notes remain
deferred as recorded in [Tasks §4.5](tasks-v1.md#45-brief-comments-and-private-notes).

- [Acceptance tests: v1 PRD set](acceptance-tests-v1-prd-set.md)
- [Away digest and time zones: "since you were away", in your own time](away-digest-and-timezones-v1.md)
- [Routines and retros: set it once, and the org keeps its own rhythm](ceremonies-and-retros-v1.md)
- [Costs and budgets: what the work costs, per agent, task, release and project](costs-and-budgets-v1.md)
- [Proposed features and the project docs model](feature-proposals-and-docs-model.md)
- [IA v2 backlog triage (for ada, 01M7880N)](ia-v2-backlog-triage.md)
- [IA v2 · Stories for ada](ia-v2-stories.md)
- [Information architecture v2: organize the app like an organization](information-architecture-v2.md)
- [Messaging: asks, hand-offs, the System sender, and chat performance](messaging-asks-v1.md)
- [Product vision and UX lens (v1.0)](product-vision-and-ux-lens.md)
- [Project insights: progress, speed and burn-up](project-insights-v1.md)
- [Planning: keys, releases, milestones and the backlog](projects-planning-v1.md)
- [Tasks: follow what you asked for, and work agents can run on](tasks-v1.md)

## UX specifications and design system

The existing product UX specs remain in place. Their newer revisions take precedence over
earlier copies preserved in [Eden’s source records](eden/README.md).

- [Design system](design-system.md): Eden’s shared visual language, components and interaction rules.
- [Models policy UX](models-ux.md): previously missing from the product specs.
- [Agent run modes ux](agent-run-modes-ux.md)
- [App shell ux](app-shell-ux.md)
- [Chat ux](chat-ux.md)
- [Costs and insights ux](costs-and-insights-ux.md)
- [Decisions and tech debt ux](decisions-and-tech-debt-ux.md)
- [Dogfood run](dogfood-run.md)
- [Employee supervision ux](employee-supervision-ux.md)
- [Feature requests ux](feature-requests-ux.md)
- [Features ux](features-ux.md)
- [Offices and teams ux](offices-and-teams-ux.md)
- [Our org in the app](our-org-in-the-app.md)
- [Processes ux](processes-ux.md)
- [Project status ux](project-status-ux.md)
- [Retrospectives ux](retrospectives-ux.md)
- [Roles ux](roles-ux.md)
- [Server installation and access ux](server-installation-and-access-ux.md)
- [Work items ux](work-items-ux.md)

## Supporting knowledge and historical records

- [Eden: research, reviews, validation reports, visual references and records](eden/README.md).
- [Emery: research, prototypes, product decisions and records](emery/README.md).
- [Architecture decisions](../adr/) and [technical designs](../design/) remain the product’s architecture references.
- [Architecture rules](../architecture-rules.md) remain unchanged.

The imported historical desktop spec, older UX revisions, journals, memory, backlogs, drafts,
message drafts, patches and retrospective records are evidence of decisions at their recorded
dates. They do not override newer product specs or current repository instructions. References
to other agents’ documents remain references to the source repository; this migration does not
import those agents’ spaces.

## Migration and provenance

The migration started from the freshly fetched `origin/v1` at `700d5a378b09568da8084982d461a42a4cb7398f`.
All source files remain available in the agents repository. The versioned copies in this
repository are now the project documentation; future product-documentation changes belong here.
The original filenames, requirement IDs and document bodies are retained, with historical
notices on earlier Eden specs and relocated Markdown links where necessary.

[The migration manifest](migration-manifest.json) records every imported source and destination,
their SHA-256 fingerprints, link changes and notices. It also accounts for excluded files:
agent role/runtime configuration, empty placeholders, and generated scratch validation output
(logs, bulk screenshots, execution scripts and git bookkeeping). Authored scratch documents,
draft messages, HTML prototypes and patches are included. Published screen images and their
catalog are included; raw bulk validation output remains in the original workspace.

Existing product docs and ADRs were preserved. Neither source space contains a separate ADR
directory; their decision records remain in the imported requirements, research and history.
