# AGENTS.md: how agents work in this repository

## Who owns what
| Area | Owner |
| --- | --- |
| `apps/org-ui/` | the **app agent** (`app-frontend-dev`) |
| `apps/org-server/`, planned `apps/org-gateway/`, `crates/org-office/`, `crates/org-client/`, and the SQL for every table in `store.rs` (messaging's included) | the **server agent** (`server-dev`) |
| `apps/org-office-host/` (the supervisor and login service), planned `apps/org-setup/`, `crates/org-sync/`, `crates/org-pki/`, `crates/org-git/`, `crates/org-github/`, `crates/org-supervise/`, `crates/org-runtime-claude/` | **River** (`ac-software-engineer-river`), Senior Backend Engineer |
| `apps/org-agent/` (the CLI agents use), `crates/org-spool/`, and the messaging library (its crate name is set in ADR-0003) | **Remy** (`ac-software-engineer-remy`), Senior Backend Engineer, since 2026-11-08 (taking over from `agy-senior-dev`, who is out of runtime credits) |
| the work-items library (its crate name is set in `docs/design/tasks-projects-docs.md`, coming), the agent runtime adapters `crates/org-runtime-codex/` and `crates/org-runtime-agy/`, and tooling in the agents repository (the inbox adapters) | the **codex agent** (`codex-dev`) |
| `crates/org-proto/`, `crates/org-sync-proto/`, `crates/org-office-launch/` and `crates/org-runtime/` (the contracts). `org-office-launch` is the pipe protocol between the host and an office server, and `office.json`, which `org-agent` reads through it. `org-runtime` is the contract the agent runtime adapters implement. River writes the first version of both, and ada reviews them like any contract change. Also `docs/` (except `docs/specs/`) and this file | **ada** (`ac-staff-engineer-ada`), Staff Engineer, who also reviews and gives the "go" on every merge |
| `docs/specs/` (the UX specs) | **Eden** (`ac-product-designer-eden`), Principal Product Designer. Her changes land through ada |

Don't edit another owner's area. If you need a change there, message its owner.

**Traits are owned by the crate that defines them** (e.g. `org-sync`'s storage and apply traits: River; `org-mailbox`'s `MessageStore`: the messaging owner). The implementer of a trait (usually server-dev, in `store.rs`) must agree to any change to it. A change to `org-proto`, `org-sync-proto`, `org-office-launch` or `org-runtime` goes through ada.

The product is named **Agentic Org**. The designs that are the reference for current work are in `docs/design/` (offices, teams, projects and GitHub), with decisions in `docs/adr/` and the UX specs in `docs/specs/`.

The owner-requested [v1 project structure](docs/design/project-structure.md) defines the target names and domain-module/vertical-slice layout. The worker and UI renames to `apps/org-server` and `apps/org-ui` are implemented; domain moves remain pending. Owners above retain the same responsibilities after the renames.

## Rules
1. **Read [`docs/architecture-rules.md`](docs/architecture-rules.md) first.** Follow the [v1 project structure](docs/design/project-structure.md): apps compose crates, policies own their required contracts, and dependencies on `org-proto` are intentional rather than mandatory for every crate. No source cycles. Logic lives in libraries, and binaries stay thin.
2. **Branches: one open branch per agent.** The full process is [`docs/processes/branch-and-merge.md`](docs/processes/branch-and-merge.md); follow it.
   - You have exactly **one** open working branch. Don't start new work, open a second branch or stack branches until it's reviewed and merged into `v1`.
   - Start every branch from `origin/v1` in your own worktree:
     `git -C <repo> worktree add ../app-<you> -b <you>/<topic> origin/v1`.
   - Keep it small, and merge `origin/v1` into it often. Never push to `main`.
   - After the merge, ada deletes the branch on origin once it's confirmed fully in `v1` (nothing unmerged is ever deleted; closed work is archived as an `archive/<branch>` tag), and tells you when your local branch and worktree can go. Then start your next branch from `origin/v1`.
3. **Before asking for the go:**
   - `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass
   - for the app, the type check, its tests and a build pass
   - **your own independent review** of your whole diff (a fresh subagent you start, with no context: valid, reliable, secure, meets the brief), with every confirmed finding fixed. This is separate from step 4, and step 4 doesn't replace it.
4. **Send it to the reviewer agent** (`reviewer`, on `org-agent`) with: repo, branch, base, commit, and the design or brief it implements (and screenshots for UI work). It reports a verdict and findings to you and ada. Fix, re-run step 3's checks on the fixed commit, and ask it to re-review, until it says **ready** for your final commit. If it doesn't answer within 30 minutes, tell ada, who starts another reviewer so work never stalls.
5. **Ask ada for the go** with: branch, commit, the final lines of every check, your own review's findings and fixes, **the reviewer's ready verdict for that exact commit** (any later commit needs a re-review), and a screenshot for UI work. After the go: `git merge --no-ff` into `v1`, then push `v1` (or ada merges and pushes for you, if your session can't push).
6. **Never write to a user's organization workspace from tests.** Use temporary directories.

## Live messaging during the standalone cutover

The installed CLI described below currently uses live Phase A spool state. The
standalone proposal does not redirect it now. Before switching the team, preserve
pending asks, replies and read/triage state through the conditional cutover in
[ADR-0003](docs/adr/0003-agent-messaging-office-first.md#live-phase-a-coordination-cutover-and-person-started-context).
Standalone person-started commands must select an explicit instance/office or saved
context; managed employees use their injected binding. Old coordination data must
not be discarded under the desktop-app no-migration decision.

## Talking to other agents
Use the **installed** `org-agent` CLI at `~/.local/bin/org-agent`, built from `apps/org-agent` on `v1` (messaging Phase A, over the spool):
```
export ORG_AGENT=<your-id>
org-agent msg register <your-id>                  # once
org-agent msg inbox | read --next | reply <id> "…"
org-agent msg send --to <agent> "…"               # fire-and-forget
org-agent msg ask  --to <agent> --timeout 300s "…" # wait for the answer
org-agent msg wait --timeout 600s                  # block until something arrives
org-agent msg send --to owner "…"                 # reaches the user
org-agent msg defer <id> --until 2h               # park a message
org-agent msg done <id>                           # close it
org-agent who                                     # the team
org-agent status "<what you're on>"               # your status line
```
Check your inbox at the start of every turn and after every step. ada is `ac-staff-engineer-ada`. Keep messages short, with the result or the question first.

## Tailwind Plus (licensed UI)
The app's look uses Tailwind Plus components, which the licence allows in our open-source end product. **Allowed:** using or modifying them (including converting them to Svelte) **inside the product UI application (`apps/org-ui`)**. **Not allowed:** publishing them separately (a shared UI kit or component package, a template or starter, a Figma kit), or building a website-builder or admin-panel product from them. Never move them into `crates/` or a standalone package.
