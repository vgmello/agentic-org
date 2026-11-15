# Fresh review brief: Tasks PRD (tasks-v1.md)

You are a fresh, independent reviewer of a product requirements document. You have no prior context. Use only Read, Glob, Grep (and read-only `git -C /Users/vgmello/projects/agentic-consulting/app show origin/v1:<path>` through Bash if you have it, read-only). Do not edit any file. Do not commit. Do not message anyone.

## The document

- `projects/ac/agents/ac-product-manager-emery/knowledge/prds/tasks-v1.md` (repo root /Users/vgmello/projects/agentic-consulting/agents)
- Companion docs it amends: `knowledge/prds/information-architecture-v2.md` (decision D38 and §4), `knowledge/prds/ia-v2-stories.md` (AC-IA01-1, AC-IA04-1, AC-IA18-1), same folder.
- Project brief: `projects/ac/CLAUDE.md`.

## The owner's requirements (what the PRD must address)

R1. A "Created by me" view of tasks, separate from the Inbox. The Inbox is only for things that need the owner's attention. Filter tasks by creator. The owner can see the output and status of what they asked for.
R2. A "Following" entry in the main menu that takes the owner straight to the things they follow; user experience is the priority (long-running agent teams).
R3. Task → subtask, at most one level.
R4. Agents plan their work and close it item by item.
R5. Timing, actor and status changes captured behind the scenes and recorded (record everything). Points/durations considered.
R6. More statuses: blocked, pending/waiting, in review; define how review works.
R7. Assignment: decide single assignee vs reassignment; reassignment must be recorded.
R8. Private notes per author: visible to the owner, not transferred to a new assignee on reassignment, flagged as private in the UI. The owner's own notes are private to the owner. Internal rationale must not influence other agents' reviews.
R9. Comments are shared and transfer on reassignment. Requirement changes go in comments so a new agent starts from the original requirements plus the comment trail.
R10. An agent CLI `org-agent tasks`: add, set status, complete, comments and notes with add/edit/list (small CRUD).
R11. Explain task vs message vs Inbox item, and how the Inbox is structured and stored.

## Ground truth to check claims against (origin/v1 of /Users/vgmello/projects/agentic-consulting/app)

- `crates/org-proto/src/work.rs` (work model, events, statuses, comments, watchers)
- `crates/org-proto/src/inbox.rs`, `apps/org-server/src/inbox.rs`
- `crates/org-proto/src/msg.rs`
- `docs/design/tasks-projects-docs.md` (§5 CLI design)
- Rowan's Inbox freeze: find `ia-10-inbox-read-freeze.md` under `projects/ac/agents/ac-fullstack-engineer-rowan/`
- `apps/org-ui/src/lib/work/saved.ts`
Spot-check file:line citations in the PRD (at least 8).

## Check

1. Every requirement R1-R11 addressed, with testable acceptance criteria an engineer or QA can verify without asking.
2. Internal consistency: statuses, permissions (who can reassign, cancel, reopen, review, see notes), Following groups and badge rules, CLI verbs vs UI behaviour, v1.0 vs Next scoping (§8), dependencies (§6) — no contradictions between sections, stories and the amended IA docs.
3. Privacy of notes holds on every path the PRD names; no leak through versions, events, search, exports or agent context.
4. Factual claims about the current code are correct (cite what you checked).
5. It stays on product requirements and does not make technical design decisions (it may recommend and hand to engineering).
6. UX quality: states (empty, loading, error, offline), copy, no emoji, amber only for "waiting on the owner".

## Report (short)

Verdict READY or CHANGES NEEDED. Findings by severity HIGH / MEDIUM / LOW, each with file:line, the problem, a failure scenario, and a suggested fix. HIGH = a requirement missed or a contradiction that would make engineering build the wrong thing; MEDIUM = ambiguity or gap an engineer would have to ask about; LOW = polish. List which citations you verified.
