# Design and plan: projects as git repositories, connected to GitHub

Status: **approved by the user** 2026-11-08 (decisions at the end). Owner: ada. It follows the user's decision: project files are git repositories, and the GitHub connection comes first. It builds on `offices-hq-and-satellites.md`.

## In one paragraph
Each project is a **git repository on GitHub**. Every office that works on a project **clones it from GitHub**, and its agents work in their own **git worktrees**, one per task, on their own branches. GitHub is the hub for files, so there's no git server of our own and no file syncing between offices: offices tell each other "I pushed" through the normal work-data stream, and the others fetch. Each office **connects to GitHub once**, through the standard "enter this code at github.com/login/device" sign-in. The token stays in that office's Keychain, is never copied to other offices, and **agents never see it**: the office fetches and pushes for them. HQ can also **create a project's repository automatically**, with a second app that only HQ holds.

## What lives where
- **Organization data** (HQ-owned, copied everywhere): the project record. It holds the project's id, name and team, and **its repository**: `github`, owner, name, numeric id, default branch, private or public. It never holds a token.
- **Office data** (each office's own): its GitHub connections ("Agentic Org" at every office, plus "Agentic Org Admin" at HQ only: which GitHub user and installation, and the tokens in its Keychain), its clones and worktrees, and each project's clone status.
- **On disk:** `<home>/offices/<office-id>/workspace/projects/<project-id>/`
  - `mirror.git` is the **office's own bare mirror**. Only the office runs git in it, and it's the only repository that talks to GitHub.
  - `work.git` is a **bare** local clone of the mirror, for agents. It has no checked-out branch, so any branch can get a worktree.
  - `worktrees/<task-id>/` holds each task's working copy, from `work.git`, on branch `task/<task-id>-<slug>`.

  Agents' git settings (`git config` in a worktree writes to `work.git/config`) never touch the repository that holds the network settings and uses the token.
- **Encryption:** these are ordinary files. FileVault protects them, not our database (ADR-0002, "Not covered here").

## Connecting GitHub (the first feature)
**Two GitHub Apps** *(decided by the user, 2026-11-08)*, registered once by you, the project's owner, both with the device flow and token expiry enabled (registration steps below). Their **client ids are public** and ship in the open-source app. Neither the device flow nor token refresh needs the client secret (GitHub's docs: the secret is required "unless the user access token was generated using the device flow"), so no secret is ever generated or shipped.
- **"Agentic Org"**, at **every office**.
  - **Permissions:** **Contents: read and write** and **Metadata: read**. It can clone, fetch and push to the repositories it's installed on, and that's all.
  - **Installed on selected repositories** (decision 4), so a leaked token reaches only project repositories.
- **"Agentic Org Admin"**, at **HQ only**, so HQ can **create project repositories automatically**.
  - **Permissions** (GitHub's narrow permissions from 13 October 2026, instead of Administration):
    - **Repository creation: write**, to create repositories (`POST /user/repos`)
    - **GitHub App installation repository access** (`installation_repositories: write`), to add a new repository to the "Agentic Org" installation
    - **Metadata: read**

    It **can't delete, transfer, make public or reconfigure** repositories, and it can't read or write code. Its tokens are kept **only at HQ**, as HQ-only data that's never copied, so a satellite's token can never create a repository.
  - **Residual risk, stated plainly:** HQ's own agents run as the same macOS user as HQ (see "Agents and git"), so a hostile agent at HQ could reach this token too. With these permissions it can't delete or edit code, but it may be able to:
    - create repositories;
    - **add any of your private repositories to any app installation on your account**, including a third-party app's, which would expose that code;
    - remove repositories from installations.

    G2a checks whether this app can change other apps' installations at all. The app shows the "Agentic Org" installation's repositories so you can spot changes. Running agents as a separate macOS user (v2) closes this.
  - Branch rulesets would need Administration, so they're **not** set up automatically. You can add them on GitHub yourself if your plan allows.

**The flow in the app:** Settings → GitHub → **Connect**, at every office, for "Agentic Org". **At HQ, a second Connect for "Agentic Org Admin"** runs the same flow, with its own code and its own install. The calls take an `app` parameter (`contents` or `admin`). `admin` is accepted only at HQ, and repository creation is offered only once it's connected.
1. **Authorize.** The office starts the device flow, and the app shows a short code and opens `github.com/login/device`. The owner enters the code. The office polls at GitHub's interval, adding 5 seconds whenever GitHub answers `slow_down`.
2. **Install.** Authorizing doesn't install the app. If the app isn't installed on your account, the app sends you to its install page, and checks again afterwards.
   - "Agentic Org": pick the project repositories.
   - "Agentic Org Admin": **All repositories**. A new repository doesn't exist yet when you pick selected ones, and a community report says the creating app may not get access to it automatically (G2a checks). With only the two narrow permissions, "all" can't delete repositories or touch code. See the residual risk above for what it can change.
3. **Keep the tokens.** User tokens last 8 hours and come with a refresh token that lasts 6 months and works once.
   - The office server stores both in **its own Keychain**, creating and reading the items itself (ADR-0002's per-program rule).
   - It refreshes one at a time, because refresh tokens are single-use.
   - An office offline for more than 6 months must connect again.
4. **Disconnect.** The office deletes the tokens locally and links the owner to GitHub's "Authorized GitHub Apps" settings to revoke the authorization. Revoking through the API would need the client secret.
   - Revoking "Agentic Org" there cancels **every office's** authorization for that app, because they share your GitHub account. So **removing an office** means revoking at GitHub, then reconnecting the other offices: the app walks the owner through it. Until the revoke, a removed office's token keeps working, for up to 6 months.

## Projects
- **Create project** (in the app, so at HQ): a name, a team (later), and either **a new repository** or **an existing one**.
  - **A new repository** is created automatically under your account, **private** (decision 2), named after the project unless you change it. This needs "Agentic Org Admin" connected at HQ.
  - **An existing repository** is picked from those the "Agentic Org" installation can see (`github.repos.list`). If the one you want isn't there, the app says to add it to the installation and offers the link (GitHub Settings → Applications → **Installed GitHub Apps** → Agentic Org → **Configure**).
- **Creating a new repository happens before the command.** GitHub calls never happen inside a command, so events stay deterministic (offices design). HQ's office server:
  1. **records the intent** in HQ's `office.db`: name, command id, time;
  2. creates the repository with the admin app, **private, with an initial commit** (`auto_init: true`, so the default branch exists for worktrees), and **with the command id in its description** as a marker, then records its numeric id;
  3. **adds it to the "Agentic Org" installation**, using App 1's installation id from App 1's connection. If G2a shows the API can't do this, the app asks you to add it, with the link;
  4. **waits until the "Agentic Org" installation can see it**, by the same lookup by id used for existing repositories;
  5. submits the "create project" command with the repository's details.
- **On a retry or after a crash**, a repository of that name whose description carries this command id is **adopted by its id**, not duplicated, whatever the clocks say. Any other repository with that name fails with "name taken".
- **A project created from a satellite's app** is relayed to HQ, which runs these steps before submitting the command.
- **For an existing repository**, HQ looks it up first (in the installation's repository list, by its numeric id). That confirms the installation can reach it and gets its owner, name, default branch and visibility.
- HQ records the project, with its repository (including GitHub's numeric repository id, which survives renames), as an organization event, so every office learns of it.
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
  - **Residual risk, stated plainly:** agents run as the **same macOS user** as the office. A determined agent could edit the mirror's files directly, or reach the office's Keychain items through the same user. The separate mirror and the hardening stop accidents and ordinary git misuse, not a hostile agent. Real isolation means running agents as a separate macOS user or in a sandbox. That's planned for v2 (decision 6), and for now offices and agents are all yours.
  - **The office's push check is the required control**, and it's tested (G2c). The apps don't set up GitHub rulesets (that would need Administration), and on GitHub Free, private repositories can't have them anyway. If your plan allows them, you can protect the default branch yourself on GitHub, as an extra guard.
- **One git operation at a time per repository** (fetch, worktree add or remove, push), which avoids `.git` lock races.
- **Commit identity** *(decided by the user, 2026-11-08)*: commits are authored under **the connected user's name and GitHub no-reply address** (`<id>+<login>@users.noreply.github.com`), with the agent added as a co-author: a `Co-authored-by: <agent name> <agent address>` trailer. Pushes aren't rejected when the account hides its email.
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
  - It covers the device flow with `slow_down`, token refresh, the current user, installations and their repositories (`/user/installations`, `/user/installations/{id}/repositories`), and, for the admin app at HQ, creating a repository and adding it to an installation.
  - It handles rate limits and gives clear errors. No async runtime.
- **`org-git`** (new adapter crate): runs the system `git` (mirror, work clone, fetch, worktree add and remove, push) with the hardening above, one operation per repository at a time. Using the system `git` from Xcode's command line tools keeps agents and the office on the same git.
- **`org-office`** (domain): project commands and events, the repository record, and the follow list. It depends on neither adapter. It uses small traits, "create repository" (implemented at HQ with `org-github`) and "clone project" (implemented with `org-git`).
- **`org-proto`** (contract) gains the calls below.
- **`org-agent`** gains `push`, which is only a socket call, so it stays tiny and doesn't depend on `org-git`.
- **Owners (AGENTS.md):** the server agent owns `org-github` and `org-git`.

## Implementation plan
Order: **after the HQ office is serving** (create organization, phase 1), and **before satellites**, as the user asked.

| Step | What | Who | Depends on |
| --- | --- | --- | --- |
| G2a | A **spike** (about half a day) with the registered apps. It confirms: the device flow from Rust; token refresh without a secret; listing the installation's repositories; a repository added to the installation becoming visible; whether installing needed a private key; for the admin app, creating a private repository under your account with **Repository creation** only, whether the creator gets access to it automatically, and adding it to the "Agentic Org" installation through the API with **installation repository access**, on a personal account, and whether this app can change **other** apps' installations. Then the office does a bare clone, a worktree and a push with the hardened commands | server-dev | you register the two GitHub Apps |
| G1 | The contract in `org-proto`, frozen **after** G2a | ada | design approved, G2a |
| G2b | `org-github`, the Keychain token store with refresh, and the `github.*` calls in the office server, tested against a local fake GitHub | server-dev | G1, G2a |
| G2c | `org-git`, the mirror and work clones, the worktree per task and `org-agent push`, tested over HTTP (`git http-backend` behind the fake GitHub, so authentication is exercised; `file://` would skip it). Includes tests with a **hostile `work.git` config** (a credential helper, `insteadOf`, a proxy, filters) and a push to the default branch, all of which must fail to leak or succeed | server-dev | G1, G2b |
| G2d | Project commands and events in `org-office` (create with a new or existing repository, list, get, follow), with HQ's repository creation or lookup before the command | server-dev | G1, G2b, the HQ event log |
| G3 | App: Settings → GitHub (code, browser, waiting, the install step, connected, disconnect, errors); Create project (name; a new repository, or pick one from the installation, with a link to add one); the HQ-only "Agentic Org Admin" connection; the project list with clone status. Built on a mock first | app-frontend-dev | G1 (mock), then G2b–d |
| G4 | End-to-end test: create an organization → connect GitHub (a fake server in CI, real GitHub as a manual check) → create a project with a new repository → an agent task gets a worktree, commits, and pushes through its office | server-dev + app-frontend-dev | all |

Each step: its own branch, an independent review, my go, then a merge into `v1`.

**G1, the contract** (drafted now, frozen after G2a):
- `github.status { app } → { connected, login?, installation?: { account, configure_url } }`
- `github.connect.start { app } → { user_code, verification_uri, expires_at, interval_s }`
- `github.connect.wait { app, timeout_ms } → { connected, login, needs_install, install_url? } | error (expired, denied, timeout)`
- `github.disconnect { app } → { revoke_url }`
- `app` is `contents` or `admin`. `admin` is accepted only at HQ.
- `github.repos.list { cursor? } → { repos, next_cursor? }`
- `project.create { command_id, project_id, name, repo: { new: { name } } | { existing: { repo_id } } } → Project`. New repositories are private, under your account.
- `project.list → { projects }` and `project.get { id } → Project`. A project includes this office's clone status.
- `project.follow { id, follow }`
- **`org-agent` → its office:** `git.push { task_id, branch }` → `{ commit }` (the office's checks, then the push)
- Events: `project.created`, `project.clone_status`
- **Work change** (office-first stream): `project.branch_pushed { project_id, branch, commit, task_id? }`. Offices use it for fetching and for task handovers.
- **The app connected to a satellite** creates a project at HQ like any organization command, relayed through its office.
- **Internal:** periodic fetch for followed projects, and the push before a handover. Both are office behaviour, not calls.

## Registering the GitHub Apps (for you, once)
### App 1: "Agentic Org" (every office)
1. On GitHub: your profile picture → **Settings** → **Developer settings** → **GitHub Apps** → **New GitHub App**.
2. **GitHub App name:** `Agentic Org` *(the user's choice)*. Names are unique across GitHub, so if it's taken, add a suffix, e.g. `Agentic Org (vgmello)`.
3. **Homepage URL:** `https://github.com/vgmello/agentic-org`.
4. **Identifying and authorizing users:**
   - Leave **Callback URL** empty. The device flow doesn't use it.
   - Tick **Expire user authorization tokens**.
   - Leave **Request user authorization (OAuth) during installation** unticked.
   - Tick **Enable Device Flow**.
5. **Post installation:** leave everything empty.
6. **Webhook:** untick **Active**. We don't use webhooks.
7. **Permissions → Repository permissions:**
   - **Contents: Read and write**
   - **Metadata: Read-only** (GitHub sets it automatically)
   - Leave everything else at **No access**, including all Organization and Account permissions.
8. **Where can this GitHub App be installed?** **Only on this account**.
9. **Create GitHub App.** On the next page, copy the **Client ID** (it starts with `Iv`) and send it to ada. **Don't generate a client secret or a private key.** We don't need either, and not creating them means none can leak.
   - If GitHub blocks installing until a private key exists: generate one, don't download or keep it, install (step 10), then **delete the key** on the same page. The G2a spike confirms which applies.
10. **Install App** (left menu) → your account → **Only select repositories** → pick the repositories you'll use for projects (you can add more later) → **Install**.

The client id goes into the app's configuration. It's public by design.

### App 2: "Agentic Org Admin" (HQ only)
Same steps as App 1, with these differences:
- **Step 2, name:** `Agentic Org Admin`.
- **Step 7, permissions:**
  - **Repository creation: Read and write**
  - **GitHub App installation repository access: Read and write** (`installation_repositories`)
  - **Metadata: Read-only**
  - everything else **No access**: no Administration and no Contents. It never reads or writes code.

  Both permissions are new (13 October 2026). If the form lists them under a different heading or name, pick the closest match and tell ada what it said.
- **Step 9:** send ada its Client ID too.
- **Step 10, install:** **All repositories**. "Only select repositories" can't cover repositories that don't exist yet. With only these permissions it can't delete repositories or touch code. It can create repositories and change which repositories apps can access (see the residual risk in "Connecting GitHub"). Its tokens stay at HQ only.

## Decisions (the user, 2026-11-08)
1. **Two GitHub Apps** (revised the same day, to create repositories automatically): "Agentic Org" (read and push) at every office, and "Agentic Org Admin" (repository creation and installation access only, no Administration) at HQ only.
2. New project repositories go under **your personal account**, **private by default**.
3. Commits: **your name and no-reply email as author, the agent as co-author**.
4. Install scope: **selected repositories** for "Agentic Org". "Agentic Org Admin" uses all repositories, with only the narrow creation permissions.
5. **Each office signs in to GitHub itself.**
6. Running agents as a separate macOS user (or sandboxed): **v2**.
