# Design and plan: projects as git repositories, connected to GitHub

Status: **approved by the user** 2026-11-08 (decisions at the end); amended by ada 2026-11-08 (owners: River for the adapters; real-GitHub testing in v2, the user's decision). Owner: ada. It follows the user's decisions: project files are git repositories, the GitHub connection comes first, and **one GitHub App, managed by HQ** (revised the same day). It builds on `offices-hq-and-satellites.md`.

> **For now: local git** *(the user, 2026-11-08: "let's use a local git for now")*. Projects use a local bare repository on disk as their remote, with the same `mirror.git`/`work.git` layout, the same office-run push and the same hardening. The remote sits behind a small interface (a URL plus a credential source), so moving to GitHub later is a configuration change. Phase 1 projects still run on local git. The GitHub pieces below (the app, tokens, repository creation) are built and tested in phase 1 against a fake GitHub. Real GitHub comes in v2 *(the user, 2026-11-08)*.

## In one paragraph
Each project is a **git repository on GitHub**. **One GitHub App, "Agentic Org"**, is installed on your account, and **HQ holds its private key**. Whenever an office needs GitHub (fetch, push, a pull request), **HQ mints it a short-lived token** (one hour) limited to **that one repository and only the permissions that job needs**, and sends it over the office's connection. Offices never sign in to GitHub and never keep a GitHub credential. Every office clones the projects it works on, and its agents work in their own **git worktrees**, one per task. GitHub is the hub for files: offices tell each other "I pushed" through the work-data stream, and the others fetch. HQ also **creates project repositories automatically**.

## What lives where
- **Organization data** (HQ-owned, copied everywhere): the project record. It holds the project's id, name and team, and **its repository**: `github`, owner, name, numeric id, default branch, private or public. It never holds a token.
- **HQ-only data** (never copied): the app's **private key**, the app's installation id, and, when the device-flow path is used, HQ's own user token for creating repositories (see "Creating repositories"). (amended 2026-11-08: key custody) Both secrets are held by **the host** on HQ's Mac, in its protected keychain items (the same access group as the office database keys, `office-host-boundary.md`), never by the office server. The host gives them to HQ's server in the **first launch line** of each start (with the app token and the database key; the launch protocol has no later host-to-server lines), and the server keeps them in memory only. A secret that HQ's server obtains while running (a `github.import_key`, or the device-flow user token) goes back to the host as a **stdout control line**, `store_secret { kind, secret }`, which `office-host-boundary.md` and `org-office-launch` add. Control lines are never logged (stderr is the log), and the host stores the secret before acknowledging it.
- **Office data** (each office's own): its clones and worktrees, each project's clone status, and **tokens in memory only**, never written to disk.
- **On disk:** `<home>/offices/<office-id>/workspace/projects/<project-id>/`
  - `mirror.git` is the **office's own bare mirror**. Only the office runs git in it, and it's the only repository that talks to GitHub.
  - `work.git` is a **bare** local clone of the mirror, for agents. It has no checked-out branch, so any branch can get a worktree.
  - `worktrees/<task-id>/` holds each task's working copy, from `work.git`, on branch `task/<ref>-<slug>`.

  Agents' git settings (`git config` in a worktree writes to `work.git/config`) never touch the repository that holds the network settings and uses the token.

  (amended 2026-11-08: agent sessions rev 3, ada) For sessions the office runs, `work.git/config` and `work.git/hooks/` are read-only to agents, and the office sets agents' hooks path and git config through the environment. See `agent-sessions.md`, "Git in worktrees". (amended 2026-11-08: agent sessions rev 3.3, ada) Agents can't write any git config, `config.worktree` included, so the office doesn't set `extensions.worktreeConfig`. This layout puts every worktree and `work.git` inside the office home: agents get the session's own worktree and the literal `work.git` commit paths as their only writable roots there, and named denies for the rest (`agent-sessions.md`, "What agents can reach under `<office home>`").
- **Encryption:** these are ordinary files. FileVault protects them, not our database (ADR-0002, "Not covered here").

## The app and HQ
**One GitHub App, "Agentic Org"** *(the user, 2026-11-08)*, registered by you on 2026-11-08: **Client ID `Iv23lidV8LP1kzk02Fnx`** (public). Registration steps below and **installed on all repositories** of your account. New repositories are covered automatically.
- **Permissions** (repository permissions):
  - **Contents: read and write**: clone, fetch, push
  - **Pull requests: read and write**: see and create pull requests
  - **Actions: read**: see workflow runs
  - **Metadata: read**
  - **Repository creation: write**: create project repositories (GitHub's narrow permission from 13 October 2026; no Administration)
  - **Workflows: no access** by default, so agents can't change CI files (open question 1)
- **The private key** is the app's master credential: it can mint any token within those permissions, on any repository.
  - You generate it on GitHub and **import it into Agentic Org at HQ** (Settings → GitHub → Import key: the app's **Client ID** and the `.pem` file), then delete the downloaded file.
  - Before storing anything, HQ checks them: it signs a JWT, calls `GET /app` and `GET /app/installations`, and finds the installation.
  - Then it sends the key to the host as a `store_secret` control line, and the host stores it in its protected keychain item, as HQ-only data (amended 2026-11-08: key custody; the office server never touches the Keychain). The Client ID and installation id are kept with HQ's data.
  - No client secret is ever generated.
- **Minting a token** (HQ only):
  1. HQ signs a short JWT with the private key: RS256, `iss` = the Client ID, `iat` backdated about 60 seconds for clock drift, `exp` at most 10 minutes later.
  2. It calls `POST /app/installations/{installation_id}/access_tokens` with **`repository_ids: [that repository]`** and **`permissions`: only what the job needs**:
     - fetch → `contents: read`
     - push → `contents: write`
     - pull request → `pull_requests: write` + `contents: read`
     - workflow runs → `actions: read`
     - **`gh`** (an agent's GitHub CLI token) → `pull_requests: write` + `contents: read` + `actions: read`

     This purpose → permissions table is the only place that maps purposes to permissions.

  The token lasts **one hour**. HQ caches it per (office, repository, permission set) and mints a new one about 5 minutes before expiry.
- **Handing it out:** an office asks HQ for a token over its authenticated connection (office-to-office protocol), naming the project and the purpose.
  - HQ checks the office works on that project (it follows it, or one of its agents has a task in it), then returns the token and its expiry.
  - The office keeps it **in memory only** and passes it to git on an inherited file descriptor (see "Agents and git").
  - HQ's own office uses the same path internally.
- **Removing an office (or one suspected compromised):** HQ stops minting for it and **revokes the tokens it gave it**: it cached them per office, and `DELETE /installation/token` revokes the token it's called with. Nothing else needs reconnecting.
- **A satellite that can't reach HQ** can't get new tokens. Its agents keep working locally (worktrees, commits), and pushes wait in its outbox until HQ is back. The one-hour limit is the trade-off for never keeping credentials on satellites.

## Creating repositories
- **Create project** (in the app, so at HQ): a name, a team, and either **a new repository** (created automatically under your account, **private**, named after the project unless you change it) or **an existing one**, picked from the installation's repositories.
- **Which credential creates it** isn't known yet. Creating a repository under a **personal** account may not work with an installation token: community reports say installation tokens can only create repositories in GitHub organizations. So phase 1 builds **both paths** against the fake GitHub, and v2's live run (G2a) picks or confirms one:
  - an installation token with `repository_creation: write`;
  - or, as the fallback, HQ signs in **once** with the same app through the device flow (Settings → GitHub → "Allow creating repositories"), and keeps that user token and its refresh token in its Keychain as HQ-only data, using it only to create repositories.
    - **Stated plainly:** a GitHub App user token can do everything the app can that you can too, on every repository the app is installed on. It isn't limited to creating repositories.
    - Its refresh token lasts 6 months and refreshes without a client secret. So it's as sensitive as the private key, with the same residual risk.
- **It happens before the command.** GitHub calls never happen inside a command, so events stay deterministic (offices design). HQ's office server:
  1. **records the intent** in HQ's `office.db`: name, command id, time;
  2. creates the repository, **private, with an initial commit** (`auto_init: true`, so the default branch exists for worktrees), and **with the command id in its description** as a marker, then records its numeric id;
  3. **waits until the installation can see it**, by looking it up by id. An all-repositories installation covers new repositories;
  4. submits the "create project" command with the repository's details.
- **On a retry or after a crash**, a repository of that name whose description carries this command id is **adopted by its id**, not duplicated, whatever the clocks say. Any other repository with that name fails with "name taken".
- **A project created from a satellite's app** is relayed to HQ, which runs these steps before submitting the command.
- **For an existing repository**, HQ looks it up first (by its numeric id). That confirms the installation can reach it and gets its owner, name, default branch and visibility.
- **Cloning:** an office clones a project (bare) the first time one of its agents needs it, or right away for projects it follows. It runs as a background job with progress (`cloning`, `ready`, `failed: reason`).

## Agents and git
- **A worktree per task.** When an agent starts a task in a project, its office runs `git worktree add` from `work.git` on branch `task/<ref>-<slug>` (amended 2026-11-08: processes; the name W7 uses). The branch starts from the project's `integration_branch` (the default branch when that's empty), or from the branch the task names. (amended 2026-11-08: processes, A56 and decision 14) The office resolves that branch to a sha in `mirror.git` (agents can move refs in `work.git`), fetches that sha into an office-made ref in `work.git` (`refs/office/base/<task>`, deleted after the add) with the hardened fetch below, runs `git worktree add -b task/… -- <path> <sha>` (never `-B`), and records the sha as the branch's base, all as one locked operation. See `processes.md` §4.2.
  - Branch names are checked with `git check-ref-format`.
  - The office always passes `--` before any path or ref that came from an agent.
  - Agents never share a working copy.
- **The office does all network git work.** Agents never hold a token that can push.
  - **Fetching:** the office fetches from GitHub into `mirror.git` with a `contents: read` token. Then it updates `work.git` from the mirror, **running in `work.git`** (so `work.git`'s own config applies, which is harmless: no token is ever present there), with the same hardening as every office git command: run with `GIT_CONFIG_GLOBAL=/dev/null` and `GIT_CONFIG_NOSYSTEM=1`, from the mirror's explicit path (never a remote name or URL from `work.git`'s config, so an `insteadOf` rule or remote URL there can't redirect it), with explicit refspecs, `protocol.allow=never` except `file`, hooks off, and no token. Agents read everything locally.
  - **Pushing:** agents push with **`org-agent push`**.
    - The office checks that the branch is the agent's own `task/…` branch, in a project of that office. (amended 2026-11-08: processes) "Own" means the agent holds the branch's open task branch record; a branch it made by hand, or one already merged, is refused (`processes.md` §4.1).
    - It copies that one branch **into `mirror.git`, running in `mirror.git`**: a fetch from `work.git` by explicit path and refspec. So `work.git`'s config (which agents can edit) is never used and none of its hooks run. The copy uses the same `GIT_CONFIG_*` and hooks-off settings, and never gets the token's file descriptor.
    - It pushes from the mirror with a `contents: write` token for that repository: never to the default branch, never a force push.
    - (amended 2026-11-08: A58) With the same token, it also pushes the `merged/<branch>` and `archive/<branch>` tags and deletes `task/*` branches it created, as "When a task is done" says. It never deletes the default or integration branch, or a tag. The delete is a compare-and-swap, `git push --force-with-lease=refs/heads/task/<b>:<head> <url> :refs/heads/task/<b>`: it removes the ref only if it's still at the checked head and never moves a ref to another commit, so it isn't a force push.
  - **Every network git command the office runs is hardened:**
    - an explicit `https://github.com/<owner>/<repo>.git` URL and a full refspec
    - `GIT_CONFIG_GLOBAL=/dev/null` and `GIT_CONFIG_NOSYSTEM=1`
    - `-c credential.helper=` to clear any helper, then only our own, scoped to that exact URL
    - `protocol.allow=never` except `https`
    - no proxy settings
    - **the token passed on an inherited file descriptor**, never in arguments (visible in `ps`) or the environment

    `worktree add` and every local command run with hooks, fsmonitor and filters off (`-c core.hooksPath=/dev/null -c core.fsmonitor=false`, no `filter.*` from agent config).
- **Pull requests and the GitHub CLI.** An agent can ask its office for a **gh token**: `org-agent gh-token` → `{ token, expires_at }`.
  - The token is limited to its project's repository, with `pull_requests: write`, `contents: read` and `actions: read`, for one hour. The agent uses it with `GH_TOKEN=… gh pr create` (and `gh pr view`, `gh run list`).
  - It **can't push**, so the push rules above still hold.
  - Opening a pull request needs the task branch pushed first, through the office.
- **Residual risk, stated plainly:** agents run as the **same macOS user** as their office.
  - A determined agent could edit the mirror's files directly, or reach the office's memory.
  - **At HQ, it could reach the app's private key** (and HQ's user token, when the device-flow path is used), which can mint any token within the app's permissions, on every repository. That includes pushing to default branches and creating repositories.
  - **With a gh token**, an agent could also update any open pull request's branch from its base (`update-branch`, which writes a merge commit), going around the "own task branch" rule for that kind of write. It still can't merge or push.

  The separate mirror, the hardening and the narrow tokens stop accidents and ordinary misuse, not a hostile agent. Real isolation means running agents as a separate macOS user or in a sandbox, planned for v2 (decision 6). Until then, keep HQ's machine and its agents trusted.
- **The office's push check is the required control**, and it's tested (G2c). The app doesn't set up GitHub rulesets (that would need Administration), and on GitHub Free, private repositories can't have them anyway. If your plan allows them, you can protect the default branch yourself on GitHub, as an extra guard.
- **One git operation at a time per repository** (fetch, worktree add or remove, push), which avoids `.git` lock races.
- **Commit identity** *(decided by the user, 2026-11-08)*: commits are authored under **your name and GitHub no-reply address** (`<id>+<login>@users.noreply.github.com`; HQ learns them once, through the app), with the agent added as a co-author: a `Co-authored-by: <agent name> <agent address>` trailer. Pushes are made by the app's installation token, so GitHub shows the app as the pusher. Commit authorship is unaffected.
- **When a task moves to another office** (a handover or reclaim, offices design):
  - The old office pushes the task branch, including work in progress, before it hands over.
  - The new office fetches it and checks out the same branch in its own worktree.
  - On a reclaim from an unreachable office, the new office continues from the branch as last pushed.
- **When a task is done:**
  - Its branch has been pushed, and the office removes the worktree.
  - (amended 2026-11-08: A58) After a merge, the office deletes the remote task branch only if its head is an ancestor of the project's `integration_branch`, checked in `mirror.git`. First it pushes a `merged/<branch>` tag at that head. A task closed without a merge gets an `archive/<branch>` tag pushed first, and then the branch is deleted. The office only deletes `task/*` branches it created, with the compare-and-swap delete above (never a force push), and removes the local worktree and branch too. See `processes.md` §4.5.
  - Merging into the default branch is a later decision (pull requests, review rules).

## Keeping offices in step
- **After a push, the office emits a work change** (office-first): "project P, branch B is now at commit C". It reaches HQ and the offices that follow P. They fetch when they need it, or straight away if they follow the project.
- **Changes made outside Agentic Org** (you pushing from your laptop, a merged pull request) are picked up as a backstop: offices fetch every few minutes for projects they follow. Webhooks would need a public server, which we don't have.

## Promotion (offices design, phase 4)
A promoted satellite needs the app's private key.
- You generate a new key on GitHub (an app can have several), import it at the new HQ with the Client ID, then delete the old key on GitHub, which cuts off the old HQ.
- If HQ had a user token for creating repositories, you also revoke the old HQ's authorization (Settings → Applications → Authorized GitHub Apps), and the new HQ runs the device flow again.
- The recovery-code escrow in the offices design may later carry the key instead.

## Code layout (architecture rules)
- **`org-github`** (new adapter crate): the GitHub calls we need, over a blocking HTTPS client (`ureq` with rustls, exact pins).
  - It covers: the app JWT (RS256), minting scoped installation tokens, revoking an installation token, finding the installation, listing its repositories, looking a repository up by id, creating a repository, and the device flow for HQ's user token (both creation paths are built in phase 1).
  - It handles rate limits and gives clear errors. No async runtime.
- **`org-git`** (new adapter crate): runs the system `git` (mirror, work clone, fetch, worktree add and remove, push) with the hardening above, one operation per repository at a time. Using the system `git` from Xcode's command line tools keeps agents and the office on the same git.
- **`org-office`** (domain): project commands and events, the repository record, and the follow list. It depends on neither adapter. It defines the small traits it needs: "create repository", "token for a job" and "clone project".
- **`org-github` and `org-git` depend on neither `org-office` nor each other.** Each exposes its own small API. **`org-office-server`**, the composition root (server-dev's), implements `org-office`'s traits by delegating to them: "create repository" and "token for a job" with `org-github` at HQ (at satellites, a request to HQ), and "clone project" with `org-git`. So the adapters stay free of the domain and SQLCipher, and an `org-office` change doesn't ripple into them.
- **`org-sync-proto`** (office-to-office contract) gains the token request. **`org-proto`** (client contract) gains the calls below.
- **`org-agent`** gains `push` and `gh-token`. Both are socket calls only, so it stays tiny and doesn't depend on `org-git`.
- **Owners (AGENTS.md):** River (`ac-software-engineer-river`) owns `org-github` and `org-git` (moved from server-dev on 2026-11-08 to spread the load). The traits ("create repository", "token for a job", "clone project") are defined by `org-office`, so they're server-dev's, and `org-office-server` (also server-dev's) implements them by delegating to River's adapters. The office side of tokens is server-dev's too: the token cache and the revocation policy per office (it decides when to revoke, and calls `org-github` to do it, since revoking calls GitHub), Keychain storage of the private key and HQ's user token, and the `github.token` request handler. All SQL stays in server-dev's `store.rs`, including the repository-creation intent in `office.db`.

## Implementation plan
Order: **after the HQ office is serving** (create organization, phase 1), and **before satellites**, as the user asked. G2a, the live run against real GitHub, comes last, in v2.

| Step | What | Who | Depends on |
| --- | --- | --- | --- |
| G1 | The contract in `org-proto` and `org-sync-proto`, built against the fake GitHub now and confirmed by G2a in v2 | ada | design approved |
| G2b | `org-github`, key import with checks and storage by the host (key custody), token minting, caching and revocation per office, and the token request from offices, tested against a local fake GitHub. Both repository-creation paths (an installation token with `repository_creation`, and the device flow with HQ's user token) are built and tested against the fake GitHub | River: the `org-github` API (the app JWT, token minting, token revocation, installation and repository lookups, repository creation, the device flow), and the host's storage of the private key and HQ's user token with the `store_secret` control line (key custody); server-dev: the office side (the token cache and the revocation policy per office, which decides when to revoke and calls `org-github`, handing the secrets to the host and taking them from the first launch line, and the `github.token` request handler); the key import's checks (the PEM parses, the Client ID matches the app on GitHub) are River's `org-github` call, and the `github.import_key` handler is server-dev's | G1 |
| G2c | `org-git`, the mirror and work clones, the worktree per task, `org-agent push` and `gh-token`, tested over HTTP (`git http-backend` behind the fake GitHub, so authentication is exercised; `file://` would skip it). Includes tests with a **hostile `work.git` config** (a credential helper, `insteadOf`, a proxy, filters) and a push to the default branch, all of which must fail to leak or succeed | River: `org-git` (clones, worktrees, hardened push); Remy (the messaging/CLI owner): the `org-agent push` and `gh-token` commands; server-dev: the office-side handlers in `org-office-server` | G1, G2b |
| G2d | Project commands and events in `org-office` (create with a new or existing repository, list, get, follow), with HQ's repository creation or lookup before the command | server-dev; `org-office-server` implements `org-office`'s traits by delegating to River's adapters | G1, G2b, the HQ event log |
| G3 | App: Settings → GitHub at HQ (import key, connection status, the installation's repositories, "allow creating repositories" for the device-flow path); Create project (name, team; a new repository, or pick one); the project list with clone status. Built on a mock first | app-frontend-dev | G1 (mock), then G2b–d |
| G4 | End-to-end test: create an organization → import the key (a fake GitHub in CI; the real-GitHub run is v2) → create a project with a new repository → an agent task gets a worktree, commits, pushes through its office, and opens a pull request with `gh` | River + server-dev + app-frontend-dev + Remy (the CLI owner), since it exercises `org-agent push` and `gh-token` | all |
| G2a | **v2.** A **spike** (about half a day) with the registered app. It confirms: the JWT and **minting a token scoped to one repository and chosen permissions**; git over HTTPS with that token; `gh` with `GH_TOKEN`; creating a private repository under your account with an installation token, or else with HQ's user token (device flow); a new repository showing up in the all-repositories installation. Then the office does a bare clone, a worktree and a push with the hardened commands. Testing against real GitHub moves to v2 (the user, 2026-11-08) | River | the app's key import (the app is already registered) |

Each step: its own branch, an independent review, my go, then a merge into `v1`.

**G1, the contract** (built against the fake GitHub now, confirmed by G2a in v2):
- App → HQ: `github.import_key { client_id, pem }` (HQ only; checked against GitHub, then stored by the host), `github.status → { app_id, installation?: { account, all_repositories }, can_create_repos }`, `github.repos.list { cursor? }`, and `github.connect.start/wait` for HQ's user token (the device-flow path; both creation paths are built in phase 1)
- `project.create { command_id, project_id, name, team_id?, repo: { new: { name } } | { existing: { repo_id } } } → Project`. New repositories are private, under your account.
- `project.list`, `project.get { id }` (with this office's clone status), `project.follow { id, follow }`
- **`org-agent` → its office:** `git.push { task_id, branch } → { commit }` and `gh.token { task_id } → { token, expires_at }`
- **Office → HQ** (`org-sync-proto`): `github.token { project_id, purpose: fetch | push | pull_request | actions | gh } → { token, expires_at }`
- Events: `project.created`, `project.clone_status`
- **Work change** (office-first stream): `project.branch_pushed { project_id, branch, commit, task_id? }`
- **Internal:** periodic fetch for followed projects, and the push before a handover

## Registering the GitHub App (for you, once)
1. On GitHub: your profile picture → **Settings** → **Developer settings** → **GitHub Apps** → **New GitHub App**.
2. **GitHub App name:** `Agentic Org`. If it's taken, add a suffix, e.g. `Agentic Org (vgmello)`.
3. **Description:** "Lets Agentic Org offices clone your project repositories, push their agents' work to task branches and open pull requests. HQ issues each office short-lived tokens limited to one repository."
4. **Homepage URL:** `https://github.com/vgmello/agentic-org`.
5. **Identifying and authorizing users:**
   - leave **Callback URL** empty
   - tick **Expire user authorization tokens**
   - tick **Enable Device Flow** (needed only if HQ has to sign in to create repositories; harmless otherwise)
   - leave **Request user authorization (OAuth) during installation** unticked
6. **Post installation:** leave everything empty. **Webhook:** untick **Active**.
7. **Permissions → Repository permissions:**
   - **Contents: Read and write**
   - **Pull requests: Read and write**
   - **Actions: Read-only**
   - **Metadata: Read-only** (set automatically)
   - **Repository creation: Read and write** (new in October 2026. If it's listed under another heading or name, pick the closest match and tell ada what it said)
   - everything else **No access**, including Administration and Workflows
8. **Where can this GitHub App be installed?** **Only on this account**.
9. **Create GitHub App.** Then, on the app's page:
   - copy the **Client ID** (and the App ID, for reference) and send it to ada. Both are public. You'll also enter the Client ID at HQ, together with the key
   - **Private keys → Generate a private key.** A `.pem` file downloads. Keep it private, and import it at HQ when the app asks (Settings → GitHub → Import key), then delete the file.
   - **Don't generate a client secret.** We don't need one.
10. **Install App** (left menu) → your account → **All repositories** → **Install**.

## Decisions (the user, 2026-11-08)
1. **One GitHub App, managed by HQ** (revised the same day). HQ holds its private key and mints short-lived tokens, each scoped to one repository and the permissions a job needs. Satellites never sign in or keep credentials. It replaces the earlier two-app plan.
2. New project repositories go under **your personal account**, **private by default**.
3. Commits: **your name and no-reply email as author, the agent as co-author**.
4. Install scope: **all repositories** (revised; scoping now happens per token).
5. ~~Each office signs in to GitHub itself.~~ Replaced by decision 1: only HQ talks to GitHub as the app.
6. Running agents as a separate macOS user (or sandboxed): **v2**.

## Open questions for the user
1. **CI files:** may agents change GitHub Actions workflows (`.github/workflows/`)? That needs the **Workflows** permission, which is left off by default.
