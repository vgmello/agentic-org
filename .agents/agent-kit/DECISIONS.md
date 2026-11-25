# Active constraints and decision index

A compact index for agents. The documents it points to decide; where this page and
one of them differ, the document wins. Architecture decisions are recorded in
`docs/adr/`.

## How work is done

- Workflow: [AGENTS.md](../AGENTS.md). Two engineers, each with a lead agent that owns
  an increment of the [implementation plan](../docs/implementation-plan.md) end to end.
  No path belongs to a named person. One writer per file at a time; at most two
  concurrent source writers within an increment, in separate worktrees.
- Integration: [branch and merge](../docs/processes/branch-and-merge.md). Branch from
  `origin/v1`; checks; fresh local review, original-author validation and a
  separate fresh final review; `git merge --no-ff` into `v1`. No pull
  request or outside reviewer. Never `main` without an explicit release
  instruction.
- Decisions: [architecture non-negotiables](../docs/architecture-non-negotiables.md),
  "Who decides what". Decide inside the non-negotiables, record the decision in the
  owning document (and an ADR when architectural), bring to the owner only what changes
  the product for people or cannot be undone.
- Blocked work: preserve the branch and its checkpoint, continue independent work.
- Delivery: [independent delivery](../docs/independent-delivery.md). An internal
  foundation is labelled as such and is not a usable feature.

## What the product is

- Authority: [documentation authority map](../docs/design/documentation-authority.md)
  and the [data contract](../docs/design/organization-data/README.md). A requirement
  document does not establish that code exists; the checklist row's evidence and
  [executable migration](../docs/design/executable-migration.md) say what exists.
- Nothing has shipped: no migration, import, compatibility shim or legacy handling.
  Nothing from the earlier coordination tool or its message queue is carried over.
- Four executables: `agentic-org` (person's command line; `serve` is the host),
  `agentic-org-office` (all domain rules, only writer of organization data),
  `agentic-org-web` (the browser-facing "web" process; no keys, no database),
  `agent-os` (the only way an agent reaches the product). No desktop app.
- Identity: UUIDv7 IDs; task references minted by the creating office (`T-B37`).
- Storage: files are the source, databases are projections; `durable/` and
  `runtime/<office-id>/` only; one writer per file. No application-level database
  encryption ([ADR-0016](../docs/adr/0016-no-application-level-data-encryption.md)):
  the host-held office secrets key wraps the office's private keys.
- Getting in: setup prints a one-time registration link and creates no account or
  organization; one administrator account; accounts by invitation from the browser;
  the signed-in administrator creates an organization. Browser sessions are stored at
  the host ([ADR-0013](../docs/adr/0013-stored-browser-sessions.md)).
- Agents: hired only through the built-in HR, started by the office, headless in a
  private home ([ADR-0004](../docs/adr/0004-headless-agent-sessions.md),
  [ADR-0015](../docs/adr/0015-agent-isolation-modes.md)). Chat is the product's own.
- Platforms: macOS and Linux, including a container
  ([ADR-0017](../docs/adr/0017-linux-and-container-hosting.md)).
- UI licensing: Tailwind Plus stays inside `apps/agentic-org-ui`, never in a published
  kit, a package or `crates/`.

## Kit

- Mode `codex-only` (`policy/model-routing.json`). Adoption state and checkout setup:
  [ADOPTION.md](ADOPTION.md).
- Current task record: [tasks/v1-state.md](tasks/v1-state.md).
