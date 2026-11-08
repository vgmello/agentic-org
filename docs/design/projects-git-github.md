# Design and plan: projects as git repositories, connected to GitHub

Status: **draft for the user**, 2026-11-08. Owner: ada. It follows the user's decision: project files are git repositories, and the GitHub connection comes first. It builds on `offices-hq-and-satellites.md`.

## In one paragraph
Each project is a **git repository on GitHub**. Every office that works on a project **clones it from GitHub**, and its agents work in their own **git worktrees**, one per task, on their own branches. GitHub is the hub for files, so there's no git server of our own and no file syncing between offices: offices tell each other "I pushed" through the normal work-data stream, and the others fetch. Each office **connects to GitHub once**, through the standard "enter this code at github.com/login/device" sign-in. The token stays in that office's Keychain, is never copied to other offices, and **agents never see it**: the office fetches and pushes for them.

## What lives where
- **Organization data** (HQ-owned, copied everywhere): the project record. It holds the project's id, name and team, and **its repository**: `github`, owner, name, numeric id, default branch, private or public. It never holds a token.
- **Office data** (each office's own): its GitHub connections (the "agentic.org" app at every office, plus the "agentic.org admin" app at HQ: which GitHub user and installation, and the tokens in its Keychain), its clones and worktrees, and each project's clone status.
- **On disk:** `<home>/offices/<office-id>/workspace/projects/<project-id>/`
  - `mirror.git` is the **office's own bare mirror**. Only the office runs git in it, and it's the only repository that talks to GitHub.
  - `work.git` is a **bare** local clone of the mirror, for agents. It has no checked-out branch, so any branch can get a worktree.
  - `worktrees/<task-id>/` holds each task's working copy, from `work.git`, on branch `task/<task-id>-<slug>`.

  Agents' git settings (`git config` in a worktree writes to `work.git/config`) never touch the repository that holds the network settings and uses the token.
- **Encryption:** these are ordinary files. FileVault protects them, not our database (ADR-0002, "Not covered here").

## Connecting GitHub (the first feature)
**Two GitHub Apps**, registered once by you, the project's owner, both with the device flow and token expiry enabled. Their **client ids are public** and ship in the open-source app. Neither the device flow nor token refresh needs the client secret (GitHub's docs: the secret is required "unless the user access token was generated using the device flow"), so no secret ever ships.
- **"agentic.org"** (every office): **Contents: read and write** and **Metadata: read**. It can clone, fetch and push, and that's all.
- **"agentic.org admin"** (HQ only): adds **Administration: write**, needed to **create** repositories. It also allows deleting them. Its tokens are kept **only at HQ**, as HQ-only data, so a satellite's token can never create or delete repositories. We never call delete.

Splitting them means a leaked satellite token can push to repositories, but never delete one. We recommend installing on **selected repositories** (open question 4).

**The flow in the app:** Settings → GitHub → **Connect**. Every office connects the "agentic.org" app. **At HQ, the app then runs the same flow a second time for "agentic.org admin"**, with its own code and its own install, so HQ can create repositories. The calls take an `app` parameter (`contents` or `admin`), and `admin` is accepted only at HQ.
1. **Authorize.** The office starts the device flow, and the app shows a short code and opens `github.com/login/device`. The owner enters the code. The office polls at GitHub's interval, adding 5 seconds whenever GitHub answers `slow_down`.
2. **Install.** Authorizing doesn't install the app. If no installation covers the chosen account or GitHub organization, the app sends the owner to the app's install page. The owner picks "All repositories" or selected ones (open question 4), and the app checks again.
3. **Keep the tokens.** User tokens last 8 hours and come with a refresh token that lasts 6 months and works once.
   - The office server stores both in **its own Keychain**, creating and reading the items itself (ADR-0002's per-program rule).
   - It refreshes one at a time, because refresh tokens are single-use.
   - An office offline for more than 6 months must connect again.
4. **Disconnect.** The office deletes the tokens locally and links the owner to GitHub's "Authorized GitHub Apps" settings to revoke the authorization. Revoking through the API would need the client secret.
   - Revoking there cancels **every office's** authorization for that app, because they share your GitHub account. So **removing an office** means revoking at GitHub, then reconnecting the other offices: the app walks the owner through it. Until the revoke, a removed office's token keeps working, for up to 6 months.

## Projects
- **Create project** (in the app, so at HQ): a name, a team (later), and either **a new repository** or **an existing repository**.
  - A new repository's owner is your account or a GitHub organization. It's private by default.
  - An existing repository is picked from the installation's repositories.
- **The repository is created before the project.** HQ's office server calls GitHub first, then submits the "create project" command, which carries the repository's details. GitHub calls never happen inside a command, so events stay deterministic (offices design).
  - **On a retry**, an **empty** repository of that name whose numeric id HQ recorded when it created it (in HQ's `office.db`, before submitting the command) is adopted, not duplicated. Any other repository with that name fails with "name taken".
  - **With a selected-repositories installation**, the new repository is added to the installation, if G2a shows that's needed and allowed. Otherwise the owner is told to add it.
- HQ records the project, with its repository, as an organization event, so every office learns of it.
- **Cloning:** an office clones a project (bare) the first time one of its agents needs it, or right away for projects it follows. It runs as a background job with progress (`cloning`, `ready`, `failed: reason`).

## Agents and git
- **A worktree per task.** When an agent starts a task in a project, its office runs `git worktree add` from `work.git` on branch `task/<task-id>-<slug>`. The branch starts from the default branch, or from the branch the task names.
  - Branch names are checked with `git check-ref-format`.
  - The office always passes `--` before any path or ref that came from an agent.
  - Agents never share a working copy.
- **The office does all network git work, and agents never hold the token.**
  - **Fetching:** the office fetches from GitHub into `mirror.git`. Then it updates `work.git` from the mirror with the same hardening as every office git command: run with `GIT_CONFIG_GLOBAL=/dev/null` and `GIT_CONFIG_NOSYSTEM=1`, from the mirror's explicit path (never a remote name or URL from `work.git`'s config, so an `insteadOf` rule or remote URL there can't redirect it), with explicit refspecs, `protocol.allow=never` except `file`, hooks off, and no token. Agents read everything locally.
  - **Pushing:** agents push with **`org-agent push`**.
    - The office checks that the branch is the agent's own `task/…` branch, in a project of that office.
    - It copies that one branch **into `mirror.git`, running in `mirror.git`**: a fetch from `work.git` by explicit path and refspec. So `work.git`'s config (which agents can edit) is never used and none of its hooks run. The copy uses the same `GIT_CONFIG_*` and hooks-off settings, and never gets the token's file descriptor.
    - It pushes it from the mirror: never to the default branch, never a force push.
  - **Every network git command the office runs is hardened:**
    - an explicit `https://github.com/<owner>/<repo>.git` URL and a full refspec
    - `GIT_CONFIG_GLOBAL=/dev/null` and `GIT_CONFIG_NOSYSTEM=1`
    - `-c credential.helper=` to clear any helper, then only our own, scoped to that exact URL
    - `protocol.allow=never` except `https`
    - no proxy settings
    - **the token passed on an inherited file descriptor**, never in arguments (visible in `ps`) or the environment

    `worktree add` and every local command run with hooks, fsmonitor and filters off (`-c core.hooksPath=/dev/null -c core.fsmonitor=false`, no `filter.*` from agent config).
  - **Residual risk, stated plainly:** agents run as the **same macOS user** as the office. A determined agent could edit the mirror's files directly, or reach the office's Keychain items through the same user. The separate mirror and the hardening stop accidents and ordinary git misuse, not a hostile agent. Real isolation means running agents as a separate macOS user or in a sandbox. That's a later decision, and for now offices and agents are all yours (decision 8).
  - **No server-side guard on GitHub Free:** rulesets that protect the default branch need GitHub Pro, Team or Enterprise for private repositories. The app detects the plan, and HQ sets them up with the admin app (rulesets need Administration) where available. Without them, **the office's push check is the required control**, and it's tested (G2c).
- **One git operation at a time per repository** (fetch, worktree add or remove, push), which avoids `.git` lock races.
- **Commit identity:** commits are authored as the agent (e.g. `ada (agentic.org)`) with the connected account's **GitHub no-reply address** (`<id>+<login>@users.noreply.github.com`). Pushes aren't rejected when the account hides its email, and the commits are still tied to it (open question 3).
- **When a task moves to another office** (a handover or reclaim, offices design):
  - The old office pushes the task branch, including work in progress, before it hands over.
  - The new office fetches it and checks out the same branch in its own worktree.
  - On a reclaim from an unreachable office, the new office continues from the branch as last pushed.
- **When a task is done:**
  - Its branch has been pushed, and the office removes the worktree.
  - The branch stays on GitHub until it's merged or deleted.
  - Merging into the default branch is a later decision (pull requests, review rules).

## Keeping offices in step
- **After a push, the office emits a work change** (office-first): "project P, branch B is now at commit C". It reaches HQ and the offices that follow P. They fetch when they need it, or straight away if they follow the project.
- **Changes made outside agentic.org** (you pushing from your laptop, a merged pull request) are picked up as a backstop: offices fetch every few minutes for projects they follow. Webhooks would need a public server, which we don't have. Fetching doesn't count against GitHub's API rate limit.

## Code layout (architecture rules)
- **`org-github`** (new adapter crate): the GitHub calls we need, over a blocking HTTPS client (`ureq` with rustls, exact pins).
  - It covers the device flow with `slow_down`, token refresh, the current user, installations and their repositories (`/user/installations/{id}/repositories`), creating a repository, and rulesets.
  - It handles rate limits and gives clear errors. No async runtime.
- **`org-git`** (new adapter crate): runs the system `git` (mirror, work clone, fetch, worktree add and remove, push) with the hardening above, one operation per repository at a time. Using the system `git` from Xcode's command line tools keeps agents and the office on the same git.
- **`org-office`** (domain): project commands and events, the repository record, and the follow list. It depends on neither adapter. It uses small traits, "create repository" and "clone project", which the server implements with the adapters.
- **`org-proto`** (contract) gains the calls below.
- **`org-agent`** gains `push`, which is only a socket call, so it stays tiny and doesn't depend on `org-git`.
- **Owners (AGENTS.md):** the server agent owns `org-github` and `org-git`.

## Implementation plan
Order: **after the HQ office is serving** (create organization, phase 1), and **before satellites**, as the user asked.

| Step | What | Who | Depends on |
| --- | --- | --- | --- |
| G2a | A **spike** (about half a day) with the registered apps. It confirms: the device flow from Rust; token refresh without a secret; the exact permission needed to create repositories under an account and under an organization; whether a new repository is reachable under a selected-repositories install; creating rulesets. Then the office does a bare clone, a worktree and a push | server-dev | you register the two GitHub Apps |
| G1 | The contract in `org-proto`, frozen **after** G2a | ada | design approved, G2a |
| G2b | `org-github`, the Keychain token store with refresh, and the `github.*` calls in the office server, tested against a local fake GitHub | server-dev | G1, G2a |
| G2c | `org-git`, the mirror and work clones, the worktree per task and `org-agent push`, tested over HTTP (`git http-backend` behind the fake GitHub, so authentication is exercised; `file://` would skip it). Includes tests with a **hostile `work.git` config** (a credential helper, `insteadOf`, a proxy, filters) and a push to the default branch, all of which must fail to leak or succeed | server-dev | G1, G2b |
| G2d | Project commands and events in `org-office` (create with a new or existing repository, list, get, follow), with the "create repository" trait implemented by `org-github` | server-dev | G1, G2b, the HQ event log |
| G3 | App: Settings → GitHub (code, browser, waiting, the install step, connected, disconnect, errors); Create project (name, new or existing repository, visibility); the project list with clone status. Built on a mock first | app-frontend-dev | G1 (mock), then G2b–d |
| G4 | End-to-end test: create an organization → connect GitHub (a fake server in CI, real GitHub as a manual check) → create a project with a new repository → an agent task gets a worktree, commits, and pushes through its office | server-dev + app-frontend-dev | all |

Each step: its own branch, an independent review, my go, then a merge into `v1`.

**G1, the contract** (drafted now, frozen after G2a):
- `github.status { app } → { connected, login?, installation?: { account, all_repositories } }`
- `github.connect.start { app } → { user_code, verification_uri, expires_at, interval_s }`
- `github.connect.wait { app, timeout_ms } → { connected, login, needs_install, install_url? } | error (expired, denied, timeout)`
- `github.disconnect { app } → { revoke_url }`
- `app` is `contents` or `admin`; `admin` is accepted only at HQ.
- `github.repos.list { cursor? } → { repos, next_cursor? }`
- `project.create { command_id, project_id, name, repo: { new: { owner, name, private } } | { existing: { repo_id } } } → Project`
- `project.list → { projects }` and `project.get { id } → Project`. A project includes this office's clone status.
- `project.follow { id, follow }`
- **`org-agent` → its office:** `git.push { task_id, branch }` → `{ commit }` (the office's checks, then the push)
- Events: `project.created`, `project.clone_status`
- **Work change** (office-first stream): `project.branch_pushed { project_id, branch, commit, task_id? }`. Offices use it for fetching and for task handovers.
- **The app connected to a satellite** creates a project at HQ like any organization command, relayed through its office. HQ, which holds the admin app, creates the repository.
- **Internal:** periodic fetch for followed projects, and the push before a handover. Both are office behaviour, not calls.

## Open questions for the user
1. **Registering the two GitHub Apps** ("agentic.org" and "agentic.org admin"): one form each on GitHub (Settings → Developer settings → GitHub Apps), with the device flow and token expiry enabled and the permissions above. Under your personal account or a GitHub organization? I'll write the exact steps.
2. **Where new project repositories go:** your personal account, or a GitHub organization (which one)? Private by default?
3. **Agents' commits:** authored under the agent's name with your account's no-reply email (proposed), or under your own name with the agent as a co-author?
4. **Install scope:** selected repositories (recommended: a leaked token reaches only project repositories) or "All repositories" (simpler: new repositories are covered automatically)?
5. **Each office signs in to GitHub itself** (proposed: simple, and all offices are yours), rather than HQ sharing its token with satellites. Is that OK?
6. **Running agents as a separate macOS user** (or sandboxed) would truly keep tokens and the office's files out of agents' reach. Not now, but should it be on the roadmap?
