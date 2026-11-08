# AGENTS.md: how agents work in this repository

## Who owns what
| Area | Owner |
| --- | --- |
| `apps/desktop/` | the **app agent** |
| `apps/org-office-server/`, `apps/org-office-host/`, `crates/org-office/`, `crates/org-client/`, `crates/org-sync/`, `crates/org-pki/`, `crates/org-github/`, `crates/org-git/`, and the SQL for every table in `store.rs` (messaging's included) | the **server agent** |
| `apps/org-agent/` (the CLI agents use), `crates/org-spool/`, and the messaging library (its crate name is set in ADR-0003) | the **messaging agent** (`agy-senior-dev`) |
| `crates/org-proto/` and `crates/org-sync-proto/` (the contracts), `docs/`, this file | **ada** (`ac-staff-engineer-ada`), Staff Engineer, who also reviews and gives the "go" on every merge |

Don't edit another owner's area. If you need a change there, message its owner. A change to `org-proto` or `org-sync-proto` goes through ada.

The product is named **Agentic Org**. The designs that are the reference for current work are in `docs/design/` (offices, teams, projects and GitHub), with decisions in `docs/adr/`.

## Rules
1. **Read [`docs/architecture-rules.md`](docs/architecture-rules.md) first.** Dependencies point one way (apps → crates; everything → `org-proto`). No cycles. Logic lives in libraries, and binaries stay thin.
2. **Branches:** work on your own branch, created from `origin/v1` in your own worktree:
   `git -C <repo> worktree add ../app-<you> -b <you>/<topic> origin/v1`.
   Merge `origin/v1` into your branch often. Never push to `main`.
3. **Before asking for the go:**
   - `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass
   - for the app, the type check, its tests and a build pass
   - an **independent review** of your whole diff (a fresh reviewer with no context: valid, reliable, secure, meets the brief), with every confirmed finding fixed
4. **Ask ada for the go** with: branch, commit, the final lines of every check, the review findings and what you changed, and a screenshot for UI work. After the go: `git merge --no-ff` into `v1`, then push `v1` and your branch.
5. **Never write to a user's organization workspace from tests.** Use temporary directories.

## Talking to other agents
Use the **already-installed** `org-agent` CLI at `~/.local/bin/org-agent`. That is the working older version, built from branch `v1-old`. `apps/org-agent` in this repo is its successor, still a stub, and it takes over once ported:
```
export ORG_AGENT=<your-id>
org-agent msg register <your-id>                  # once
org-agent msg inbox | read --next | reply <id> "…"
org-agent msg send --to <agent> "…"               # fire-and-forget
org-agent msg ask  --to <agent> --timeout 300s "…" # wait for the answer
org-agent msg wait --timeout 600s                  # block until something arrives
```
Check your inbox at the start of every turn and after every step. ada is `ac-staff-engineer-ada`. Keep messages short, with the result or the question first.

## Tailwind Plus (licensed UI)
The app's look uses Tailwind Plus components, which the licence allows in our open-source end product. **Allowed:** using or modifying them (including converting them to Svelte) **inside `apps/desktop`**. **Not allowed:** publishing them separately (a shared UI kit or component package, a template or starter, a Figma kit), or building a website-builder or admin-panel product from them. Never move them into `crates/` or a standalone package.
