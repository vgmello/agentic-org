# Smart features: the copilot

Status: **library implementation plus target integration proposal, rev 3**
(2026-11-15). The existing org-copilot registry/rendering/validation/fake-runtime
contract remains current; real execution integration is not established here.
[Organization-data execution](organization-data/11-model-profiles-and-agent-execution.md),
[provider pools](organization-data/14-model-providers-and-connection-pools.md),
[accounting](organization-data/10-usage-and-cost-accounting.md),
[recovery](organization-data/16-recovery-and-office-continuity.md) and
[budgets](organization-data/17-budgets-and-controls.md) own the selected data/control
boundaries. This document does not select areas, staffing automation or a separate
copilot process owner. Author: the copilot work (cloud). Reviewer: ada.
Builds on [ADR-0001](../adr/0001-app-is-a-client-office-is-a-server.md) (the app is a client; the office server does the work) and the [architecture rules](../architecture-rules.md).
Compatible requirements from Eden's earlier smart-feature candidates are integrated below. The registry entries distinguish implemented library contracts from deferred candidates; listing a candidate neither registers nor activates it.

## What it is
The **copilot** drafts text for a person to review and confirm. It has no domain
write path. The current library has a separate one-prompt `CopilotRuntime` port;
real native adapters, connection selection and office wiring remain integration
work. A runtime choice does not embed credentials/model settings in an employee
identity or authorize launching outside shared execution controls.

The first set (the phases below are these three features):

1. **Draft or improve any text.** One component for every text field.
2. **Report a bug from a sentence.** The draft fills the normal report form, and the person confirms.
3. **Staff a team from requirements (deferred draft feature).** Proposes a valid
   home-team assignment, roles and separate execution pool/model choices with an
   explicitly qualified cost estimate. Each hire is reviewed; no automatic hire.

### Candidate experience (deferred unless an implemented entry is identified below)

The existing `text.improve` and `bug.from_sentence` library entries remain the first two implemented contracts. Their additional candidate UX and every other row require separate selection, schemas, authorized context and integration. Candidate numbers retain the earlier product references; they are not registry IDs.

| Candidate | Proposed draft/output and review |
| --- | --- |
| 1 · Bug from a sentence | Fill the ordinary report form with field-level Drafted provenance; the person checks/edits then uses Report bug. Optional severity stays an explicit choice/suggestion. Use selected task accountability, not the old area field. |
| 2 · Duplicate finder | At report/triage, up to three likely duplicates with a one-line reason each; the person chooses This is the same bug or ignores them. No automatic close/link. |
| 3 · Triage suggestion | Suggest authorized accountable scope, severity, priority and assignment with reasons from visible similar bugs; the person edits before Mark as triaged. No automatic triage/assignment. |
| 4 · Text draft/improvement | Field text plus Shorter/Clearer/Plain language/Spelling/Custom intent produces a diff; Use this, Use selected parts or Discard precedes ordinary save/send. |
| 5 · Hire from description | Draft title, role summary, responsibilities, focus, valid home team, reporting suggestion and name; use the normal hire review/confirmation, never provision from the draft. |
| 6 · Definition of done | Draft one or two checkable sentences from task title/description, editable in the normal task form; no new completion engine. |
| 7 · Task/breakdown | A goal becomes one task or three to seven proposals with checkable completion text, order and suggested assignees; edit/drop before an explicit Create N tasks confirmation. Keep proposals distinct from accepted task IDs and selected mutation services. |
| 8 · Fix task from bug | Draft a fix task's title, description and completion text from the visible bug into New task; ordinary confirmation creates it. No direct repository access. |
| 9 · Reply suggestions | From the visible question and explicitly prepared recent turns, draft two or three short replies; pick/edit then use ordinary Send. |
| 10 · Linked summaries | Three to five read-only lines from authorized activity/thread facts, each linked to its source. Private away context follows the separate account-only digest boundary, never shared project/agent context. |
| 11 · Doc/brief draft | A topic and authorized project facts produce Markdown in the editor; ordinary reviewed Save/publication persists it with selected document scope. |
| 12 · Release notes | Selected completed work/verified fixes in a date range produce user-facing draft notes in the editor; Save follows the existing release/document contract. |
| 13 · Ask the organization | The office searches authorized records first and supplies bounded prepared context; return a read-only answer with source links. The copilot performs no live search or tool invocation. |
| 14 · Team suggestion | Visible roles/projects suggest a team grouping with a reason; prefill the ordinary New team form for review, without moving employees automatically. |
| 15 · Staff a team | Draft a named team/lead and three to six role/title/focus/reporting proposals; edit/drop/add and review each hire. Separate execution suggestions require owner confirmation and selected pool/model data; no old monthly model quote or role default. |
| 16 · Route a request | Request text and authorized projects/roles/recent requests produce suggested project/type and up to three scopers with reasons; choose/edit or Just save, then ordinary Send. Configured rule fallback remains explicitly labelled. |

**Common target UX:** disclose the exact context under the button (“Uses this text and this project's open bugs”). The office prepares only the feature registry's authorized, bounded data; source text remains untrusted data, never instructions. No candidate gains tools, file/network/MCP access, credentials or private-note/other-account activity access. Any later prepared repository context needs an explicit reviewed registry/context contract; these candidates grant none.

Use one secondary Draft with copilot pattern, progress with Stop, per-field Drafted markers and reasoned suggestions; forms stay editable and manual actions remain usable. Preview is separate from ordinary Save/Send/Report/Hire. Hide controls when disabled/unconfigured; show the reason for a temporarily unavailable configured feature. Keep the person's input on failure and clear a panel's stale error on retry or Dismiss. Cost labels/reads use the actual route's ledger coverage and existing scoped amount rules; no separate monthly Copilot accounting model is added.

**Target acceptance:** a candidate cannot mutate a domain record before ordinary confirmation; selected rewrite parts preserve the remaining original text; an attempted context read outside scope fails before prompting. Duplicate/scoper results never exceed three, breakdown proposals stay within three to seven when splitting, and reply suggestions number two or three. Cancellation remains pending until the execution adapter proves cleanup; an unavailable runtime never blocks the manual form.

## Principles
1. **Drafts only.** A copilot result is a value the app shows in an editable form. Nothing is saved, sent, hired or run until the person presses the normal confirm button. The copilot has no write path into the office.
2. **Read-only, tool-less runs.** The runtime is started with no tools, no file access, no network and no MCP servers, in an empty temporary directory. If a runtime cannot be started that way, it is not offered as a copilot runtime.
3. **Inputs are data, never instructions.** Everything that comes from a person or from the workspace (the text, a task's body, a message) is escaped, fenced in one data block, and introduced by a fixed template that says so. Nothing the person types ever becomes part of the template.
4. **Outputs are validated.** The runtime must answer with one JSON document. It is parsed and checked against the feature's schema before anyone sees it. Anything else is `copilot_failed`, and so is a draft whose required text is only whitespace or that carries something shaped like a secret. Strings in a result are shown as plain text, never as markup and never run.
5. **No secrets in prompts.** The office adds only what the feature's registry entry names. It never adds environment variables, tokens, keys, file contents or paths. The input is also checked for obvious secret shapes (private keys, access tokens, the 64-hex app token), and a match refuses the run without echoing it.
6. **The office owns integration.** In the target browser deployment authenticated
   HQ operations route to the owning office. org-copilot does not spawn; the office
   execution integration owns routes/permits/process cleanup. The `copilot.*` shapes
   below remain library/mock contracts until real handlers are implemented.
7. **Unavailable states are explicit.** With no runtime set up, `copilot.features` is empty and the app hides every smart button. For a configured feature temporarily unavailable, disable it with the concrete reason while retaining the manual path; do not present a functional control that will only fail. This UI requirement does not turn an unavailable production handler into an implemented feature.
8. **Honest cost.** Document 10 owns immutable usage, actual consumer and scope
   attribution. No automatic separate “Copilot” cost category is introduced. A
   non-employee copilot consumer maps to one_off only when that is the actual
   execution identity; never fabricate an employee from the requesting person.
   Missing tokens/cost stay partial/unavailable, with actual route/attempt evidence
   when available. Task/agent budgets use document 17; release caps warn only.
   Compatible cost requirements are integrated in those existing contracts;
   older contradictory proposals grant no additional authority.
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
| `id`, `version` | `text.improve` v1, `bug.from_sentence` v2. A change to the template or either schema bumps `version`. An id is never the name of a real office call (hence `bug.from_sentence`, not `bug.report`). |
| `title` | Shown on the button and in `copilot.features`. |
| `template` | The versioned prompt text with exactly two holes, `{{schema}}` and `{{data}}`. Fixed at compile time. |
| `input_schema` | The JSON Schema of `input`. It carries the per-field caps. |
| `draft_schema` | The JSON Schema of the `draft` the runtime must return. |
| `context` | The kinds of context the office may attach (data, capped). None for `text.improve`; `area` for the current v2 compatibility schema of `bug.from_sentence`; areas remain deferred in the selected target. |

The schema dialect is a small subset of JSON Schema (`type`, `properties`, `required`, `additionalProperties`, `items`, `enum`, `minLength`, `maxLength`, `minItems`, `maxItems`, `minimum`, `maximum`), which the crate checks itself. A `minLength` of 1 or more also refuses a string of only whitespace. A schema using anything else is rejected when the registry is built. The same documents can be handed to a runtime that has native structured output (`claude -p --json-schema`), but the crate never relies on that: it always validates.

#### 1. `text.improve` (implemented library registry entry)
- Input: `{ mode: "draft" | "improve", field: string (<= 80), text: string (<= 8000), instruction?: string (<= 500) }`. `field` is the label of the field ("Task description"), which helps tone.
- Draft: `{ text: string (1..8000) }`.
- Behaviour: `improve` keeps the meaning and fixes clarity and grammar. `draft` writes a first version from `instruction` and `text` as notes. It never invents facts: unknowns stay as `[?]`.

#### 2. `bug.from_sentence` (implemented library entry, version 2)

**Current-contract compatibility.** The exact v2 output fields/area context below
match crates/org-copilot/src/builtin.rs and the existing `bug.report` shape. Keep
them compatible until a reviewed versioned adapter replaces them; documentation
reconciliation does not silently rename commands or remove accepted fields.

The selected domain destination is one extensible task entity of type bug, with
required accountable team, optional project, immutable creator/reporter and
independent nullable assigned_team_id/assignee_id (changed atomically with expected
revision). A project bug uses its project's owner team; a projectless bug uses its
team scope. New records use T- references such as P-8/T-42 or TEAM-12/T-42; legacy
B-/REQ- IDs require explicit mappings, never prefix substitution or renumbering.
The confirmation form must resolve these fields through the normal task mutation,
retain bug detail and create its durable event history. Areas are unselected:
this v2 area behavior is legacy evidence, not a target area requirement. A future
feature version must define the selected task-form mapping and omit unselected
area dependencies rather than promoting them through context resolution.

- Input: `{ sentence: string (<= 1000) }`. Context: the project's areas, one `area` item each (`label`: the name, `text`: the id), resolved by the office.
- Draft: the fields of the real `bug.report` call (`org-proto`), so a draft fills the report form as it is: `{ title (1..120), what_happened (1..2000), severity: "critical" | "major" | "minor" | "trivial", steps?: string (one string, one step per line), expected?: string, area_id?: string, environment?: string }`. Only the first three are required. The model leaves out what the sentence doesn't say. `area_id` is only meaningful if it is the id of one of the areas given as context: the copilot drops an `area_id` that is not one of the ids it was given as context (the rest of the draft stays), and the dialog ignores one that is not an area of the project (the area the person had chosen stays).
- Behaviour: in the real report dialog (`ReportBugDialog`, opened from a project's Bugs tab) the person types a sentence and presses **Draft the report**. The dialog's fields fill with the draft, marked "Drafted by the copilot", and the person edits them and presses **Report bug**, which is the normal `bug.report` call. **Undo draft** puts back exactly what the person had before the first draft. The copilot never calls `bug.report`.
- Target UI checks: mark each filled field **Drafted** until edited/explicitly kept. A drafted severity says **Suggested by the copilot** or remains a nonselected hint; it never silently becomes the person's deliberate severity choice. Editing one field clears only its own marker. Clear a draft/improve panel's prior error on its next attempt or Dismiss, retaining the person's inputs and Undo draft. These are requirements from the earlier pinned UX review, not proof of current UI behavior.

#### 3. `team.staff` (deferred draft feature; not built or registered)

- Input proposal: requirements (up to 4000 characters), optional constraints and
  only caller-visible existing home teams/role titles and ready execution choices.
- Draft proposal: a lead candidate and up to 12 hire cards, each with name/role,
  required home_team_id, optional reporting line and responsibilities; separate
  optional execution recommendation `{model_profile_id, model_id}` for an agent.
  A person has no execution config. This is not a new wire schema.
- Each card can be edited/dropped before normal authorized hire/team commands.
  A lead must be a direct member of the selected team and manages its subtree;
  ordinary membership in an ancestor team grants no managerial rights. Creating
  identity does not start a runtime. An owner/HR employee requires a valid home
  team too; missing bootstrap team blocks that record rather than inventing one.
- Execution recommendations use one pool/canonical model, never credentials or
  machine-specific home paths. Office placement and local binding/readiness are
  separate reviewed choices. The office validates references and authorization;
  accepting a model's recommendation confers no authority or provider capacity.
- A cost range, if offered, must identify its basis and uncertainty. Recorded
  API-price-equivalent usage is not a monthly subscription fee or staffing quote.
  Missing prices/usage cannot become zero cost. Expose amounts only within COST-7
  and document 17 access scope. This remains a proposal pending an estimation rule.

### The runtime port
```rust
pub trait CopilotRuntime {
    fn available(&self) -> bool;                       // a runtime is set up here
    fn run(&self, request: &RunRequest, cancel: &CancelToken) -> Result<RunOutput, RuntimeError>;  // trait CopilotRuntime: Send + Sync + 'static
}
pub struct RunRequest { prompt: String, output_schema: Value, timeout: Duration, max_output_bytes: usize }
pub struct RunOutput  { text: String, usage: Option<Usage> }   // text = the one JSON document
pub enum RuntimeError { NotSetUp, TimedOut, Failed }           // no payload: nothing to leak
```
The adapter's duties: run tool-less and read-only (principle 2), cap the bytes it reads, return the model's final message, and **poll `cancel` while it waits and kill its child when it is set**.

**The crate owns the deadline.** It runs the adapter on its own worker thread and waits at most `Limits::timeout` (60 s). At the deadline it sets the run's `CancelToken` and answers `timeout`, whether or not the adapter has returned, and a late answer is thrown away. So a hung adapter never holds the person's slot (it is freed as the call returns) nor the office's worker (the call is not blocked on the adapter). What the crate can't do is stop the thread itself: an adapter that ignores `cancel` leaves a thread and its child behind. `Limits::max_concurrent_runs` (8) counts every adapter call that hasn't returned, timed out ones included, and past it a call is `copilot_busy` (with its own message, "too many copilot runs are in flight", and the same `busy` log outcome), so a stuck adapter can't pile up threads without bound. The price: adapters that never honour `cancel` lock the copilot for everyone once eight runs are stuck, until they return or the office restarts. Honouring `cancel` is therefore not optional. A panic in the adapter is a failed run. The service also checks the size, the JSON and the schema of what comes back.

This existing port is separate from org-runtime: one prompt and one JSON answer,
no permission dialogue or conversation state. org-copilot depends on neither the
session port nor OS libraries. The office-side adapter must compose real process
operations through org-supervise and the single office recovery owner.

### Selected execution adaptation requirements

The current port has no pool/model, placement, operation receipt, permit or actual
route fields, and its Usage reports only optional input/output tokens. It is not
already a complete document 11 runner. Before wiring a real model, explicitly map
its non-employee identity and immutable draft request to a reviewed execution
admission extension; do not invent an employee, silently bypass HQ permits or
claim the employee-only RunTurn contract already accepts one-offs. Until that
mapping and conformance exist, real copilot execution is unavailable.

The adapter integration must preserve ordered same-model selection, local private
bindings, fresh entitlement, compatible no-tool policy, shared connection capacity
and cooldowns. There is no first-configured-runtime fallback. Retain actor-bound
operation IDs/digests and protected process ownership across unknown replies and
crashes. A 60-second library response timeout/cancel token is not proof of process
termination and cannot release a permit or owned-process slot. Reconcile all
remaining work before replacement; do not automatically reissue an unknown draft
operation. Ordinary user tools inherit no credential. “No network” for tool access
cannot mean the provider transport is forbidden; provider access is only through
the approved adapter context.

Persist known/partial/unavailable usage and route evidence in the durable accounting
archive, including timed-out attempts; the library response is not the ledger.
Applicable budget holds/manual/security reasons remain independent, with durable
extensions and stop_pending/held acknowledgement. Unknown usage warns instead of
being treated as free or an automatic stop; release caps warn only. A project
manager sees only their project amounts; org-wide agent totals/history stay owner
scoped. The own-agent CLI budget exception grants no copilot raw-history access.

### The contract sketch
Existing library/mock shapes, not proof of live office socket handlers. **Not added to `org-proto` by this design**: the shapes live in `org-copilot` and the app's mock, and become `org-proto` types, additive, when the office wires them (ada's review then).

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
| `copilot_unavailable` | No runtime is set up, or the feature isn't offered here (an unknown feature name is the same). |
| `copilot_failed` | The runtime failed, or its answer was not one JSON document that matches the schema. |
| `timeout` | The run took longer than the limit. The late answer is thrown away. |
| `copilot_busy` | A run of the same feature by the same person is still going, or too many runs are in flight. (A fourth code, proposed here, so the app can say "still working".) |

Bad input uses codes `org-proto` already has: **`too_large`** for anything over a cap (a field over its length, the data over the byte cap, too many context items), and **`invalid_request`** for input that breaks its schema in another way, context of a kind the feature doesn't take, or text that looks like a secret.

### Limits
- **One run per person per feature at a time.** A second call gets `copilot_busy` at once (no queue). The slot is freed when the call returns (at the deadline at the latest), even on a panic. At most 8 adapter calls in flight across everyone (`Limits::max_concurrent_runs`).
- **60 s timeout** per run (`Limits::timeout`), owned by the crate (see the port).
- **Input cap:** each field by its schema (in characters), and 64 KiB for the escaped data block, which is what goes into the prompt.
- **Output cap:** 64 KiB read from the runtime.
- No retries inside the copilot. The person presses the button again.

### Logging without bodies
When wiring the office, log one non-body diagnostic record per run: feature id and version (`unknown` for a name the registry doesn't have: caller text is never logged), the user id, outcome (`ok`, `invalid_input`, `busy`, `unavailable`, `timeout`, `failed`; an unknown feature counts as `unavailable`), duration, input and output sizes in bytes, and authorized usage metadata (the durable ledger/access filters remain separate). **Never the input, the context, the prompt or the output.** The record type has no field that could hold a body, and the crate's errors carry fixed messages only (a runtime error has no payload at all). Adapters follow the same rule for their stderr.

### Prompt rendering
The template is fixed text. The renderer puts the feature's output schema in the `{{schema}}` hole and the data in `{{data}}`:

- The data is one JSON object, `{"input": ..., "context": [...]}`, so every newline and quote inside a value is escaped.
- `<`, `>`, `&` and backticks are escaped too (`\u003c` and so on, still valid JSON), so a value can never contain the fence lines `<<<DATA` and `DATA>>>`, nor a markdown fence or a tag. The holes of the template are filled in one pass, so data that looks like `{{schema}}` is never expanded. The renderer checks the escaping again and refuses to render if the data region could be closed early.
- The template says the data block is data, and repeats the reminder after the block.

## Phases
| Phase | Content | State |
| --- | --- | --- |
| P1 | Existing library registry, rendering, validation, limits and fake runtime; UI/mock evidence is separate from live execution. | library implemented; no new implementation here |
| P2 | Reviewed non-employee admission/pool/permit/receipt/accounting mapping, restricted real adapters and authenticated office wiring; versioned target task form mapping without areas. | implementation/adaptation required |
| P3 | `team.staff`, once teams and hiring exist. Then the candidates from Eden's list that fit the same shape (#7, #13, #16, the Reporter's narrative). | later |

## Not decided
1. Explicit copilot execution pool/model and non-employee admission mapping. No implicit first-runtime default is selected.
2. Budget evaluation follows document 17 and actual ledger attribution. There is no monthly project budget; no invented non-employee agent allowance. The precise admission adaptation remains implementation work; integrated requirements do not establish a completed service.
3. ~~Whether `copilot_busy` is a new code or reuses `unavailable`.~~ *Decided (ada, PR review): keep `copilot_busy`.*
