# Kickoff prompt for v1 implementation

Send the prompt below to each engineer's agent, with the track and the first increment
filled in. The tracks and increments are defined in the
[implementation plan](../implementation-plan.md). Update this file when the plan changes.

```text
You are working on Agentic Org in the repository vgmello/agentic-org. Start from the
latest origin/v1-integration.

Read first, in this order:
1. docs/START-HERE.md: what the product is, where each kind of requirement is written,
   and how to go from a feature to working code.
2. AGENTS.md: how to work in this repository.
3. docs/implementation-plan.md: the milestones and increments. Read your increment and
   every document it lists under "Read".

Your track is <A, B or C> and your first increment is <for example A1 or B1>.

Rules that always apply:
- Nothing has shipped. Write no migration, import, compatibility shim, version
  negotiation for old clients or legacy handling. Replace, do not adapt.
- The requirement documents decide behaviour. Code that disagrees with them is what you
  change. If a requirement is missing or two documents disagree, stop and ask; do not
  invent.
- Build the smallest path a person or agent can use, not a layer.

Before writing code, reply with a short plan for your increment: what a person can do
when it is finished, the files you will own, what you will delete, the acceptance
criteria you will make pass, and any question the documents do not answer.

For every increment: branch from origin/v1-integration, keep the checks green, update
the row in docs/FEATURES.md with evidence, get an independent review, then merge into
v1-integration. Never push to main. Tests never touch a real organization's data.
```

Suggested start: one engineer takes Track A from A1, the other takes Track B from B1.
B1, the store foundation, is the largest increment and starts with a written plan agreed
with the owner.
