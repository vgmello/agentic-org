# P4b note: org-process render, and inputs for the cache layer (Rory)

## Input for the cache layer's owner (org-office processes: server-dev / River)

Ruled 2026-11-13 by ada (processes.md §3.3, option a):

- `org_process::fingerprint` keys a set by the handbook commit, the project's integration commit
  (or none), the project and the team chains (any order). It doesn't include the agent, so a
  cache keyed by it alone holds one agent's sets.
- **Place names are not in the fingerprint.** A set carries them (`Inputs::names`) in its
  "Turned off for <name>", conflict and summary lines. When a team or project is renamed, the
  cache's owner drops the cached sets that name it. Otherwise the key stays the same and the
  old name keeps being served.
- `EffectiveSet.inputs` lists places in chain order. A set cached under the
  order-independent key can come back with that list in a different order. Winners and
  conflicts don't depend on it.
- `render` returns `delivered()`: the (key, blob) pairs whose full text it emitted. Only those
  count as read, once the runtime accepts the start or wake (§3.4). Never mark the named-only
  ones as read.
- `RenderInput.texts[key]` must be the winning file's blob and text: render can't check it
  (a winner carries no blob), so a stale entry would be delivered under the winner's version.
- For a fresh context (a new main session or delegate, or after a compaction, §3.5), pass an
  empty `read`, so every applicable text is offered again. Agent-wide read history doesn't
  prove that a new context holds the text.

## Choices made in render (each one pinned by a test)

- **The cap** is 24 KiB of UTF-8 for the whole section. Summaries come first, then changed
  texts, then moment prompts (they are reminders; the set and the texts are rules). Only whole
  entries are emitted.
- **Changed texts** are a prefix, organization first. Once one is named (`process show <key>`),
  every later one is named too. If even the names don't fit, an "and N more changed" line is used.
- **Summaries are a prefix:** an oversized early summary hides the later ones (the count line
  says how many), so level-then-key order holds.
- **Room is reserved:** when something changed or a moment is On, the summaries leave room
  for that part's heading and its count line, so a change is never left unmentioned.
- **A winner with no text from the office** counts as changed and is named, never delivered.
- **The label** is the place: "org", the team's or project's name (else its id or key), or
  "private" for an agent's own process.
- **Wording I chose,** since the spec has no exact text: the conflict lines "T's version
  applies until the project decides." (a can't-tell seal) and "No version applies until the
  project decides."
