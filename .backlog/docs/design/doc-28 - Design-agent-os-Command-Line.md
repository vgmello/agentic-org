---
id: doc-28
title: 'Design: agent-os Command Line'
type: specification
created_date: '2026-11-21T00:18:00Z'
updated_date: '2026-11-24T10:53:00Z'
tags:
  - design
  - cli
  - agent-os
  - v1
---
> [!NOTE] Review note (2026-11-21)
> Reviewed against Organization Data contracts, ADR-0014, ADR-0028, ADR-0029, and `doc-1` (Vision).
> - **Stale §9 ('What the code does today'):** `K1` (`crates/org-command`) is already implemented on `v1`, though `apps/agent-os` command conversion and live office RPC rows arrive incrementally in `A5` (`TASK-35`), `B2`/`B3` (`TASK-34`, `TASK-36`), `C2`/`C3`/`C5` (`TASK-40`, `TASK-41`, `TASK-43`), `B6`/`B7` (`TASK-44`, `TASK-45`), `B5` (`TASK-48`), and `C7` (`TASK-50`).
> - Aligned with `doc-1` CLI-first kernel architecture and selective MCP posture.

# The agent command line

How `agent-os` is shaped: one grammar, one answer format, one table of commands, and
which commands milestone one and milestone two deliver. It merges the owner's
[REQ-08](../specs/doc-59%20-%20REQ-08-agent-os-CLI-and-Kernel-Primitives.md) into the selected
architecture. The review of that requirement, with the decisions taken and the
questions for the owner, is at the end of the requirement document. The increments are
in the [implementation plan](../doc-5%20-%20v1-Implementation-Plan.md); the first is
[K1](../doc-5%20-%20v1-Implementation-Plan.md#k1--the-command-contract).

This document owns the command line's conventions and the list of commands. It does
not own what a command means: each row of section 3 names the contract that does.

## 1. The shape in one page

- **`agent-os` is the only way an agent reaches the product**
  ([non-negotiable 7](../doc-7%20-%20Architecture-Non-Negotiables-and-Who-Decides-What.md)). Every call goes to the
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
  ([non-negotiable 9](../doc-7%20-%20Architecture-Non-Negotiables-and-Who-Decides-What.md)).

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

- A resource and a verb are single lower-case words. A hyphen may join two words where
  one will not do; no command needs it today.
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
- **Every verb is a verb, and the common ones are the same everywhere,** so that an
  agent can guess: `list`, `inspect`, `create`, `update`. A resource has its own verb
  only for an action the common ones do not say (`send`, `ask`, `reply`, `transition`,
  `assign`, `hire`, `push`).
- **One resource for one kind of record.** A name is the word the data contract and the
  office's calls already use, so the command an agent types, the call on the wire and
  the record in the file read the same.

Options every command takes:

| Option | On | Meaning |
| --- | --- | --- |
| `--format json\|yaml\|compact` | all | How the answer is printed (2.3). Default `json` |
| `--fields <a,b,c>` | all | Keep only these fields of `data`, for one record or for each item of a list; a dot reaches into a nested field. The record's own field order is kept |
| `--limit <n>`, `--cursor <token>` | lists | Page size, 1 to 200 (default 20), and where to continue; the next token is `meta.next_cursor`. A size outside 1 to 200 is `invalid_request` |
| `--schema` | all | Print the command's input and output schema and stop (2.5) |
| `--help` | all | Print the command's flags in plain text and stop. It is read only where a flag is read: after a flag that takes a value it is that flag's value (`--body --help` gives the body the text `--help`). Where it is read, help is printed whatever else is wrong with the line |
| `--json '<object>'` | commands with input | The whole input, as one JSON object |
| `--input-file <path>`, `--stdin` | commands with input | The content, read as bytes; or, with `--input-format json` or `yaml`, the whole input (2.2) |
| `--dry-run` | writes | Decide and report; change nothing (2.6) |
| `--idempotency-key <key>` | writes | A key the caller chooses, for safe retries (2.7) |
| `--if-match <revision>` | writes to a record with a revision | The revision the caller last saw (2.7) |
| `--durable` | writes | Answer only after the drive has it ([ADR-0014](../../decisions/decision-14%20-%20ADR-0014-Acknowledgement-classes-and-the-agent-os-latency-target.md) decision 4) |

### 2.2 Input that survives a shell

A command's input is one object; `--schema` shows it. There are three ways to give it,
and they combine:

- **Flat flags** for short values: `--status in_progress`.
- **`--json '<object>'`** for the whole object, or the part the flags do not give. A
  field given both ways with different values is `invalid_request`. So is a name given
  twice in one object, in `--json`, in a JSON file or in YAML: no reader is left to
  choose which of the two values counts.
- **`--input-file <path>` or `--stdin`** for the command's **content field**. A command
  that carries long text declares exactly one content field: a message's body, a
  task's description, a comment's body. The bytes are taken unchanged as UTF-8, so
  Markdown and YAML with quotes, backticks, dollar signs and new lines need no
  escaping. Short content can still be given by the field's own flag or in `--json`. A
  command with no content field refuses both flags. The two cannot be given together,
  and bytes that are not valid UTF-8 are refused with `invalid_request`.
- **`--input-format json` or `--input-format yaml`** makes the file or standard input
  the whole input object instead of the content field. This is how an input with
  structure, such as an ask with several questions, is given without quoting anything
  for the shell. The default, `text`, is the content field.

**What a command reads is bounded.** Text and JSON from `--input-file` or `--stdin`,
and the value of `--json`, are at most 1 MiB (1,048,576 bytes). YAML is at most 64 KiB
(65,536 bytes), the bound of the YAML reader. Input over the limit is `invalid_request`
with the limits in the hint, and nothing is sent. The limit is part of the `Sources`
contract: the library asks for at most that many bytes, and an implementation reads no
more than one byte past it, however long the file or the stream is. `--help` states the
numbers.

**A whole number is the same however it is given.** A flag and `--json` read the same
range (a signed or an unsigned 64-bit integer), and a number written with no
fractional part, such as `2.0` in JSON or YAML, is the integer 2, as JSON Schema's
`integer` says. The office receives 2.

`agent-os` reads the file itself, as the agent, inside the agent's isolation. The
content travels in the request. **The office never opens a path that a caller names.**
A command reads standard input only when `--stdin` is given, and no command ever
prompts.

### 2.3 The answer

Every command prints exactly one envelope on standard output and nothing else: no
colour, no progress, no second line. There are two stated exceptions: `--help` prints
plain text, and `--format compact` prints `data` on success, without the envelope.

```json
{"ok":true,"data":{"reference":"T-B37","status":"in_progress","revision":4},"meta":{"status":"completed","operation_id":"0193…","ack":"committed"},"error":null}
```

```json
{"ok":false,"data":null,"meta":{"operation_id":"0193…"},"error":{"code":"conflict","message":"T-B37 changed since revision 3.","hint":"Read the task again and repeat the change against its current revision.","retry_with":{"if-match":4}}}
```

- **`ok`** is true when the command did what was asked or reached one of the stated
  waiting outcomes below. It is false when the command was refused or failed.
- **`data`** is the result: one record, or a list. Field order is fixed and lists have
  a stated order, so the same state prints the same bytes. **`data` is exactly what
  the command's row declares, in every format.** A field the office answers that the
  row does not declare is dropped, at every depth (a field the row declares as open
  JSON is kept whole). An answer without a required field, or with a field of the
  wrong type, is not printed: the command fails with `unavailable` and exit code 1,
  and the message names the field. For a write the hint says the change may have been
  made, and `meta.operation_id` is reported. A row declares every field `data` can
  hold under any `meta.status`; a field only one outcome carries is optional. The
  contract tests hold an office to the same rule and fail on an undeclared or a
  missing field, so the drop is a guard, not a way to ship one. An office leaves out
  an optional field that has no value; `null` is a value only in a field of open JSON.
- **`meta`** carries what is about the call, not the record: `status` on every
  answer with `ok: true`; the recipient's `presence` on a send or an ask
  ([ADR-0028](../../decisions/decision-28%20-%20ADR-0028-An-agent-is-always-available-a-persons-message-and-a-task-assignment-also-start-it.md) decision 6); `operation_id` and `ack` (`committed` or `durable`) for a write; `revision`
  when the write produced one; `next_cursor` for a list; `dry_run: true` for a dry run.
- **`meta.status`** is one of:

  | Status | Meaning | What the agent does |
  | --- | --- | --- |
  | `completed` | Done. Every read answers this | Continue |
  | `timed_out` | An ask that is not required got no answer within its wait. `data.default` holds the ask's default when it has one | Continue with the default, or without the answer |
  | `withdrawn` | The ask was withdrawn or cancelled | Continue without it |
  | `suspended` | A required ask waited its time and no answer came: a required answer is pending ([ADR-0020](../../decisions/decision-20%20-%20ADR-0020-An-ask-waits-with-a-timeout-a-required-ask-suspends-the-agent-until-it-is-answered.md) decision 5). `data` names what is awaited and from whom | Close out background work and end the turn. The office stops the agent and starts it again, in the same session, with the answer |
  | `waiting` | A question is still open and the short hold of this command has ended ([ADR-0020](../../decisions/decision-20%20-%20ADR-0020-An-ask-waits-with-a-timeout-a-required-ask-suspends-the-agent-until-it-is-answered.md) decision 15). `data` names the question and the time left of its wait | Continue with other work, or call `msg wait --ask <id>` |
  | `gate_required` | The change needs an approval before it takes effect. No command returns it in milestones one and two: a hire has no approval ([ADR-0029](../../decisions/decision-29%20-%20ADR-0029-Only-someone-who-may-hire-can-hire-there-is-no-approval.md)). It stays in the contract for a later approval | Continue with other work; the outcome arrives as a message |

  `org-command` has the six.

  These are the requirement's `SUSPENDED` and `GATE_REQUIRED`, and the four results of
  an ask in ADR-0020.
- **Formats.** `json` prints the envelope on one line. `yaml` prints the same envelope
  as YAML, with long text as block text; it is the format for reading a document's
  body. `compact` prints `data`: one line for each item of a list, the chosen
  fields separated by tabs, or `name: value` lines for one record. A failure prints the
  JSON envelope in every format.
- **A `compact` value is one cell on one line.** Text prints as it is. It prints as a
  JSON string when it is empty (`""`), is the word `null`, starts with a double quote,
  reads as `meta:` (in any case, after white space or characters that do not show), or holds a tab, a new line, another control character or a Unicode line or paragraph
  separator (U+2028, U+2029); those characters are then written as escapes, inside a
  list or an object too. `null` prints as `null`. In a list, a field an item does not
  have is an empty cell, so empty text, `null` and an absent field are three different
  cells. A number, `true`, `false`, a list and an object print as JSON; text that
  reads like a number, `true`, `false`, a list or an object is not told apart from
  that value, and the schema says which it is. A name that is not ASCII letters, digits, `_`, `-` and `.` prints as a JSON string,
  and so does a field named `meta` in any case.
- **`compact` prints `meta` only when the caller cannot act without it.** When
  `meta.status` is not `completed`, or `meta.next_cursor` is present, the last line is
  `meta: ` followed by `meta` as one line of JSON. Otherwise `data` is printed alone.
  So a page with another after it ends with its cursor, and a `suspended` or
  `gate_required` answer says so. A line that starts with a bare `meta:` is only ever
  this line: a field named `meta` prints as `"meta": `, and text that reads as
  `meta:` prints as a JSON string, so no record or cell begins with the bytes `meta: `. The
  guarantee is on bytes, not on what shows: text that only looks like it (a zero-width joiner or
  a direction mark in front, an invisible character inside the word, a letter from another
  script) prints as it is. A reader finds the call's line by matching the exact bytes `meta: `
  at the start of the last line, never by how a line looks.
- **Exit code.** 0 when `ok` is true, 1 when it is false, 2 when the command line
  itself could not be read. The envelope is printed in all three cases. The command
  line is read first: a line that cannot be read exits 2 without contacting the office,
  whether or not there is a binding.

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
with the same `--idempotency-key`, which `retry_with` carries), `operation_reused` (the key was used for a different
change; use a new key), `invalid_request` (the hint lists the valid flags or values).

### 2.5 Finding out without loading everything

- **At launch an agent receives a short catalogue,** not the schemas: one line for each
  command, generated from the table, at most 4 KiB. It reaches the agent as its own
  input beside the processes that apply ([processes UX](../specs/doc-54%20-%20Spec-Organization-Processes-and-Standards-UX.md),
  PROC-28).
- **`agent-os --help`** lists the resources; `agent-os <resource> --help` lists its
  verbs; `agent-os <resource> <verb> --help` lists the flags. Plain text.
- **`agent-os <resource> <verb> --schema`** prints, in an envelope, the command's
  `input` and `output` as JSON Schema (draft 2020-12), its `content_field`, the error
  codes it can return and its acknowledgement class. It makes no call to the office.
- **`agent-os skill list` and `skill read`** give the product's built-in guidance on
  using the commands ([processes design](doc-34%20-%20Design-Organization-Processes-and-Standards.md), decision 4), on demand. How an
  agent is told to behave, and the skills there are, is the
  [agent instructions](doc-27%20-%20Design-Agent-Instructions-Assembly.md) design.

The schemas are produced at build time from the same types the parser fills. A test
feeds every command's example input through both the schema and the parser, so the two
cannot drift.

### 2.6 `--dry-run`

A dry run goes to the office like the real call. The office does everything the real
call does up to the point of commitment: is the caller allowed, is the input valid,
does the stated revision still hold, does a rule that the code enforces refuse it, and
does the change apply to the projection. It then undoes all of it and answers with
`meta.dry_run: true`. It changes nothing: no file, no journal entry, no receipt.

In the [office store](doc-66%20-%20Design-Office-Store-Architecture.md) the writer takes a mark, runs the decision and
applies the entry to the projection, and a failure at either step undoes to the mark. A
dry run is those two steps followed by the same undo, without framing or appending an
entry. The store gains one entry point for it, added with the first writing command on
the new store (B2). There is no second copy of any rule. Three things follow:

- **Nothing is used up.** A number, a reference or a position the decision took is
  returned by the undo. B2 proves it (CLI-4).
- **The answer has the shape of the real one, without what only a real change mints:**
  no new ID, no reference, no revision.
- **A dry run takes no `--idempotency-key`.** The two together are `invalid_request`.
- **Fail-closed when unimplemented:** write rows whose office RPC does not yet implement server-side dry-run validation (including `msg`, `channel`, `status`, `task`, `checklist`, `task-note`, `repo`, `worktree`, `hr`/`role`/`agent`, and `process` writes) MUST refuse `--dry-run` with `unsupported` before invoking any office RPC, never executing a live mutation (`TASK-95`).

This is the requirement's `grant check`. It is not a second command.

### 2.7 Safe retries and stale writes

- **`--idempotency-key`** is a key the caller chooses: up to 64 letters, digits and
  hyphens. Every change carries an operation ID
  ([non-negotiable 5](../doc-7%20-%20Architecture-Non-Negotiables-and-Who-Decides-What.md)), and the key is what a
  caller holds of it. For the same caller, repeating a call with the same key and the
  same input changes nothing further and returns the first answer; that is also how an
  agent that lost an answer gets it back. The same key with different input is
  `operation_reused`. A write that carries no key is given one by `agent-os`, reported
  in `meta.operation_id` on success and on failure. How a key becomes the stored
  operation ID is B2's decision.
- **Where the key comes from.** The library makes no randomness. It owns a contract,
  `KeySource`, that the executable supplies, and asks it for one key for a write that
  is not a dry run and carries no `--idempotency-key`, after the command line is read
  and before the office is called. A read, a dry run, `--help`, `--schema` and a line
  that cannot be read ask for none. The key goes to the office with the request.
  `meta.operation_id` is the operation ID the office reports, and the key when the
  office reports none. For `unavailable` and `not_ready` the library puts the key in
  `retry_with` as `idempotency-key`, beside what the office already said to change;
  the key there is always the one the request carried. A
  key source that gives something that is not a key fails the command before anything
  is sent.
- **`--if-match`** is the record's revision as the caller last read it. Where the data
  contract requires an expected revision (assignment, for one), the flag is required. A
  stale revision is `conflict`, and `retry_with` carries the current one.

### 2.8 Who is calling

The office starts each run and gives it a binding
([ADR-0003](../../decisions/decision-3%20-%20ADR-0003-Agent-messaging-is-office-first.md) decision 12,
[agent sessions](doc-29%20-%20Design-Agent-Sessions-and-Runtime-Isolation.md)). At launch it sets:

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
([identity and numbering](../organization-data/doc-80%20-%20Organization-Data-06-—-Identity-and-Numbering.md)); one that
names another project is checked like any other call.

`agent-os` refuses to run without a binding, except for `--help`, `--schema` and
`--version`, which make no call. There is no `--as`, no `--office`, and no
variable that names the caller.

### 2.9 Several commands in one turn

- **In a shell.** One line of JSON for each command, so `&&`, pipes and `jq` work.
- **`agent-os session attach`** is the long-running mode of ADR-0014 decision 5. It
  binds once, then reads one command for each line of standard input and writes one
  envelope for each line:

  ```json
  {"command":"msg inbox","input":{"limit":5},"options":{"fields":"id,from,subject"}}
  ```

  It is the same table, parser and envelope, and an option is spelled as its flag. It
  answers every line: a line that fails does not stop the next. An agent reaches it by
  sending several commands in one shell call, and pays for one process start and one
  bind. The target of ADR-0014 is measured through it. A single one-shot command is
  bounded by the start of its process, as that decision says.

### 2.10 Where MCP is used

- **No product primitive is an MCP tool.** Asking a question, reading a task or
  sending a message is always a command. This closes the open point of the processes
  design and replaces the `ask_user` tool route in the alignment of
  [REQ-04](../specs/doc-55%20-%20REQ-04-Channel-Messaging-and-Question-Components.md).
- **MCP is for connectors to things outside the product** (a browser, a language
  server, a third-party service), granted to an agent through its execution
  configuration. Milestones one and two grant none: the MCP configuration the office
  writes for a run is empty, and an adapter refuses a start that reports any server.
- **The thin MCP wrapper for a runner without a shell is not built.** Every agent in v1
  runs where its office runs, with a shell. If such a runner is ever selected, the
  wrapper is generated from the command table and adds nothing to it.

### 2.11 Which commands are fast

Owner, 2026-11-18: the common commands are fast; not every command has to be.

- **The common commands** are the ones an agent runs all day: every read from its own
  office, and every write in the committed class of
  [ADR-0014](../../decisions/decision-14%20-%20ADR-0014-Acknowledgement-classes-and-the-agent-os-latency-target.md) decision 3.
  In this catalogue: `msg inbox`, `msg read`, `msg send`, `msg reply`, `msg done`,
  `msg defer`, `status set`, `session inspect`, `task list`, `task inspect`,
  `task create`, `task comment`, `task watch` and `checklist check`.
- **Their target.** One command run by itself answers in single-digit milliseconds; it
  is bounded by the start of its process. Inside `session attach` the target is a
  median of 1 ms and a 95th percentile of 2 ms on the reference Mac (ADR-0014
  decision 1).
- **Everything else has no latency target:** a write that waits for the drive (a status
  change, an assignment), a command that waits for a person (`msg ask`, `msg wait`), one that
  reaches the network (`repo push`), and rare ones (`hr hire`, `repo attach`).
- **No command holds its caller longer than the short hold,** 90 seconds unless
  changed ([non-negotiable 18](../doc-7%20-%20Architecture-Non-Negotiables-and-Who-Decides-What.md)). A longer wait
  returns `waiting` and is continued by another call.
- **What keeps the common path short.** Before its call, `agent-os` does nothing the
  call does not need: it loads no schema and no catalogue, reads no file except one
  named with `--input-file`, and makes no extra round trip to the office. A change that
  slows a common command is a regression.

## 3. The commands, and the contract behind each

"With" names the increment that delivers the command. A person's command is one the
office accepts only from a person in the stated position; an agent that calls it gets
`not_authorized` with a hint naming who may.

### 3.1 The run and what it may do

| Command | What it answers | Contract | With |
| --- | --- | --- | --- |
| `session inspect` | The organization, office, employee, home team and the leads above it, the project and task of this run, the session and its state, and what it is waiting on | [Agent sessions](doc-29%20-%20Design-Agent-Sessions-and-Runtime-Isolation.md) | C3 |
| `session attach` | The long-running mode (2.9) | ADR-0014 decision 5 | B6 |
| `grant list` | What the office holds about this run's reach: the teams the employee leads and whether it may hire into them, the project and task, the worktrees and other writable places, the isolation mode, the tools and connectors of its execution configuration | [Employees](../organization-data/doc-78%20-%20Organization-Data-04-—-Agent-Structure.md), [ADR-0015](../../decisions/decision-15%20-%20ADR-0015-Agent-isolation-is-a-selectable-mode-with-a-dedicated-OS-user-provisioned-at-setup.md), [execution](../organization-data/doc-88%20-%20Organization-Data-11-—-Model-Profiles-and-Agent-Execution.md) | C3 |

`session suspend` and `session resume` are not commands. Suspension is the `suspended`
outcome of a required ask, and the office performs both steps (ADR-0020 decisions 6 and
8). `grant check` is `--dry-run` (2.6).

### 3.2 The organization and its people

| Command | What it does | Contract | With |
| --- | --- | --- | --- |
| `org inspect` | The organization, its offices and its top team | [Organization](../organization-data/doc-75%20-%20Organization-Data-01-—-Organization-Structure.md) | B2 |
| `team list`, `team inspect` | The teams; one team (default: the caller's home team) with its lead, its members and their presence | [Organization](../organization-data/doc-75%20-%20Organization-Data-01-—-Organization-Structure.md), ADR-0003 decision 14 | B2 |
| `agent inspect`, `person inspect` | One employee's card: team, role, lead, status. The two are one call under the name that fits; each answers for either kind and says which it is | [Employees](../organization-data/doc-78%20-%20Organization-Data-04-—-Agent-Structure.md) | B2 |
| `status set`, `status clear` | The caller's own status line (`status.set`, committed) | ADR-0003 decision 14 | B6 |
| `project list`, `project inspect` | The projects the caller can see; one project with its team, repositories and processes | [Projects](../organization-data/doc-76%20-%20Organization-Data-02-—-Project-Structure.md) | B3 |
| `team create`, `team update`, `person add` and the like | A person's commands: the owner changes structure. B2 names them under this grammar | [Who may change what](../organization-data/doc-78%20-%20Organization-Data-04-—-Agent-Structure.md) | B2 |
| `project create` | A person's command: the owner | [Who may change what](../organization-data/doc-78%20-%20Organization-Data-04-—-Agent-Structure.md) | B3 |
| `role list`, `role inspect` | The organization's roles; one role's body and requirements. Reading only: no command writes a role in v1 | [Roles UX](../specs/doc-47%20-%20Spec-Agent-Roles-Directory-UX.md) | C2 |
| `hr hire` | Hires a new agent. Only a caller who may hire into the team can use it: the owner, or an employee with Allow hiring, into the teams it leads and the teams under them ([ADR-0029](../../decisions/decision-29%20-%20ADR-0029-Only-someone-who-may-hire-can-hire-there-is-no-approval.md)). Anyone else gets `not_authorized` with the hint "You don't have access to hire. Ask your team lead.", and nothing is recorded. With every required field given (`--team`, `--role` or the fields of the proposal, `--reason`) the call is its own confirmation and the office provisions the agent. With words only, the HR lead drafts a proposal and the call returns the request in `drafting`; the draft arrives as a message, and `hr hire --id <request>` confirms it. When the caller gave only words and the HR lead cannot run, it creates nothing and is `unavailable`, with a hint to fill in the fields. There is no approval and nothing waits for anyone | [The hire flow](../specs/doc-45%20-%20Spec-Hiring-Profiles-and-Team-Moves-UX.md) (STAFF, HRL, APPR) | C2 |

### 3.3 Messages, questions and approvals

There is one family, `msg`, for one conversation and message model
([messaging](../organization-data/doc-90%20-%20Organization-Data-15-—-Messaging-and-Chat.md)). Whether a message goes to a
person, an agent or a channel is a matter of whom it is addressed to, not of which
command is used. The names are the ones the office's calls already have (`msg.send`,
`msg.ask`, `msg.reply`), so nothing is renamed between the command and the wire.

| Command | What it does | Contract | With |
| --- | --- | --- | --- |
| `msg send` | Sends a message `--to` a person, an agent or a channel, with a kind where the chat rules ask for one: `--kind handoff`, or `--kind announcement --reason <reason>`. Writing `@handle` in the text mentions that person or agent; the answer lists the mentions the office recorded, and any `@handle` that did not reach its employee. A hand-off or a mention sent to an idle agent starts a turn for it (C8) | [Chat UX](../specs/doc-40%20-%20Spec-Chat-Channels-and-Structured-Asks-UX.md) section 5; [ADR-0023](../../decisions/decision-23%20-%20ADR-0023-An-agents-ask-hand-off-or-mention-and-an-answer-to-its-own-question-start-a-turn-for-an-idle-agent.md) | B6 |
| `msg ask` | Asks one question, or up to eight at once, with the `ask_user` schema: `choice` (single or multiple), `yes_no` or `free_text`, each with an optional default; `--required`, `--wait`. `--to` names a person or an agent, or a team or project, in which case the office addresses it to the lead of that team or of the project's accountable team. It holds for at most the short hold (90 seconds unless changed) and returns `completed` with the answer, `waiting` with the question's ID when the answer has not come yet, `timed_out` when the wait of the ask has ended, or, when required, `suspended` (ADR-0020 decisions 15 to 17). The ID is in every answer, so a repeated call with the same idempotency key never asks twice. `--to` a channel with nobody named is a question that does not hold, and `--required` is refused there: it returns the question's ID at once and opens a watch on the responses, which the channel's watcher feeds back to the asker's inbox; `meta` says whether the question is watched, and why not when it is not (C9). An ask to an idle agent starts a turn for it (C8) | [REQ-04](../specs/doc-55%20-%20REQ-04-Channel-Messaging-and-Question-Components.md), ADR-0020, [ADR-0023](../../decisions/decision-23%20-%20ADR-0023-An-agents-ask-hand-off-or-mention-and-an-answer-to-its-own-question-start-a-turn-for-an-idle-agent.md); [employees](../organization-data/doc-78%20-%20Organization-Data-04-—-Agent-Structure.md) | B7 |
| `msg reply` | Replies in a thread (B6); answers an ask, naming each question and option by its ID (B7). A person and an agent answer the same way | Messaging; ADR-0020 | B6, B7 |
| `msg accept` | `--id <message>`: accepts a hand-off addressed to the caller | Chat UX (CHAT-18) | B6 |
| `msg withdraw` | Withdraws an ask the caller made | ADR-0020 decisions 7 and 12 | B7 |
| `msg inbox`, `msg read` | Everything addressed to the caller, unread first, each entry with the reason it is there (`addressed`, `mention`, `thread_reply`, `answer_candidate`) and, for an answer, the question it answers; `--because <reason>` and `--thread <id>` narrow it. One message with its thread. Nothing else from a channel is in the inbox | ADR-0003 decisions 10 and 11; [ADR-0024](../../decisions/decision-24%20-%20ADR-0024-An-agents-inbox-holds-what-is-addressed-to-it-and-each-channel-has-a-watcher-that-feeds-responses-back-to-the-agents-who-asked.md) | B6 |
| `msg list` | The messages of one channel, conversation or thread, when the agent chooses to read it | Messaging (`msg.list`) | B6 |
| `msg done`, `msg defer` | `--id <message>`: the caller's own triage state. `msg done` on the caller's own channel question closes it and ends its watch | ADR-0003 decision 11; ADR-0024 | B6 |
| `msg wait` | Holds for at most the short hold until something arrives; `--ask <id>` (or `--thread <id>`) waits for the answer to one question and returns `completed`, `waiting`, `timed_out` or `suspended` as `msg ask` does | [Agent sessions](doc-29%20-%20Design-Agent-Sessions-and-Runtime-Isolation.md) (holds are capped); ADR-0020 decision 15 | B6 |
| `channel list`, `channel join`, `channel leave` | The channels the caller may see; joining and leaving a manual channel | Chat UX (CHAT-19) | B6 |
| `channel summarize` | `--channel <channel> --about "<what is needed>"`: answers a question about a channel's recent messages with a short text and the IDs of the messages it rests on, so that the agent does not load the channel into its context. It reads exactly what `msg list` would return to the caller, at most the latest 200 messages, and its answer is marked as derived from channel content | [ADR-0024](../../decisions/decision-24%20-%20ADR-0024-An-agents-inbox-holds-what-is-addressed-to-it-and-each-channel-has-a-watcher-that-feeds-responses-back-to-the-agents-who-asked.md) decision 18 | C9 |

**Approvals in milestone one.** There are none. A hire is made by someone who may
hire and is refused for everyone else
([ADR-0029](../../decisions/decision-29%20-%20ADR-0029-Only-someone-who-may-hire-can-hire-there-is-no-approval.md)). An agent that needs a go-ahead
for anything asks a required yes-or-no question with `msg ask`; that is a question,
not a recorded approval. `gate open` and `gate status` are in section 6.

**How an agent gets the answer to a channel question.** It does not read the channel.
It asks with `msg ask --to <channel>` and keeps the ID. A reply in the question's thread
comes to its inbox as `thread_reply`. A message posted in the channel outside the
thread is judged by the channel's watcher, a function of the office that runs a cheaper
model once for all the open questions of that channel, and comes to its inbox as
`answer_candidate` when it responds to the question. If the agent is still working, it
sees the response with `msg inbox` or `msg wait --thread`. If it has ended its turn, a
thread reply, or a confident match on a person's message, starts a turn for it in the
same session.

**Asking structured questions, and answering them.** The asker is an agent; the
answerer may be a person in the browser or another agent on the command line. Both give
the same answer record, and the office checks it against the questions.

One question fits in flags:

```sh
agent-os msg ask --to sam --question-type yes_no --prompt "Rename Open organization?" --default-value no
agent-os msg ask --to sam --question-type choice --prompt "Which environment first?" \
  --option dev=Development --option prod=Production --default-value dev
```

Several questions go in a file, with nothing to escape:

```yaml
# ask.yaml
to: sam
required: true
body: Before I set up the release pipeline I need four decisions.
questions:
  - id: env
    question_type: choice
    prompt: Which environment first?
    options:
      - {id: dev, label: Development}
      - {id: prod, label: Production}
    default_value: dev
  - id: regions
    question_type: choice
    allow_multiple: true
    prompt: Which regions?
    options:
      - {id: eu, label: Europe}
      - {id: us, label: United States}
      - {id: ap, label: Asia Pacific}
  - id: downtime
    question_type: yes_no
    prompt: Is five minutes of downtime acceptable?
  - id: notes
    question_type: free_text
    prompt: Anything else I should know?
    optional: true
```

```sh
agent-os msg ask --input-file ask.yaml --input-format yaml
```

A person sees one form and sends once. An agent that receives the ask is given the
questions with their IDs and option IDs, at the start of its turn or with `msg read`,
and answers in one call:

```sh
agent-os msg reply --id <ask> --answer env=prod --answer regions=eu --answer regions=us \
  --answer downtime=yes --answer notes="Use the blue cluster."
```

With one question the ID may be left out: `--answer yes`. The asker's `msg ask` returns
one result, keyed by question:

```json
{"ok":true,"data":{"by":"sam","answers":{"env":{"option_ids":["prod"]},"regions":{"option_ids":["eu","us"]},"downtime":{"option_ids":["yes"]},"notes":{"text":"Use the blue cluster."}}},"meta":{"status":"completed"},"error":null}
```

The rules:

- **Answered together.** Every question that is not `optional` needs its answer. There
  is no partial answer.
- **No branching.** No question depends on another's answer. An agent that needs to
  branch asks again with what it learned.
- **Checked by the office,** at the ask and at the answer: unique IDs, options that fit
  the question type, one option where only one is allowed, no unknown question or
  option. A refusal is `invalid_request`, and its hint lists the valid IDs.
- **Defaults are for each question.** When an ask that is not required times out, the
  asker receives the default of each question that has one.
- **A question to a channel with nobody named is free text only.** A structured answer
  needs one answerer.

### 3.4 Tasks

| Command | What it does | Contract | With |
| --- | --- | --- | --- |
| `task create` | Creates a task, bug or request (`--type`), in the run's project unless one is named | [Tasks](../organization-data/doc-77%20-%20Organization-Data-03-—-Features-and-Work.md), [ADR-0019](../../decisions/decision-19%20-%20ADR-0019-Task-numbers-are-minted-by-the-creating-office.md) | B5 |
| `task list`, `task inspect` | Tasks by project, assignee, status or type; one task with its history | [Operational data](../organization-data/doc-79%20-%20Organization-Data-05-—-Operational-Data-and-Private-Notes.md) | B5 |
| `task update` | Title, description, priority, effort | The same | B5 |
| `task transition` | `--task <reference> --status <status>`, with `--reason` for `blocked`: changes the status, from the closed list | The same; [work items UX](../specs/doc-49%20-%20Spec-Work-Items-and-Tasks-UX.md) | B5 |
| `task assign` | Changes the assigned team and assignee together, at a stated revision | [Assignment and history](../organization-data/doc-84%20-%20Organization-Data-12-—-Task-Assignment-and-History.md) | B5 |
| `task comment`, `task watch` | Adds a public comment, read by everyone on the task; starts or stops watching | Operational data | B5 |
| `task note add`, `task note list`, `task note update`, `task note remove` | The caller's private notes on a task (`--task`, `--text`, `--id`): its plan, reasoning and test notes. Read by the author and by people, never by another agent ([ADR-0031](../../decisions/decision-31%20-%20ADR-0031-Any-employee-changes-tasks-a-private-note-is-read-by-its-author-and-by-people.md)) | [Operational data](../organization-data/doc-79%20-%20Organization-Data-05-—-Operational-Data-and-Private-Notes.md#private-notes-attached-to-tasks) | L8 |
| `checklist add`, `checklist check`, `checklist update`, `checklist remove` | A task's checklist | Work items UX (CHK) | B5 |

### 3.5 Processes, standards and guidance

| Command | What it does | Contract | With |
| --- | --- | --- | --- |
| `process list` | The processes and standards that apply to the caller's current work, or to a named task, with where each comes from and which must be followed. `--kind process` or `--kind standard` keeps one kind | [Processes design](doc-34%20-%20Design-Organization-Processes-and-Standards.md) (`process.effective`), PROC-8 | C5 |
| `process inspect` | One document's full text by `--key` or `--id` (`content`, `total_bytes`, `next_offset`, `parent`, paginated via `--id <document.id> --revision <document.revision> --offset <next_offset>` whenever `next_offset` is present); with `--why` (`--key` only), the resolution `chain` and `winner` from organization to team to project | The same (`process.show`, `process.why`) | C5 (`TASK-95`) |
| `skill list`, `skill read` | `skill read --name <key>`: the product's built-in guidance on its own commands, and the skills of the caller's project's development process for the caller's role; each names its source | Processes design, decision 4; [development processes](doc-32%20-%20Design-Development-Processes-Process-Packages.md#6-how-skills-reach-an-agent) | C5; L12 for a development process's skills |
| `process create`, `process update` | Add an organization process or standard at the scope of a project of the caller's own team, or change one of that project's. Never at team or organization scope, never non-overridable, never a key inherited from above | [Processes design](doc-34%20-%20Design-Organization-Processes-and-Standards.md#5-changing-processes), decision 8 (PROC-36) | L12 |
| `dev-process inspect` | The development process of the caller's project: its summary, stages and tabs, and the caller's role in it. Reading only: a person creates, edits and imports one in the browser | [Development processes](doc-32%20-%20Design-Development-Processes-Process-Packages.md) | L12 |

Processes and standards are the same kind of document, inherited the same way and
read through the same calls, so they share one resource. A development process is a different
primitive with its own resource; its skills are read with the `skill` commands. `process apply` and
`policy check` are in section 6.

### 3.5a Project documents

The owner's REQ-06 and REQ-07 are in the repository, and these commands are their
form in this grammar ([development processes](doc-32%20-%20Design-Development-Processes-Process-Packages.md#8-the-project-documentation-tree),
[desired state specifications](doc-31%20-%20Design-Desired-State-Specifications-DSS.md#6-the-contract)). They
come with L11 and L13. A call for a project that is not the caller's own is refused
with `other_project`.

| Command | What it does | Contract |
| --- | --- | --- |
| `doc index` | The project's information-architecture index in one read: every section, document and media file with its path, kind, title, status and what it is for | `doc.index` |
| `doc list`, `doc inspect` | A folder or a filtered listing; one document: its latest issued version by default, `--working` for the working text, `--version <n>` for an earlier one | `doc.list`, `doc.read` |
| `doc create`, `doc update`, `doc move` | Start a document from its kind's template, change it, or move it; the index is written in the same accepted change | `doc.write` |
| `doc put`, `doc get` | Store a diagram or an image in the tree; copy one into the run's temporary folder | `doc.upload`, `doc.write`, `doc.read` |
| `doc-section create`, `doc-section update`, `doc-section move` | Add, describe or reorder a section of the tree | `doc.write` |
| `doc issue` | Fix the working text as the next version, with a note and the versions it was derived from | `doc.issue` |
| `doc diff`, `doc versions`, `doc status` | Differences between versions or the working text; a document's versions with their tasks and progress; every versioned document of the project | `doc.diff`, `doc.versions`, `doc.status` |
| `doc link-code`, `doc unlink-code`, `doc code-links` | Say where a document is realized in code; list the links of a document or the documents that link a file; `--check` marks stale ones | `doc.link_code`, `doc.code_links` |

`task create` and `task update` take `--realizes <document>@<version>`. `hr hire` takes
`--role <key>`, and `role inspect` also prints what a hire proposal for the role needs.

### 3.6 Repositories and worktrees

| Command | What it does | Contract | With |
| --- | --- | --- | --- |
| `repo list` | The repositories of a project with their integration branches | [Git design](doc-35%20-%20Design-Projects-Git-and-GitHub.md) | C7 |
| `repo attach` | A person's command: links a registered repository to a project, with its integration branch | The same; [projects](../organization-data/doc-76%20-%20Organization-Data-02-—-Project-Structure.md) | C7 |
| `worktree list` | The caller's worktrees with task, repository, branch, path and state | The same, "A worktree per task and repository" | C7 |
| `repo push` | `--task <reference>`, with `--repo` when the task has several: asks the office to push the task's branch (`git.push`). The agent holds no credential | The same | C7 |

`worktree create` and `worktree cleanup` are not commands. The office creates a task's
worktrees when a turn is started for that task, before the run is launched, because a
run's writable places are fixed at launch. So an agent can write code only for the task
of its turn: work it finds for another task becomes a task, done in a turn of its own. It removes them after
the task closes, under the Git design's own checks. `task transition` reports the state
of each repository's worktree.

### 3.7 What today's commands become

| Today | Becomes |
| --- | --- |
| `msg send`, `msg ask`, `msg reply`, `msg inbox`, `msg read`, `msg done`, `msg defer`, `msg wait` | The same names, in the grammar of section 2: named flags only, the envelope, the office for every call |
| `whoami` | `session inspect` |
| `who` | `team inspect`, `agent inspect`, `person inspect` |
| `status` | `status set`, `status clear` |
| `profile set`, `msg register`, `msg deregister` | Deleted. An agent exists only through a hire |
| `msg notify`, `msg due`, `relay`, the file queue, `--as`, `$ORG_AGENT`, `--office` | Deleted with the file queue (B6) |

Nothing is kept beside its replacement
([non-negotiable 14](../doc-7%20-%20Architecture-Non-Negotiables-and-Who-Decides-What.md)).

### 3.8 Names that differ from the requirement

The requirement's own rule is one uniform grammar. These names are changed to keep that
rule, and for no other reason.

| Requirement | Here | Why |
| --- | --- | --- |
| `channel post`, `channel ask`, `consult ask`, `thread list`, `thread reply` | `msg send`, `msg ask`, `msg inbox`, `msg reply` | Owner, 2026-11-18: keep `msg`. One record, one family; the addressee decides where it goes. `channel` stays for the channels themselves |
| `team roster` | `team inspect` | `roster` is not a verb; `inspect` is the verb an agent guesses |
| `hr roles`, `hr inspect-role` | `role list`, `role inspect` | The same, and the thing read is a role. `hr hire` stays |
| `policy list` | `process list --kind standard` | A standard is a document of the same model as a process, so it is read through the same two commands |
| `process catalog` | `process list` | The same. `catalog` is left free for a catalogue of Process Packages |
| `session suspend`, `session resume`, `grant check`, `worktree create`, `worktree cleanup`, `gate open`, `gate status` | Not commands | Sections 3.1, 3.3, 3.6 and 6 |

## 4. Milestone one: one office on a Mac

The contract comes first and is small. After it, each increment adds its own rows to
the table.

| Step | Increment | Delivers for the command line |
| --- | --- | --- |
| 1 | **K1** (new) | The shared library: the command table, the parser, the three ways to give input, the envelope and its three formats, `--fields`, `--limit` and `--cursor`, `--help`, `--schema`, exit codes, and the contract tests of section 8, run against a test office that is a stub inside the test process. No command of today is converted |
| 1 | A5 | `agentic-org` uses the library for a signed-in person, with the first two rows on the real path: `org inspect` and one of the owner's writes, served by today's office |
| 2 | B2, B3 | On the new store: `org inspect`, `team list`, `team inspect`, `agent inspect`, `person inspect`, the owner's commands for teams and people, `project list`, `project inspect`, `project create`. With the first write: `--dry-run`, `--idempotency-key`, `--if-match`, `--durable` |
| 3 | C2 | `role list`, `role inspect`, `hr hire` and its refusal for a caller who may not hire |
| 3 | C3 | The binding and the launch variables of 2.8; `agent-os` refuses to run without a binding; `session inspect`, `grant list`; the short catalogue in the agent's instructions; the empty MCP configuration |
| 3 | C5 | `process list`, `process inspect`, for processes and standards alike; `skill list`, `skill read` |
| 4 | B6 | `msg send` with mentions read from `@handle`, `reply`, `accept`, `inbox` with its reasons, `read`, `list`, `done`, `defer`, `wait`; `channel list`, `join`, `leave`; `status set`, `clear`; `session attach`. Every `msg` command goes to the office; the file queue and the relay are deleted |
| 4 | B7 | `msg ask`, answering with `msg reply`, `msg withdraw`, and the `timed_out` and `withdrawn` outcomes |
| 4 | C6 | The `suspended` outcome of a required ask |
| 4 | C8 | No new command: an ask, a hand-off or a mention from an agent, and an answer to the agent's own question, start a turn for an idle agent, and `meta` says whether one started |
| 4 | C10 | No new command: a person's message or mention, and a task assigned to the agent, start it too; the answer to a send or an ask carries the recipient's `presence` |
| 4 | C9 | `channel summarize`; a channel question opens a watch for its asker, and responses arrive in the inbox as `answer_candidate` |
| 5 | B5 | `task` and `checklist` |
| 5 | C7 | `repo list`, `repo attach`, `worktree list`, `repo push` |

At the end of milestone one an agent can find out who and where it is, read what
applies to its work, talk and ask, be suspended on a required answer and resumed with
it, work a task in a worktree and have its branch pushed, with every call in one
grammar.

## 5. Milestone two: Linux

Milestone two adds no command and changes none. It proves the same table on Linux.

| Increment | Delivers for the command line |
| --- | --- |
| X1b | `agent-os` and the shared library take default locations and file reads only from `org-platform` (`dirs`, `fs`). Neither contains a platform branch, and the platform check enforces it |
| X2 | The contract tests that need an office pass on Linux for the commands that exist then |
| X5 | `agent-os` is in the container image's release `bin/`, first on an agent's `PATH`, and the image has a shell. No wrapper |
| X6 | The contract tests run in CI on both systems. The catalogue and every schema are byte-identical on macOS and Linux. One-shot timings are recorded on Linux, and `session attach` timings once B6 has landed; the target of ADR-0014 is held on the reference Mac |

Linux and milestone one advance side by side, so "the commands that exist then" grows
with each merge: a command added by milestone one must pass on Linux in the same
change, or the Linux CI job of X6 fails. The same holds for the binding, which C3
builds: its check that the caller descends from the started process uses the `process`
capability of `org-platform`, on both systems.

## 6. Not in milestone one or two

| Requirement | Why not yet | Where it goes |
| --- | --- | --- |
| `milestone`, `release` | Features, releases and deployments are a later increment | L7, in this grammar |
| `ia`, `fs`, `link` | They are `doc index`, the `doc` commands and `doc link-code` (3.5a), from REQ-06 and REQ-07 | L11 and L13 |
| `process apply`, `block run` | A development process is content that agents read, with no engine: nothing applies a process or runs a block ([ADR-0026](../../decisions/decision-26%20-%20ADR-0026-A-development-process-is-content-the-product-runs-no-methodology.md)). Its role-bound skills are read with `skill list` and `skill read` (3.5) | Not commands |
| `artifact publish`, `artifact drift` | They are `doc issue` and `doc status`: an issued version of a document, and whether the tasks that realize it are finished (3.5a) | L13 |
| `event emit`, `event subscribe` | A development process is content with no engine: nothing in the product acts on a stage or on an event ([ADR-0026](../../decisions/decision-26%20-%20ADR-0026-A-development-process-is-content-the-product-runs-no-methodology.md) decision 2). An agent acts because it was addressed or assigned a task | Not commands |
| `gate open`, `gate status` | An approval is a record with its own rules (3.3). A development process names no approver and grants nothing (ADR-0026 decision 1), so nothing declares a gate | Not commands. A hire has no approval; a required yes-or-no ask for anything else |
| `worktree create`, `worktree cleanup` | The office does both, at task start and after closure (3.6) | Not commands |
| `grant check` | It is `--dry-run` (2.6) | Not a command |
| `session suspend`, `session resume` | They are outcomes the office performs (3.1) | Not commands |
| `policy check` | A written standard cannot be checked by a program. Rules the code enforces are checked by `--dry-run` | With machine-checked policies, if they are added |
| `runner spawn` | Only the office starts a run ([non-negotiable 10](../doc-7%20-%20Architecture-Non-Negotiables-and-Who-Decides-What.md)). An agent gets a colleague running by asking it or handing work to it ([ADR-0023](../../decisions/decision-23%20-%20ADR-0023-An-agents-ask-hand-off-or-mention-and-an-answer-to-its-own-question-start-a-turn-for-an-idle-agent.md)) | Not a command |
| `logs inspect` | The session view is for people in the browser (C4). No rule says which agent may read which session | Later |
| `metrics query` | Usage and costs | L2 |
| A gate resolved by policy alone | Approvals are a person's | Not in v1 |
| The thin MCP wrapper | No runner without a shell | Not built |

## 7. Where the code lives

- **The shared library** holds the table, the parser, the envelope, the formats, the
  schemas and the client side of the session mode. It is the crate `crates/org-command`
  (K1's decision: a crate of its own, so that the contract and its tests need no socket).
  It depends on `org-proto` for the error codes and on `org-docs` for the bounded YAML
  reader, and on nothing that knows a database or an operating system. It owns three
  contracts that the executable supplies: `Office`, the one call a command makes;
  `Sources`, where `--input-file` and `--stdin` read from, each read bounded (2.2); and
  `KeySource`, where the key of a write that carries none comes from (2.7). The connection through
  `org-client` is the executable's implementation of `Office` (A5), and the client side
  of the session mode arrives with B6. The contract checks of section 8 are the crate's
  `testkit` feature, which an increment runs on the table it adds rows to. The checks
  take a function that makes the office to run against; the default is a stub that
  answers each row's example. Against any office they fail when an answer holds a field
  its row does not declare or lacks a required one.
- **`apps/agent-os` and `apps/agentic-org`** stay thin: each chooses how the caller is
  identified and hands the arguments to the library
  ([non-negotiable 15](../doc-7%20-%20Architecture-Non-Negotiables-and-Who-Decides-What.md)).
- **A domain's rows** are declared in the domain's crate, beside the mapping to its
  wire call, by the increment that builds the domain; never in `org-proto`, which
  `org-command` depends on for the error codes and so cannot depend on it. Until
  domain crates exist, the rows are one library module shared by both executables.
  Adding a command touches the table and nothing in the parser.
- **The office** owns authorization, the decision and the dry run. Its wire calls keep
  their own names.
- **The built-in guidance** (`skill`) ships in the release as files the office reads.

## 8. Tests that prove it

| ID | Passing evidence |
| --- | --- |
| CLI-1 | For every row of the table: a positional argument, an unknown flag and a missing required flag are each refused with `invalid_request`, a hint and exit code 2, and nothing reaches the office |
| CLI-2 | For every row, standard output in `json` and `yaml` is exactly one envelope that validates against the envelope's schema, and `json` is one line; `compact` prints `data` on success, with a last `meta:` line only when the command did not complete or there is another page (2.3), and the JSON envelope on failure; `--help` prints text |
| CLI-3 | Every `--schema` output is valid JSON Schema, and every row's example input and recorded output validate against it |
| CLI-4 | `--dry-run` on every writing row leaves the store's positions, numbers, files and receipts unchanged; it reports the refusal the real call reports, including one raised while applying to the projection; with `--idempotency-key` it is refused |
| CLI-5 | The same `--idempotency-key` twice makes one change and returns the same answer; the same key with other input is `operation_reused` |
| CLI-6 | A stale `--if-match` is `conflict` with the current revision in `retry_with`, and the retry with it succeeds |
| CLI-7 | Markdown with quotes, backticks, dollar signs, new lines and non-ASCII text arrives byte-identical through `--input-file`, `--stdin` and `--json` |
| CLI-8 | With standard input closed and no terminal, every row completes; no row reads standard input without `--stdin` |
| CLI-9 | Without a valid binding every call is refused. Changing `AGENT_OS_ORG`, `AGENT_OS_OFFICE`, `AGENT_OS_AGENT_ID`, `AGENT_OS_SESSION_ID`, `AGENT_OS_PROJECT` or `AGENT_OS_TASK` does not change who the office says is calling |
| CLI-10 | Each person's command is refused to an agent by the office when the wire call is made directly, without `agent-os` |
| CLI-11 | The launch catalogue is generated from the table and is at most 4 KiB; a snapshot of the catalogue and of every schema fails the build when a row changes without it, and is identical on macOS and Linux |
| CLI-12 | The MCP configuration written for a run is empty, and a start that reports any MCP server is refused |
| CLI-13 | `session attach` answers each line with the envelope the one-shot command prints |
| CLI-14 | A required ask returns `completed` with an answer that arrives within its wait, and `suspended` when the wait ends without one; an ask that is not required returns `timed_out` with its default; no single command holds its caller longer than the short hold, and returns `waiting` instead; `hr hire` by a caller who may not hire returns `not_authorized` at once |
| CLI-15 | Each common command of 2.11 is measured by itself and inside `session attach`, on the reference Mac and on Linux, with the method recorded; a guard fails the build when one slows past its recorded bound |

K1 makes CLI-1, 2, 3, 7, 8 and 11 pass against a test office. Each later increment
makes them pass for its own rows, and the rest pass with the increment that builds
what they test: CLI-4, 5 and 6 with B2, CLI-9, 10 and 12 with C3, CLI-13 with B6,
CLI-14 with C2, B7 and C6, and CLI-15 with B6 for messages and B5 for tasks.

## 9. What the code does today

`apps/agent-os/src/main.rs` parses arguments by hand, with positional arguments, prints
text unless `--json` is given, and uses several exit codes. Its commands are the left
column of 3.7; the `msg` names stay and their parsing does not. Most write to a file queue; five go to an office when one is named. The
caller is named by `$ORG_AGENT` or `--as` and is not authenticated on the file-queue
path. None of it is kept.
