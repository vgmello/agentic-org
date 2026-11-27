# Processes and standards inheritance

Organization governance in Agentic Org uses typed process and standard documents resolved along a single inheritance chain.

## Single-chain resolution

- Every piece of work resolves along one chain from broadest to most specific: `organization -> accountable team chain -> project`.
- For project work, the chain follows the project's accountable owning team and its ancestor teams, ending at the project layer. Contributing teams do not alter the chain.
- For projectless tasks, the chain follows the task's accountable team. Outside a task, standing guidance follows an employee's home team.
- List effective processes and standards for your scope with `agent-os process list`, or target a scope with `agent-os process list --project <project>` or `agent-os process list --task <task>`.

## Must-follow vs overridable precedence

- Documents are tagged as either `process` (how work flows) or `standard` (rules and constraints).
- Each document declares `overridable` (`true` by default, or `false` for non-overridable must-follow rules).
- When a higher layer sets `overridable: false` for a key, lower team or project layers cannot replace or weaken it; the non-overridable higher layer always wins and any conflicting lower override is flagged as a resolution problem.
- Overridable documents may be replaced cleanly by a more specific layer sharing the same key.
- Must-follow documents are delivered whole at session boot (`agent-os boot`) and never truncated mid-document; overridable guidance fits within the standing instruction budget.

## Tracing decisions with `--why`

- Inspect a single document with `agent-os process inspect --key <key>`.
- Trace every layer in the inheritance chain and see which layer won (and why others were replaced or blocked) with `agent-os process inspect --key <key> --why`.
- Large documents can be paged cleanly using `--offset` on `agent-os process inspect --help`.
- Create or revise process and standard documents with `agent-os process create --help` and `agent-os process update --help`, passing `--non-overridable` when establishing a mandatory standard.
