# Smart features: the copilot

Status: **proposed** (W11). Author: the copilot work (cloud). Reviewer: ada.
Builds on [ADR-0001](../adr/0001-app-is-a-client-office-is-a-server.md) (the app is a client; the office server does the work) and the [architecture rules](../architecture-rules.md).
Eden's smart-features candidate list is not in `docs/specs/` on this branch. The specs only refer to it by number (#7 break a feature into tasks, #13 "why?" answers, #16 suggest who scopes a request, P2 the Reporter's narrative), so this design takes the user's first set and leaves room for those.

## What it is
The **copilot** uses the user's own agent runtime (`claude -p`, `codex exec`), in the background, to **draft** things. It proposes and a person confirms. It never acts.

The first set (the phases below are these three features):

1. **Draft or improve any text.** One component for every text field.
2. **Report a bug from a sentence.** The draft fills the normal report form, and the person confirms.
3. **Staff a team from requirements.** Proposes a lead, roles, runtimes and a cost range. Each hire is reviewed.

## Principles
1. **Drafts only.** A copilot result is a value the app shows in an editable form. Nothing is saved, sent, hired or run until the person presses the normal confirm button. The copilot has no write path into the office.
2. **Read-only, tool-less runs.** The runtime is started with no tools, no file access, no network and no MCP servers, in an empty temporary directory. If a runtime cannot be started that way, it is not offered as a copilot runtime.
3. **Inputs are data, never instructions.** Everything that comes from a person or from the workspace (the text, a task's body, a message) is escaped, fenced in one data block, and introduced by a fixed template that says so. Nothing the person types ever becomes part of the template.
4. **Outputs are validated.** The runtime must answer with one JSON document. It is parsed and checked against the feature's schema before anyone sees it. Anything else is `copilot_failed`, and so is a draft whose required text is only whitespace or that carries something shaped like a secret. Strings in a result are shown as plain text, never as markup and never run.
5. **No secrets in prompts.** The office adds only what the feature's registry entry names. It never adds environment variables, tokens, keys, file contents or paths. The input is also checked for obvious secret shapes (private keys, access tokens, the 64-hex app token), and a match refuses the run without echoing it.
6. **It runs in the office server.** Not in the WebView and not in the app's Rust shell. The app asks `copilot.features` and `copilot.draft` like any other call.
7. **Hidden when it can't work.** With no runtime set up, `copilot.features` is empty and the app hides every smart button. A screen never shows a smart button that will only fail.
8. **Honest cost.** Each run reports its usage when the runtime does, so costs can show "Copilot" per project (`costs-and-insights-ux.md`, COST-6).
9. **Replaceable.** The runtimes sit behind a port the crate owns. The crate has no process or network code.

## The pieces

```
app (TS)  --copilot.*-->  office server  --uses-->  org-copilot  --port-->  runtime adapter
                                                    registry,                (claude -p, codex exec:
                                                    render, limits,           River's runtime work)
                                                    validate
```

`crates/org-copilot` is a library with no dependencies except `serde` and `serde_json`. The office server owns the wiring (calls, resolving `context_refs`, logging), and a runtime adapter implements the port. Dependencies point one way: office server -> `org-copilot` <- adapter.

### The feature registry
A feature is a registry entry. Nothing else in the system knows what a feature does.

| Field | Meaning |
| --- | --- |
| `id`, `version` | `text.improve` v1. A change to the template or either schema bumps `version`. |
| `title` | Shown on the button and in `copilot.features`. |
| `template` | The versioned prompt text with exactly two holes, `{{schema}}` and `{{data}}`. Fixed at compile time. |
| `input_schema` | The JSON Schema of `input`. It carries the per-field caps. |
| `draft_schema` | The JSON Schema of the `draft` the runtime must return. |
| `context` | The kinds of context the office may attach (data, capped). Empty for features 1 and 2. |

The schema dialect is a small subset of JSON Schema (`type`, `properties`, `required`, `additionalProperties`, `items`, `enum`, `minLength`, `maxLength`, `minItems`, `maxItems`, `minimum`, `maximum`), which the crate checks itself. A `minLength` of 1 or more also refuses a string of only whitespace.  A schema using anything else is rejected when the registry is built. The same documents can be handed to a runtime that has native structured output (`claude -p --json-schema`), but the crate never relies on that: it always validates.

#### 1. `text.improve` (built in this PR)
- Input: `{ mode: "draft" | "improve", field: string (<= 80), text: string (<= 8000), instruction?: string (<= 500) }`. `field` is the label of the field ("Task description"), which helps tone.
- Draft: `{ text: string (1..8000) }`.
- Behaviour: `improve` keeps the meaning and fixes clarity and grammar. `draft` writes a first version from `instruction` and `text` as notes. It never invents facts: unknowns stay as `[?]`.

#### 2. `bug.report` (built in this PR)
- Input: `{ sentence: string (<= 1000) }`.
- Draft: `{ title (1..120), actual (<= 800), expected (<= 800), steps: string[] (<= 10, each <= 300), severity: "low" | "medium" | "high" | "critical" }`. These are the fields of the normal bug report form.
- Behaviour: the app opens the report form filled with the draft and marked "Drafted by the copilot". The person edits it and presses **Report bug**. Dropping the draft costs nothing.

#### 3. `team.staff` (designed only; not built, not registered)
- Input: `{ requirements: string (<= 4000), constraints?: { max_monthly_cost?: number, runtimes?: string[] } }`. Context (from the office, as data): the organization's existing teams and role titles, and the runtimes set up here.
- Draft: `{ lead: Hire, hires: Hire[] (<= 12), cost_range: { low, high, unit: "usd_month" }, rationale: string }`, with `Hire = { role: string, title: string, runtime: string (one of the runtimes offered), model?: string, responsibilities: string[] }`.
- Behaviour: the app shows each hire as its own card to accept, edit or drop (**each hire reviewed**). Accepting creates nothing by itself: the person then uses the normal *hire* and *team* commands. The cost range is an estimate computed from the offered runtimes' rates by the office, and the copilot's own range is shown only as a hint.
- Needs the teams and org-chart calls to exist. That is why it comes last.

### The runtime port
```rust
pub trait CopilotRuntime {
    fn available(&self) -> bool;                       // a runtime is set up here
    fn run(&self, request: &RunRequest) -> Result<RunOutput, RuntimeError>;
}
pub struct RunRequest { prompt: String, output_schema: Value, timeout: Duration, max_output_bytes: usize }
pub struct RunOutput  { text: String, usage: Option<Usage> }   // text = the one JSON document
pub enum RuntimeError { NotSetUp, TimedOut, Failed }           // no payload: nothing to leak
```
The adapter's duties: run tool-less and read-only (principle 2), enforce the timeout by killing the process, cap the bytes it reads, and return the model's final message. The service does not trust the adapter: it checks the elapsed time itself, the size, the JSON and the schema. The port is synchronous. The office calls it from a worker thread, one blocking call per run. The crate cannot stop a call that never returns, and that run keeps the person's slot: an adapter that ignores its timeout is a bug in the adapter, and the office should run the call on a worker it can abandon. A run that ends after the deadline, with an answer or an error, is reported as `timeout`.

This port is not `org-runtime` (the session port in `docs/design/agent-sessions.md`, which drives long-lived interactive sessions). A copilot run is one prompt, one JSON answer and a process that ends, with no permission prompts and no session state. In P2 the adapters may share launch planning with `org-runtime-claude` and `org-runtime-codex`, but the office, not this crate, decides how; `org-copilot` depends on neither.

### The contract sketch
Calls on the office server's socket. **Not added to `org-proto` in this PR**: the shapes live in `org-copilot` and the app's mock, and become `org-proto` types, additive, when the office wires them (ada's review then).

```
copilot.features {}                                  -> { features: [{ id, version, title }] }
copilot.draft { feature, input, context_refs? }      -> { draft, notes }
```
- `copilot.features` is empty when no runtime is set up. That is how the app knows to hide the buttons.
- `context_refs` are references (`{ kind, id }`), never text: the office resolves them with the caller's own rights and passes the text to the crate as capped data.
- `draft` is the feature's `draft_schema` value. `notes` are up to 5 short strings from the model ("assumed the bug is in the app, not the office"), shown beside the draft.

New error codes, additive in `ErrorCode`:

| Code | When |
| --- | --- |
| `copilot_unavailable` | No runtime is set up, or the feature isn't offered. |
| `copilot_failed` | The runtime failed, or its answer was not one JSON document that matches the schema. |
| `timeout` | The run took longer than the limit. The late answer is thrown away. |
| `copilot_busy` | A run of the same feature by the same person is still going. (A fourth code, proposed here.) |

Bad input uses the existing `invalid_request`: unknown feature, input that breaks its schema, over the cap, or looks like a secret.

### Limits
- **One run per person per feature at a time.** A second call gets `copilot_busy` at once (no queue). The slot is freed when the run ends, even on a panic.
- **60 s timeout** per run (`Limits::timeout`).
- **Input cap:** each field by its schema (in characters), and 64 KiB for the escaped data block, which is what goes into the prompt.
- **Output cap:** 64 KiB read from the runtime.
- No retries inside the copilot. The person presses the button again.

### Logging without bodies
Per run the office logs one record: feature id and version (`unknown` for a name the registry doesn't have: caller text is never logged), the user id, outcome (`ok`, `invalid_input`, `busy`, `unavailable`, `timeout`, `failed`), duration, input and output sizes in bytes, and usage. **Never the input, the context, the prompt or the output.** The record type has no field that could hold a body, and the crate's errors carry fixed messages only (a runtime error has no payload at all). Adapters follow the same rule for their stderr.

### Prompt rendering
The template is fixed text. The renderer puts the feature's output schema in the `{{schema}}` hole and the data in `{{data}}`:

- The data is one JSON object, `{"input": ..., "context": [...]}`, so every newline and quote inside a value is escaped.
- `<`, `>`, `&` and backticks are escaped too (`\u003c` and so on, still valid JSON), so a value can never contain the fence lines `<<<DATA` and `DATA>>>`, nor a markdown fence or a tag. The holes of the template are filled in one pass, so data that looks like `{{schema}}` is never expanded. The renderer checks the escaping again and refuses to render if the data region could be closed early.
- The template says the data block is data, and repeats the reminder after the block.

## Phases
| Phase | Content | State |
| --- | --- | --- |
| P1 | The design, `org-copilot` (registry, rendering, validation, limits, a fake runtime), the "Draft or improve" component and the "Report a bug from a sentence" flow in the app on a mock. | this PR |
| P2 | The real adapters (`claude -p`, `codex exec`, tool-less and read-only) in River's runtime work, and the office wiring: `copilot.*` in `org-proto`, `context_refs` resolution, the run log, the setting that says which runtime is the copilot. The app's mock is replaced by the real calls. | next |
| P3 | `team.staff`, once teams and hiring exist. Then the candidates from Eden's list that fit the same shape (#7, #13, #16, the Reporter's narrative). | later |

## Not decided
1. Which runtime is "the copilot" when several are set up (probably a setting per organization, default the first set up).
2. Whether the cost of copilot runs counts against a project budget. `costs-and-insights-ux.md` shows it as "Copilot", but doesn't say.
3. Whether `copilot_busy` should be a new code or reuse `unavailable`. Proposed: new, so the app can say "still working".
