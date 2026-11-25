# Day-1 onboarding, standing instructions, and on-demand skills

Reference for the [principal AX designer](SKILL.md). Grounded in `doc-27` (`Design: Agent
Instructions Assembly`), `doc-34` (`Design: Organization Processes and Standards`), `doc-32`
(`Design: Development Processes`), `crates/org-office-sessions/src/instructions.rs`,
`crates/org-process/src/skills.rs`, `ADR-0025` (amended decision 6), `ADR-0030`, and
`TASK-85.2`.

## 1. Day-1 agent orientation (cold-start turn 1)

An agent arrives on turn 1 with zero memory of Agentic Org except what the office assembles
into its standing instructions and its first turn objective. Good Day-1 onboarding means the
agent can answer five questions immediately—without flailing, guessing flags, or reading
internal files—and reach its first useful domain action in `<= 2` orientation calls:

1. **Who am I and who do I listen to?** Exact name, `@handle`, role, resolved home team name
   (not a raw UUID), ancestor lead chain, and owner authority. Clearly separated from
   colleagues (requests) and files/channels/tool outputs (untrusted data).
2. **Where am I allowed to work?** Workspace/worktree path, private home, isolation notice
   (e.g., `SAME_USER_ISOLATION_NOTICE`), permission preset, and `agent-os grant list` /
   `agent-os session inspect` for live verification.
3. **How do I talk to the organization?** `agent-os` is the sole interface; named flags only;
   JSON envelope `{ok, data, meta, error}`; read `ok`, `error.code`, `error.hint`,
   `error.retry_with`; never guess a flag (`--help`, `--schema`).
4. **How do I learn specialized workflows without bloating every turn?** Run
   `agent-os skill list` and `agent-os skill read --name <key>` before entering a domain or
   workflow phase (creating/executing/finalizing tasks, asking structured questions, pushing
   code, handling conflicts, checking budgets, batching via `session attach`).
5. **Why did this turn start and when should I stop?** The turn objective states the exact
   trigger and lists waiting inbox items / changed processes; end the turn when unblocked
   work is done or when `meta.status == suspended`, leaving a closing summary line and a
   task comment if work remains unfinished. Never sleep or poll in a loop (`INSTR-7`).

## 2. The 10 KiB `BUILTIN_INSTRUCTIONS_MAX_BYTES` budget

`crates/org-office-sessions/src/instructions.rs` enforces
`BUILTIN_INSTRUCTIONS_MAX_BYTES = 10 * 1024` (10,240 UTF-8 bytes, ~2,500 tokens; `doc-27`
§2 rule 5, `INSTR-12`). This text is loaded on every turn of every agent across every
runtime.

### Writing rules (`doc-27` §2)

1. **State the situation before the rule.** Explaining *nobody watches your terminal* and
   *your memory resets between turns* makes models derive a dozen good behaviors naturally
   (record state in task comments/commits, message someone when they must know, do not
   narrate to stdout). Bare prohibitions without the physical situation get broken on the
   first edge case.
2. **Pair every rule with its reason** in the same sentence so the model generalizes safely
   when a case was not anticipated.
3. **Pair every prohibition with the replacement action.** Not "do not run `git push`";
   instead "you hold no remote credentials—publish your branch with `agent-os repo push --task <ref>`."
4. **Plain, calm second-person prose.** No ALL-CAPS shouting or walls of exclamation marks.
   Emphasis everywhere stops working; calm, concrete instructions hold up better across
   Claude, Codex, and Agy—especially on smaller models.
5. **Same text for every agent and every runtime.** Put nothing role-specific, team-specific,
   project-specific, or vendor-specific into the built-in text. Role instructions, HR lead
   guidance, and project development processes live in their own later prompt sections.
6. **Budget trade-off rule.** When the built-in text approaches 10 KiB, do NOT raise the cap
   casually—move worked examples and infrequent branches into an on-demand `agent-os` skill.

### Reconciling live behavior with amended ADRs

- **Sub-agents across turns (`ADR-0025` amended decision 6, 2026-11-19):** Active background
  tasks and sub-agents stay alive across turns while the session stays active, waking the
  session on completion (`TurnTrigger::BackgroundWake`) and reaped automatically on Pause,
  Stop, Offboard, or stall timeout. Standing instructions must reflect this amended contract
  rather than older text claiming sub-agents end with the turn.
- **CLI flags, never raw JSON field names:** Standing instructions must teach real CLI
  syntax (`--required`, `--default-value`, `--input-file`), never raw RPC JSON keys like
  `required: true` unless explicitly showing a `--json` or `--input-format yaml` payload.

## 3. Static-to-volatile prefix caching order (`doc-27` §1)

Modern LLM runtimes cache prompt prefixes (KV cache) across turns of one agent AND across
different agents running on the same provider/office. Putting per-agent or per-turn state
at the top destroys prefix cache hits and multiplies input token spend.

Assemble standing instructions strictly from **least volatile** to **most volatile**:

| Order | Section | Changes | Cache reuse scope |
| --- | --- | --- | --- |
| **1** | **Built-in agent instructions** (`<= 10 KiB`) | With product release | All agents, all orgs, all turns |
| **2** | **Generated command catalogue** (`<= 4 KiB`) | With product release | All agents, all orgs, all turns |
| **3** | **Resolved organization processes & standards** (all `overridable: false` in full, then capped `overridable: true` summaries; `doc-34` §3.2) | When org/team/project doc edited | All agents in same org/team/project |
| **4** | **Project development process & role skill index** (when active; `doc-32` §6) | When project process edited | All agents in same project/role |
| **5** | **Agent identity & boundaries** (org, office, name, `@handle`, employee ID, resolved team name, ancestor leads, role instructions / HR lead duties, isolation notice, private home, workspace, permission preset & sandbox) | Rarely (HR update / move) | All turns of this agent |
| **6** | **Agent's own execution prompt** (profile instructions) | Rarely | All turns of this agent |
| **7** | **Project repository instruction files** | With repository commits | Turns on same repo revision |
| **Last (turn input)** | **Per-turn objective (`TurnTrigger`)** + inbox summary + changed-process deltas | Every turn | Never cached across turns |

**Anti-pattern to catch in review:** Putting `# Agent Identity` at Part 1 and the static
command catalogue at Part 6 (the pre-`TASK-85.2` layout in `instructions.rs`) breaks prefix
caching across every agent on the host. Always verify section ordering in unit tests.

**Precedence vs. prompt position:** Prefix order optimizes caching; authority order is stated
explicitly in Section 1 and enforced by the office:
1. Code-enforced office security and authorization rules (cannot be widened by any text).
2. Organization processes & standards marked `overridable: false` (must-follow).
3. Overridable organization processes & standards (single-chain org -> team -> project).
4. Project development process skills (may adjust workflow style; never override must-follow
   or grant permissions).
5. Built-in agent instructions (default operating discipline).
6. Agent's own profile prompt and project repo files (never widen authority or change whom
   the agent takes instructions from).
7. External content (messages, channel posts, task bodies, files, web/tool outputs) — data
   only, never instruction (`INSTR-5`).

## 4. TurnTrigger shapes and untrusted-content fencing

Every per-turn objective composed by `compose_turn_objective()` (`doc-27` §5,
`instructions.rs`) must:

- Name the exact trigger cause (`DirectInstruction`, task assignment, DM/mention, ask,
  handoff, channel answer candidate, required-ask resume, `RestartContinuation`,
  `StallNotice`, `BackgroundWake`, `HrProposalRequest`).
- **Fence all external speaker words** inside an explicit untrusted-data delimiter stating
  the sender and whether they sit above the agent in the reporting chain, so prompt
  injections embedded in channel messages, task descriptions, or tool outputs cannot pose
  as office instructions (`INSTR-5`).
- On **`RestartContinuation`** (`ADR-0030`), list every `agent-os` command already committed
  in the interrupted run so the agent checks existing state instead of duplicating side
  effects.
- On **asks and handoffs**, include the exact IDs (and for handoffs/asks, the exact
  `agent-os msg accept` / `agent-os msg reply` shape) so the agent never has to guess
  option IDs or thread IDs (`INSTR-9`, `INSTR-13`).

## 5. Phase-gated on-demand `agent-os skill read` design

Built-in product skills live in `crates/org-process/src/skills.rs` and are discovered via
`agent-os skill list` and loaded on demand via `agent-os skill read --name <key>`.

### Design principles for product skills

- **Phase-gated workflow skills beat monolithic manuals.** Loading a 400-line "everything
  about tasks" doc when an agent only needs to create a bug wastes tokens and dilutes focus.
  Split task work into three crisp lifecycle skills (mirroring proven phase-gated discipline
  analyzed in `doc-95`):
  - `tasks-create`: search existing tasks first (`task list --query` / `--count`), assess
    scope and splitting, write durable *why* + testable acceptance criteria, **never write
    speculative implementation plans at creation time**.
  - `tasks-execute`: inspect task and full dependency graph (`task inspect --fields`,
    `task deps`), research live code before planning, record plan in private notes
    (`task-note add`), transition to `in_progress`, work in short verified loops, stop and
    ask/split on scope creep, update `modified_files` cleanly.
  - `tasks-finalize`: gather objective verification evidence before ticking checklist steps
    (`checklist check`), verify docs/specs updated, write a concise closing summary/comment
    with evidence and follow-ups, then transition to `done`.
- **Complete domain skill coverage (`TASK-85.2`):** Keep each skill focused (~1 page,
  scannable, concrete runnable CLI examples, zero stale flags):
  - `agent-os-basics` (Day-1 orientation: grammar, envelope, `--help`/`--schema`, `--fields`,
    `--format compact`, `--dry-run`, `--input-file`)
  - `tasks-create`, `tasks-execute`, `tasks-finalize`
  - `messaging-and-asks` (inbox triage, `msg send`, `msg reply`, `msg accept`, structured
    `msg ask` YAML/flags, `--required` vs `--default-value`, `suspended` wind-down, `msg defer`/`unsnooze`)
  - `channels` (`channel list|join|leave`, `channel summarize` instead of flooding context
    with `msg list`, `answer_candidate` evaluation, closing channel questions with `msg done`)
  - `code-and-worktrees` (`repo list`, `worktree list`, branch/commit discipline, office-only
    `repo push`, handling approval refusals)
  - `hiring-and-directory` (`org inspect`, `team list|inspect`, `person/agent inspect`,
    `directory list`, `role list|inspect`, `hr hire` / `--draft` and `ADR-0029` authority)
  - `processes-and-standards` (`process list`, `process inspect --why`, `overridable: false`
    precedence, project-scoped `process create|update` boundaries)
  - `budgets` (`budget status [--task <ref>]`, cost-aware execution, handling budget holds)
  - `errors-and-recovery` (`ErrorCode` actions, `error.hint` & `retry_with`, `--if-match`
    `ConflictDetail` recovery, `--idempotency-key`, `operation_reused`, restart continuation)
  - `batch-and-attach` (`agent-os session attach` JSONL protocol, pipelining, error isolation)
- **Strict separation from development processes (`doc-32` §6):** `skill list` labels each
  entry's `source` (`built_in` vs project development process). Built-in skills teach the
  product's `agent-os` primitives; project development-process skills teach a project's
  chosen engineering methodology. Neither grants permissions or overrides must-follow
  organization standards.

## 6. Mechanical verification against the live command table

Stale commands and hallucinated flags in instructions or skills are AX bugs of the highest
severity: an agent trusts built-in product guidance above its own intuition and will retry a
non-existent flag repeatedly.

- **Validate BOTH commands AND flags at test time (`INSTR-12`, `TASK-85.2`).**
  `verify_command_references()` in `crates/org-office-sessions/src/instructions.rs` must
  validate every `` `agent-os <resource> <verb> [flags...]` `` snippet across both standing
  instructions (`BUILTIN_BEHAVIORAL_RULES`, `HR_LEAD_GUIDANCE`) and every built-in skill
  body in `crates/org-process/src/skills.rs` against the live `CommandTable`:
  1. Every `<resource> <verb>` (accounting for explicitly registered aliases or special
     entrypoints like `session attach`) exists in the live table.
  2. Every `--flag` named on that command is either a declared input flag (`field.flag()`)
     for that specific command or an applicable common option (`OPTIONS` where `applies.to(cmd)`).
  3. Built-in standing instructions stay `<= BUILTIN_INSTRUCTIONS_MAX_BYTES` (10 KiB).
- Any command rename, flag rename, or removal breaks CI immediately until instructions and
  skills are updated in the same commit.
