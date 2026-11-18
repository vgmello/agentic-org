# The agent command line

How `agent-os` is shaped: one grammar, one answer format, one table of commands, and
which commands milestone one and milestone two deliver. It merges the owner's
[REQ-08](../specs/req-08-agent-os-cli-and-kernel-primitives.md) into the selected
architecture. The review of that requirement, with the decisions taken and the
questions for the owner, is at the end of the requirement document. The increments are
in the [implementation plan](../implementation-plan.md); the first is
[K1](../implementation-plan.md#k1--the-command-contract).

This document owns the command line's conventions and the list of commands. It does
not own what a command means: each row of section 3 names the contract that does.

## 1. The shape in one page

- **`agent-os` is the only way an agent reaches the product**
  ([non-negotiable 7](../architecture-non-negotiables.md)). Every call goes to the
  agent's own office over its local socket. Nothing is offered to an agent as a tool of
  another kind.
- **Every command is `agent-os <resource> <verb>` with named flags** and answers with
  one envelope: `ok`, `data`, `meta`, `error`.
- **A command is one row in one table** in a shared library. The parser, `--help`,
  `--schema`, the short catalogue an agent receives at launch and the contract tests are
  all generated from that table. A command that is not in the table does not exist.
- **The same table serves people.** `agentic-org` offers a signed-in person the same
  commands from the same library. The executable decides how the caller is identified:
  the binding of a run for `agent-os`, a signed-in person's grant for `agentic-org`.
  The office decides what the caller may do.
- **The command line adds no record and no rule.** A command is delivered by the
  increment that builds the thing it acts on, and not before. REQ-08 adds no record to
  milestone one.
- **The office enforces; `agent-os` asks.** An agent can run any program, so nothing
  checked only inside `agent-os` protects anything
  ([non-negotiable 9](../architecture-non-negotiables.md)).

```text
 agent's shell ──▶ agent-os ──────────┐
                   (binding of a run) │   one command table        ┌────────────┐
                                      ├──▶ parser · envelope ─────▶│ the office │
 person's shell ─▶ agentic-org ───────┘   schema · help             │ authorizes │
                   (signed-in grant)      (shared library)          │ and decides│
                                                                    └────────────┘
```

## 2. The contract every command follows

### 2.1 Grammar

```text
agent-os <resource> <verb> [--flag <value>]...
```

- A resource and a verb are lower-case words; a hyphen joins two words
  (`inspect-role`).
- **No positional arguments.** A bare word after the verb is refused with
  `invalid_request` and a hint naming the flag that was probably meant.
- A flag is `--kebab-case`, followed by its value as the next argument or as
  `--flag=value`. A yes-or-no flag takes no value. A flag that takes several values is
  repeated.
- An unknown flag, a missing required flag or a value of the wrong shape is refused
  before anything is sent to the office, and the hint lists the flags the command
  takes.
- A task is named by its reference (`T-B37`). Every other record is named by its ID,
  or by its key or slug where the data contract gives it one.
- Verbs are reused across resources so that an agent can guess: `list`, `inspect`,
  `create`, `update`. A domain verb is used where the requirement names one
  (`transition`, `post`, `ask`, `hire`).

Options every command takes:

| Option | On | Meaning |
| --- | --- | --- |
| `--format json\|yaml\|compact` | all | How the answer is printed (2.3). Default `json` |
| `--fields <a,b,c>` | all | Keep only these fields of each item in `data`; a dot reaches into a nested field |
| `--limit <n>`, `--cursor <token>` | lists | Page size (default 20) and where to continue; the next token is `meta.next_cursor` |
| `--schema` | all | Print the command's input and output schema and stop (2.5) |
| `--help` | all | Print the command's flags in plain text and stop |
| `--json '<object>'` | commands with input | The whole input, as one JSON object |
| `--input-file <path>`, `--stdin` | commands with a content field | The content, read as bytes (2.2) |
| `--dry-run` | writes | Decide and report; change nothing (2.6) |
| `--idempotency-key <id>` | writes | The operation ID (2.7) |
| `--if-match <revision>` | writes to a record with a revision | The revision the caller last saw (2.7) |
| `--durable` | writes | Answer only after the drive has it ([ADR-0014](../adr/0014-acknowledgement-classes-and-agent-os-latency.md) decision 4) |

### 2.2 Input that survives a shell

A command's input is one object; `--schema` shows it. There are three ways to give it,
and they combine:

- **Flat flags** for short values: `--status in_progress`.
- **`--json '<object>'`** for the whole object, or the part the flags do not give. A
  field given both ways with different values is `invalid_request`.
- **`--input-file <path>` or `--stdin`** for the command's **content field**. A command
  that carries long text declares exactly one content field: a message's body, a
  task's description, a comment's body. The bytes are taken unchanged as UTF-8, so
  Markdown and YAML with quotes, backticks, dollar signs and new lines need no
  escaping. Short content can still be given by the field's own flag or in `--json`. A
  command with no content field refuses both flags.

`agent-os` reads the file itself, as the agent, inside the agent's isolation. The
content travels in the request. **The office never opens a path that a caller names.**
A command reads standard input only when `--stdin` is given, and no command ever
prompts.

### 2.3 The answer

Every command prints exactly one envelope on standard output and nothing else: no
colour, no progress, no second line.

```json
{"ok":true,"data":{"reference":"T-B37","status":"in_progress","revision":4},"meta":{"status":"completed","operation_id":"0193…","ack":"committed"},"error":null}
```

```json
{"ok":false,"data":null,"meta":{"operation_id":"0193…"},"error":{"code":"conflict","message":"T-B37 changed since revision 3.","hint":"Read the task again and repeat the change against its current revision.","retry_with":{"if-match":4}}}
```

- **`ok`** is true when the command did what was asked or reached one of the stated
  waiting outcomes below. It is false when the command was refused or failed.
- **`data`** is the result: one record, or a list. Field order is fixed and lists have
  a stated order, so the same state prints the same bytes.
- **`meta`** carries what is about the call, not the record: `status`, `operation_id`
  and `ack` (`committed` or `durable`) for a write, `revision` when the write produced
  one, `next_cursor` for a list, `dry_run: true` for a dry run.
- **`meta.status`** is one of:

  | Status | Meaning | What the agent does |
  | --- | --- | --- |
  | `completed` | Done | Continue |
  | `timed_out` | An ask that is not required got no answer within its wait. `data.default` holds the ask's default when it has one | Continue with the default, or without the answer |
  | `withdrawn` | The ask was withdrawn or cancelled | Continue without it |
  | `suspended` | A required answer is pending ([ADR-0020](../adr/0020-required-asks-suspend-and-resume.md) decision 5). `data` names what is awaited and from whom | Close out background work and end the turn. The office stops the agent and starts it again, in the same session, with the answer |
  | `gate_required` | The change needs a person's approval before it takes effect. `data` names the approval and the approver | Continue with other work; the outcome arrives as a message |

  These are the requirement's `SUSPENDED` and `GATE_REQUIRED`, and the four results of
  an ask in ADR-0020.
- **Formats.** `json` prints the envelope on one line. `yaml` prints the same envelope
  as YAML, with long text as block text; it is the format for reading a document's
  body. `compact` prints `data` only: one line for each item of a list, the chosen
  fields separated by tabs, or `name: value` lines for one record. A failure prints the
  JSON envelope in every format.
- **Exit code.** 0 when `ok` is true, 1 when it is false, 2 when the command line
  itself could not be read. The envelope is printed in all three cases.

### 2.4 Errors an agent can act on

`error` has four fields.

| Field | Content |
| --- | --- |
| `code` | One value from the closed list in `org-proto` (`ErrorCode`), in lower case. The same codes the browser API uses. An agent branches on this |
| `message` | What happened, in a sentence, for a person or an agent. Never a path outside the caller's view, never SQL |
| `hint` | What to do next, in a sentence |
| `retry_with` | The flags to change for a retry that can succeed, as an object from flag name to value. Absent when repeating the call cannot help |

There is one spelling of each code. The requirement's examples map as follows.

| Requirement | Code | `retry_with` |
| --- | --- | --- |
| `CONFLICT_REV` | `conflict` | `{"if-match": <current revision>}` |
| `SCOPE_FORBIDDEN` | `not_authorized` | Absent. The hint names who may do it |
| `POLICY_VIOLATION` | `policy_violation` (new), or the rule's own code where clients already branch on one, such as `checklist_incomplete` | Absent, or the flag the rule asks for. The hint names the rule |

Three more that an agent meets often: `unavailable` and `not_ready` (repeat the call
with the same `idempotency-key`), `operation_reused` (the key was used for a different
change; use a new key), `invalid_request` (the hint lists the valid flags or values).

### 2.5 Finding out without loading everything

- **At launch an agent receives a short catalogue,** not the schemas: one line for each
  command, generated from the table, at most 4 KiB. It reaches the agent as its own
  input beside the processes that apply ([processes UX](../specs/processes-ux.md),
  PROC-28).
- **`agent-os --help`** lists the resources; `agent-os <resource> --help` lists its
  verbs; `agent-os <resource> <verb> --help` lists the flags. Plain text.
- **`agent-os <resource> <verb> --schema`** prints, in an envelope, the command's
  `input` and `output` as JSON Schema (draft 2020-12), its `content_field`, the error
  codes it can return and its acknowledgement class. It makes no call to the office.
- **`agent-os skill list` and `skill read`** give the product's built-in guidance on
  using the commands ([processes design](processes.md), decision 4), on demand.

The schemas are produced at build time from the same types the parser fills. A test
feeds every command's example input through both the schema and the parser, so the two
cannot drift.

### 2.6 `--dry-run`

A dry run goes to the office like the real call. The office runs the same decision:
is the caller allowed, is the input valid, does the stated revision still hold, does a
rule that the code enforces refuse it. It then discards the decision and answers what
the real call would have answered, with `meta.dry_run: true`. It changes nothing: no
file, no journal entry, no receipt, no number used.

In the [office store](office-store.md) a change is decided before it is applied, under
a mark the writer can undo to. A dry run is that decision followed by the undo. The
store gains one entry point for it, added with the first writing command (B2). There is
no second copy of any rule.

`agent-os grant check --command "<resource> <verb>"` with the same input is the same
dry run, answering only allowed or refused.

### 2.7 Safe retries and stale writes

- **`--idempotency-key`** is the operation ID that every change carries
  ([non-negotiable 5](../architecture-non-negotiables.md)). `agent-os` creates one for
  a write that does not carry one and reports it in `meta.operation_id`, on success and
  on failure. Repeating a call with the same key and the same input changes nothing
  further and returns the first answer. The same key with different input is
  `operation_reused`.
- **`agent-os operation new`** prints a fresh key without calling the office, for an
  agent that wants to hold the key before its first attempt.
  **`agent-os operation inspect --id <key>`** answers what became of an operation, for
  an agent that lost the answer.
- **`--if-match`** is the record's revision as the caller last read it. Where the data
  contract requires an expected revision (assignment, for one), the flag is required. A
  stale revision is `conflict`, and `retry_with` carries the current one.

### 2.8 Who is calling

The office starts each run and gives it a binding
([ADR-0003](../adr/0003-agent-messaging-office-first.md) decision 12,
[agent sessions](agent-sessions.md)). At launch it sets:

| Variable | Value | Used by |
| --- | --- | --- |
| `AGENT_OS_SOCKET` | The exact path of the office's socket | `agent-os`, to connect |
| `AGENT_OS_SESSION_TOKEN` | A random secret for this run, held only in office memory | `agent-os`, to bind |
| `AGENT_OS_SESSION_MARK` | A random value for this session, not a secret | The office, to find orphans |
| `AGENT_OS_ORG` | The organization's slug | The agent's own scripts |
| `AGENT_OS_OFFICE` | The office's ID | The agent's own scripts |
| `AGENT_OS_AGENT_ID` | The agent's employee ID | The agent's own scripts |
| `AGENT_OS_SESSION_ID` | The session's ID | The agent's own scripts |
| `AGENT_OS_PROJECT`, `AGENT_OS_TASK` | The project key and task reference of this run, when it has them | The agent's own scripts |

**The last five are context, never identity.** The office knows the organization, the
agent, the session, the project and the task from the binding: the token, and the
caller's descent from the process the office started. It never reads them from a
variable or from a flag. A command that leaves out `--project` gets the run's project
([identity and numbering](organization-data/06-identity-and-numbering.md)); one that
names another project is checked like any other call.

`agent-os` refuses to run without a binding, except for `--help`, `--schema` and
`operation new`, which make no call. There is no `--as`, no `--office`, and no
variable that names the caller.

### 2.9 Several commands in one turn

- **In a shell.** One line of JSON for each command, so `&&`, pipes and `jq` work.
- **`agent-os session attach`** is the long-running mode of ADR-0014 decision 5. It
  binds once, then reads one command for each line of standard input and writes one
  envelope for each line:

  ```json
  {"command":"thread list","input":{"limit":5},"options":{"fields":["id","from","subject"]}}
  ```

  It is the same table, parser and envelope. An agent that sends a batch through it
  pays for one process start and one bind. It stops at the first `ok: false` unless
  `--keep-going` is given.

### 2.10 Where MCP is used

- **No product primitive is an MCP tool.** Asking a question, reading a task or
  sending a message is always a command. This closes the open point of the processes
  design and replaces the `ask_user` tool route in the alignment of
  [REQ-04](../specs/req-04-channel-messaging-and-question-components.md).
- **MCP is for connectors to things outside the product** (a browser, a language
  server, a third-party service), granted to an agent through its execution
  configuration. Milestones one and two grant none: the MCP configuration the office
  writes for a run is empty, and an adapter refuses a start that reports any server.
- **The thin MCP wrapper for a runner without a shell is not built.** Every agent in v1
  runs where its office runs, with a shell. If such a runner is ever selected, the
  wrapper is generated from the command table and adds nothing to it.

## 3. The commands, and the contract behind each

"With" names the increment that delivers the command. A person's command is one the
office accepts only from a person in the stated position; an agent that calls it gets
`not_authorized` with a hint naming who may.

### 3.1 The run and what it may do

| Command | What it answers | Contract | With |
| --- | --- | --- | --- |
| `session inspect` | The organization, office, employee, home team and the leads above it, the project and task of this run, the session and its state, and what it is waiting on | [Agent sessions](agent-sessions.md) | C3 |
| `session attach` | The long-running mode (2.9) | ADR-0014 decision 5 | B6 |
| `grant list` | What the office holds about this run's reach: the teams the employee leads, the project and task, the worktrees and other writable places, the isolation mode, the tools and connectors of its execution configuration | [Employees](organization-data/04-agent-structure.md), [ADR-0015](../adr/0015-agent-isolation-modes.md), [execution](organization-data/11-model-profiles-and-agent-execution.md) | C3 |
| `grant check` | Whether a named command with a given input would be allowed (2.6) | The command's own contract | B2 |
| `operation new`, `operation inspect` | A fresh key; what became of an operation (2.7) | [Office store](office-store.md), receipts | K1, B2 |

`session suspend` and `session resume` are not commands. Suspension is the `suspended`
outcome of a required ask or a gate, and the office performs both steps (ADR-0020
decisions 6 and 8).

### 3.2 The organization and its people

| Command | What it does | Contract | With |
| --- | --- | --- | --- |
| `org inspect` | The organization, its offices and its top team | [Organization](organization-data/01-organization-structure.md) | B2 |
| `team roster` | The members of a team (default: the caller's home team) with their leads and presence | [Organization](organization-data/01-organization-structure.md), ADR-0003 decision 14 | B2 |
| `agent inspect` | One employee's card: person or agent, team, role, lead, status | [Employees](organization-data/04-agent-structure.md) | B2 |
| `agent status` | Sets or clears the caller's own status line (`status.set`, committed) | ADR-0003 decision 14 | B6 |
| `project list`, `project inspect` | The projects the caller can see; one project with its team, repositories and processes | [Projects](organization-data/02-project-structure.md) | B3 |
| `team create`, `team update`, `person add` and the like | A person's commands: the owner changes structure. B2 names them under this grammar | [Who may change what](organization-data/04-agent-structure.md) | B2 |
| `project create` | A person's command: the owner | [Who may change what](organization-data/04-agent-structure.md) | B3 |
| `hr roles`, `hr inspect-role` | The organization's roles; one role's body and requirements | [Roles UX](../specs/roles-ux.md) | C2 |
| `hr hire` | Asks for a new agent. Creates a hire proposal and returns `gate_required` with the proposal and its approver. Nothing is provisioned before a person approves | [The hire flow](../specs/our-org-in-the-app.md) (STAFF, HRL, APPR) | C2 |

### 3.3 Messages, questions and approvals

`channel`, `consult` and `thread` are three intents over one conversation and message
model ([messaging](organization-data/15-messaging-and-chat.md)). They write the same
records through the same calls (`msg.send`, `msg.ask`, `msg.reply` and the
recipient-state calls); the wire names belong to `org-proto`.

| Command | What it does | Contract | With |
| --- | --- | --- | --- |
| `channel list`, `channel join`, `channel leave` | The channels the caller may see; joining and leaving a manual channel | [Chat UX](../specs/chat-ux.md) (CHAT-19) | B6 |
| `channel post` | Posts in a conversation or to a person, with a kind: `handoff` or `announcement` with its reason | Chat UX section 5 | B6 |
| `channel ask` | Asks a question with the `ask_user` schema: `choice`, `yes_no` or `free_text`, an optional default, `--required`, `--wait`. To a person it holds for the answer; to a channel with nobody named it is a question that does not hold | [REQ-04](../specs/req-04-channel-messaging-and-question-components.md), ADR-0020 | B7 |
| `consult ask` | The same ask, addressed to an agent, or to a team or project, in which case the office addresses it to the lead of that team or of the project's accountable team | Messaging; [employees](organization-data/04-agent-structure.md) | B7 |
| `thread list`, `thread read` | Everything addressed to the caller, whatever its kind, unread first; one thread in full | ADR-0003 decisions 10 and 11 | B6 |
| `thread reply` | Replies in a thread; answers an ask | Messaging; ADR-0020 | B6, B7 |
| `thread done`, `thread defer` | The caller's own triage state | ADR-0003 decision 11 | B6 |
| `thread wait` | Holds until something arrives or the wait ends | [Agent sessions](agent-sessions.md) (holds are capped) | B6 |
| `gate open` | Asks for an approval: a required yes-or-no ask linked to a task, addressed to the named approver or, with none named, to the nearest person above the caller in its lead chain, else the owner. Returns `suspended` | ADR-0020; the approver rule of [the hire flow](../specs/our-org-in-the-app.md) section 7.1 | C6 |
| `gate status` | Whether an approval is waiting, approved or rejected, by whom and why | The same | C6 |

A gate is an ask. It adds no record. An agent never approves a gate.

### 3.4 Tasks

| Command | What it does | Contract | With |
| --- | --- | --- | --- |
| `task create` | Creates a task, bug or request (`--type`), in the run's project unless one is named | [Tasks](organization-data/03-features-and-work.md), [ADR-0019](../adr/0019-office-coded-task-numbers.md) | B5 |
| `task list`, `task inspect` | Tasks by project, assignee, status or type; one task with its history | [Operational data](organization-data/05-operational-data.md) | B5 |
| `task update` | Title, description, priority, effort | The same | B5 |
| `task transition` | Changes the status, from the closed list | The same; [work items UX](../specs/work-items-ux.md) | B5 |
| `task assign` | Changes the assigned team and assignee together, at a stated revision | [Assignment and history](organization-data/12-task-assignment-and-history.md) | B5 |
| `task comment`, `task watch` | Adds a comment; starts or stops watching | Operational data | B5 |
| `checklist add`, `checklist check`, `checklist update`, `checklist remove` | A task's checklist | Work items UX (CHK) | B5 |

### 3.5 Processes, standards and guidance

| Command | What it does | Contract | With |
| --- | --- | --- | --- |
| `process catalog` | The processes that apply to the caller's current work, or to a named task, with where each comes from and which must be followed | [Processes design](processes.md) (`process.effective`), PROC-8 | C5 |
| `process inspect` | One process's full text by key; with `--why`, the chain from organization to team to project and which level won | The same (`process.show`, `process.why`) | C5 |
| `policy list`, `policy inspect` | The same two reads for documents of kind `standard` | The same | C5 |
| `skill list`, `skill read` | The product's built-in guidance on its own commands | Processes design, decision 4 | C5 |

`process apply` and `policy check` are in section 6.

### 3.6 Repositories and worktrees

| Command | What it does | Contract | With |
| --- | --- | --- | --- |
| `repo list` | The repositories of a project with their integration branches | [Git design](projects-git-github.md) | C7 |
| `repo attach` | A person's command: links a registered repository to a project | The same; who may change what | C7 |
| `worktree create` | Asks the office to create the caller's worktree for a task it is assigned, one for each repository the task targets. The office creates it under its own rules and answers the path. Repeating it answers the same path | Git design, "A worktree per task and repository" | C7 |
| `worktree list` | The caller's worktrees with task, repository, branch and state | The same | C7 |
| `worktree cleanup` | Asks the office to remove the worktree of a closed task. Refused while it holds work that is not pushed | The same | C7 |
| `repo push` | Asks the office to push the task's branch (`git.push`). The agent holds no credential | The same | C7 |

### 3.7 What today's commands become

| Today | Becomes |
| --- | --- |
| `whoami` | `session inspect` |
| `who` | `team roster`, `agent inspect` |
| `status` | `agent status` |
| `msg send` | `channel post` |
| `msg ask` | `channel ask`, `consult ask` |
| `msg reply`, `msg inbox`, `msg read`, `msg done`, `msg defer`, `msg wait` | `thread reply`, `thread list`, `thread read`, `thread done`, `thread defer`, `thread wait` |
| `profile set`, `msg register`, `msg deregister` | Deleted. An agent exists only through a hire |
| `msg notify`, `msg due`, `relay`, the file queue, `--as`, `$ORG_AGENT`, `--office` | Deleted with the file queue (B6) |

Nothing is kept beside its replacement
([non-negotiable 14](../architecture-non-negotiables.md)).

## 4. Milestone one: one office on a Mac

The contract comes first and is small. After it, each increment adds its own rows to
the table.

| Step | Increment | Delivers for the command line |
| --- | --- | --- |
| 1 | **K1** (new) | The shared library: the command table, the parser, the envelope and its three formats, `--help`, `--schema`, exit codes, `operation new`, and the contract tests of section 8 run against a test office. No command of today is converted |
| 1 | A5 | `agentic-org` uses the library for a signed-in person |
| 2 | B2, B3 | The first real commands: `org inspect`, `team roster`, `agent inspect`, `project list`, `project inspect`, `project create`. With the first write: `--dry-run`, `--idempotency-key`, `--if-match`, `grant check`, `operation inspect` |
| 3 | C2 | `hr roles`, `hr inspect-role`, `hr hire` and the `gate_required` outcome |
| 3 | C3 | The binding and the launch variables of 2.8; `agent-os` refuses to run without a binding; `session inspect`, `grant list`; the short catalogue in the agent's instructions; the empty MCP configuration |
| 3 | C5 | `process catalog`, `process inspect`, `policy list`, `policy inspect`, `skill list`, `skill read` |
| 4 | B6 | `channel list`, `join`, `leave`, `post`; `thread list`, `read`, `reply`, `done`, `defer`, `wait`; `agent status`; `session attach`. Today's `msg` commands, the file queue and the relay are deleted |
| 4 | B7 | `channel ask`, `consult ask`, and the `timed_out` and `withdrawn` outcomes |
| 4 | C6 | The `suspended` outcome; `gate open`, `gate status` |
| 5 | B5 | `task` and `checklist` |
| 5 | C7 | `repo` and `worktree` |

At the end of milestone one an agent can find out who and where it is, read what
applies to its work, talk and ask, wait for an approval, work a task in a worktree and
have its branch pushed, with every call in one grammar.

## 5. Milestone two: Linux

Milestone two adds no command and changes none. It proves the same table on Linux.

| Increment | Delivers for the command line |
| --- | --- |
| X1b | `agent-os` and the shared library reach the socket's location, the peer check and file reads only through `org-platform` (`dirs`, `socket`, `fs`). Neither contains a platform branch, and the platform check enforces it |
| X2 | The binding on Linux: the token, and the caller's descent from the started process through the `process` capability. The contract tests that need an office pass on Linux for the commands that exist then |
| X5 | `agent-os` is in the container image's release `bin/`, first on an agent's `PATH`, and the image has a shell. No wrapper |
| X6 | The contract tests run in CI on both systems. The catalogue and every schema are byte-identical on macOS and Linux. One-shot and `session attach` timings are recorded on Linux; the target of ADR-0014 is held on the reference Mac |

Linux and milestone one advance side by side, so "the commands that exist then" grows
with each merge: a command added by milestone one must pass on Linux in the same
change, or the Linux CI job of X6 fails.

## 6. Not in milestone one or two

| Requirement | Why not yet | Where it goes |
| --- | --- | --- |
| `milestone`, `release` | Features, releases and deployments are a later increment | L7, in this grammar |
| `ia`, `fs`, `link` | Project documents are L7. Their meaning (the Information Architecture index, project storage, links to DSS versions) is defined in REQ-01 to REQ-07, which this repository does not hold. Until they are settled, `agent-os doc` in the [knowledge UX](../specs/decisions-and-tech-debt-ux.md) (AUTH-5, TPL-3) states the behaviour and its command names are provisional | L7, after the owner's answer |
| `process apply`, `block run`, role-bound skills | Process Packages. Processes here are documents that are read, and writing them from the command line is not in v1 (PROC-8) | After the owner's answer |
| `artifact publish`, `artifact drift` | No record of typed artifact versions or derivation exists | After the owner's answer |
| `event emit`, `event subscribe` | The office has one change stream, for its own readers. Agents publishing events and packages subscribing to them is new | After the owner's answer |
| `policy check` | A written standard cannot be checked by a program. Rules the code enforces are checked by `--dry-run` | With machine-checked policies, if they are added |
| `runner spawn` | Only the office starts a run ([non-negotiable 10](../architecture-non-negotiables.md)) | See the owner's question 2 in REQ-08 |
| `logs inspect` | The session view is for people in the browser (C4). No rule says which agent may read which session | Later |
| `metrics query` | Usage and costs | L2 |
| A gate resolved by policy alone | Approvals are a person's | Not in v1 |
| The thin MCP wrapper | No runner without a shell | Not built |

## 7. Where the code lives

- **The shared library** holds the table, the parser, the envelope, the formats, the
  schemas and the client side of the session mode. It depends on `org-proto` and
  `org-client`, and on nothing that knows a database or an operating system. Whether it
  is a module of `org-client` or a crate of its own is K1's decision.
- **`apps/agent-os` and `apps/agentic-org`** stay thin: each chooses how the caller is
  identified and hands the arguments to the library
  ([non-negotiable 15](../architecture-non-negotiables.md)).
- **A domain's rows** are declared beside that domain's wire types, by the increment
  that builds the domain. Adding a command touches the table and nothing in the parser.
- **The office** owns authorization, the decision and the dry run. Its wire calls keep
  their own names.
- **The built-in guidance** (`skill`) ships in the release as files the office reads.

## 8. Tests that prove it

| ID | Passing evidence |
| --- | --- |
| CLI-1 | For every row of the table: a positional argument, an unknown flag and a missing required flag are each refused with `invalid_request`, a hint and exit code 2, and nothing reaches the office |
| CLI-2 | For every row and every format, standard output is exactly one envelope that validates against the envelope's schema; `json` is one line |
| CLI-3 | Every `--schema` output is valid JSON Schema, and every row's example input and recorded output validate against it |
| CLI-4 | `--dry-run` on every writing row leaves the store's position, files and receipts unchanged, and reports the refusal the real call reports |
| CLI-5 | The same `--idempotency-key` twice makes one change and returns the same answer; the same key with other input is `operation_reused`; `operation inspect` returns the stored answer |
| CLI-6 | A stale `--if-match` is `conflict` with the current revision in `retry_with`, and the retry with it succeeds |
| CLI-7 | Markdown with quotes, backticks, dollar signs, new lines and non-ASCII text arrives byte-identical through `--input-file`, `--stdin` and `--json` |
| CLI-8 | With standard input closed and no terminal, every row completes; no row reads standard input without `--stdin` |
| CLI-9 | Without a valid binding every call is refused. Changing `AGENT_OS_ORG`, `AGENT_OS_OFFICE`, `AGENT_OS_AGENT_ID`, `AGENT_OS_SESSION_ID`, `AGENT_OS_PROJECT` or `AGENT_OS_TASK` does not change who the office says is calling |
| CLI-10 | Each person's command is refused to an agent by the office when the wire call is made directly, without `agent-os` |
| CLI-11 | The launch catalogue is generated from the table and is at most 4 KiB; a snapshot of the catalogue and of every schema fails the build when a row changes without it, and is identical on macOS and Linux |
| CLI-12 | The MCP configuration written for a run is empty, and a start that reports any MCP server is refused |
| CLI-13 | `session attach` answers each line with the envelope the one-shot command prints |
| CLI-14 | A required ask and a gate return `suspended`; a hire request returns `gate_required`; neither blocks past its stated wait |

K1 makes CLI-1, 2, 3, 7, 8 and 11 pass against a test office. Each later increment
makes them pass for its own rows, and the rest pass with the increment that builds
what they test: CLI-4, 5, 6 and 10 with B2, CLI-9 and 12 with C3, CLI-13 with B6,
CLI-14 with C2, B7 and C6.

## 9. What the code does today

`apps/agent-os/src/main.rs` parses arguments by hand, with positional arguments, prints
text unless `--json` is given, and uses several exit codes. Its commands are the left
column of 3.7. Most write to a file queue; five go to an office when one is named. The
caller is named by `$ORG_AGENT` or `--as` and is not authenticated on the file-queue
path. None of it is kept.
