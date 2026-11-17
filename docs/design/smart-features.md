# Smart features: the copilot

The copilot drafts text for a person to review and confirm. It has no domain write
path. `crates/org-copilot` holds its registry, rendering, validation and limits.
Execution, accounting and budgets follow the data contract:
[execution](organization-data/11-model-profiles-and-agent-execution.md),
[provider pools](organization-data/14-model-providers-and-connection-pools.md),
[accounting](organization-data/10-usage-and-cost-accounting.md),
[recovery](organization-data/16-recovery-and-office-continuity.md) and
[budgets](organization-data/17-budgets-and-controls.md).

The copilot is parked: the library stays compiling and tested, and it is wired to a
model only after the first vendor runtime adapter exists
([project structure](project-structure.md#libraries-built-but-not-yet-reached)).

## What it offers

1. **Draft or improve any text.** One component for every text field.
2. **Report a bug from a sentence.** The draft fills the normal bug form, and the
   person confirms.

Other candidates (duplicate finder, triage suggestion, hire from description,
definition of done, task breakdown, reply suggestions, summaries, release notes,
asking the organization, staffing a team) are not part of v1. Each would need its
own registry entry, schemas and authorized context.

## Principles

1. **Drafts only.** A result is a value the UI shows in an editable form. Nothing is
   saved, sent, hired or run until the person presses the normal confirm button.
2. **Read-only, tool-less runs.** The runtime starts with no tools, file access,
   network or MCP servers, in an empty temporary directory. A runtime that cannot be
   started that way is not offered. Its provider connection is reached only through
   the approved adapter.
3. **Inputs are data, never instructions.** Everything from a person or the
   organization is escaped, fenced in one data block and introduced by a fixed
   template that says so. Nothing a person types becomes part of the template.
4. **Outputs are validated.** The runtime answers with one JSON document, parsed and
   checked against the feature's schema before anyone sees it. Anything else is
   `copilot_failed`, and so is a draft whose required text is only whitespace or that
   carries something shaped like a secret. Strings are shown as plain text, never as
   markup.
5. **No secrets in prompts.** The office adds only what the feature's registry entry
   names: never environment variables, tokens, keys, file contents or paths. Input is
   checked for obvious secret shapes (private keys, access tokens, 64-hex tokens), and
   a match refuses the run without echoing it.
6. **The office owns integration.** Authenticated requests reach the office, which
   resolves context, admits the run under the execution contract and owns process
   cleanup. `org-copilot` never starts a process.
7. **Unavailable states are explicit.** With no runtime set up, `copilot.features` is
   empty and the UI hides every copilot button. A configured feature that is
   temporarily unavailable is disabled with its reason, and the manual path stays.
8. **Honest cost.** Usage is recorded with its actual consumer and scope (document
   10). There is no separate "Copilot" cost category. A copilot run is never
   attributed to an invented employee. Missing tokens or cost stay partial or
   unavailable.
9. **Replaceable.** Runtimes sit behind a port the crate owns. The crate has no
   process or network code.

## Common UX

The control discloses the exact context it uses ("Uses this text and this project's
open bugs"). One secondary **Draft with copilot** pattern is used everywhere, with
progress and Stop, per-field **Drafted** markers and suggestions with reasons. Forms
stay editable and the manual actions stay usable. Preview is separate from the
ordinary Save, Send or Report. On failure the person's input is kept, and a panel's
earlier error clears on retry or Dismiss.

Acceptance: a draft never changes a record before the ordinary confirmation;
selecting parts of a rewrite keeps the rest of the original text; a context read
outside the caller's scope fails before any prompt is built; an unavailable runtime
never blocks the manual form.

## The pieces

```
UI (TS)  --copilot.*-->  office  --uses-->  org-copilot  --port-->  runtime adapter
                                            registry, render,
                                            limits, validate
```

`crates/org-copilot` depends only on `serde` and `serde_json`. The office owns the
calls, the resolution of `context_refs` and logging; a runtime adapter implements
the port. Dependencies point one way: office → `org-copilot` ← adapter.

### The feature registry

A feature is a registry entry. Nothing else knows what a feature does.

| Field | Meaning |
| --- | --- |
| `id`, `version` | For example `text.improve` v1. A change to the template or either schema bumps `version`. An ID is never the name of a real office call. |
| `title` | Shown on the button and in `copilot.features`. |
| `template` | The versioned prompt text with exactly two holes, `{{schema}}` and `{{data}}`, fixed at compile time. |
| `input_schema` | The JSON Schema of `input`, with the per-field caps. |
| `draft_schema` | The JSON Schema of the `draft` the runtime must return. |
| `context` | The kinds of context the office may attach, as capped data. |

The schema dialect is a small subset of JSON Schema (`type`, `properties`,
`required`, `additionalProperties`, `items`, `enum`, `minLength`, `maxLength`,
`minItems`, `maxItems`, `minimum`, `maximum`), checked by the crate itself. A
`minLength` of 1 or more also refuses whitespace only. A schema using anything else
is rejected when the registry is built. A runtime with native structured output may
receive the same schema, but the crate always validates.

#### 1. `text.improve`

- Input: `{ mode: "draft" | "improve", field: string (<= 80), text: string (<= 8000), instruction?: string (<= 500) }`.
  `field` is the field's label ("Task description"), which helps tone.
- Draft: `{ text: string (1..8000) }`.
- `improve` keeps the meaning and fixes clarity and grammar. `draft` writes a first
  version from `instruction` with `text` as notes. It never invents facts: unknowns
  stay as `[?]`.

#### 2. `bug.from_sentence` (implemented library entry, version 2)

The draft fills the form that creates a task of type bug
([task data](organization-data/03-features-and-work.md)): required accountable team,
optional project, immutable reporter, and independent assignment. A project bug's
accountable team is the project's owner team.

- Input: `{ sentence: string (<= 1000) }`. No context.
- Draft: the bug fields of the task creation form, `{ title (1..120), what_happened (1..2000), severity: "critical" | "major" | "minor" | "trivial", steps?: string (one step per line), expected?: string, environment?: string }`.
  Only the first three are required; the model leaves out what the sentence does not
  say.
- In the bug form the person types a sentence and presses **Draft the report**. The
  fields fill, each marked **Drafted** until edited or kept. A drafted severity is
  shown as **Suggested by the copilot** and never silently becomes the person's
  choice. Editing a field clears only its marker. **Undo draft** restores exactly what
  the person had before the first draft. The person presses **Report bug**, which is
  the normal creation call; the copilot never calls it.

### The runtime port

```rust
pub trait CopilotRuntime: Send + Sync + 'static {
    fn available(&self) -> bool;
    fn run(&self, request: &RunRequest, cancel: &CancelToken) -> Result<RunOutput, RuntimeError>;
}
pub struct RunRequest { prompt: String, output_schema: Value, timeout: Duration, max_output_bytes: usize }
pub struct RunOutput  { text: String, usage: Option<Usage> }   // text = the one JSON document
pub enum RuntimeError { NotSetUp, TimedOut, Failed }           // no payload: nothing to leak
```

An adapter runs tool-less and read-only, caps the bytes it reads, returns the
model's final message, and polls `cancel` while it waits, killing its child when it
is set.

**The crate owns the deadline.** It runs the adapter on its own worker thread and
waits at most `Limits::timeout` (60 s). At the deadline it sets the run's
`CancelToken` and answers `timeout`; a late answer is discarded. A hung adapter
therefore never holds the person's slot or the office's worker. An adapter that
ignores `cancel` leaves a thread and its child behind, so
`Limits::max_concurrent_runs` (8) counts every adapter call that has not returned,
including timed-out ones, and beyond it a call is `copilot_busy`. Honouring `cancel`
is mandatory. A panic in the adapter is a failed run.

This port is separate from `org-runtime`: one prompt, one JSON answer, no
conversation state. The office-side adapter starts and stops processes through
`org-supervise`.

### Execution

The port has no profile, model, placement, receipt, permit or route fields. Before a
real model is wired, the office maps a copilot run, which is not an employee's turn,
to an execution admission under document 11: a selected pool and model, local private
bindings, shared connection capacity and cooldowns, a turn permit, and an
actor-bound operation ID. There is no fallback to the first configured runtime. The
60-second library timeout does not prove the process ended and does not release a
permit or process slot; remaining work is reconciled before anything replaces it,
and an unknown draft operation is never reissued automatically. Until this mapping
exists, real copilot execution is unavailable.

Usage, partial or unavailable, and the route are recorded in the accounting archive,
including timed-out attempts. Budget holds apply as for any execution, unknown usage
warns rather than counting as free, and release caps warn only. Amounts are visible
under the [cost access rules](../specs/costs-and-insights-ux.md).

### The contract

```
copilot.features {}                                  -> { features: [{ id, version, title }] }
copilot.draft { feature, input, context_refs? }      -> { draft, notes }
```

- `copilot.features` is empty when no runtime is set up.
- `context_refs` are references (`{ kind, id }`), never text: the office resolves them
  with the caller's own rights and passes the text to the crate as capped data.
- `draft` is a value of the feature's `draft_schema`. `notes` are up to five short
  strings from the model, shown beside the draft.

The shapes live in `org-copilot` and become `org-proto` types when the office wires
them. Error codes:

| Code | When |
| --- | --- |
| `copilot_unavailable` | No runtime is set up, or the feature is not offered here (an unknown feature name is the same). |
| `copilot_failed` | The runtime failed, or its answer was not one JSON document matching the schema. |
| `timeout` | The run took longer than the limit. The late answer is discarded. |
| `copilot_busy` | A run of the same feature by the same person is in progress, or too many runs are in flight. |
| `too_large` | A field over its cap, the data over the byte cap, or too many context items. |
| `invalid_request` | Input that breaks its schema in another way, context of a kind the feature does not take, or text that looks like a secret. |

### Limits

- **One run per person per feature at a time.** A second call gets `copilot_busy` at
  once; there is no queue. The slot is freed when the call returns, at the deadline at
  the latest, even on a panic. At most 8 adapter calls are in flight overall.
- **60 s timeout** per run, owned by the crate.
- **Input cap:** each field by its schema, in characters, and 64 KiB for the escaped
  data block.
- **Output cap:** 64 KiB read from the runtime.
- No retries inside the copilot; the person presses the button again.

### Logging without bodies

The office logs one record per run: feature ID and version (`unknown` for a name
not in the registry; caller text is never logged), the user ID, the outcome (`ok`,
`invalid_input`, `busy`, `unavailable`, `timeout`, `failed`), duration, input and
output sizes in bytes, and usage metadata. **Never the input, context, prompt or
output.** The record type has no field that could hold a body, the crate's errors
carry fixed messages only, and adapters follow the same rule for their stderr.

### Prompt rendering

The template is fixed text. The renderer puts the output schema in `{{schema}}` and
the data in `{{data}}`:

- The data is one JSON object, `{"input": ..., "context": [...]}`, so every newline
  and quote inside a value is escaped.
- `<`, `>`, `&` and backticks are escaped as well (`<` and so on), so a value can
  never contain the fence lines `<<<DATA` and `DATA>>>`, a Markdown fence or a tag. The
  holes are filled in one pass, so data that looks like `{{schema}}` is never
  expanded. The renderer checks the escaping again and refuses to render if the data
  region could be closed early.
- The template says the data block is data, and repeats that after the block.

## What the code does today

`crates/org-copilot` implements the registry, rendering, validation, limits and a
fake runtime; nothing depends on it. Its `bug.from_sentence` v2 entry
(`crates/org-copilot/src/builtin.rs`) still takes the project's areas as context and
returns an optional `area_id`; areas are not part of the product, so the entry drops
that context and field when it is wired to the task form. The UI's copilot controls
use a mock.
