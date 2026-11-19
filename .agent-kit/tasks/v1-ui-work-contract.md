# Contract: v1 UI · Work items (plan step 5, plus step 4's project Work tab)

Branch `v1-ui-work`. Builder: principal designer (implement job). Reviews run after the commit.

## Outcome

A person can follow, capture and act on every task type (task, bug, request) in the browser against the
mock API: the cross-project Work queue at `/work`, the project Work tab at `/projects/[id]/work`, and each
item's full page at `/work/[ref]`. In production every call refuses (`unavailableCall`) and each place
says so; nothing shows sample data.

## Routes

| Route | Shows |
| --- | --- |
| `/work` | Tasks · Bugs · Requests (remembered per organization), Show menu (`?view=` Needs me, Mine, By project, By assignee), saved filter menu, filter menus, search, Board/List (remembered per scope), New task / Report bug / New request |
| `/projects/[id]/work` | The same queue scoped to one project (accountable team derived from the project) |
| `/work/[ref]` | The task, bug or request page: main column + side column |

## Acceptance (from the specs; IDs are theirs)

- WORK-1, 2, 4, 5, 8, 9, 10; TRI-7: type tabs, T- references, project/no-project/team filters, saved filters
  (Owned by my team, Assigned to my team, Assigned to me, Reported by me, Blocked, All), Show views with
  `?view=`, Needs me rows link their Inbox item, empty copy "Nothing needs you in Work".
- TSK-1..4, 9, 10: board columns To do, In progress, Blocked, Done (Done last 7 days + Show all done), drag,
  ⋯ Move to, keyboard (Alt+←/→), live-region announcements, priority-then-age sort, Blocked needs a reason,
  priority shown only when not Normal with icon and word.
- BUG-10..15: bugs grouped by assigned team (Unassigned team explicit), severity counts with words incl.
  Not set; group by severity/priority/status/accountable team/assigned team/assignee/none; empty, no open
  bugs and no-match copy from §6.
- RQS-7, 14, 15: requests view groups by status; a team-only route is never shown as accepted.
- Task page (§7.1, §9, §9.1–9.3; TSK-8, 11–14, 17, 18; CHK-1..4, 7): fields, checklist (add inline, Enter,
  pasted lines, 1–200 chars, tick provenance, Required, reorder by drag/menu/Alt+↑↓ announced), shared
  discussion, private notes (author edits/deletes, people read others' read-only, explicit publish
  preview), immutable activity incl. "Unknown change", output (resolved and unresolved evidence, "No
  outcome recorded"), per-repository evidence, Why block (decisions-and-tech-debt §5), Cost and time
  (costs §2: cost, tokens, By agent, budget bar, lead time split with "the rest", "so far").
- Assignment pair (§5, TRI-2/3/6, TSK-13): team-only, individual-only, both, neither; expected revision;
  conflict shows current vs desired pair with actor/time and explicit retry.
- Bug page (§7; TRI-1..6, FIX-1..9, 11, BUG-14): triage region, reported vs current provenance, fix tasks
  (Create fix task), Mark as fixed (fix task, commit or note), Verify (Fixed / Still happens with note,
  reopen count), Close as (Done, Duplicate needs a resolvable original and no self link, Won't fix and
  Can't reproduce need a reason, Cancelled), Reopen needs a note.
- Request page (feature-requests §3; RQS-9, 12): original text, shared conversation, outcome (feature,
  documents, draft tasks, estimate), Approve and create with an editable preview, Ask for changes, Decline
  (reason), Merge into (another open request); the scoper never approves its own outcome.
- Capture (§4, §9, feature-requests §1; BUG-2, 4, 5, 7, 9, TSK-5, RQS-2): New task, Report bug and New
  request dialogs with accountable team derived from the project or chosen, optional project, assignment
  pair, priority Normal, severity explanations and Not set, similar bugs after 3 title characters with
  "This is the same bug", similar requests with "Add to", discard confirmation, "Bug T-A42 saved".
- Budgets (costs §4; BUD-2, 3): set or raise dialog (unit, value, exact micro-USD), Saving…, field-linked
  refusals, "Save not confirmed" + Check status; reached actions Raise budget…, Allow 20% more once (only
  at base reach, once), Clear budget, Stop (confirm names the transition); agent managers see "Only a
  person can raise, clear or extend a budget."; chips on rows/cards show budget state in words.

## States

Every read: skeleton shaped like the page with Loading/Still loading, error with Retry, unavailable
(production), empty with one real action, not found (`/work/[ref]`). Stale copy is read-only with a label.
Every write: busy label (-ing word), refusal inline with focus, confirmation naming the effect.
Titles: `Work · Agentic Org`, `T-A42 <title> · Work · Agentic Org`, `Task not found · Work · Agentic Org`.
Focus: page h2 after client navigation (after the shell's tick); removed rows move focus to a neighbour.

## Ownership

- New: `src/lib/modules/work/**`, `src/routes/(shell)/work/**` (rebuilt), `e2e/fakes/modules/work.ts`,
  `e2e/fakes/data/work/**`, `e2e/work.spec.ts` (rewritten).
- Edited: `src/routes/(shell)/projects/[id]/work/+page.svelte` (renders the work module scoped to the project),
  `e2e/gate-manifest.json`.
- Not touched: shared `src/lib/components/*` (the old slide-overs `TaskSheet`, `BugSheet`, `RequestSheet` and
  dialogs stay as they are), `src/lib/work/**` (legacy engine, still used by the top-bar New request), Chat,
  the messaging module, nav, shell, `package.json`, `docs/plans/*`.

## Non-goals (spec "Not in v1" or owned elsewhere)

Completion gates, step promotion, checklist templates, Undo, attachments, Following, Requested-by, named
review, planning, automatic propagation from fix tasks; copilot drafting (owned by the copilot module);
live updates over a feed (needs the web API's event stream: TSK-7, WORK-6 not built); the top-bar New menu
wiring (shared shell file: the lead switches it to the work module's dialogs); attribution correction in
By agent (costs module); @name combobox and Message instead in New request.
