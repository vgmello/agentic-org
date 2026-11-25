# Contract: v1 UI · Processes (organization processes, standards, development processes)

Branch `v1-ui-processes`, from `v1-ui`. Plan row: `docs/plans/v1-ui-screens.md` Step 3,
"Processes: Standards tab, editor, development processes" (`/settings/processes`).

## Outcome

A person opens Organization settings › Processes and sees, for the organization or any team,
project or agent, which organization processes and standards apply, where each comes from and
why. They write new ones, override inherited ones for a team or project, fix broken files and
rename clashing keys. On a separate page they browse the catalogue of development processes
(built in and the organization's own), read one, create, copy, import, download and edit the
organization's own. The team page and the member profile keep their Processes tabs, now read
through the processes module's public index. Everything runs against the module's API; the
production API refuses every call (`unavailableCall`) and the screens say so. The mock lives in
`e2e/fakes/modules/processes.ts` with JSON under `e2e/fakes/data/processes/`.

## Sources (in order of authority)

`docs/design/organization-data/README.md`, `01-organization-structure.md` (structure documents),
`18-development-processes-and-project-documentation.md`; `docs/specs/processes-ux.md` (all);
`docs/design/development-processes.md` §2, §9; `docs/specs/req-07-…` (what the UI shows);
`docs/specs/app-shell-ux.md` §0.1, §2, §4, §6, §6a.

## Routes

| Route | Shows |
| --- | --- |
| `/settings/processes?place=<kind:id>&kind=standard` | "Processes · <label>": Processes and Standards tabs (link tabs, `kind` in the URL), each with its definition line; Show select (Organization, teams, projects, agents); the inheritance view; a document's sheet (text, facts, why) |
| `/settings/processes/documents/new?kind=&place=&from=&template=` | Editor for a new document at a scope; `from` = override (copies the parent's text, same key, new ID); `template=review-and-merge` |
| `/settings/processes/documents/[documentId]` | Editor for an existing document (edit, Fix) |
| `/settings/processes/development` | "Development processes: how a project builds software": the catalogue; New, Import |
| `/settings/processes/development/[processId]` | One development process: summary, seed roles, roles, skills, stages, tabs, sections, projects using it; Edit a part, New role/skill/template, Copy, Download |
| `/teams/[teamId]/processes`, `/members/[memberId]/processes` | The same inheritance view, embedded (`PlaceProcesses` from `$lib/modules/processes`) |

## Acceptance criteria

- PROC-1/9: rows show title, key, source level (word + icon) with team/project name, revision,
  relation (replaces <level> / added here / inherited), not overridable, and whether it applies.
  The sheet shows source, scope, lifecycle, owner, key, revision, kind, path, attribution and the
  chain organization → team(s) → project with each layer's outcome.
- PROC-2: the mock resolves organization → accountable team ancestry → project's owning team
  chain → project; agents use their current work's project, else the organization.
- PROC-3/36 (people's side): Override is offered only for an overridable inherited document at a
  team or project place; a save that replaces a non-overridable document is refused inline with
  the spec's words; an existing such file shows the refused-replacement problem.
- PROC-10: agent place: current work line, or "No current work. Showing the organization's
  processes. Team and project processes apply when it starts a task."
- PROC-16: skeleton rows while loading; "Can't reach the organization repository. Showing the
  last copy from <time>." with the rows of that copy; invalid front matter names the field, with
  Fix, body still readable, left out; a key clash shows on both rows with Rename…, both left out.
- PROC-18: level badges and kinds are a word plus an icon.
- PROC-31/32/34: the Show select is a history entry (push), URL carries the place, reload keeps
  it, Back/Forward move between places; heading "Processes · <label>", title
  "<label> · Processes · Agentic Org"; option labels "Organization", "Web (team)",
  "Atlas (project)", "Noor Haddad (agent)".
- PROC-33: below 640 px the heading block stacks above the Show select.
- PROC-35: a project's (or an agent's current project's) development process is named in a line
  above the list with a link to it; no skill is listed.
- PROC-37: two tabs with definition lines; development processes are never rows of this page.
- Empty: "Write down how your agents work. Start with how changes get reviewed and merged." with
  **Use the review-and-merge template**.
- PKG-2/3/4/16 (UI side): built-in processes cannot be edited (Copy instead); a write refused
  by validation names the field inline; draft/current/deprecated shown; copy gives a new process;
  import arrives as a draft; download offers the process file.
- Every screen: loading skeleton shaped like the page, unavailable, error with Retry, not found,
  stale copy, refusals inline, busy labels, conflict on expected revision keeps the text.
- Layout at 1440/1024/640/390/320 and 320 px with 200% text: no sideways scroll, 44 px targets
  below 640 px, pills keep whole words. Focus per §6a; colours only through look tokens.

## Non-goals (spec "Not in v1" or other owners)

Private processes, approvals, turning off, drift review, guidance moments/checklists, one open
branch rule, autonomy levels; `agent-os` commands and launch delivery (office); choosing a
project's development process, the seed roster and project tabs (project settings, Step 4);
deleting a development process (no call in the contract).

## Ownership

Owned: `src/lib/modules/processes/**`, `src/routes/(shell)/settings/processes/**`,
`e2e/fakes/modules/processes.ts`, `e2e/fakes/data/processes/**`, `e2e/processes.spec.ts`,
`e2e/processes-bugs.spec.ts`, the processes entries of `e2e/gate-manifest.json`.
Touched by hand-over in this task: the Processes tabs of `modules/teams` (TeamProcesses,
api/types/fake `processes` call removed) and `modules/members` (ProcessesTab, ProcessRows,
api/types/fake `processes` call and `data/members/processes.json` removed); `e2e/focus-primitives*`
are not edited (their selectors keep working).
