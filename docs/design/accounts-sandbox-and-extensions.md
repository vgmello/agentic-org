# Design: dedicated accounts, the agent sandbox and extensions

Status: **proposed (owner direction 2026-11-20; not yet accepted; ADRs follow the
spikes)**. Nothing in this note is built, and nothing here changes an accepted
decision yet. Where it differs from an accepted ADR, the ADR still holds until the
follow-up ADRs of [section 10](#10-adrs-and-designs-to-write-or-update-after-the-spikes)
are accepted. Plan rows L16 and L17 of the
[implementation plan](../implementation-plan.md#later-in-no-fixed-order); checklist
rows FC-191 to FC-194 of the [feature checklist](../FEATURES.md).

How an instance should run once it no longer runs as the person's own login account:
which operating-system accounts exist, how the office starts agents as their own
accounts without being an administrator, how one command sets it all up, where each
secret lives, how the office's own sandbox sits inside those accounts, and how
optional capabilities (containers, Xcode) are added as extensions without the core
depending on them. It builds on
[ADR-0015](../adr/0015-agent-isolation-modes.md) (isolation modes),
[ADR-0016](../adr/0016-no-application-level-data-encryption.md) (secrets),
[ADR-0017](../adr/0017-linux-and-container-hosting.md) (platforms),
[ADR-0008](../adr/0008-gateway-macos-sandbox.md) (web confinement),
[ADR-0032](../adr/0032-browser-driven-cli-signin-and-remote-token-distribution.md)
(sign-in custody), the [Linux host design](linux-host.md), the
[host boundary design](office-host-boundary.md) and the
[standalone design](standalone-server.md).

## Owner decisions and direction (2026-11-20)

Recorded so that a fresh session loses nothing. Each line is the owner's direction of
2026-11-20 unless dated otherwise; where it is already recorded elsewhere, the link
goes there.

| # | Decision or direction | Recorded in |
| --- | --- | --- |
| D1 | Claude Code background tasks and sub-agents stay on across turns; use the harness fully (owner clarification 2026-11-19) | [ADR-0025](../adr/0025-agents-start-sub-agents.md) decision 6, merged with PR 17 |
| D2 | Claude Code's own sandbox is a per-agent switch in the agent's Advanced settings, on by default (follow-up, not built) | [FC-189](../FEATURES.md) |
| D3 | Taking over an agent's session from a terminal is paused, moved to late v1, built only if needed | [FC-151](../FEATURES.md) |
| D4 | `os-sandbox` is v1 (late): the office's own OS sandbox around every agent, with a standard and an iOS profile | [FC-190](../FEATURES.md), ADR-0015 owner note, [section 7](#7-the-sandbox-inside-the-accounts) |
| D5 | Dedicated non-administrator accounts: a service account for the instance and agent accounts for the agents; the office starts agents as their accounts through a grant made at setup | [Section 3](#3-accounts-proposed), [section 4](#4-starting-agents-as-their-accounts-without-being-an-administrator), [FC-191](../FEATURES.md) |
| D6 | Every administrator step the core needs happens in the one setup command, never afterwards; an extension may need one later, kept minimal | [Section 5](#5-one-command-setup); the accepted exceptions are D23 |
| D7 | The closed-list privileged helper installed by setup is the preferred way to avoid prompts after setup, to be proven by the spike | [Section 5.2](#52-administrator-rights-after-setup) |
| D8 | Isolation by accounts is the default for an install; development uses `--isolation off`; one setting | [Section 3.4](#34-isolated-and-not-isolated-how-few-switches) |
| D9 | Extensions are built-in modules behind a contract, in `extensions/`, contributing menu items, settings and panels; built after milestone one, while milestone one keeps the door open | [Section 8](#8-extensions), [FC-192](../FEATURES.md) |
| D10 | Containers through Colima on macOS and Linux (rootless fallback without KVM); no `agent-os` container commands: agents use `docker` | [Section 8.5](#85-the-containers-extension), [FC-193](../FEATURES.md) |
| D11 | An Xcode extension with its checklist and sandbox allowances | [Section 8.6](#86-the-xcode-extension), [FC-194](../FEATURES.md) |
| D12 | The web app's extension slot is proposed to the UI stream, which agrees it with the backend stream | [Section 8.4](#84-the-ui-slot-proposal), [UI stream request](../workstreams/ui.md#request-from-the-backend-stream-the-extension-ui-slot) |
| D13 | Data at rest is protected by disk encryption, file permissions and isolation (ADR-0016); private keys and provider credentials stay wrapped by the office secrets key | [Section 6](#6-secrets), [ADR-0016](../adr/0016-no-application-level-data-encryption.md) |
| D14 | Not adopted (owner, after evaluation): the host staying with the person on a desktop Mac. The host runs as the service account everywhere under `accounts` | [Section 6.5](#65-option-the-host-stays-with-the-person-on-a-desktop-mac) |
| D15 | Linux: each office is an instance of one shared template unit, `agentic-org-office@.service`, with a polkit rule letting the service account start and stop only `agentic-org-office@*`; the host unit keeps `NoNewPrivileges=yes`; macOS starts offices as today | [Section 4.1](#41-options) |
| D16 | Host secrets are owner-only files for now, on macOS too; the Keychain with a Developer-ID-signed host is a later release option; development never requires signing; per-office service accounts stay a later option | [Section 6.3](#63-no-dependency-on-the-macos-login-keychain) |
| D17 | Each extension has its own small `platform` module, an exception to LNX-5 for extensions | [Section 8.3](#83-where-the-code-lives) |
| D18 | ui-dev leads the extension UI slot with the backend stream, when L17 starts | [Section 8.4](#84-the-ui-slot-proposal) |
| D19 | Agent accounts are per office and separate from the service account: agents never run as the account that holds office data and keys | [Section 3.1](#31-who-runs-what) |
| D20 | The closed-list helper is confirmed (user-friendly but secure), pending the spike | [Section 5.2](#52-administrator-rights-after-setup) |
| D21 | Disk encryption: keep check and warn (ADR-0016); do not require it | [Section 6.1](#61-what-protects-data-and-keys-at-rest-today) |
| D22 | The Git design's `dedicated-user` rules carry over | [Section 11.4a](#114a-git-across-accounts) |
| D23 | The named exceptions are accepted: `uninstall` and changing the isolation setting each ask once | [Section 5.2](#52-administrator-rights-after-setup) |

## Requirements

The requirements this note proposes, with stable IDs so that later code and the spikes
can be checked against them. They are proposed, not accepted: the ADRs of
[section 10](#10-adrs-and-designs-to-write-or-update-after-the-spikes) accept, change or
drop each one.

| ID | Requirement | Section |
| --- | --- | --- |
| ASX-1 | One instance setting, `--isolation off\|accounts`, chosen at setup and recorded in the instance record; `accounts` is the default for an install, `off` for development. | [3.4](#34-isolated-and-not-isolated-how-few-switches) |
| ASX-2 | The composition roots select one adapter per seam from that setting; no other code reads it; both adapters of each seam pass the same conformance tests. | [3.4](#34-isolated-and-not-isolated-how-few-switches) |
| ASX-3 | `off` is never taken silently: it is asked and recorded only on yes; an `accounts` instance missing its launcher, helper or an agent account refuses the affected agents | [3.4](#34-isolated-and-not-isolated-how-few-switches), [5](#5-one-command-setup) |
| ASX-4 | Under `accounts`, a non-administrator service account runs the host, the web process and the offices from boot with nobody logged in. | [3.1](#31-who-runs-what) |
| ASX-5 | Agents run as non-administrator agent accounts, one per office first, started through a grant made at setup; the launcher validates every request and keeps ADR-0015 decision 4's evidence | [3.1](#31-who-runs-what), [4](#4-starting-agents-as-their-accounts-without-being-an-administrator) |
| ASX-6 | The data root and host secrets are outside the person's home, owned by the service account, closed to agents and to the person's other programs. | [3.2](#32-the-data-root-moves-out-of-the-persons-home), [6](#6-secrets) |
| ASX-7 | Socket admission uses explicit allowed UIDs; admission is never authorization | [3.3](#33-what-the-person-still-does), [9](#9-architecture-changes) |
| ASX-8 | One administrator prompt at setup does every privileged step the core needs; the core's everyday use never asks again; only named exceptions ask, once each | [5](#5-one-command-setup), [5.2](#52-administrator-rights-after-setup) |
| ASX-9 | Nothing of ours runs as root continuously; root code runs only a closed list of operations, checks every value, runs from root-owned release-verified code and logs each operation | [5](#5-one-command-setup), [5.2](#52-administrator-rights-after-setup) |
| ASX-10 | The office secrets key never reaches an agent; an agent receives only a run copy of its own sign-in; agent-writable paths are opened without following links and with owner and link-count checks | [4.2](#42-the-launcher), [6.0](#60-the-secrets-chain-in-plain-words) |
| ASX-11 | Every agent runs inside the os-sandbox standard profile; an extension adds allowances only for agents that have it enabled | [7](#7-the-sandbox-inside-the-accounts) |
| ASX-12 | An agent cannot read the person's home, another account's home, the service data or secrets, or another office's data; cannot signal processes it does not own; cannot gain administrator rights | [7](#7-the-sandbox-inside-the-accounts), [11.2](#112-security-checks) |
| ASX-13 | Agents never hold a credential that can push; the office runs Git that reaches a remote, under the Git design's `dedicated-user` rules | [11.4a](#114a-git-across-accounts) |
| ASX-14 | The core never depends on an extension; removing an extension's folder and its composition line does not break the product; an extension's per-system adapter fails closed | [8.3](#83-where-the-code-lives) |
| ASX-15 | The extension contract covers install steps with pinned checksums, privileged steps from setup's closed list, a checklist, health, sandbox allowances, agent environment and instructions, lifecycle hooks, menu items, a settings section stored and validated by the office, panels and cleanup | [8.2](#82-the-extension-contract) |
| ASX-16 | Milestone one keeps the door open: composition, agent launch, navigation, settings sections and setup's plan are list-driven where that is cheap | [8.1](#81-what-an-extension-is) |
| ASX-17 | The Containers extension gives each agent account its own Colima VM (rootless fallback without KVM), plain `docker` through `DOCKER_HOST`, never the `docker` group, and no `agent-os` container commands | [8.5](#85-the-containers-extension) |
| ASX-18 | The Xcode extension is macOS only, with its checklist, its allowances and a signing keychain per agent account unlocked by the office | [8.6](#86-the-xcode-extension) |

ASX-14 to ASX-16 are verified without a spike. A CI check of dependency sources, to be
added with L17, covers ASX-14's first clause (no core crate or app depends on anything
under `extensions/`); review covers the rest of ASX-14, and ASX-15 and ASX-16.

## Where this is tracked

| Item | Checklist row | Plan row | Section here |
| --- | --- | --- | --- |
| Dedicated accounts and one-command setup | [FC-191](../FEATURES.md) | L16 ([plan](../implementation-plan.md#later-in-no-fixed-order)) | [3](#3-accounts-proposed) to [6](#6-secrets) |
| `dedicated-user` agents (ADR-0015) | [FC-161](../FEATURES.md) | L6 | [3.1](#31-who-runs-what), [4](#4-starting-agents-as-their-accounts-without-being-an-administrator) |
| `os-sandbox` | [FC-190](../FEATURES.md) | L15 | [7](#7-the-sandbox-inside-the-accounts) |
| Claude Code's sandbox switch | [FC-189](../FEATURES.md) | (C3 follow-up) | [7](#7-the-sandbox-inside-the-accounts) |
| Session take-over (paused) | [FC-151](../FEATURES.md) | Late v1, only if needed | [Owner decisions](#owner-decisions-and-direction-2026-11-20) D3 |
| Extensions framework | [FC-192](../FEATURES.md) | L17 | [8](#8-extensions) |
| Containers extension | [FC-193](../FEATURES.md) | L17 | [8.5](#85-the-containers-extension) |
| Xcode extension | [FC-194](../FEATURES.md) | L17 | [8.6](#86-the-xcode-extension) |
| Where to start | [Backend stream](../workstreams/backend.md) | | [11](#11-spikes) (spikes first) |

## 1. In one paragraph

Today everything runs as the person's own account. The proposal is two walls. The
**outer wall is accounts**: setup creates a hidden, non-administrator **service
account** that runs the host, the web process and the offices, started at boot
whether or not anyone is logged in; and non-administrator **agent accounts** that run
the agents. The person's own account keeps only the command line and the browser.
Nothing of ours runs as an administrator continuously: with the recommended design,
after setup only a tiny helper with a closed list of operations runs as root, briefly,
for one listed operation (the alternatives are in [section 4](#4-starting-agents-as-their-accounts-without-being-an-administrator)
and [section 5.2](#52-administrator-rights-after-setup)). The
person authenticates as an administrator once, in the one setup command, and not again
for the core's everyday use ([section 5.2](#52-administrator-rights-after-setup)).
Isolation can also be switched off, for development and testing, by one setting chosen
at setup ([section 3.4](#34-isolated-and-not-isolated-how-few-switches)). The **inner wall is the office's own
OS sandbox** (`os-sandbox`, [FC-190](../FEATURES.md)) around each whole agent
process. Optional capabilities that agents need (containers, Xcode) are
**extensions**: the product installs and manages them, the core works without them,
and each adds only the sandbox allowances it needs for the agents that have it
enabled. Extensions come later in v1, not in milestone one.

## 2. What runs today

Every statement in this table is read from the linked source. Nothing here is
proposed.

| Topic | Today | Source |
| --- | --- | --- |
| Which account runs the instance | The person's own account. On macOS a per-user launchd agent in `~/Library/LaunchAgents`, loaded into `gui/<uid>`; on Linux a systemd user unit in `~/.config/systemd/user`; in a container, `agentic-org serve` in the foreground as UID 61000 | [Linux host design 3.4](linux-host.md#34-service-lifecycle), [`service/macos.rs`](../../crates/org-platform/src/service/macos.rs), [ADR-0017](../adr/0017-linux-and-container-hosting.md) decision 10 |
| Running without a login | macOS: not possible; a launchd agent in the person's `gui/<uid>` domain runs only while the person is logged in. Linux: only with lingering on (`loginctl enable-linger`), which setup reports and does not run | [Linux host design 3.4](linux-host.md#34-service-lifecycle) |
| Root | `serve` and `setup` refuse to run as root; `host.sock` admits root only for `admin reset` | [ADR-0017](../adr/0017-linux-and-container-hosting.md) decisions 10 and 11 |
| Data root | Inside the person's home: `~/Library/Application Support/agentic.org/instances/<name>/` on macOS, `${XDG_DATA_HOME:-~/.local/share}/agentic-org/instances/<name>/` on Linux, a mounted volume in a container | [Standalone design 3](standalone-server.md#3-instance-selection-and-storage) |
| Host secrets | macOS: the Data Protection keychain, in the signed host's access group. Linux: an owner-only directory outside the data root, or a mounted secrets volume | [Host boundary: the office secrets key](office-host-boundary.md#the-office-secrets-key), [ADR-0017](../adr/0017-linux-and-container-hosting.md) decision 8 |
| Provider sign-ins | Wrapped by the office under the office secrets key in `durable/local/offices/<office-id>/auth/<connection-id>/` (accepted, not built) | [ADR-0016](../adr/0016-no-application-level-data-encryption.md) decision 3, [ADR-0032](../adr/0032-browser-driven-cli-signin-and-remote-token-distribution.md) |
| Agent isolation | `same-user` only. `dedicated-user` (one account per instance in v1) is accepted and not built; it needs a privileged launcher. `os-sandbox` is v1 scope since 2026-11-20, design proposed | [ADR-0015](../adr/0015-agent-isolation-modes.md) decisions 2, 4, 12; [FC-161, FC-190](../FEATURES.md) |
| Agent private home | Accepted for both modes at `durable/local/offices/<office-id>/agents/<employee-id>/home/`; with a `CLAUDE_CODE_OAUTH_TOKEN`, Claude needs no Keychain and both `HOME` and `CLAUDE_CONFIG_DIR` can point into it (macOS only; Linux unverified) | [ADR-0015](../adr/0015-agent-isolation-modes.md) decisions 10 and 11, [C1 spike](../research/c1-sign-in-spike.md) |
| Starting an agent | `org-supervise`'s `SpawnSpec` has a program, arguments, a working directory and an environment; no account or isolation field. The office can only signal processes of its own user | [`spawn.rs`](../../crates/org-supervise/src/spawn.rs), [ADR-0015](../adr/0015-agent-isolation-modes.md) context |
| The office socket | Admits only the office's own user (`org_socket::peer_allowed(&stream, expected_uid)`) | [`serve.rs`](../../apps/agentic-org-office/src/serve.rs), [ADR-0015](../adr/0015-agent-isolation-modes.md) decision 5 |
| `host.sock` | `0600` in a `0700` directory, plus a peer UID check; on Linux a native client is admitted by UID alone | [Host boundary 3](office-host-boundary.md#3-what-the-host-does), [Linux host design 3.1](linux-host.md#31-process-admission) |
| Recovery presence on macOS | The host requires a peer with its own effective UID that is the current console user, in the same local console audit session | [ADR-0011](../adr/0011-protected-recovery-delivery.md), "Presence adapter" |
| What a same-user process can do on Linux | Read every host secret, change the data, replace the host's executable or unit before its next start | [Linux host design 5](linux-host.md#5-what-a-local-attacker-can-still-do-on-linux) |

That last row is the main reason for this note: the Linux host design already says
that "`dedicated-user` (L6) closes this for agents; a separate service account closes
it for the person's other programs."

## 3. Accounts (proposed)

### 3.1 Who runs what

| Account | Kind | Runs | Owns | Started by |
| --- | --- | --- | --- | --- |
| The person's own account | Unchanged, may be an administrator | `agentic-org` commands and the browser only | Nothing of the instance | The person |
| **Service account** (one per instance) | Hidden, non-administrator, no login shell, no password | `agentic-org serve` (the host), `agentic-org-web`, every `agentic-org-office` | The data root, the host's secrets, the office sockets' directory | At boot, with nobody logged in: a system launchd daemon with `UserName` on macOS; a system systemd unit with `User=` on Linux; the container runtime in a container |
| **Agent account**, one per office first, made at setup or later by the helper | Hidden, non-administrator, no login shell, no password | The agents of that office | Only the agents' private homes and workspaces (ADR-0015 decisions 2 and 10) | The office, through the launcher of [section 4](#4-starting-agents-as-their-accounts-without-being-an-administrator) |
| Agent account, one per agent (later) | As above | One agent | That agent's home and workspaces | As above |
| Office account, one per office (optional, later) | Hidden, non-administrator | One `agentic-org-office` | That office's data | The host, through the same kind of grant |

- **Agent accounts per office first.** This is ADR-0015's `dedicated-user` mode with
  one change: one account per office instead of one per instance. With one account
  per office, agents of two offices on one machine cannot read each other's homes;
  agents within an office still share one account, so between them the barrier stays
  the sandbox and the office's rules (ADR-0015 decision 11). Per-agent accounts are
  the later step that makes the operating system separate agents too.
- **Per-office service accounts are optional and later.** Offices usually run on
  separate machines; a second office on the same instance shares the service account
  until there is a reason to split it.
- **Nothing of ours runs as an administrator continuously.** Our administrator code is
  setup's fixed-plan privileged step ([section 5](#5-one-command-setup)) and the
  closed-list helper it installs ([section 5.2](#52-administrator-rights-after-setup)),
  which runs as root only for one listed operation at a time. With the recommended
  launcher grant (option 1 of [section 4](#4-starting-agents-as-their-accounts-without-being-an-administrator))
  the launcher never runs as root. Under every option the
  launcher is a fifth program, already accepted by ADR-0015 decision 4, so
  non-negotiable 7 ("four programs") and ADR-0005 decision 7 ("four executables") would
  be updated to name it, and the closed-list helper too, unless it is a mode of the
  launcher or of `agentic-org` ([section 10](#10-adrs-and-designs-to-write-or-update-after-the-spikes)). The fallback grant (option 3) also adds a long-lived root daemon,
  which ADR-0005 decision 7 rules out today ("No extra host, updater or
  agent-supervisor daemon exists"); choosing it needs the owner's decision and an ADR.

### 3.2 The data root moves out of the person's home

The service account cannot be expected to traverse the person's home, and the
person's other programs must not reach the data. The data root moves to a
system location owned by the service account that no other account can list. The
parents of agent homes and workspaces allow traversal only, for the agent accounts
(for example mode `0711`, or an access-control entry for the agent accounts' group):

| Platform | Proposed data root | Proposed host secrets |
| --- | --- | --- |
| macOS | `/Library/Application Support/agentic.org/instances/<name>/` | A separate owner-only directory outside the data root ([section 6](#6-secrets)) |
| Linux | `/var/lib/agentic-org/instances/<name>/` | `/var/lib/agentic-org-credentials/<instance-id>/` or a path setup records, under ADR-0017 decision 8's rules (owner, mode, filesystem, disjoint from the data root) |
| Container | Unchanged: mounted volumes | Unchanged: a mounted secrets volume |

The exact paths are for the spike to confirm. The rule from the
[standalone design](standalone-server.md#3-instance-selection-and-storage) stays:
setup records the path, and services and agents never infer it from `HOME`.
Agent workspaces and private homes stay where the storage contract puts them; the
agent account reaches them through parent directories it may traverse and not list
(ADR-0015 decision 2).

### 3.3 What the person still does

The person runs `agentic-org` and uses the browser. Their command line reaches the
host over `host.sock`, which now belongs to another account, so the host admits the
person's UID by an explicit allowed-UID list ([section 9](#9-architecture-changes)).
Authority still comes only from a session the person approved in the browser
([Linux host design 3.1](linux-host.md#31-process-admission)); admission is never
authorization. Because the data root is closed to the person, the command line finds
the instance through a small public instance record and a socket directory that the
person may reach (for example owned by the service account, with a group-searchable
directory and a group-writable socket for a group setup creates for the people allowed
to use the command line; the allowed-UID check still applies); the exact location and
modes are for the accounts ADR. The initial registration link of
[ADR-0022](../adr/0022-host-creates-its-registry-no-reverse-bootstrap.md) is then asked
for across accounts, by setup as the person.

### 3.4 Isolated and not isolated: how few switches

Owner direction 2026-11-20: isolation must be switchable off, so that local development
and testing never ask for a password. The architecture supports both, with one
setting.

- **One instance-level setting**, chosen at setup and recorded in the instance record:
  `agentic-org setup --isolation off|accounts`. **Owner decision 2026-11-20: `accounts`
  is the default for an install; development uses `--isolation off`** (the default of a
  development build). Changing it later is a maintenance
  command that re-runs setup's plan (ADR-0015 decision 3 already allows changing the
  mode later). It also moves the data root out of the person's home or back, and moves
  the host secrets between custodies, while the instance is stopped; that move is part
  of the work below, and a fresh instance is the simpler path when nothing is kept.
- **`off`** is today's layout: everything runs as the person's account, a per-user
  launchd agent or systemd user unit, the data root and secrets in the person's home,
  and agents in ADR-0015's `same-user` mode, still inside the os-sandbox.
  **`accounts`** is this note: the service account, the system-level service, the data
  root outside the home, and agents in ADR-0015's `dedicated-user` mode (one account
  per office). The owner's per-agent override of ADR-0015 decision 6
  (for example `same-user` for an iOS agent) is a **weaker state** under `accounts`:
  the agent would run as the service account, which owns every office's data and every
  host secret, so only the os-sandbox would stand between it and all of them (today on
  macOS the Keychain access group still keeps the keys from such an agent). The
  proposal therefore does not allow that override under `accounts`; an agent that
  cannot run as an agent account runs in an office on an instance set to `off`, or on
  its own Mac, or the owner accepts it
  as a recorded exception that the os-sandbox must cover (SEC-12; question 11).
- **Never a silent fallback** (ADR-0015 decision 8). `off` is a choice the person makes
  and the product shows. Interactive setup without administrator rights asks "continue
  without isolation?" and records `off` only on yes; a non-interactive setup that asks
  for `accounts` without administrator rights fails before any change. An `accounts`
  instance whose launcher, helper or agent account is missing refuses to run the
  affected agents; it never switches itself to `off`.
- **The composition root selects adapters once.** The host and the office read the
  setting at start and pick one adapter for each seam below. No other code reads the
  setting, so there are no scattered `if isolated` checks
  ([architecture rules](../architecture-rules.md#planned-variation-goes-behind-an-adapter-from-day-one)).

| Seam | `off` | `accounts` | Where it lives |
| --- | --- | --- | --- |
| 1. Process launch | Direct spawn, as `org-supervise` does today | Through the launcher, as the agent account | The launch contract (`org_platform::isolation`, [Linux host design 3.5](linux-host.md#35-agent-isolation)) |
| 2. Service registration | User level: launchd agent, systemd user unit | System level: launchd daemon with `UserName`, systemd system unit with `User=` | `org_platform::service` |
| 3. Socket admission | Allowed UIDs: the person (who is also the service user) | `host.sock`: the person; office sockets: the agent accounts; with matching directory and socket modes | Data passed to the peer check (today one UID, `peer_allowed(&stream, expected_uid)`), not a second adapter |
| 4. Data-root and secrets locations | In the person's home (`org_platform::dirs` today) | System locations owned by the service account ([section 3.2](#32-the-data-root-moves-out-of-the-persons-home)) | `org_platform::dirs` |
| 5. Secrets custody | Today's credential provider | The custody the spike chooses for a service account ([section 6.3](#63-no-dependency-on-the-macos-login-keychain)) | `org_platform_host::credentials` |
| Setup's privileged plan | Empty | The full plan of [section 5](#5-one-command-setup) | Setup |

The os-sandbox is the same in both settings; only the paths in its profile differ.

**Conformance.** Each seam's two adapters pass the same contract tests: start, stop,
cleanup and orphan recovery for launch; register, start, stop and status for the
service; admit and refuse for admission; the location and ownership checks; add, load
and delete for custody.

**How each is tested.**

- `off`: the ordinary local and CI tests, with no root and no prompt, as today.
- `accounts` on Linux: CI jobs that run as root in a disposable container or VM, create
  the accounts, run the conformance suites and the security checks of
  [section 11.2](#112-security-checks) as the agent account, and are thrown away.
- `accounts` on macOS: the spike on a dedicated test machine or VM first; then a
  scheduled job on a dedicated Mac, or on hosted macOS runners if they allow
  passwordless `sudo` (to verify).

**Complexity, honestly.** Five seams; four need a second adapter on each of two
systems (about eight adapter implementations), and admission is a data change. On top
of that: the launcher program, the closed-list helper, setup's privileged plan, the
move of data and secrets when the setting changes, and a
test matrix that doubles the conformance suites (two settings on two systems, four
cells), two of which need root and one a dedicated Mac. The launcher, the system-level
service and secrets custody are most of the work; locations and admission are small.
Nothing in the consuming code changes per setting, which is what keeps it at five
seams.

## 4. Starting agents as their accounts without being an administrator

An ordinary process cannot start or signal a process of another user
([ADR-0015](../adr/0015-agent-isolation-modes.md) decision 4). The owner's idea: at
setup, grant the service account the right to start processes **as the listed agent
accounts only**, and nothing else. Three ways to grant it; the spike chooses, within
the owner's answer for Linux below.

### 4.1 Options

| | (1) sudoers rule to a small launcher | (2) Linux: systemd transient units with a polkit rule | (3) Root launcher daemon (ADR-0015 decision 4's privileged launcher) |
| --- | --- | --- | --- |
| How | `/etc/sudoers.d/agentic-org-<instance-id>` lets the service account run exactly one program, our launcher, only as members of our agent-account group, `NOPASSWD`, with no other command. The launcher runs **as the agent account**, validates the request and starts the agent | The office calls systemd over D-Bus to start a transient unit with `User=<agent account>`; a polkit rule allows the service account to manage units with our name prefix | A root daemon listens on a socket only the service account can reach, validates each request and starts the agent with `setuid` |
| What runs as root | `sudo` itself (system code), which may stay as a root parent of the agent for its whole run (see below). None of our code | systemd (system code). None of our code | Our daemon, all the time |
| Security | Smallest grant: our code never holds root, and the grant names the accounts and the program. Risks: a sudoers mistake; `sudo` must be present; on Linux the host unit's `NoNewPrivileges=yes` ([Linux host design 3.4](linux-host.md#34-service-lifecycle)) blocks `sudo`, and because no-new-privileges is inherited and cannot be cleared, the offices (children of the host) can use `sudo` only if the host unit drops it for the host, the web process and every office, or if each office runs in a unit of its own | **Unverified and likely too broad:** polkit sees the unit name and verb, not the unit's properties, so a rule that allows starting transient units may let the service account start one as root. The spike must prove a rule that limits `User=`, or this option is rejected | Largest attack surface: a long-lived root process that parses requests. Works with `NoNewPrivileges=yes` because nothing is executed set-user-ID |
| Friction | One file at setup; no daemon | One rule file at setup; Linux only | A second service to install, update and keep running |
| macOS | Yes (`sudo` and `/etc/sudoers.d` are standard; verify the include directive on current releases) | No | Yes |
| Linux | Yes, where `sudo` is installed; not in minimal containers unless the image adds it | Yes, with systemd and polkit; not in a container | Yes |
| Container | Image adds `sudo` and the rule; the runtime must not set `no-new-privileges`, which the repository's documented hardened run sets today (`deploy/container/README.md`) | No | Image adds the daemon; runs root inside the container |
| Whose child the agent is | The office's (through `sudo` and the launcher), so the office supervises it as today | systemd's: the office supervises it through the unit, and the launcher's role (validation, the inner wall) runs as the unit's command; the ownership mark and orphan rules must be restated | The daemon's: the office asks the daemon to signal it; the evidence rules of ADR-0015 decision 4 and "agents are never left orphaned" are restated for that |

**Recommendation for the spike to confirm:** option 1, because none of our code runs
as root and the grant is visible in one file.

**Owner's answer for Linux (2026-11-20).** Each office runs as its own instance of one
shared systemd template unit, `agentic-org-office@.service`, which setup installs, and
which does not set `NoNewPrivileges`, so the office's `sudo` to the launcher works. A
polkit rule, also installed at setup, lets the service account start and stop only
`agentic-org-office@*`. The host unit keeps `NoNewPrivileges=yes`, and so does the web
process. A new office needs no new file and no administrator. Consequence for the
design: on Linux the host no longer starts the office itself, so Linux admission
(the verified memory-file start of ADR-0017 decision 7) and the rendezvous peer check
must be redone for an office that systemd starts; the accounts ADR designs it and the
spike checks it (ACCT-6). On macOS the issue does not exist: the host starts offices
as today, and any per-office system file goes through the helper.

If `sudo` is absent, option 3 is the fallback on that system, with the cost named in
[section 3.1](#31-who-runs-what). Option 2 is kept only if the spike shows polkit
can restrict the account. Rejected without a spike: giving the office executable
`CAP_SETUID`, because that capability also allows becoming root.

**How `sudo` must be configured (likely; to verify).** With a PAM session, a
pseudo-terminal (`use_pty`, the default in recent `sudo` releases) or I/O logging,
`sudo` stays as a root monitor process between the office and the agent for the whole
run. The office's direct child would then be a root process, which affects the
ownership mark, pidfd and start-time evidence and signalling. The rule therefore
names the launcher with an empty argument list and disables these for it (for
example `Defaults!<launcher> !use_pty, !pam_session` and no `log_output`), and the
spike checks the process tree and signalling on both systems (ACCT-8).

### 4.2 The launcher

The launcher is one small, separately installed program (root-owned, not writable by
the service account or the agent accounts). In option 1 it runs as the target agent
account; in option 3 the daemon calls the same validation code.

- **It validates every request** against a registration file that only root writes (setup, and later the helper)
  (root-owned, readable): the pair (calling service account, target agent account)
  must be registered; the program must be one of the registered vendor runtimes or an
  extension's registered helper; the working directory and home must be under that
  account's fixed root prefixes, written when the account is made (by setup or the
  helper). Because organization and office folders are created later, that prefix is in
  practice the instance's data root: the check adds little, and file permissions are
  what decide. The binding of an account to an office is the office's record. The
  request is a fixed, length-prefixed structure at the start of standard input, never
  free text on the command line; after it, standard input passes to the vendor tool,
  whose headless protocol uses it ([ADR-0004](../adr/0004-headless-agent-sessions.md)).
  `sudo` closes other descriptors, so the spike confirms this framing (ACCT-9). Anything else is refused and logged.
- **It applies the inner wall.** It enters the os-sandbox profile ([section 7](#7-the-sandbox-inside-the-accounts))
  before it executes the vendor program, so the agent never runs unconfined.
- **It keeps ADR-0015 decision 4's evidence.** Start, stop, cleanup and orphan recovery
  go through it, with the same ownership mark and process-identity evidence the office
  uses today (`SessionMark`, start time, pidfd on Linux). Stop and cleanup are
  launcher requests as well: the launcher, running as the agent account, signals only
  processes of that account that carry the requested mark.
- **No silent downgrade** (ADR-0015 decision 8): if the launcher, the grant or the
  account is missing, an agent configured for its account is not runnable.
- **Agent-writable paths are opened defensively.** The service account now handles
  files an agent account can write: the run copy of a sign-in it takes back, worktrees
  and the shared parts of the Git cache, the agent's keychain. The office and the
  launcher open every such path without following links, beneath a held directory,
  and check the owner and the link count before reading or writing. Otherwise an agent
  could swap its token file for a link to a host secret and have the office "take it
  back" (SEC-11). macOS has no hard-link protection, so the link-count check matters
  there.

## 5. One-command setup

`agentic-org setup` stays the one command (ADR-0005 decision 8). It runs as the person,
asks for administrator rights **once**, and does every privileged step in that one
prompt:

1. Create the service account and one agent account, bound to the first office when
   that office is created (setup creates no office, ADR-0005 decision 2) (hidden,
   non-administrator, no password, no login shell); later accounts are made by the
   helper without a prompt, or come from a pool made here under the alternative
   ([section 5.2](#52-administrator-rights-after-setup)).
2. Create the data root and the host secrets directory owned by the service account,
   outside the person's home ([section 3.2](#32-the-data-root-moves-out-of-the-persons-home)).
3. Install the launcher and write the grant (sudoers rule, polkit rule or root
   launcher daemon, per [section 4](#4-starting-agents-as-their-accounts-without-being-an-administrator)),
   and install the closed-list helper with its grant ([section 5.2](#52-administrator-rights-after-setup)).
4. Register the system-level service (launchd daemon with `UserName`, or a systemd
   system unit with `User=`) and start it.
5. Run the privileged steps declared by the extensions the person chooses to
   prepare (for example adding the agent accounts to the `kvm` group,
   [section 8.5](#85-the-containers-extension), or the `_developer` group,
   [section 8.6](#86-the-xcode-extension)).
6. Print a short plain checklist for what it cannot automate (for example "accept the
   Xcode licence"), each line with the command to run and how to check it.

Rules:

- **The privileged step runs a fixed plan, and checks every value.** The unprivileged
  part computes the plan; the privileged part executes only a closed list of step kinds
  (create account, set ownership, install file from the release, write grant, register
  service, add to group, install a system package from the system's package manager at a pinned
  version with its checksum, and the system settings named under "Platform steps" below), and it
  validates every value itself, whoever computed it: paths only under the instance's
  fixed prefixes, accounts only with our prefix, groups only from a fixed list that
  never includes `admin`, `wheel`, `sudo` or `docker`, files only with the digest the
  verified release names. A plan that fails any check is refused whole.
- **Root verifies the release itself.** The privileged step and the helper check the
  release's signature against a root-owned trust registry (the publisher key) that
  setup installs ([ADR-0009](../adr/0009-complete-release-verification.md)); they
  never trust a manifest or digests read from the service account's release store.
- **The grant does not change at run time.** The launcher grant names a group of agent
  accounts as its run-as list (`(%<agent group>)`), so a new account needs only a group
  membership and `/etc/sudoers.d` is never rewritten while the service runs; when setup
  writes it, it is validated (`visudo -cf`) and installed atomically (PRIV-2).
- **Root runs only root-owned code.** The privileged step and the helper run from a
  root-owned, release-verified copy, never from the service account's release store,
  the entry point the service account repoints, or the person's home; otherwise a
  compromised service account could plant code that root runs at the next privileged
  operation (spike PRIV-1). Extensions contribute steps from that same
  list; they never run their own code as root. A system package (for example QEMU on
  Linux) is the one step that runs third-party code as root, through the package's
  own install scripts; it is listed in the plan by name so the person sees it, and the
  extensions ADR decides whether it is allowed or the checklist asks the person to
  install it.
- **Platform steps** that need root and that setup also performs in the one prompt,
  or lists in the checklist where the system does not allow automating them:
  `subuid`/`subgid` ranges for agent accounts (rootless containers); on macOS,
  `DevToolsSecurity -enable` and accepting the Xcode licence (Xcode extension); on
  Ubuntu 24.04 and later, loading the AppArmor profile that grants `userns` to the
  vendor sandbox helper ([Linux host design 3.5](linux-host.md#35-agent-isolation)).
  Without that profile no agent starts on such a host, in either isolation setting, so
  a setup without administrator rights cannot start agents there and says so.
- **Changing the isolation setting later** (for example from `off` to `accounts`)
  re-runs this plan and asks once; it is one of the named exceptions of
  [section 5.2](#52-administrator-rights-after-setup).
- **Without administrator rights** setup does not fall back by itself: it offers
  `--isolation off` (today's per-user service, agents `same-user` inside the
  os-sandbox, ADR-0015 decision 3) and records it only when the person accepts; the
  product then shows it where the mode is shown
  ([section 3.4](#34-isolated-and-not-isolated-how-few-switches)).
- **No silent downgrade** (ADR-0015 decision 8). An instance set up with accounts never
  quietly runs an agent in `same-user`.
- **Repeatable.** Running setup again resumes or verifies; a step already done is
  checked, not redone. Removal is a maintenance command that undoes each step it
  recorded and preserves data by default (ADR-0005 decision 5).
- **Container.** The image creates the accounts and the grant at build time; setup
  inside the container needs no administrator step (ADR-0017 decision 4).
- **Not again for the core's everyday use.** Every administrator step the core's
  everyday use needs happens in this one command: creating organizations, adding an
  office on the instance, hiring and starting agents, and the service and upgrade
  commands run without a prompt afterwards. The named exceptions are listed in
  [section 5.2](#52-administrator-rights-after-setup).

### 5.1 If the person is an administrator, can we skip the prompt?

No, not entirely. Being an administrator of the machine means the person **may**
elevate; on macOS and Linux each elevation still needs them to authenticate once
(password, or Touch ID where `sudo` is set up for it). Creating accounts, writing
`/etc/sudoers.d`, `/Library/LaunchDaemons` or a systemd system unit, and changing group
memberships all need root. So:

| Can avoid a prompt | Cannot avoid one |
| --- | --- |
| The core's everyday use after setup (section 5.2 lists the exceptions) | The first creation of the accounts, the data root outside the home, the launcher grant and the system service |
| A container: the image does the privileged steps at build time | A machine where the person has no administrator rights: setup offers the explicit `--isolation off` choice and records it only on yes |
| A Linux machine whose administrator already set passwordless `sudo` for the person: elevation still happens, without a prompt | |
| A second step in the same setup run: `sudo` keeps its timestamp for a few minutes, and setup runs every privileged step in one invocation anyway | |

The person sees one authentication, at setup, and not again in the core's everyday
use; section 5.2 names the few exceptions.

### 5.2 Administrator rights after setup

Some later actions could need root: an agent account for a new office or (later) for a
new agent, a group membership for an extension enabled later (`kvm`, `_developer`), or
an extension's system part (for example QEMU on Linux). The options:

| | (a) A tiny privileged helper with a closed allow-list | (b) Pre-provision at setup | (c) Platform approval flows |
| --- | --- | --- | --- |
| How | Setup installs a root-owned helper the service account may run as root (one more sudoers line, or a polkit rule with `pkexec` on Linux). It is not a daemon: it runs for one request and exits. It performs only listed operations: create or remove an agent account with our prefix, add it to a group from a fixed list, update the launcher grant, install the launcher or an extension's file from the verified release, install an extension's system part from a release-pinned package with its checksum, and start this instance's service | Setup creates what later steps would need: a pool of agent accounts (for example one per expected office plus spares), their group memberships for extensions the person chooses at setup, the launcher grant for the whole pool, and the system parts of chosen extensions | macOS: `SMAppService` registers a privileged helper that the person approves once in System Settings. Linux: polkit rules for specific actions |
| Later prompts | None | None, until the pool or the chosen extensions run out | None after the one approval |
| Root code | Ours, reachable from the service account on demand | None after setup | Ours (macOS helper daemon); none beyond (a) on Linux |
| Attack surface | A compromised service account can call every listed operation: it can create accounts and add them to the listed groups, but never to `admin`, `wheel`, `sudo` or `docker`, and never install an unpinned package. The parser and each operation are root code to review and keep small | Idle accounts and grants exist before they are used; each is non-administrator, passwordless and without a login shell, so an unused account adds little. Group memberships granted early are broader than needed | macOS: needs an app bundle signed by us and a graphical session for the approval; the product is a command line and a service, so this adds a bundle to maintain. Linux: as (a) |
| Fit | Everything after setup, including per-agent accounts later, where hiring creates an account | Agent accounts per office, and the extensions known at setup | Not a fit for v1 |

**Service lifecycle and upgrades.** The core's maintenance commands (`service
start`, `stop`, `status`, `logs`, `upgrade`, `uninstall`;
[standalone design](standalone-server.md#service-and-updates)) act on a system-level
service, which normally needs root. To keep them free of prompts:

- **A stable entry point.** Today the unit and the plist name the pinned executable
  ([Linux host design 3.4](linux-host.md#34-service-lifecycle)), so changing the pin
  would rewrite a root-owned file. Instead the system unit or daemon names a stable
  entry point that the service account repoints to the active release, so an upgrade
  never rewrites it.
- **Linux start and stop:** a polkit rule that lets the person (or a group setup
  creates) start, stop and restart only this instance's unit by name; here polkit fits,
  because it sees the unit name and the verb.
- **macOS start and stop:** today every stop exits 0 and the plist restarts only on
  failure (`KeepAlive { SuccessfulExit = false }`), so a stopped daemon could be
  started again only with root (`launchctl kickstart`). With the helper, "start this
  instance's service" is one of its listed operations, which the command-line group may
  also call; the spike compares it with a stop that quiesces the instance while the
  host keeps running, and with on-demand start through launchd's `Sockets`.
- **Releases:** a release store owned by the service account, per instance (today one
  store is shared by every instance on the machine, [standalone design](standalone-server.md#service-and-updates);
  with a service account per instance that store needs a per-instance copy or an owner
  of its own), so the host can stage and activate an upgrade without root, and the
  person's other programs cannot replace the executables (the
  [Linux host design 5](linux-host.md#5-what-a-local-attacker-can-still-do-on-linux)
  case). Root-owned releases would close that case for the service account too, at the
  price of a prompt for every upgrade; the spike weighs the two.
- **The launcher** is root-owned ([section 4.2](#42-the-launcher)), so a release
  that changes it needs root; the helper installs it from the verified release. It is
  kept small, stable and versioned on its own.
- **Logs, status and doctor:** the host's logs live in a directory owned by the service
  account that the command-line group may read, so `logs`, `status` and `doctor` work
  when the host is down; the exact place is for the accounts ADR.
- **`uninstall`** removes accounts, grants and the system service, so it asks for
  administrator rights once; it is the end of the instance, not everyday use.

**Recommendation: the closed-list helper (a)**, confirmed by the owner on 2026-11-20 (D20),
still to be proven by the spike (HLP checks in
[section 11.8](#118-administrator-rights-after-setup)). With it, everything after setup
runs without a prompt: a new office's agent account, a launcher update, preparing an
extension, a macOS start after a stop, and later per-agent accounts when an agent is
hired. Its safeguards:

- It is not a daemon: it runs for one request and exits.
- Only the service account may run it (and the command-line group, for "start this
  instance's service" alone).
- The host calls it only for an operation an authorized person asked for (an
  administrator's grant, checked by the host), and the helper checks its own list
  whatever the caller: it never adds an account to `admin`, `wheel`, `sudo` or
  `docker`, never touches an account without our prefix, never installs an unpinned
  package or a file that is not in the verified release.
- Its requests are a fixed structure, it logs every operation, and it is kept small
  enough to review whole.

**The alternative: pre-provisioning (b).** Setup creates a pool of agent accounts
(sized by a setup option: the offices on this instance plus spares) and prepares the
extensions the person chooses. Nothing of ours then runs as root after setup, but
growing the pool (`agentic-org setup --grow-pool`), a launcher update, preparing an
extension that setup did not prepare (`agentic-org extension prepare <name>`) and, on
macOS unless the spike picks the quiesce or `Sockets` start, a start after a full stop
each ask for administrator rights once, from a terminal. The spike records which of the two
holds up.

**Named exceptions** in both: `uninstall` asks once (it removes the accounts, the
grants, the helper and the system service, and ends the instance), and so does
changing the isolation setting later ([section 5](#5-one-command-setup)). The existing
recovery step stays as it is: on Linux, `admin reset` runs as root
([ADR-0017](../adr/0017-linux-and-container-hosting.md) decision 11).

## 6. Secrets

### 6.0 The secrets chain in plain words

The owner asked where the password for the private key is. There is no password. The
chain today, as accepted and as the code holds it:

1. **The private keys and provider credentials are encrypted records.** An office's
   private key (the CA key at HQ) and, when built, each provider sign-in are
   AES-256-GCM records (`org_pki::wrap`), each bound to its purpose, organization and
   office ([ADR-0016](../adr/0016-no-application-level-data-encryption.md) decision 3,
   [ADR-0032](../adr/0032-browser-driven-cli-signin-and-remote-token-distribution.md)).
2. **They are keyed by the office secrets key, which is random, not a password.** The
   host makes it from 32 random bytes (`generate_office_secrets_key` in
   [`crates/org-host/src/secrets.rs`](../../crates/org-host/src/secrets.rs)), one per
   office.
3. **The host keeps that key in its credential provider**
   ([`crates/org-host/src/keys/mod.rs`](../../crates/org-host/src/keys/mod.rs)):
   - macOS (release builds): a Data Protection keychain item in the signed host's
     access group, available after first unlock, this device only; other code gets
     "item not found", never a prompt
     ([host boundary](office-host-boundary.md#the-office-secrets-key));
   - Linux: owner-only files in the host's credentials directory outside the data root
     ([Linux host design 3.2](linux-host.md#32-credential-provider)), or a mounted
     secrets volume in a container;
   - development builds and tests: plain files in a development folder, never a real
     keychain.
4. **The host hands it to the office worker at launch, in memory, never on the first
   line.** The worker's first line on stdin holds no secret. The worker connects to a
   per-spawn rendezvous socket and sends `ReadyForSecrets`; the host checks that the
   connected peer is the admitted child and answers with one handoff line on that
   socket carrying the key
   ([host boundary: admission and the verified rendezvous](office-host-boundary.md#admission-and-the-verified-rendezvous)).
   The key is in the host's memory only from creation or load until that handoff is
   written; the worker keeps one copy in memory
   ([host boundary](office-host-boundary.md#admission-and-the-verified-rendezvous),
   [`contract.rs`](../../crates/org-office-launch/src/contract.rs)). (The module doc
   of `crates/org-host/src/keys/mod.rs` still says "the first line of the launch pipe";
   that comment is stale and is a code follow-up.)
5. **Agents never receive it** (non-negotiable 10, ADR-0016 decision 3). An agent
   receives only a copy of its own sign-in for the run, which the office places in its
   private home and takes back at the end
   ([ADR-0015](../adr/0015-agent-isolation-modes.md) decision 11).

**What changes under a service account nobody logs into.** Step 3 on macOS is the one
link in question: whether the Data Protection keychain serves a launchd daemon running
as an account that never logs in is unverified, and that account's login keychain is
locked. The options, for the spike to decide (SECR-1 to SECR-3):

- an owner-only file, as on Linux (the same directory store already serves development
  builds on macOS);
- a dedicated keychain file that the helper or the host unlocks at start;
- a key bound to the hardware: the Secure Enclave on macOS, a TPM on Linux.

Steps 1, 2, 4 and 5 do not change.

### 6.1 What protects data and keys at rest today

Recorded as the owner asked; nothing here changes a decision.

- **Data is not encrypted by the application.** ADR-0002 first required SQLCipher for
  every office database; [ADR-0016](../adr/0016-no-application-level-data-encryption.md)
  superseded that (accepted 2026-11-17, implemented in increment A1): the office
  databases, the host registry and every file under `durable/` and `runtime/` are plain,
  and the workspace no longer builds SQLCipher
  ([ADR-0002](../adr/0002-storage-sqlite-sqlcipher-one-writer.md) status line).
- **What protects data at rest** is the operating system's disk encryption (FileVault on
  macOS, for example LUKS on Linux), private file permissions and agent isolation
  (ADR-0016 decision 2). Setup **checks disk encryption and warns** when it is off; it
  does not require it (ADR-0016 decision 2, ENC-3). The macOS check reads
  `diskutil info -plist` for the data root's volume
  ([Linux host design 3.4](linux-host.md#34-service-lifecycle)). The Linux check
  ([`disk/linux.rs`](../../crates/org-platform/src/disk/linux.rs)) follows the data
  root's device to dm-crypt (LUKS and other formats) through sysfs without privilege;
  it cannot see encryption by the drive, a hypervisor or a cloud provider, or fscrypt
  on a folder, and says so.
- **The office private key** (the organization CA key, at HQ) is stored only wrapped:
  AES-256-GCM, `org_pki::wrap`, authenticated-data domain
  `agentic-org/office-private-key/v1`, under the office secrets key (ADR-0016
  decision 3). The record is the file
  `<data root>/<organization-id>/durable/local/offices/<office-id>/security/ca-key.v1.json`,
  written in HQ creation's acceptance-journal entry
  ([office store design](office-store.md)). The store writes its files `0600` and
  its directories `0700` (`crates/org-store/src/files.rs`, `disk.rs`); the owner is the
  account that runs the office, today the person's own account.
- **The office secrets key** that wraps it is in the host's credential provider: on
  macOS the Data Protection keychain (service `org.agentic.office-key`, or `org.agentic.office-key.instance.<id>` for a
  registered instance, account the office ID, the signed host's access group, available after first unlock, this device only);
  on Linux an owner-only directory outside the data root, or a mounted secrets volume
  ([host boundary](office-host-boundary.md#the-office-secrets-key)).
- **Provider credentials** are wrapped by the office worker with the same record format
  under the separate domain `agentic-org/provider-credential/v1`, bound to the
  organization, office and connection, in
  `durable/local/offices/<office-id>/auth/<connection-id>/` (ADR-0016 decision 3,
  [ADR-0032](../adr/0032-browser-driven-cli-signin-and-remote-token-distribution.md);
  accepted, not built).

### 6.2 Where each secret lives (proposed)

| Secret | Proposed custody | Readable by | Source of the rule |
| --- | --- | --- | --- |
| Host secrets: each office secrets key, the web TLS key, the pending recovery code, writer custody | Files in the host secrets directory owned by the service account, directory `0700`, items `0600`, on an encrypted disk, outside the data root | The service account | ADR-0016 decision 3, ADR-0017 decision 8 (its Linux rules extended to macOS), ADR-0007 |
| Office private keys (CA key; later TLS identity and invitation keys) | Unchanged: wrapped records in the office store under the office secrets key | The service account (wrapped) | ADR-0016 decision 3 |
| Provider credentials | Unchanged: wrapped under `agentic-org/provider-credential/v1` in `durable/local/offices/<office-id>/auth/<connection-id>/` | The service account (wrapped) | ADR-0016 decision 3, ADR-0032 |
| An agent's run copy of its sign-in | The agent's private home, for the run; taken back at the end | That agent account (and, while agents share an account, its other agents) | ADR-0015 decision 11 |
| Xcode signing identities | A dedicated keychain file per agent account, created by the Xcode extension; its password held by the office (wrapped like a provider credential); unlocked by the office at agent start, through the launcher | That agent account, while unlocked | This note, [section 8.6](#86-the-xcode-extension) |

### 6.3 No dependency on the macOS login keychain

A service account that never logs in has no unlocked login keychain, and whether the
Data Protection keychain the host uses today works for a launchd daemon running as
such an account is **unverified**. **Owner's answer (2026-11-20):** host secrets are
owner-only files for now, on macOS too, as on Linux; development never requires
signing. The Keychain with a Developer-ID-signed host stays an option for releases
later, and per-office service accounts stay a later option.

Owner-only files lose one macOS property the
[Linux host design 5](linux-host.md#5-what-a-local-attacker-can-still-do-on-linux)
names: the Keychain gives secrets only to the signed host. Under a service account
that runs only our binaries, the processes that can read the files are the
host, the web process (which is confined and holds no key-store access, ADR-0008) and
the offices; agents and the person's programs are other accounts. Within the service
account there is no separation: every office worker could read every office's secrets
key, the web TLS key and the pending recovery code. The web process cannot: once
confined it opens no file, on Linux by the seccomp filter alone even without Landlock
(ADR-0017 decision 9, [Linux host design 3.3](linux-host.md#33-confinement-of-the-web-process)).
Per-office service accounts would narrow this (a later option). When the release
option is taken up, a headless keychain or the Keychain with a signed host is compared
with the files (SECR-2); a headless keychain needs its own unlock secret, which is
itself a file of the service account.

Claude's sign-in needs no Keychain: the C1 spike showed that a
`CLAUDE_CODE_OAUTH_TOKEN` avoids it on macOS ([C1 spike](../research/c1-sign-in-spike.md),
macOS only). The accounts spike repeats that under an agent account with no login.

### 6.4 Options for the spike to evaluate

These are options, not decisions. The spike records each one's cost and what it adds.

| Option | What it adds | Cost or doubt |
| --- | --- | --- |
| Require disk encryption at setup (refuse instead of warn); **not taken** (owner, D21: check and warn) | Data and wrapped keys are never on an unencrypted disk | Blocks machines that cannot turn it on; a container cannot see the host's disk |
| macOS Secure Enclave for the office secrets key (wrap it with a Secure Enclave key) | The key cannot be copied off the machine | Needs keychain access from a daemon under a service account (unverified); Secure Enclave keys are P-256 only, so the wrap is an ECIES step |
| Linux TPM 2.0 for the office secrets key, for example `systemd-creds` with `LoadCredentialEncrypted=` in the system unit | The key file is useless on another machine; systemd decrypts it at service start | Needs a TPM and a systemd system unit (this note's proposal); not in a container |
| A headless keychain file for the service account on macOS | Keychain semantics without a login | Its unlock secret is another file of the service account (section 6.3) |
| Files only (**chosen for now**, D16) | Simplest; same rules on both systems | Relies on disk encryption and the account boundary |

### 6.5 Option: the host stays with the person on a desktop Mac

**Not adopted (owner, 2026-11-20).** Kept as the record of an evaluated option; nothing
below is planned, and the KEY checks of [section 11.7a](#117a-the-host-with-the-person-on-a-desktop-mac)
are not planned. The host runs as the service account everywhere under `accounts`.

The owner's question of 2026-11-20, evaluated. The idea: on a desktop Mac, the
host (the service setup registers) keeps running as the person who installed it, as
the per-user launchd agent does today, so it has that person's Keychain without any
unlock tricks. Only the offices, the web process and the agents move to other
accounts.

**How it would work.**

- The host runs as the person, from the per-user launchd agent of today
  ([`service/macos.rs`](../../crates/org-platform/src/service/macos.rs)). It keeps each
  office secrets key in the Data Protection keychain in its signed access group, as
  today ([section 6.0](#60-the-secrets-chain-in-plain-words)).
- It starts the offices and the web process as the service account, through the grant
  setup made: the person may run our launcher as the service account (and only that),
  in addition to the service account's grant to the agent accounts.
- **Who calls the helper.** [Section 5.2](#52-administrator-rights-after-setup) lets
  only the service account run the helper, and has the host call it; here the host runs
  as the person. In this placement the person's UID is never granted the helper, not
  even the command-line group's "start this instance's service" operation: the person
  is left out of that group's helper grant, because their host is a per-user launchd
  agent and needs no such start. The host relays each request through a process of the
  service account (an office, or a small relay the launcher starts as the service
  account), which runs the helper; the helper's own checks stay the same (HLP-2
  restated for this placement).
- **The relay must authenticate its caller** in the same way as the launcher (the
  host's audit token on a socket it verifies, or a one-time secret only the host
  holds). Otherwise any of the person's programs could reach the relay and get helper
  operations run (KEY-9).
- It hands the secrets down in memory exactly as now: the launch contract's per-spawn
  rendezvous socket and its handoff line, then the descriptors of the transfer
  records, including the writer-control connection
  ([host boundary: admission and the verified rendezvous](office-host-boundary.md#admission-and-the-verified-rendezvous);
  `org_office_launch::writer` is "writer-custody control over a host-transferred
  connected descriptor"). A Unix socket carries a descriptor between accounts, so the
  handoff itself needs no change beyond the rendezvous directory and socket being
  reachable by the service account and the peer check naming the service account's UID.
- The office hands each agent only its own sign-in copy, as everywhere else.
- **The launcher must authenticate the host** in this placement, because the grant
  belongs to the person's UID: for example it checks that its caller is the signed host
  by the caller's code identity (audit token), or requires a secret only the host
  holds. Under sudoers option 1 the launcher's parent is `sudo`, not the host, so the
  check cannot look at the parent process: the host presents a token over a socket
  whose peer the launcher verifies (the host's audit token), or a one-time secret. It
  accepts the office and web images and the pair (person, service account) for them,
  and does not apply the agent sandbox profile to them: the web process confines
  itself (ADR-0008), and the office keeps its own rules.

| | Pros | Cons |
| --- | --- | --- |
| Secrets | The Keychain works as today: the key stays in the signed host's access group; no headless keychain, no unlock secret on disk. Resolves the locked-Keychain question of [section 6.3](#63-no-dependency-on-the-macos-login-keychain) on a desktop Mac | The person's own programs run as the same user as the host; on macOS the Keychain access group still keeps the keys from them (as today) |
| Recovery | ADR-0011's presence check holds unchanged: the peer is the console user with the host's own UID, in the same console session. Resolves that open item on a desktop Mac | — |
| When it runs | — | The host runs only while the person is logged in. Fine for a desktop Mac; not for a headless server. Headless Macs and every Linux server use the service-account host of [section 3](#3-accounts-proposed) |
| Trust | The core (host) is trusted; the untrusted code, the agents, still runs in other accounts | The host runs with the person's file access, so a fault in the host can reach the person's files. The person's other programs are **not** kept from the offices: the grant (person → service account) belongs to the person's UID, so any of the person's programs could start a genuine office image and supply the handoff itself, unless the launcher authenticates the host (see above); the host's plist in `~/Library/LaunchAgents` and its launch path are writable by the person's programs (the [Linux host design 5](linux-host.md#5-what-a-local-attacker-can-still-do-on-linux) case, now on macOS too); and the group or access-control entry the host needs on office folders opens them to every program of the person |
| Host's access to office folders | — | Read and limited write: the host reads the catalog and `office.json`, moves refused folders to `.refused/<id>`, writes the office logs and runs the orphan sweep. `office.json` is accepted today only when owned by the service user, the host's own UID ([host boundary](office-host-boundary.md#officejson)); that rule changes to the service account |
| Admission | — | Today the host spawns the worker suspended and checks its code identity before it runs ([host boundary](office-host-boundary.md#admission-and-the-verified-rendezvous) step 1 and 2). Through a grant to another account the host is no longer the direct parent, so it cannot spawn it suspended; admission must instead check the connected peer's code identity through its audit token at the rendezvous, or the launcher must do the suspended spawn. Needs design and the spike |
| Control | — | The host cannot signal processes of the service account: the stop's final kill and orphan cleanup go through the launcher. It cannot run the helper either: each privileged request is relayed through a service-account process, one more path to build and test |
| Complexity | — | A second host placement to build and test (person-hosted on a desktop Mac, service-hosted elsewhere): one more layout in the test matrix of [section 3.4](#34-isolated-and-not-isolated-how-few-switches) |

**It resolves:** the locked Keychain and ADR-0011's recovery presence, on a desktop
Mac. On a Linux desktop the same placement would keep ADR-0017 decision 11's recovery
peer (the host's own user) as it is, without the Keychain benefit; the spike may weigh
that too. **It adds:** admission without a suspended spawn, signalling through the
launcher, the host's read access to service-owned folders, a person-to-service-account
grant, and a second host placement. It does not help headless servers.

**Recommendation at the time (not adopted), if the spike (KEY-1 to KEY-9) held:** use it on desktop Macs under
`accounts`, chosen by setup on macOS when it runs in a graphical session, and the
service-account host everywhere else. It is not a third value of the isolation setting,
but it is a second choice setup records (the host placement), read only by the
composition roots. It changes more than service registration: process launch (offices
and the web process through the launcher), secrets custody (the Keychain, not files),
process admission (no suspended spawn), the host's access to office folders and the
`office.json` owner rule.

## 7. The sandbox inside the accounts

| Layer | What it stops | Owner |
| --- | --- | --- |
| Accounts (outer wall) | Reading the data, the host's secrets, the person's home, other offices' agents' homes; signalling other accounts' processes; gaining administrator rights | Setup and the launcher, this note |
| `os-sandbox` (inner wall) | Writing anywhere outside the agent's private home and workspace; reading outside them and read-only system tools; a tool that ignores `HOME` | The office, [FC-190](../FEATURES.md), ADR-0015 decision 12 and its owner note |
| The vendor tool's own sandbox (optional extra layer) | What the vendor applies to the commands it runs | The vendor; Claude Code's per-agent switch, default on ([FC-189](../FEATURES.md)) |
| The office's permission policy and fixed denies | Tool calls the office does not allow | The office ([agent sessions](agent-sessions.md)) |

- **One profile mechanism per system, applied by the launcher** to the whole agent
  process: Seatbelt on macOS; on Linux the spike of FC-190 decides between Landlock,
  the mount-namespace root of ADR-0015 decision 12, and bubblewrap. A system without a
  working adapter refuses to start the agent (non-negotiable 13).
- **The standard profile:** the agent's private home, its task workspaces and temporary
  directories read and write; the parts of the Git cache a worktree commit writes
  (objects, task branches and their logs, the worktree's administrative directory,
  as the [Git design](projects-git-github.md) lists); its office's socket for
  `agent-os`; network to the provider API and to package registries; system tools and
  installed toolchains read-only; nothing else.
- **Extensions add allowances** to the profile, only for agents that have that
  extension enabled ([section 8](#8-extensions)). The iOS profile in ADR-0015's owner
  note becomes the standard profile plus the Xcode extension's allowances.
- In `off`, and for any `same-user` agent, the inner wall is all there is between an
  agent and the data, and the product says so (ADR-0016 decision 2).
- **Nested sandboxes on macOS (open risk).** A program already inside a Seatbelt
  profile generally cannot apply another one
  ([architecture overview](../architecture/architecture-overview.html)), and Claude
  Code's Bash sandbox, SwiftPM and `xcodebuild` apply their own. Whether the vendor
  sandbox can stay on inside the office's profile, and whether Swift builds work there,
  is checked by DEV-6 and DEV-7; if not, the fallback is the vendor sandbox off inside
  the office's profile, or the build tool's own sandbox disabled, recorded per kind of
  agent.

## 8. Extensions

### 8.1 What an extension is

An optional capability the product installs and manages for agents, such as
containers or Xcode. The core works without any extension. An extension is enabled
per instance and offered per agent; an agent that does not have it enabled gets none
of its allowances or environment. In v1 extensions are **built-in modules behind the
contract** (owner, 2026-11-20), compiled into the release and switched on or off.

**Timing.** Extensions are **not part of milestone one**. They come later in v1 (plan
row L17), after the accounts work they rely on (L16). Milestone one builds nothing for
them. It only avoids closing the door where that is cheap:

| Phase-one code | Keep open | Do not |
| --- | --- | --- |
| Composition roots (`agentic-org`, `agentic-org-office`) | Assemble capabilities from a list, so one more entry can be added later | Build an extension registry or the contract now |
| The agent launch | Build the environment and the sandbox profile as a base plus a list of additions | Hard-code the full profile or environment in one closed function |
| The web app's navigation and settings sections | Render the menu and the settings sections from data (an ordered list), as the shell spec already orders them | Add an extension slot, or placeholders, before the contract exists |
| Setup | Express its privileged work as a plan of step kinds | Add extension steps |

Nothing else in milestone one changes for extensions.

### 8.2 The extension contract

The core owns a small, consumer-owned contract
([architecture rules](../architecture-rules.md#planned-variation-goes-behind-an-adapter-from-day-one)).

| An extension may contribute | Notes |
| --- | --- |
| Install steps | Pinned downloads with SHA-256 checksums, into the product's own folder; the pins ship inside the signed release, so the release verifier covers them. Never from inside an agent turn ([standalone design 2](standalone-server.md#2-release-install-and-runtime-boundaries)) |
| Privileged steps | Only from setup's closed list ([section 5](#5-one-command-setup)) |
| A checklist | Plain-language items, each with how to check it and, where possible, a step setup can automate |
| A health check | Reports `ready`, `needs attention` (with the checklist item) or `not available on this system` |
| Sandbox allowances | Added to the os-sandbox profile for agents that have it enabled; expressed in the contract's own neutral vocabulary (paths read-only or read-write, devices, local sockets, macOS service names) and translated by each os-sandbox adapter; an allowance the adapter cannot express is refused |
| Agent environment and instructions | Variables (for example `DOCKER_HOST`) and a short instruction telling the agent the capability exists and how to use it |
| Lifecycle hooks | Start and stop per agent account (for example a VM), run through the launcher as that account |
| Menu items | Entries for the web app's navigation (main or settings), shown only to people the office authorizes ([section 8.4](#84-the-ui-slot-proposal)) |
| A settings section | Its own settings page with stored settings; the extension declares their shape and validation, and the office stores and validates them, never the browser alone |
| Panels or pages | Its own screens, for example the Containers list ([section 8.4](#84-the-ui-slot-proposal)) |
| Cleanup and removal | Undo its install and per-account state; never touch organization data |

| An extension is told | |
| --- | --- |
| The agent and its office | Identifiers only |
| The agent account | Name and UID |
| The folders | The agent's private home, its workspaces and the extension's own folder under the product folder |

### 8.3 Where the code lives

- **Top-level `extensions/` folder**, one crate per extension
  (`extensions/containers/`, `extensions/xcode/`). An extension depends on the
  contract; nothing else in the core.
- **The core never depends on an extension.** The composition roots (`agentic-org` for
  setup and the host, `agentic-org-office` for agents) wire the enabled ones.
  Removing an extension's folder and its line in the composition must not break the
  product. No source cycles.
- **Contract home:** a small crate (working name `org-extension`) holding only the
  contract types, consumed by setup, the office and the web API. This is a new
  exception to "the consumer owns its contract": ADR-0017 decision 6's exception covers
  operating-system capabilities only, and an extension contract is not one. The
  alternative is one small contract per consumer (setup steps, agent launch
  contributions, web descriptions), each owned by its consumer, which an extension
  implements. **Open:** the extensions ADR decides, with the owner.
- **Per-system adapters fail closed** ("not available on this system"). Today only
  `org-platform` and `org-platform-host` may hold `target_os` conditionals
  (ADR-0017 LNX-5). **Owner's answer (2026-11-20):** each extension has its own small
  `platform` module, the only place in it with platform conditionals, recorded as an
  exception to LNX-5 for extensions (the LNX-5 check admits `extensions/<name>/src/platform/`).
- **v1: built in, optional.** Extensions ship in the release and are enabled at setup
  or later. **Later:** separate out-of-process extension programs for third parties,
  over the same contract.
- **UI.** See the slot proposal in [section 8.4](#84-the-ui-slot-proposal).

### 8.4 The UI slot (proposal)

Proposed here and **not agreed**: the UI stream owns `apps/agentic-org-ui`. **Owner's
answer (2026-11-20):** ui-dev leads the slot's design with the backend stream when L17
starts; neither side builds it before then. The request to ui-dev is in
[the UI stream's file](../workstreams/ui.md).

- **What an extension can put in the web app.** Menu items (in the main navigation
  or as a section under Organization settings), a settings section, and its own panels
  or pages. Its status and checklist appear on its settings section.
- **How the web app discovers extensions.** The web app asks the office, through
  `agentic-org-web`, for the enabled extensions: for each, its identifier, name,
  menu items, settings description, status and checklist, filtered by what the signed-in
  person may see. The office answers from what the composition root wired; a browser
  never decides that an extension is enabled. The route is added to the
  [web API](web-api-v1.md) when built.
- **Where the UI code lives.** With the extension, in `extensions/<name>/ui/`, built
  into the web app at build time (a build-generated list of the extension UI modules
  present, so removing an extension's folder removes its screens). A menu item whose
  UI module is not in the build is not shown.
- **A declarative baseline.** The web app renders a settings form from the declared
  settings and a status card from the health check, so an extension needs UI code only
  for its own panels.
- **Licence rule.** Extension UI uses shadcn-svelte and Bits UI primitives, through the
  UI-foundation package or the slot (see the next point). Licensed Tailwind Plus components never move into
  `extensions/` (AGENTS.md; non-negotiable 16); if an extension screen needs one, that
  screen stays inside `apps/agentic-org-ui`.
- **Dependency direction, without a cycle.** If the web app builds in extension UI
  modules and those import the app's primitives, the two packages depend on each
  other. Two ways out, for the agreement with ui-dev: (1) move the shared,
  non-licensed primitives into a separate UI-foundation package that both the app and
  extension UI depend on (Tailwind Plus components stay in the app); or (2) invert the
  direction: extension UI modules receive what they render with through props,
  snippets or callbacks from the slot, and import nothing from the app.
- **Settings storage.** The office stores an extension's settings as office data and
  validates every change. The record's place in the data contract is decided with the
  extensions ADR.

### 8.5 The Containers extension

Agents already know Docker. The extension gives each agent account its own Docker,
with no new `agent-os` container commands (owner: do not reinvent Docker).

| | macOS | Linux with KVM | Linux without KVM (fallback) |
| --- | --- | --- | --- |
| Engine | Colima + Lima + docker CLI, one VM per agent account | Same, VM on QEMU with KVM | Rootless Podman or rootless Docker per agent account |
| Virtualization | Apple Virtualization.framework (`vz`); no administrator rights (to verify) | `/dev/kvm` through the `kvm` group, granted at setup | None |
| One-time privileged step | None expected | Add the agent account to `kvm` | `subuid`/`subgid` ranges for the agent account |
| Never | The `docker` group, which is root-equivalent; Docker Desktop | | |
| Agent sees | `DOCKER_HOST` pointing at its own VM's socket in its private home; plain `docker` | Same | Its rootless socket |

- **Where the fallback applies:** cloud VMs without nested virtualization, and an
  instance hosted in a container (ADR-0017). In a container, rootless containers need
  user namespaces that Docker's default seccomp profile and Ubuntu's AppArmor
  restriction refuse; the repository's optional profiles allow only a sandbox that does
  not mount a fresh `/proc` ([Linux host design 3.5](linux-host.md#35-agent-isolation)),
  and rootless Podman or Docker does mount one, so in a container the fallback is
  unverified and may be unavailable (CON-6).
- **Rootless fallback inside the Linux inner wall (open risk).** Landlock and seccomp
  confinement need no-new-privileges, which also disables the set-user-ID
  `newuidmap` and `newgidmap` that rootless Podman and Docker use for `subuid`
  mapping. The fallback may therefore not work inside the os-sandbox; CON-6 checks it,
  and if it fails the extension's fallback is unavailable on that system and says so.
- **Isolation:** each agent account's VM sees only that account's private home and
  workspaces as mounts (Colima's default mounts must be replaced; to verify); one
  account's containers cannot see another's.
- **Resources:** each VM reserves CPU, memory and disk. The extension sets limits per
  VM, starts the VM with the agent and stops it when the agent has been idle; the
  spike measures the cost.
- **The panel** lists an agent account's containers and lets a person stop and clean
  them.
- **Licences:** Colima MIT, Lima Apache-2.0, docker CLI Apache-2.0; no Docker Desktop.

### 8.6 The Xcode extension

macOS only; on every other system its health check says "not available on this
system".

- **Checklist:** Xcode installed; licence accepted; the agent accounts in the
  `_developer` group; developer tools security enabled (`DevToolsSecurity`); a signing
  keychain for the agent account ([section 6.2](#62-where-each-secret-lives-proposed)).
- **Sandbox allowances:** Xcode read-only, the account's DerivedData, CoreSimulator and
  the simulator services, signing (the agent's keychain).
- **Open risk:** simulators for an account with no graphical login session. Fallbacks:
  an auto-logged-in agent account on that Mac (as CI machines do), or iOS agents in
  an office on an instance set to `off`, or its own Mac, or the recorded exception of question 11 ([section 3.4](#34-isolated-and-not-isolated-how-few-switches)).
- **Proposed low-friction signing (not decided).** The Xcode extension stores an App
  Store Connect API key for the agent, and the agent builds with
  `xcodebuild -allowProvisioningUpdates` and that key: no Apple ID sign-in and no
  certificate export. The spike (XC-4) checks whether a keychain is still needed for
  the development certificate Xcode creates.
- **Signing identities are secrets an agent holds.** Non-negotiable 10 says an agent
  never holds the organization's secrets; an iOS agent that signs builds holds its
  signing identity in its keychain. This is named here as an exception for the owner to
  accept (question 12), limited to the agent accounts that have the extension.

## 9. Architecture changes

| Area | Change | Owning document |
| --- | --- | --- |
| Setup (A1/A2) | One administrator prompt running a fixed privileged plan; the explicit `--isolation off` choice (asked, recorded only on yes) | [Standalone design 4](standalone-server.md#4-setup-and-service-lifecycle), installation UX |
| Service registration | A system-level variant of each `org_platform::service` adapter: launchd daemon with `UserName`, systemd system unit with `User=`, and on Linux the office template unit `agentic-org-office@.service` with its polkit rule (D15); the per-user adapters stay as the `off` adapters | [Linux host design 3.4](linux-host.md#34-service-lifecycle) |
| Process launch | `org-supervise` gains "run as account": the isolation mode and target account in the spawn, carried out through the launcher; stop and cleanup through it too | ADR-0015 decision 4, [Linux host design 3.5](linux-host.md#35-agent-isolation) |
| Local socket admission | Explicit allowed UIDs: the person's UID on `host.sock` for the command line; the agent accounts on the office socket for `agent-os`; socket and directory permissions to match. Admission is never authorization | ADR-0015 decision 5, ADR-0017 decision 11, [host boundary 3](office-host-boundary.md#3-what-the-host-does) |
| Data root | A system location owned by the service account, outside the person's home | [Standalone design 3](standalone-server.md#3-instance-selection-and-storage) |
| Web confinement | ADR-0008's Seatbelt profile and the Linux filter, rechecked under the service account and a system daemon | ADR-0008, [Linux host design 3.3](linux-host.md#33-confinement-of-the-web-process) |
| Recovery presence and authority | ADR-0011's macOS check requires the peer to share the host's UID and console session; on Linux the recovery peer is the service user's kernel UID (ADR-0011, ADR-0017 decision 11). Under a service-account host the person's command line is neither: on macOS the host is not in the person's login session and cannot run the LocalAuthentication confirmation; on Linux the person is not the service UID and would need `sudo`. Needs a new design, on desktop Macs too (open, question 3) | ADR-0011, ADR-0017 |
| Extensions | The contract crate, the top-level `extensions/` folder, composition-root wiring, the LNX-5 exception for each extension's `platform` module | [Project structure](project-structure.md) |
| Secrets custody | Host secrets as owner-only files on macOS too; the Xcode keychain | ADR-0016, [host boundary](office-host-boundary.md#the-office-secrets-key) |
| Isolation setting | One instance-level setting (`off` or `accounts`) chosen at setup; the composition roots select one adapter per seam; conformance suites for both | This note, ADR-0015, the accounts ADR |
| Administrator rights after setup | The closed-list helper installed by setup (recommended), or pre-provisioning (alternative) | This note, the accounts ADR |
| Web app | The extension UI slot: discovery through the web API, menu items, settings sections, panels, UI modules built in from `extensions/<name>/ui/` | [Web API](web-api-v1.md), [shell spec](../specs/app-shell-ux.md) (UI stream), extensions ADR |
| Extension settings | Stored and validated by the office | [Data contract](organization-data/README.md), extensions ADR |

All of these keep the architecture rules: contracts owned by their consumers (or
shared by the platform exception), planned variation behind adapters that fail
closed, no source cycles, and logic in libraries with thin binaries.

## 10. ADRs and designs to write or update after the spikes

Nothing below is changed by this note. Each is written once the spikes of
[section 11](#11-spikes) have results.

| Document | New or update | What |
| --- | --- | --- |
| "The instance runs under dedicated non-administrator accounts" | New ADR | The service account, agent accounts per office then per agent, the data root outside the person's home, the launcher grant chosen by the spike, one-prompt setup, and the isolation setting with its `off` choice |
| "Extensions" | New ADR | The contract, the `extensions/` folder, built-in-but-optional in v1 and out-of-process later, the contract crate's placement, the LNX-5 answer, the UI slot agreed with the UI stream |
| [ADR-0015](../adr/0015-agent-isolation-modes.md) | Update | Decision 1: the account and the os-sandbox become two layers that combine, where decision 1 carries one isolation mode per launch; the instance-level isolation setting and how it maps onto `same-user` and `dedicated-user`; decision 3's "setup continues in `same-user`" and criterion ISO-3 become an explicit choice; decisions 2 to 5: agent accounts per office (not per instance), per-agent accounts as the later step, the launcher chosen by the spike, the socket allowing agent accounts; decision 6 (the owner may set `same-user` per agent): this note proposes forbidding it under `accounts` (question 11); decision 7 (`same-user` stays the default until the spike passes): changed by the owner's direction of 2026-11-20 that `accounts` is the default for an install, pending the spike, and its spike folded into the accounts spike; the `os-sandbox` mode as the inner wall |
| [ADR-0005](../adr/0005-standalone-server-v1.md) | Update | Decisions 2, 5 and 8: setup's one administrator prompt, the system-level service, the data root's location; decision 7: the launcher as a fifth executable, and a root daemon only if launcher option 3 is chosen |
| [Architecture non-negotiables](../architecture-non-negotiables.md) | Update | Rule 7's four programs: name the launcher (accepted by ADR-0015 decision 4) and the closed-list helper, and a root launcher daemon if launcher option 3 is chosen |
| [ADR-0022](../adr/0022-host-creates-its-registry-no-reverse-bootstrap.md) | Update | The initial registration link asked for across accounts: setup as the person, the host as the service account |
| [Git design](projects-git-github.md) | Update | Its `dedicated-user` rules restated for a service account and per-office agent accounts (GIT-1, GIT-2) |
| [ADR-0017](../adr/0017-linux-and-container-hosting.md) | Update | Decision 2's service lifecycle and credential provider rows (system units; files on macOS), decision 8's location, decision 10's "setup refuses root" (the privileged step), decision 11's admission by explicit allowed UIDs |
| [ADR-0008](../adr/0008-gateway-macos-sandbox.md) | Update | The web process confined under the service account as a system daemon; recheck its profile and the host's post-accept checks there |
| [ADR-0016](../adr/0016-no-application-level-data-encryption.md) | Update | A secrets custody note: owner-only files for host secrets on macOS too; the Xcode keychain password as a wrapped office secret |
| [ADR-0011](../adr/0011-protected-recovery-delivery.md) | Update (found while writing this note) | The macOS presence check assumes the peer shares the host's UID and console session, and the Linux recovery peer is the service user's kernel UID; a host under a service account needs a new presence and authority design on both systems, desktop Macs included |
| [Linux host design](linux-host.md) | Update | System-level service adapters; the `agentic-org-office@.service` template unit and its polkit rule, with admission for an office systemd starts; `dedicated-user` and accounts in section 3.5; section 5's attacker model with a service account |
| [Host boundary design](office-host-boundary.md) | Update | Host secrets custody; `host.sock` admission of the person's UID |
| [Project structure](project-structure.md) | Update | The `extensions/` folder, the contract crate, composition wiring |
| [Standalone design](standalone-server.md) | Update | Section 3's data roots and service table; section 4's setup flow and the rule of no administrator prompt for the core's everyday use |
| [Web API](web-api-v1.md), [shell spec](../specs/app-shell-ux.md) | Update (with the UI stream) | The extensions discovery route; the menu and settings slot |
| [Data contract](organization-data/README.md) | Update | Where an extension's settings are stored |
| [Installation UX](../specs/server-installation-and-access-ux.md) | Update (with the UI stream, which owns `docs/specs/`) | The per-user service as the default; item 4's "without administrator rights it continues in `same-user`"; the Linux `admin reset` run with `sudo` as the instance's user, which becomes the service account |

## 11. Spikes

Late v1, before the ADRs. Each runs on macOS and on Linux unless it says otherwise,
on a disposable machine or VM, never against a real organization's data
(non-negotiable 17). Results are recorded in `docs/research/` when the spikes run,
with tool and system versions, as the [C1 spike](../research/c1-sign-in-spike.md) was;
no result files exist yet.

### 11.1 Accounts and launcher

Proves: ASX-4, ASX-5, ASX-8, ASX-9.

Steps: run setup with one administrator prompt; reboot; do not log in; start an agent
through the chosen grant; send the launcher bad requests.

| ID | PASS | FAIL |
| --- | --- | --- |
| ACCT-1 | Setup asks for administrator rights exactly once and creates the service account, one agent account, bound to the first office when it is created (or the pool, under alternative (b)), the data root, the secrets directory, the launcher, the helper (under (a)), their grants and the system service | A second prompt; any of those items left to the checklist; or a half-done state after a cancelled prompt |
| ACCT-2 | After a reboot with nobody logged in, the host, the web process and an office run as the service account and the browser reaches the instance | Any of them needs a login, or runs as another account |
| ACCT-3 | The office starts an agent as its agent account through the launcher; `ps` shows that account; stop, cleanup after a crash and orphan recovery work through the launcher with the ownership mark | The agent runs as the service account, or stop or cleanup fails |
| ACCT-4 | The launcher refuses an unregistered account, an unregistered program, a working directory outside the account's roots and extra arguments, and logs each | Any of them starts a process |
| ACCT-5 | No process of ours runs as root except setup's privileged step, the closed-list helper for one listed operation, and, in launcher option 3 only, the root launcher daemon; no root process sits between the office and the agent; `sudo -l` for the service account lists only the launcher and the helper | Any other root process (including a `sudo` monitor kept for the agent's run) or a broader grant |
| ACCT-6 | On Linux, a new office starts as `agentic-org-office@<id>` with no new file and no prompt; the polkit rule lets the service account start and stop only `agentic-org-office@*` (another unit is refused); the host and web keep no-new-privileges; the office's `sudo` to the launcher works; the host still verifies the office it hands secrets to | The option silently needs a weaker unit than recorded |
| ACCT-7 | In a container built from the repository's image, the accounts and grant made at build time work without an administrator step, including under the documented hardened run (`--security-opt no-new-privileges`, all capabilities dropped but three; `deploy/container/README.md`), or the spike records that `accounts` in a container needs a less hardened run | Any step needs root at run time, or the grant does not work |
| ACCT-8 | Option 1: the process tree is office → launcher-run agent with no root process between them; the office's ownership mark, pidfd (Linux) and start time identify the agent; stop and kill reach it | A root process between them, or evidence or signals that do not reach the agent |
| ACCT-9 | The launcher reads its length-prefixed request from standard input and then hands standard input to the vendor tool, whose headless session works end to end through `sudo` | The request cannot be framed this way, or the session breaks |
| PRIV-1 | A plan with a value outside the fixed prefixes or groups, a file whose digest is not the release's, a manifest forged or re-signed in the service account's store, or a privileged-step or helper executable changed by the service account is refused whole, with nothing applied | Any step applied |
| PRIV-2 | Adding an agent account changes only a group membership; `/etc/sudoers.d` is not rewritten after setup, and setup's sudoers file passes `visudo -cf` and appears atomically | The grant file is rewritten at run time, or a malformed file can be installed |

### 11.2 Security checks

Proves: ASX-6, ASX-7, ASX-10, ASX-12.

Run as the agent account, inside the standard os-sandbox profile.

SEC-1 to SEC-5 and SEC-7 pass when the attempt fails with a permission error from the
operating system; SEC-6 passes when no elevation is obtained (`sudo` and `su` refuse in
their own words); SEC-8 to SEC-10 pass when they succeed as described; SEC-11 passes
when the office refuses; SEC-12 passes when the read fails; SEC-13 passes when the reads fail and the admission is as stated.

| ID | Check |
| --- | --- |
| SEC-1 | Reading or listing the person's home |
| SEC-2 | Reading another agent account's home (another office's agent) |
| SEC-3 | Reading the data root outside the agent's own home, workspaces and the listed parts of the Git cache: the office store, other caches' configuration and the host secrets |
| SEC-4 | Reading another office's data on the same instance |
| SEC-5 | Signalling a process the agent account does not own (host, office, web, another account's agent) |
| SEC-6 | Gaining administrator rights: `sudo`, `su`, set-user-ID programs, the launcher used against another account |
| SEC-7 | Writing outside the private home and workspace under the os-sandbox profile |
| SEC-8 | Claude signs in with a token and no Keychain under the agent account with no login session; the run copy is taken back at the end |
| SEC-9 | `agent-os` from the agent account is admitted by the office socket and still needs its binding; a third account is refused |
| SEC-10 | Codex runs under the agent account with `CODEX_HOME` in the private home and the sign-in the office placed there, with no login session (ADR-0015 decision 7 and ISO-6 cover every supported runtime) |
| SEC-11 | The agent replaces its run copy of a sign-in with a symbolic link and, separately, a hard link to a host secret or another office file; at the end of the run the office refuses to take it back and never places that file in an agent's home |
| SEC-12 | Only if the owner accepts a `same-user` agent under `accounts`: inside its os-sandbox profile it cannot read the host secrets directory or any office's data outside its own home and workspaces |
| SEC-13 | A process of the person, and one of a second ordinary account, cannot read or list the data root or the host secrets; `host.sock` admits the person's command line and refuses that second account |

### 11.3 Frictionless

Proves: ASX-1, ASX-2, ASX-3, ASX-8.

| ID | PASS | FAIL |
| --- | --- | --- |
| FRI-1 | One command from a clean machine to the registration link; the time is recorded | More than one command, or an unrecorded time |
| FRI-2 | The checklist is plain: each item says what, why, the command and how to check it; a person who has not seen it before follows it unaided | An item missing any of the four parts, or a tester who needed help to finish |
| FRI-3 | Without administrator rights, interactive setup offers `--isolation off`, records it only on yes, says so in its output, and the product shows it; non-interactive setup asking for `accounts` fails before any change | A silent fallback to `off`, or a half-provisioned instance |
| FRI-4 | A development build's setup defaults to `--isolation off` and never asks for a password; the ordinary test suites run in that setting | Any prompt in development or ordinary CI |
| FRI-5 | The same conformance suites pass for both adapters of each seam of [section 3.4](#34-isolated-and-not-isolated-how-few-switches) on both systems (the `accounts` cells as root in Linux CI and on the macOS test machine), and no code outside the composition roots reads the setting (checked by a source search) | A suite that passes for one adapter only, or the setting read elsewhere |
| FRI-6 | An `accounts` instance with its launcher, helper or an agent account removed refuses to run the affected agents and reports why; it never switches to `off` | Any agent starts, or the setting changes by itself |

### 11.4 Developer workloads under accounts and the standard profile

Proves: ASX-11, ASX-13 (DEV-5).

Each passes when the build and its tests succeed as the agent account inside the
standard profile, with the toolchain installed system-wide and caches in the private
home; it fails if any needs a write outside home and workspace or a broader read.

| ID | Workload |
| --- | --- |
| DEV-1 | Rust: `cargo build`, `cargo test` |
| DEV-2 | .NET: `dotnet build`, `dotnet test` |
| DEV-3 | Node and Bun: install from a lockfile, test, build |
| DEV-4 | Python: a virtual environment, install, `pytest` |
| DEV-5 | Local Git as the agent account: commit, branch, worktree; the agent holds no credential that can push (non-negotiable 10) |
| DEV-6 | Claude Code with its own Bash sandbox on, inside the office's profile (nested sandbox on macOS; [section 7](#7-the-sandbox-inside-the-accounts)); the result and any fallback recorded |
| DEV-7 | Swift: `swift build` and `xcodebuild` with Swift packages inside the profile (macOS); the result and any fallback recorded |

### 11.4a Git across accounts

Proves: ASX-13.

The [Git design](projects-git-github.md) already defines the `dedicated-user` rules:
the office fetches and pushes from its own bare cache; the agent account writes only
objects, task branches, their logs and its worktree's administrative directory,
through a group shared with the service account; `safe.directory` names exact paths;
and the cache's canonical configuration is checked before every command.

| ID | PASS | FAIL |
| --- | --- | --- |
| GIT-1 | Those rules hold with the cache owned by the service account and a per-office agent account: an agent commits in its worktree, and the office fetches and pushes over SSH and HTTPS with its own credential, with `safe.directory` naming exact paths | The office cannot fetch or push, needs `safe.directory=*`, or the agent can write outside the listed parts of the cache |
| GIT-2 | Repository configuration or hooks written by the agent account never run as the service account: the canonical-configuration check refuses a changed cache configuration before any office command | Any agent-written configuration or hook runs in an office command |

### 11.5 Xcode extension (macOS)

Proves: ASX-11, ASX-18.

| ID | PASS | FAIL |
| --- | --- | --- |
| XC-1 | `xcodebuild` builds an iOS app as the agent account with nobody logged in | Needs a login session |
| XC-2 | A simulator boots and the app's tests run, with nobody logged in | Needs a graphical session; then XC-3 |
| XC-3 | The fallback (an auto-logged-in agent account) passes XC-1 and XC-2; the spike records its cost | The fallback also fails; then iOS agents run in an office on an instance set to `off`, or its own Mac, or the recorded exception of question 11 |
| XC-4 | Code signing with the agent account's own keychain, unlocked by the office at agent start, never with the password on a command line | A prompt, a login keychain, or the password visible in a process listing |

### 11.6 Containers extension

Proves: ASX-11, ASX-17.

| ID | PASS | FAIL |
| --- | --- | --- |
| CON-1 | macOS: Colima starts a VM as the agent account without administrator rights | Needs administrator rights |
| CON-2 | `docker build` and `docker run` work with the extension's `DOCKER_HOST`, from inside the agent's os-sandbox profile with the extension's allowances | Either fails, or works only with a broader allowance than the extension declares |
| CON-3 | Two agent accounts' containers cannot see each other, and neither VM mounts anything beyond its account's home and workspaces | Any shared container, image store or extra mount |
| CON-4 | Cleanup removes an account's containers, images and VM | Leftovers |
| CON-5 | Linux with KVM: CON-1 to CON-4, with the account in `kvm` and never in `docker` | Any of them fails, or needs the `docker` group |
| CON-6 | Linux without KVM: rootless Podman or Docker after the one-time `subuid`/`subgid` setup, inside the os-sandbox profile ([section 8.5](#85-the-containers-extension)), on a host and in an instance container | It needs no-new-privileges off, or a broader allowance; then the fallback is recorded as unavailable there |
| CON-7 | The memory, CPU and disk cost of one VM per account is recorded, idle and under a build | Not recorded |

### 11.7 Secrets

Proves: ASX-6, ASX-10.

| ID | PASS | FAIL |
| --- | --- | --- |
| SECR-1 | Host secrets as owner-only files under the service account: the host starts unattended after a reboot; nothing else can read them; a scan finds no secret elsewhere (ENC-2) | Any other account can read them, or the host needs a login |
| SECR-2 | Later, for releases only: the Keychain with a Developer-ID-signed host (or a headless keychain) is evaluated against owner-only files and recorded with a recommendation | No recommendation when the release option is taken up |
| SECR-3 | Each option of [section 6.4](#64-options-for-the-spike-to-evaluate) is tried where the system has it (Secure Enclave from a daemon under the service account; TPM-sealed `systemd-creds`; required disk encryption is not tried, D21) and recorded with a recommendation | An option skipped without a recorded reason |
| SECR-4 | A copy of the data root alone, taken to another machine, yields no usable private key or provider credential (ENC-5) | Any usable key or credential |

### 11.7a The host with the person on a desktop Mac

**Not planned:** the option of [section 6.5](#65-option-the-host-stays-with-the-person-on-a-desktop-mac) was not adopted (owner, 2026-11-20). The checks are kept as the record of what it would have needed.

| ID | PASS | FAIL |
| --- | --- | --- |
| KEY-1 | The host runs as the person from the per-user launchd agent and keeps the office secrets keys in its Keychain access group with no unlock step | Any prompt or unlock step |
| KEY-2 | The host starts an office and the web process as the service account through the grant; `ps` shows the service account | They run as the person, or the grant allows more |
| KEY-3 | Admission holds without a suspended spawn: the host checks the connected peer's code identity (audit token) at the rendezvous, and refuses a wrong image or another account | A wrong image or account is admitted |
| KEY-4 | The handoff line and the transfer descriptors, including the writer-control connection, reach the office across accounts; the office opens and confirms its writer head | Any part fails |
| KEY-5 | ADR-0011's recovery release works with the person at the console | Presence fails |
| KEY-6 | Stop, the final kill and orphan cleanup reach the service account's processes through the launcher; after logout the instance stops and says so at the next login | A process is left, or the stop hangs |
| KEY-7 | A process of the person that is not the host runs the launcher as the service account: it cannot get an office to start or to serve data | It can |
| KEY-8 | The catalog, `office.json` (owned by the service account), the move to `.refused/<id>`, the office logs and the orphan-key sweep all work across the two accounts | Any of them fails |
| KEY-9 | A program of the person that is not the host connects to the service-account relay and asks for a helper operation: it is refused, and no helper operation runs | A helper operation runs |

### 11.8 Administrator rights after setup

Proves: ASX-8, ASX-9.

| ID | PASS | FAIL |
| --- | --- | --- |
| NOADM-1 | After setup, with no cached or elevated administrator session (`sudo -k` first): create an organization, add a second office on the instance, hire agents, start, stop and clean up agents. No administrator prompt appears, and the system's `sudo` and authorization logs show no elevation except the launcher's grant and listed helper operations | Any prompt, or any other elevation |
| NOADM-2 | With launcher option 1 or 2: no process of ours runs as root after setup except the helper, for one listed operation at a time, and it exits | Any long-lived root process of ours |
| NOADM-3 | `service stop`, `service start` (including after a stop), `service status`, `logs` and `doctor` (also with the host down), `upgrade` (without rewriting the unit or plist) and a launcher update work without a prompt ([section 5.2](#52-administrator-rights-after-setup)); `uninstall` asks exactly once | A prompt for any of the first seven, other than one the spike records as an exception and which then asks exactly once; or more than one prompt for `uninstall` |
| NOADM-4 | With the helper: adding an office creates its agent account, and enabling an extension installs its system part, with no prompt | Any prompt |
| PRE-1 | Alternative (b) only: the pool's unused accounts have no password, no login shell, no group beyond those setup recorded, and cannot be logged into | Any of these |
| PRE-2 | Alternative (b) only: with the pool used up, adding an office is refused with a plain message naming `agentic-org setup --grow-pool`, which asks exactly once; enabling an extension setup did not prepare needs exactly one prompt through `agentic-org extension prepare`, in a terminal, never from the browser | A silent failure, a shared account, more than one prompt, or a browser-triggered elevation |
| HLP-1 | The helper refuses every operation outside its list, including adding an account to `admin`, `wheel`, `sudo` or `docker`, creating an account without our prefix, removing an account it did not create, installing an unpinned package or one with a wrong checksum, and installing from a manifest that does not verify against the root-owned publisher key | Any of these succeeds |
| HLP-2 | Only the service account can invoke the helper, and the command-line group only its "start this instance's service" operation; the agent accounts cannot invoke it at all. | Another account can, or the group can call another operation |
| HLP-3 | A simulated compromise of the service account cannot obtain a root shell or write an arbitrary root-owned file through the helper | It can |
| HLP-4 | The helper is not running between requests; each request and its result are logged | A resident helper, or an unlogged operation |

## 12. Open questions for the owner

Answered on 2026-11-20 unless marked **open**.

1. **Launcher grant.** Answered (D15): option 1 (sudoers to the launcher); on Linux each
   office runs in the template unit `agentic-org-office@.service` with a polkit rule,
   and the host unit keeps `NoNewPrivileges=yes`
   ([section 4.1](#41-options)).
2. **Host secrets.** Answered (D16): owner-only files for now, on macOS too; the
   Keychain with a Developer-ID-signed host later for releases; per-office service
   accounts a later option.
3. **Recovery presence and authority. Open.** Under a service-account host, ADR-0011's
   macOS check fails (the host is not in the person's login session and cannot run the
   LocalAuthentication confirmation) and so does its Linux rule (the person is not the
   service user and would need `sudo`; ADR-0017 decision 11). With
   [section 6.5](#65-option-the-host-stays-with-the-person-on-a-desktop-mac) not adopted, this holds on desktop Macs too. A new design is needed
   before an `accounts` install ships (L16).
4. **Extensions and LNX-5.** Answered (D17): a small `platform` module in each extension.
5. **UI slot.** Answered (D18): ui-dev leads it with the backend stream when L17 starts.
6. **Agent accounts per office.** Answered (D19): per office, separate from the service
   account.
7. **Administrator rights after setup.** Answered (D20): the closed-list helper, pending
   the spike.
8. **Disk encryption.** Answered (D21): check and warn; not required.
9. **Git across accounts.** Answered (D22): the `dedicated-user` rules carry over.
10. **Exceptions to "no prompt after setup".** Answered (D23): `uninstall` and changing
    the isolation setting each ask once.
11. **A `same-user` agent under `accounts`. Open.** Recommendation: forbid it; an iOS
    agent uses the Xcode extension under its agent account, or an office on an instance
    set to `off`. (D19's principle, that agents never run as the account holding office
    data and keys, points the same way.)
12. **Signing identities for iOS agents. Open.** Proposal: the Xcode extension stores an
    App Store Connect API key for the agent and builds with `xcodebuild
    -allowProvisioningUpdates` and that key, with no Apple ID sign-in and no certificate
    export ([section 8.6](#86-the-xcode-extension)). It still needs the decision on an
    exception to non-negotiable 10.
13. **The host with the person on a desktop Mac.** Answered (D14): not adopted.

## What the code does today

Nothing in this note is built. The service, socket, launch and secrets code that
exists is described in [section 2](#2-what-runs-today) with its sources. There is no
`extensions/` folder, no contract crate, no launcher and no system-level service
adapter.
