# Kickoff prompt for v1 implementation

Written 2026-11-17, after the architecture review. Send the prompt below to each
engineer's agent, with the track letter filled in. Two tracks keep two writers out of
each other's files. Update this file when the order of work changes.

```text
You are working on Agentic Org in the repository vgmello/agentic-org. Start from the
latest origin/v1-integration.

Read first, in this order:
1. AGENTS.md (how to work here, and the goal).
2. docs/design/conflict-resolution-plan.md, section "Handoff for implementing agents".
3. docs/design/documentation-authority.md, then the documents it names for your track.
4. docs/FEATURES.md rows for your track. That checklist is where status is recorded.

The goal: get the two engineers and their agents working inside this product as early
as it can carry them. Creating an organization, signing in, people and teams, tasks and
chat come first.

Rules that override anything older you find:
- Nothing has shipped. Write no migration, import, compatibility shim, version
  negotiation for old clients or legacy handling. Replace, do not adapt.
- The requirements are the documents. Code that disagrees with them is what you change.
  If a requirement is missing or two documents disagree, stop and ask; do not invent.
- IDs are UUIDv7. Task references are minted by the creating office (T-B37).
  The browser-facing process is "web", the agent CLI is agent-os.
- Paths are durable/ and runtime/<office-id>/ only.

Your track is: <A or B>.

Track A, the way in (host, setup, web, sign-in):
  one launch contract with per-person grants (host boundary design, FC-017);
  no database encryption, private keys wrapped (ADR-0016, FC-162);
  setup and create organization with the selected payload (FC-012, FC-018);
  stored browser sessions (ADR-0013, FC-159); the web process serving the UI and the
  work reads it needs (FC-013).
  Outcome: a person runs setup, creates an organization and signs in from a browser.

Track B, what they do once in (office store and domain):
  rebuild the office store against the selected data contract, files as the source and
  SQL as the projection, behind engine-independent contracts (ADR-0018, FC-165,
  FC-141); organization, single home team and people (FC-028); one typed task with the
  status list and office-minted numbers (FC-040, FC-142, ADR-0019); messaging and asks
  (document 15, ADR-0014, ADR-0020, FC-160, FC-166); remove areas (FC-056).
  Outcome: people and agents create teams, tasks and messages through agent-os and
  the web API.

Before writing code, reply with a short plan for your track: the first three
increments, what a person can do after each, the files you will own, and any question
the documents do not answer. The storage rebuild needs its plan agreed first.

For every increment: branch from origin/v1-integration, keep the checks green
(cargo fmt, clippy, tests; the UI checks for UI changes), update the FEATURES row with
evidence, get an independent review, then merge into v1-integration. Never push to
main. Tests never touch a real organization's data.
```

## Known gaps to settle in the first plans

- Track B: how the first directory generation is admitted, and how a newly created task
  first enters the acceptance journal
  ([recovery contract](../design/organization-data/16-recovery-and-office-continuity.md#no-adoption-or-staging-interface)).
- Track A: Linux process admission, the Linux credential provider and the Linux
  confinement adapter each need a short design
  ([ADR-0017](../adr/0017-linux-and-container-hosting.md)).
- Later tracks, not started by this prompt: running agents (ADR-0015, the first vendor
  adapter, the sign-in spike), the session view
  ([REQ-05](../specs/req-05-virtual-console-conversation-log-viewer.md)), question
  components ([REQ-04](../specs/req-04-channel-messaging-and-question-components.md))
  and local Git ([Git design](../design/projects-git-github.md), with its open questions).
