---
kind: spec
title: "Standalone server: installation, instances and browser access (UX)"
status: current
owner: lead
created: "2026-11-09"
updated: "2026-11-17"
requested_by: owner
builds_on: [offices-and-teams-ux, agent-run-modes-ux, employee-supervision-ux, processes-ux, chat-ux]
---

# Spec · Standalone server: installation, instances and browser access

## 0. Scope and decisions

V1 is a standalone Rust service on macOS and Linux, including inside a container
([ADR-0017](../adr/0017-linux-and-container-hosting.md)). HQ serves the static
SvelteKit application and its HTTP API. Satellites run their own offices and agents
and connect outward to HQ. Opening the website never installs or enrolls an office.
There is no desktop application. Installers and tests never erase existing user
directories.

The design is [standalone-server.md](../design/standalone-server.md) and the decision
is [ADR-0005](../adr/0005-standalone-server-v1.md).

The executables are `agentic-org` (the person's CLI and the service host),
`agentic-org-web` (the browser server), `agentic-org-office` (one office worker)
and `agent-os` (the agent CLI); `agentic-org-ui` holds the static browser assets.
[Executable migration](../design/executable-migration.md) records which commands
exist today.

## 1. Install once; select an instance

A small bootstrap command selects the platform release, downloads and verifies
one release executable, then hands off to that executable's setup command.
Setup obtains/verifies the matching worker, web assets and agent helper through downloads or a complete local bundle. Embedded payloads may be unpacked through the same verifier. Users do not build the application or
install a JavaScript server to run it.

| Invocation | New instance | Existing instance |
| --- | --- | --- |
| `agentic-org setup` | Create HQ on `default`, register/start its service | Reuse `default`; start if stopped; otherwise show status and URL |
| `agentic-org setup --name lab` | Create HQ on `lab`, register/start its service | Reuse `lab` with the same identity, configuration and data |
| Either, with `--non-interactive --config <file>` | Validate explicit inputs and perform the same setup | Reconcile the same inputs; incompatible changes fail with an actionable error |

The bootstrap forwards the instance selector and automation options. Running it
twice has the same instance semantics as running setup twice. It does not silently
upgrade, restart a running service, rotate credentials, change office roles or
enroll another satellite. Upgrades are explicit. Names are local selectors, not
office identities. A name resolves to an immutable instance ID and saved paths;
new IDs are generated once and persisted before provisioning.

Setup checks name validity, existing state, conflicting ports and data ownership.
Names cannot contain path separators or traversal. A second process using the
same data directory is refused before storage opens. An installation interrupted
halfway resumes its recorded operation; ambiguous state is reported for repair,
never silently replaced. Setup uses a lock too, so concurrent installations
cannot create competing identities.

## 2. Wizard and automation

For a new instance the wizard establishes, in order:

1. Instance name and data location, using platform defaults unless explicitly set.
2. **Create an organization and its HQ** as the first office. Collect the name,
   explicit catalog-unique kebab-case slug, remote organization repository and
   `state_branch`. Refuse a local-only organization repository. Show which branch
   will hold organization data. Validate URL/branch syntax and local credential
   references/configuration without logging secrets. In offline mode perform no
   fetch, remote-auth probe or other external request; keep remote connectivity
   and publication explicitly **Unverified — offline** or **Pending** until online
   operation is authorized. The mandatory registration remains remote. Select/create
   the valid home team required for each bootstrap employee, including HR and any optional owner-person profile. The initial team's
   name/lead is not inferred. No create-or-join selector, invitation input, satellite
   role or --hq join option is present; later HQ-issued enrollment is separate.
3. HQ instance-admin and owner access, web listening and TLS configuration, and
   protected CA escrow and recovery-code provisioning (setup offers the code once
   HQ is created, before step 6 starts the service; collecting it and acknowledging
   its off-HQ retention can be deferred: §4a.2).
4. Agent isolation ([ADR-0015](../adr/0015-agent-isolation-modes.md)): setup offers
   the isolation mode and, for `dedicated-user`, performs the one administrator step
   that creates the agent account and installs its launcher; without administrator
   rights it continues in `same-user` and says so. Setup also reports whether
   operating-system disk encryption is on and which file permissions it set
   ([ADR-0016](../adr/0016-no-application-level-data-encryption.md)).
5. Execution readiness: choose the agent's model profile and canonical model;
   reuse an organization provider connection and save its office-local credential
   binding. Runnable agents need fresh authentication/model evidence and a permit.
   Offline setup checks local configuration/prerequisites only, performs no provider
   auth/model probe and leaves that readiness unverified. Profile connection order
   is separate from employee identity. Dependency/auth/model/permit failures keep
   agents in a specific **Needs you** state while HQ can be ready.
6. Automatic service registration/start and an HQ URL/readiness summary. No separate install or service install command is needed. Missing service-manager support reports foreground agentic-org serve instructions without discarding completed setup.

Automation uses the same validation and state transitions, with one stated
exception: noninteractive setup, and setup over SSH on a Mac,
finish with the HQ created and its recovery code waiting, because showing the
code needs someone at the HQ machine (§4a.2, §4a.6). Automation receives no code or protected code output, and the administrator collects it later at the HQ machine. Missing mandatory
input causes a structured failure instead of waiting for stdin. Secrets enter
through a protected descriptor/file or a dedicated stdin option, not arguments
or ordinary configuration fields. Machine-readable output contains stable error
codes and status fields, never credentials. Initial automation accepts HQ configuration only; satellite/enrollment input is rejected before provisioning with instructions to use HQ's Add office flow. The separate HQ-issued enrollment flow can report awaiting_approval under the existing approval policy.

The default service runs as its configured user, not root. macOS uses a per-user
launchd service; Linux uses an appropriate systemd user service, with foreground
execution available when a user service manager is absent. Logout/reboot behavior
and credential availability are reported honestly. Running after logout on Linux
may require explicitly configured lingering; setup does not silently enable it.
Before-login macOS execution is outside this v1 default.

## 3. Files, identity and tools

The instance resolves the configured organization data root, with protected sources under `durable/` and caches under `runtime/<office-id>/`;
executables live separately. Organization Git is remote and its latest content is
mirrored as ordinary durable files. `organization.yaml` stores stable ID, slug,
name, repository ID and state branch. Team/employee/project/task paths use stable
IDs. IDs use UUIDv7.
Readable task references are office-minted T- for every type, never implicit B-/REQ-
renames. [The storage tree](../design/standalone-server.md#3-instance-selection-and-storage)
defines durable state, authority, journal, usage archive, bindings and checkpoints
separately from runtime office projections, one cache per office/repository, and
employee/task worktrees. Project integration branches belong to each association.

Every employee has one home team. Optional role/reporting and a direct-member lead
are separate; a lead manages the subtree. HR bootstrap cannot be teamless. An owner
account's security rights remain host-owned even if it has no employee profile;
an optional representing person needs the same valid team as any other employee.
An agent's profile.md is identity, execution.md is runnable configuration, and
current office placement is a projection backed by durable generation/fencing.
Home-team changes, office movement and task assignment are distinct operations.

Authorized users may inspect actual server paths read-only. No browser folder
picker, relocation, scanner or automatic upload is provided. The target task
workspace hierarchy is scoped by office, employee/team, optional project, task and
repository IDs; it survives handoff until closure. Managed-state, Git-metadata,
credential and other-task denies are derived for these paths and verified; a renamed
prefix is not a verified sandbox.

Protected chat/attachments, usage archives, budget policy/history/extensions/holds,
command receipts, authority and execution/process evidence must survive office
projection loss. Private notes use a separate durable author-only DB, with no
administrator exception. Git alone does not restore these sources. Bindings contain
private local references, never credential bytes in Git. Rebuild verifies trusted
identity/epochs/digests and retained high-waters, restores legitimate credentials,
and reconciles owned processes before launch. A copied/restored PID or missing
cache is not cleanup proof. Missing evidence must report recovery required.

Setup installs `agent-os` from the same release. It locates or installs only the
selected supported harnesses, checks versions, connection account identity and
model entitlement when online operation is authorized, and reports readiness
without putting secrets in tool environments. Offline setup uses only local version/
configuration checks; no external sign-in, model discovery, repository request or
background update runs. HQ readiness does not assert remote repository publication
or agent-provider readiness. Git is a prerequisite for coding workflows. Server readiness and runtime
readiness are distinct: the owner can use HQ to finish setup while agents remain
in **Needs you** with a specific reason.

## 4. HQ browser access and satellites

The browser authenticates to HQ and receives only authorized organization data.
No signed desktop identity or office certificate substitutes for a human session.
First setup must establish owner access before administration is available.
Remote browser access uses HTTPS; local access binds to loopback by default.
Satellite enrollment/replication uses its separately authenticated connection.

HQ displays its durable copies of organization, task and message data. Runtime
summaries include last-seen information. Live session inspection and control are
relayed to the owning office and depend on its availability. The UI never reports
an offline office's control action as completed. Durable operations distinguish
**Queued**, **Accepted** and **Completed** where those states apply; rejected and
unknown outcomes remain visible. A retry of the same operation cannot duplicate it.

On browser reconnect, reload a consistent snapshot or resume the event cursor.
During the gap use LIVE-8 exactly: `Live updates paused. Showing what was loaded at <HH:MM>.`, or `Live updates paused.` if nothing has loaded, with Retry / Retrying…. A stale cursor triggers an explicit refresh.
Only a durable server response warrants **Saved**. Unsaved browser text may remain
in the open tab, with an honest unsaved label; v1 provides no background browser
outbox or guarantee that closing the tab preserves drafts.

Satellite onboarding starts from HQ's authenticated UI or an HQ-issued command, separately from initial setup. It reuses the invitation, HQ identity verification, owner
approval and certificate model in the offices design. Restart uses the enrolled
identity and resumes streams. A satellite opens no public application listener.
The local agent CLI and local administration remain available on its machine.
HQ approvals work from any authorized browser, regardless of which machine runs
that browser. Neither remote browser access nor a same-user local connection
creates office trust automatically.

If HQ is unavailable, its web application is unavailable. Satellite-local work
continues within existing cached policy and runtime limits; organization commands
and cross-office delivery retain the existing outbox rules. Last-known copies are
not backups. Automatic HQ promotion and a satellite-hosted offline web UI are
outside this pivot.

§4a defines the screens for signing in, the session, first owner access and the
recovery code.

## 4a. Sign in, first access and recovery code

Accounts sign in; people and agents don't. The recovery code is delivered only at
the HQ machine ([ADR-0011](../adr/0011-protected-recovery-delivery.md)).
Placeholders in angle brackets, such as `<HQ machine>`, are filled from HQ's data.
Open questions are in §4a.9.

**Naming the instance in CLI copy.** Every command shown to the user, in the CLI
or the browser, includes `--name <instance>` whenever the instance isn't
`default`, and leaves it out for `default`. `--name` always comes after the
command it selects for, as in §1 and §5: `agentic-org setup --name lab`,
`agentic-org account create --name lab`,
`agentic-org recovery release --name lab --org 'Lumen Labs'`. The examples
below use `default`. The sign-in page can't know the instance's name, so it always
shows `--name <instance>` as text to replace (§4a.3).

### 4a.1 Accounts: who can sign in

- An **account** belongs to this instance, not to an organization. It has a
  username and a password. It is an **instance admin** or not, and it is linked
  to the organizations it owns.
- `agentic-org setup` creates the first account. That account is an instance
  admin and owns the first organization.
- Whoever creates an organization owns it ([offices §5a](offices-and-teams-ux.md#5a-create-organization-instance-admin-only)).
- **Only accounts sign in.** People in an organization's People view
  ([offices §7.4](offices-and-teams-ux.md#74-add-edit-and-remove-a-person)) are
  records that agents can reach. They never sign in, and nothing invites them.
  Agents never sign in to the browser either.
- In v1.0 another account is added only on the HQ machine, by an instance
  admin, with `agentic-org account create`. It can make the new
  account an instance admin, and it links the account to the organizations it
  owns. No browser screen lists, adds or edits accounts. If a later screen lists
  accounts, it shows that command and has no form.
- Later, on the HQ machine, an instance admin links an account to an
  organization (`agentic-org account link`), unlinks it (`account unlink`), or
  revokes the account (`account revoke`), which signs it out everywhere and
  disables it (§4a.2) (2026-11-10). These, and `account
  create`, can name any organization on this instance, not only the admin's own.
- Two rules always hold, and a command that would break one is refused: the
  instance keeps at least one instance admin, and every organization keeps at
  least one linked account.
- Passwords are changed only on the HQ machine:
  - `agentic-org account set-password` changes your own password. It asks for
    the current one.
  - `agentic-org account reset-password <username>` is the way back from a
    forgotten password. It needs no sign-in. On a Mac it needs someone at the
    HQ machine to confirm it's them (Touch ID or the Mac's login password); on
    Linux it needs root (`sudo agentic-org account reset-password <username>`).
  - Either one ends every session of that account, in every browser.

  The browser has no "forgot password" flow and no form to change a password.
- The password is checked with a maintained library (O-1). The page never
  says whether the username or the password was wrong.

### 4a.2 First owner access

Setup validates organization Git/slug/branch and required bootstrap home teams in
step 2, then creates the account in step 3 (§2). This proposed transcript shows
only the lines that matter here; `…` stands for the other steps.

```
$ agentic-org setup
Setting up instance "default".
…
Create your organization. This machine becomes its HQ.
Organization name: Lumen Labs
Organization slug: lumen-labs
Remote organization repository: https://git.example.invalid/lumen-labs/organization.git
State branch: organization-state
Home team for bootstrap employees: <explicitly chosen valid team>
…

Your account
You'll sign in to HQ in a browser with this account. It will own
Lumen Labs and be this instance's admin, so it can also create
other organizations.
Username: alex
Password (at least 12 characters, not shown as you type):
Password again:
Account "alex" created. Instance admin: yes. Owns: Lumen Labs.
…
Lumen Labs created. This machine is its HQ.

Recovery code for Lumen Labs
It's shown only here on the HQ machine, never in the browser.
Collect it now? [Y/n]: n
Not collected. It's waiting on this machine. Collect it later with:
  agentic-org recovery release --org 'Lumen Labs'
Recovering an organization after losing the HQ machine isn't supported yet.
…
Service running.
HQ ready.

Open http://127.0.0.1:<port> in a browser and sign in as alex.
```

- **Username:** required, trimmed. Empty: "Enter a username." Other rules are
  A81; until then, setup refuses only an empty username.
- **Password:** not shown as it's typed, asked twice. The strength rule is
  "at least 12 characters" (A81). Too short: "Use at least 12
  characters." If the two entries differ: "The passwords don't match. Enter
  them again." Setup then asks for both again.
- Setup never prints the password, and never puts it in a command argument,
  a log or JSON status (SRV-5). Noninteractive setup takes it through the
  protected input in §2.
- The last line prints the configured browser address and the username.
- **Order**: setup creates the
  account and the HQ, then offers the recovery code, and only after that
  registers and starts the service. So the recovery block comes before
  "Service running", and HQ isn't open in a browser until the code has been
  collected, left waiting, or shown but not confirmed.
- **Collect it now? Y** runs the same steps as `recovery release` (§4a.6),
  using the account just created, so it doesn't ask to sign in again. It still
  asks the person to confirm it's them, once (§4a.6).
- **What setup leaves behind**. In every case below
  setup goes on, registers and starts the service, and ends with "HQ ready":
  starting the service never depends on the acknowledgement.
  - **n**, or the confirmation cancelled or failed, so the code wasn't shown:
    the code is **Waiting to be collected**, and setup prints the
    `agentic-org recovery release --org '<organization>'` command, as in the
    transcript above.
  - **The code was shown, and the answer at the last prompt wasn't `yes`:** the
    code is **Shown, not confirmed** (§4a.6, RCV-8). Setup prints the
    `--redeliver` command instead:

    ```
    Not recorded. The code was shown, but not confirmed as stored.
    Once the code and the recovery package are stored away from this
    machine, show the code again and confirm:
      agentic-org recovery release --redeliver --org 'Lumen Labs'
    Recovering an organization after losing the HQ machine isn't supported yet.
    …
    Service running.
    HQ ready.
    ```

  - **`yes`:** the code is **Collected**, and setup prints §4a.6's "Recorded
    on…" lines.
  - **Setup itself ends** in the recovery block (Ctrl-C, closed terminal): the
    service hasn't started yet. Before the code was shown, the code stays
    **Waiting to be collected**; once it was shown, it's **Shown, not
    confirmed**. Running setup again resumes (SRV-6) and prints the matching
    command, `recovery release` or `recovery release --redeliver`, as a repeated
    setup does (below).

  Finishing setup without recovery acknowledgement follows the current standalone
  design: service readiness and recovery-material collection are distinct. It does
  not establish supported HQ-loss recovery (REC-1).
- **A repeated setup** (SRV-2) asks for no account. It ends with "HQ is running.
  Open <address> and sign in as alex." While the code is waiting, it also prints
  the `agentic-org recovery release` command. While it was shown but not
  confirmed (§4a.6), it prints the same command with `--redeliver`.
- **Setup without a local session**. Noninteractive setup,
  and setup run over SSH on a Mac, can't show the code: showing it needs someone
  at the HQ machine (§4a.6). They don't ask "Collect it now?". They finish with
  the HQ created and the code **waiting**, which isn't a failure: setup exits
  successfully, and its status output reports HQ ready and the recovery code
  waiting (`pending`), never the code. This is a stated exception to §2's "same
  state transitions": automation never gets the code. The owner collects it later with `agentic-org recovery
  release`, at the machine. Setup over SSH on a Mac ends its recovery block with:

  ```
  Recovery code for Lumen Labs
  It can't be shown over SSH. Showing it needs you at this Mac, to confirm
  it's you with Touch ID or the Mac's login password. At the Mac itself,
  or through Screen Sharing, run:
    agentic-org recovery release --org 'Lumen Labs'
  Recovering an organization after losing the HQ machine isn't supported yet.
  ```

  Noninteractive setup prints, in its text output:

  ```
  Recovery code for Lumen Labs: waiting to be collected.
  It's shown only in an interactive session on this machine. Later, run:
    agentic-org recovery release --org 'Lumen Labs'
  On a Mac, run it at the Mac itself or through Screen Sharing.
  Recovering an organization after losing the HQ machine isn't supported yet.
  ```

  Over SSH on Linux, with a terminal, setup offers to collect it as usual: the
  password typed there confirms it's you.
- **The first browser visit** is an ordinary sign-in (§4a.3). After it, the first
  organization opens (OPEN-1), with the recovery notice while the code is waiting.

**Other account commands (HQ machine only)**. They match the host's
operations: `account create` is `account.create`, `account set-password` is
`account.set_password`, and `account reset-password` is
`account.reset_password`.

**Add an account.** Only an instance admin can, and they sign in first:

```
$ agentic-org account create
Add an account to instance default

Sign in as an instance admin of this instance.
Username: alex
Password (not shown as you type):

New account
Username: sam
Password (at least 12 characters, not shown as you type):
Password again:

An instance admin can create organizations, add accounts and show
recovery codes. It doesn't open organizations it doesn't own.
Make this account an instance admin? [y/N]: n

Which organizations will sam own? Type their numbers separated by
commas, or press Enter for none.
  1) Lumen Labs
  2) Side Project
Organizations: 1

Confirm it's you in the system dialog (Touch ID or this Mac's login password).
Account "sam" created. Instance admin: no. Owns: Lumen Labs.
sam can sign in at http://127.0.0.1:<port>.
```

- The admin question defaults to No: Enter means no. **y** makes the new account
  an instance admin, and the last line says "Instance admin: yes."
- The list shows every organization on this instance, including ones the
  signed-in admin doesn't own (2026-11-10). With no organizations to list, it
  doesn't ask. With none chosen, the result reads "Owns: none." and adds "sam
  doesn't own any organization yet."
- On a Mac, the system dialog names the action and the organizations: "add the
  account sam, owner of Lumen Labs". On Linux there's no dialog: the password
  typed at sign-in confirms it's you, so that line doesn't appear.
- Over SSH on a Mac, or with no session at the Mac's screen, it can't confirm
  that. It says so before asking anyone to sign in (the table below). Over SSH
  on Linux, with a terminal, it works.

**Change your own password.** Any account can, over SSH too:

```
$ agentic-org account set-password
Change your password on instance default
Username: alex
Current password (not shown as you type):
New password (at least 12 characters, not shown as you type):
Password again:
Password changed for alex. Every session of alex has ended, so sign in
again in the browser with the new password.
```

- A password change ends every session of that account, in every browser
 . An open page shows "Sign in again" (§4a.4).

**Reset a forgotten password.** It needs no sign-in. Instead it needs someone at
the HQ machine: on a Mac, the person confirms it's them; on Linux, it runs as
root.

```
$ agentic-org account reset-password alex
Reset the password for alex on instance default
This signs alex out everywhere. The reset is recorded with the time,
never the password.
New password for alex (at least 12 characters, not shown as you type):
Password again:
Confirm it's you in the system dialog (Touch ID or this Mac's login password).
Password reset for alex. Every session of alex has ended. alex can sign
in with the new password now.
```

On Linux:

```
$ sudo agentic-org account reset-password alex
[sudo] password for ops:
Reset the password for alex on instance default (Linux user ops)
This signs alex out everywhere. The reset is recorded with the time,
never the password.
New password for alex (at least 12 characters, not shown as you type):
Password again:
Password reset for alex. Every session of alex has ended. alex can sign
in with the new password now.
```

- It ends every session of that account and clears any sign-in limit on it, so
  "Too many sign-in attempts" no longer applies.
- On a Mac the system dialog reads "reset the password for alex". Over SSH, or
  with no session at the Mac's screen, it stops before asking for a password
  (the table below).
- **On Linux,** run it with `sudo` as the Linux user who runs the instance. It
  finds that user's instance; `--name` picks among that user's instances, after
  the command as everywhere: `sudo agentic-org account reset-password --name lab
  alex`. Signed in as root without `sudo`, name that user with `--user <user>`.
  The first line names the Linux user, so a reset never lands on another user's
  instance unnoticed.
- Without `sudo` on Linux, a forgotten password can't be reset. The only
  other way back is removing the instance and its data (§5). No copy suggests
  that; the not-root error says to use `sudo`.

**Link, unlink and revoke an account**. They match the host's `account.link`,
`account.unlink` and `account.revoke`. Each one asks an instance admin to sign in, then asks y/N in
words that name the account and the organization, then (on a Mac) asks the admin
to confirm it's them in a system dialog that names the same. Enter means no.

```
$ agentic-org account link sam --org 'Side Project'
Link an account on instance default

Sign in as an instance admin of this instance.
Username: alex
Password (not shown as you type):

sam will own Side Project: sam can sign in and open it, as its owner.
Link sam to Side Project? [y/N]: y
Confirm it's you in the system dialog (Touch ID or this Mac's login password).
sam is linked to Side Project. sam owns: Lumen Labs, Side Project.
```

```
$ agentic-org account unlink sam --org 'Side Project'
Unlink an account on instance default

Sign in as an instance admin of this instance.
Username: alex
Password (not shown as you type):

sam will no longer own Side Project or open it. This signs sam out
everywhere; sam can sign in again to open the organizations it still owns.
Side Project stays linked to: alex.
Unlink sam from Side Project? [y/N]: y
Confirm it's you in the system dialog (Touch ID or this Mac's login password).
sam is unlinked from Side Project. sam owns: Lumen Labs.
```

```
$ agentic-org account revoke sam
Revoke an account on instance default

Sign in as an instance admin of this instance.
Username: alex
Password (not shown as you type):

This signs sam out everywhere and disables the account: sam can't sign in.
sam owns Lumen Labs. Lumen Labs stays linked to its other accounts: alex.
Revoke sam? [y/N]: y
Confirm it's you in the system dialog (Touch ID or this Mac's login password).
sam is revoked. Every session of sam has ended, and sam can't sign in.
```

- `link` and `unlink` can name any organization on this instance, including one
  the signed-in admin doesn't own. `--org` takes the organization's
  name, quoted for the shell as in §4a.6.
- `revoke`'s question names the account and every organization it owns; an
  account that owns none reads "sam doesn't own any organization." The dialog
  reads "revoke the account sam".
- On a Mac the dialogs read "link the account sam to Side Project" and "unlink
  the account sam from Side Project". On Linux there's no dialog: the password
  typed at sign-in confirms it's you, so the "Confirm it's you" line doesn't
  appear. Over SSH on a Mac they stop before any prompt (the table below).
- Unlinking and revoking end every session of that account, so an
  open page of that account shows "Sign in again" (§4a.4). Revoking the account
  you signed in with ends your own sessions too. After an unlink, signing in
  again on a page of the unlinked organization opens the organization list
  instead of that page; a revoked account can't sign in again (§4a.4,
  SIGN-43).
- Signing in with a revoked account shows the sign-in page's usual "That
  username and password don't match. Try again." (§4a.3), so the page never
  says an account exists or was revoked.
- **The two rules** (§4a.1). A command that would leave the instance with no
  instance admin, or an organization with no linked account, is refused before
  the y/N question, and nothing changes (the table below).

**When account commands fail** (nothing is changed in any case):

| Case | CLI output | Exit |
| --- | --- | --- |
| `create`, `set-password`, `link`, `unlink`, `revoke`: wrong username or password at sign-in | "That username and password don't match. Try again." Asks again. Too many tries: "Too many sign-in attempts. Try again in a few minutes." | — / Error |
| `create`: the account that signed in isn't an instance admin | "sam isn't an instance admin, so it can't add accounts. Sign in as an instance admin of instance default." | Error |
| `link`, `unlink`, `revoke`: the account that signed in isn't an instance admin | "sam isn't an instance admin, so it can't change accounts. Sign in as an instance admin of instance default." | Error |
| `link`, `unlink`, `revoke`: anything but y at the question | "Nothing changed." | Error |
| `link`, `unlink`, `revoke`: unknown username | "There's no account called sam on instance default." | Error |
| `link`, `unlink`: unknown organization | "There's no organization called "Side Project" on instance default." | Error |
| `link`, `unlink`: no `--org` | "Name the organization: agentic-org account link <username> --org <organization>" (`unlink` in the unlink command) | Error |
| `link`: already linked | "sam is already linked to Side Project. Nothing changed." Asks nothing else. | Success |
| `unlink`: not linked | "sam isn't linked to Side Project. Nothing changed." | Error |
| `unlink`: the organization's only linked account | "sam is the only account linked to Side Project, so it can't be unlinked: every organization keeps at least one. Link another account first: agentic-org account link <username> --org 'Side Project'" | Error |
| `revoke`: the only linked account of an organization | "sam is the only account linked to Side Project, so it can't be revoked: every organization keeps at least one. Link another account to Side Project first: agentic-org account link <username> --org 'Side Project'" (it names every such organization) | Error |
| `revoke`: the instance's only instance admin | "sam is the only instance admin of instance default, so it can't be revoked: the instance keeps at least one. Add another instance admin first with agentic-org account create." | Error |
| `revoke`: already revoked | "sam is already revoked. Nothing changed." | Error |
| `create`: username already used | "There's already an account called sam. Choose another username." Asks again. Usernames that differ only in capital letters count as the same. | — |
| `create`: empty username | "Enter a username." Asks again. | — |
| `create`: anything but y, n or Enter at the admin question | "Type y or n." Asks again. | — |
| `create`: an organization number that isn't in the list | "Type numbers from the list, separated by commas, or press Enter for none." Asks again. | — |
| `create`, `reset-password`, `link`, `unlink`, `revoke` on a Mac: the confirmation is cancelled or fails (the system dialog) | "Not confirmed, so nothing was changed. Run this command again when you're at the machine." | Error |
| `create` over SSH on a Mac, or no session at the Mac's screen | "Adding an account needs you at this Mac, to confirm it's you with Touch ID or the login password, and that can't be done over SSH. Run this command at the Mac itself or through Screen Sharing." Shown before any prompt. | Error |
| `link`, `unlink`, `revoke` over SSH on a Mac, or no session at the Mac's screen | "Changing an account needs you at this Mac, to confirm it's you with Touch ID or the login password, and that can't be done over SSH. Run this command at the Mac itself or through Screen Sharing." Shown before any prompt. | Error |
| `reset-password` over SSH on a Mac, or no session at the Mac's screen | "Resetting a password needs you at this Mac, to confirm it's you with Touch ID or the login password, and that can't be done over SSH. Run this command at the Mac itself or through Screen Sharing." Shown before any prompt. | Error |
| `reset-password` on Linux without root | "Resetting a password on Linux needs root. Run it with sudo: sudo agentic-org account reset-password alex" (with `--name lab` for a named instance). | Error |
| `reset-password` as root, not through `sudo`, without `--user` | "Name the Linux user who runs the instance: agentic-org account reset-password --user <user> alex" | Error |
| `reset-password` on Linux: that user has no instance by that name | "Linux user ops has no instance called default." | Error |
| `reset-password` on Linux: the instance's files aren't owned by that user | "Instance default's files aren't owned by ops, so nothing was changed. As ops, run agentic-org doctor." | Error |
| `reset-password` without a username | "Name the account: agentic-org account reset-password <username>" | Error |
| `reset-password`: unknown username | "There's no account called sam on instance default." | Error |
| Password too short (any command) | "Use at least 12 characters." Asks again. | — |
| The two password entries differ | "The passwords don't match. Enter them again." Asks for both again. | — |
| Instance not running, or the host can't be reached | "Instance default isn't running. Start it with agentic-org service start, then run this command again." For a named instance: "Instance lab isn't running. Start it with agentic-org service start --name lab, then run this command again." Under `sudo`: "Instance default of Linux user ops isn't running. As ops, start it with agentic-org service start, then run this command again." | Error |
| Not an interactive terminal, and no protected password input given | "Run this in a terminal, or pass the password through the protected input (§2)." | Error |

`agentic-org service start` is §5's.

### 4a.3 Sign in

The sign-in page is the first thing any visitor to the instance URL sees until
they sign in. It shows no organization data.

```
 ┌────────────────────────────────────────────────────┐
 │  Agentic Org                                        │
 │  Sign in to HQ at 127.0.0.1:7420                    │
 │                                                     │
 │  ┃ ⚠ That username and password don't match.       │  ← error area (hidden
 │  ┃   Try again.                                     │     until needed)
 │                                                     │
 │  Username                                           │
 │  [ alex                                         ]   │
 │  Password                                           │
 │  [ ••••••••••••                        ] [ Show ]   │
 │                                                     │
 │  [                 Sign in                      ]   │
 │                                                     │
 │  Forgot your password? At the HQ machine, run:      │
 │    agentic-org account reset-password               │
 │      --name <instance> <username>                   │
 │  On Linux, run:                                     │
 │    sudo agentic-org account reset-password          │
 │      --name <instance> <username>                   │
 │  Need an account? An instance admin adds it at the  │
 │  HQ machine: agentic-org account create             │
 │    --name <instance>                                │
 └────────────────────────────────────────────────────┘
```

- "Sign in to HQ at <host:port>" uses the address in the browser's own address
  bar, so two instances on one machine (default and lab) are told apart without
  HQ revealing anything before sign-in.
- The reset and add-account commands show `<instance>` and `<username>`
  literally, as text to replace: the page can't know the instance's name before
  sign-in. Each command is one line of code text; the wireframe wraps it only
  for width. The reset help names the Linux form too, because the
  page can't know the HQ machine's system before sign-in: `sudo agentic-org
  account reset-password --name <instance> <username>`. It shows no
  `set-password` command: someone who knows their password signs in first.
- **Show password** is a toggle that reveals the password. Its label stays the
  same; its pressed state says whether the password is shown.
- Enter in either field submits.

| State | What shows |
| --- | --- |
| Loading | "Loading…" while the page checks for a session that is still valid. No form, no organization data. |
| Ready | The form. Focus is in Username, or in Password when Username is already filled. |
| Signing in | **Signing in…**, disabled. Both fields are read-only. |
| Empty field | No request is sent. Under the field: "Enter your username." or "Enter your password." Focus moves to the first empty field. |
| Wrong username or password | "That username and password don't match. Try again." The same words for both cases. Password is cleared, Username kept, focus moves to Password. |
| Too many attempts, with a retry time | "Too many sign-in attempts. Try again in <n> minutes." ("1 minute" when one is left.) **Sign in** stays disabled until then. The minutes round up and update once a minute. |
| Too many attempts, no retry time | "Too many sign-in attempts. Wait a few minutes, then try again." Limits are A82. |
| HQ unreachable | "Can't reach HQ. Check that the instance is running on the HQ machine (`agentic-org service status --name <instance>`), then try again." **Try again**. Both fields are kept. |
| HQ error | "HQ couldn't sign you in because of a problem on the HQ machine. Try again. If it keeps happening, run `agentic-org doctor --name <instance>` there." **Try again**. |
| Page out of date | "This page is out of date. Reload to get the current version." **Reload**. (API and page versions differ, standalone design §5.) |
| Signed in | Opens what §4a.5 says. |

A note above the form says why the visitor is here, when the page knows:

| Arrived because | Note |
| --- | --- |
| They signed out | "You signed out." |
| Their session ended for any other reason, or the reason is unknown | "Your session ended. Sign in again." |

When HQ is stopped, the browser can't load this page at all and shows its own
error. The table covers a page that loaded and then can't reach HQ.

### 4a.4 Session end, signing in again, and sign out

**When a session ends.** It expires (30 minutes idle or 12 hours in all), the
account's password is changed or reset, or the account is unlinked from an
organization or revoked (`agentic-org account unlink` or `revoke`, §4a.2).
A restart or update of HQ or the web process never ends a session; sessions are
stored at the host ([ADR-0013](../adr/0013-stored-browser-sessions.md)). Signing out
in another tab is different: every tab goes to the sign-in page (Sign out, below). A missing session proof (cleared site
data, a private window) counts as an ended session.

**In an open page, the user signs in again where they are:**

```
 ┌ Sign in again ─────────────────────────────────────┐
 │ Your session ended. Sign in again to carry on.      │
 │ Text you haven't saved stays on this page.          │
 │                                                     │
 │ Account: alex                                       │
 │ Password                                            │
 │ [                                      ] [ Show ]   │
 │                                                     │
 │ Sign in as someone else          [Not now] [Sign in]│
 └────────────────────────────────────────────────────┘
```

- The dialog opens over the page the user was on, the first time HQ refuses a
  request or the event stream because the session ended. Its first line is
  "Your session ended. Sign in again to carry on.", except for the unlink case
  below.
- While HQ is stopped or restarting, the page shows LIVE-8's "Live updates
  paused" line, as for any lost connection, and the session stays valid.
- It re-authenticates the same account: the username is shown as text, not a
  field. Its errors are §4a.3's, shown inside the dialog.
- **After signing in,** the dialog closes and the user is on the same page, at
  the same scroll position, with focus back where it was. Live updates resume
  by cursor or snapshot (SRV-12).
- **Text typed but not saved is kept.** Fields keep their text the whole time.
  A save that HQ refused because the session had ended is marked "Not saved" by
  that field. Nothing is resent on its own: after signing in, the user presses
  Save (or Send) again.
- **If the account no longer owns the page's organization** *(review of
  0802754)*, because it was unlinked from it, signing in again still works, but
  the page doesn't come back. The organization list opens (OPEN-1) with "You no
  longer own Side Project." above it, and text that wasn't saved on that page is
  gone. An account left with no organizations sees §4a.5's no-organization page,
  with the same line above it. Focus goes to the heading of that list (or
  page), and "You no longer own Side Project." is announced through the polite
  live region (§4a.7).
  - The dialog warns about this beforehand only when HQ says the session ended
    because the account was unlinked from this page's organization. Its
    first line then reads "You no longer own Side Project, so you were signed
    out." and its second "Signing in, here or in another tab, opens your list of
    organizations. Text you haven't saved on this page will be lost: choose Not
    now to copy it first." After Not now, the bar reads "You're signed out. Text
    you haven't saved here is lost when you sign in, here or in another tab:
    copy it first."
  - Without that reason, the dialog can't know, so it doesn't warn: it shows its
    usual copy.
- **A revoked account can't sign in again.** The dialog shows SIGN-4's message,
  "That username and password don't match. Try again." (§4a.3), as for any wrong
  password, so it never says the account was revoked (§4a.2).
  **Not now** still keeps the page, so text that
  isn't saved can be copied, and **Sign in as someone else** goes to the sign-in
  page.
- **Other tabs of this HQ** show the same dialog. Signing in in any one of them
  closes the dialog in the others, because they share one session. A tab on a
  page of an organization the account no longer owns goes to the organization
  list too, with "You no longer own <organization>." above it, and the text that
  wasn't saved there is gone, even in a tab that chose Not now *(review of
  18d3f56)*. Tabs on organizations it still owns stay on their page, with their
  text.
- **Not now** (or Esc) closes the dialog. The page stays visible but changes
  nothing, with a bar at the top: "You're signed out. Text you haven't saved is
  still here: copy it, or sign in to save it." **Sign in** reopens the dialog.
- **Sign in as someone else** goes to the sign-in page. A browser has one
  session per HQ, so when a different account signs in, every other open tab of
  this HQ goes to the sign-in page with "You're now signed in as sam in another
  tab. Reload to continue as sam." and **Reload**; none of them keeps showing the
  previous account's data, and none can sign the previous account back in over
  the new session. If the page has text
  that isn't saved, it asks first: "Discard text you haven't saved on this
  page?" **Keep editing** · **Discard and sign in**.
- Reloading or closing the tab loses text that isn't saved, as §4 says. A reload
  while signed out goes to the sign-in page. After signing in, the user lands on
  that same page, unless it belongs to an organization the account can't open:
  then the organization list opens (§4a.5).

**Sign out.** The account menu is the username button in the top bar of every
page:

```
 [ alex ▾ ]
 ┌────────────────────────┐
 │ Signed in as alex      │
 │ Instance admin         │  ← only with the admin grant
 ├────────────────────────┤
 │ Sign out               │
 └────────────────────────┘
```

- With text that isn't saved in any open tab of this HQ, it asks first: "Sign out? Text you
  haven't saved in any open tab of this HQ will be lost." **Stay** · **Sign out**.
- Sign out ends this browser's session at HQ. Every open tab of this HQ goes to
  the sign-in page with "You signed out." The browser's Back button doesn't show
  organization data again.
- Sessions in other browsers are not affected.
- While HQ ends the session, the menu item reads **Signing out…** and is
  disabled. If HQ can't be reached, nothing changes and the page shows "Couldn't
  sign out: HQ can't be reached. Try again when it's back." with **Try again**;
  the session stays valid until HQ ends it or it expires.

**Copy follows the available flow.** On a page with no editable fields, the
Not now bar is simply "You're signed out." with **Sign in**. In the unlink
case, keep "You no longer own <organization>, so you were signed out." and
"Signing in, here or in another tab, opens your list of organizations."; omit
the extra unsaved-text sentence and use the same simple Not now bar. When
editable fields are present, the retention and loss warnings above apply; text
the user entered survives the sign-in dialog as SIGN-10/11 require. This is a
copy distinction, not permission to drop a draft or omit a real loss warning.

### 4a.5 After sign-in: what opens

1. The page the user was trying to open, if it is a page of this HQ. Any other
   return target is ignored. A page of an organization the account can't open
   is ignored too, and the organization list opens instead. When the session
   ended while that page was open (the dialog, or its bar after Not now), the
   list shows "You no longer own <organization>." above it, with the name the
   page was showing (§4a.4).
2. Otherwise the last authorized organization, or the organization list (OPEN-1).
3. An account with no organizations sees:
   - Instance admin: "No organizations yet." with **Sign out**. When the real
     **Create organization** flow (offices §5a) is available, add "Create one to
     get started." and its action. Until then omit both; show no fake or disabled
     Create button.
   - Not an admin: "This account doesn't own any organizations on this HQ.
     Ask an instance admin of this HQ to link it to one." **Sign out**.
     The admin grant alone opens no organization. An admin links an existing account with `agentic-org account link
     <username> --org <organization>` (§4a.2).

### 4a.6 Recovery code

Selected implementation profile: [ADR-0011](../adr/0011-protected-recovery-delivery.md)
REC-DEL-1 governs the native macOS flow below. Its system reason is fixed and
sanitized and names both showing the code and recording it as stored; it does
not interpolate organization names. Actual local console/audit-session and
foreground TTY checks precede credentials and are repeated after authentication.
The SDK device-owner policy may use the methods supported by macOS. Each new
invocation authenticates afresh; same-run acknowledgement uses private
connection-bound permission, not a second dialog. Linux secret delivery remains
unavailable until its own reviewed presence policy, superseding the prospective
Linux examples below for this selected implementation.

The CLI asks for an explicit new package file, refuses overwrite, and requires
the final code group plus typed `yes` for **both** package and code retained off
HQ. An interrupted acknowledgement may report deletion pending; no later command
can redisplay the code while that durable deletion is being reconciled. Status
reports persisted delivery facts only and always retains the REC-1 limitation.

The recovery code, with the organization's encrypted recovery package, will let
the owner recover the organization's certificate authority if the HQ machine is
lost. Under O-2 the host generates the code, the worker creates the authority and
the encrypted package, and the host keeps the code until it is collected with
`agentic-org recovery release` on the HQ machine. This is the same for every
organization: the first one from setup, and each one created in the browser.
**The code never passes through the web process or the browser.** It shows only its
state.

**Who can show the code**. Only an **instance admin**,
on the HQ machine, in a session at that machine:
- `agentic-org recovery release` first asks the admin to sign in with their
  username and password. Setup's own offer (§4a.2) uses the account it just made.
- Then it asks them to confirm it's them, **once per run**, before it shows the
  code (2026-11-10). That one confirmation covers
  both showing the code and recording that it's stored, and the prompt names
  both: on a Mac, Touch ID or the Mac's login password, in a system dialog that
  reads "show the recovery code for Lumen Labs and record it as stored" (with
  `--redeliver`: "show the recovery code for Lumen Labs again and record it as
  stored"). On Linux, the password typed at the terminal counts.
- The acknowledgement is still recorded only after the person types `yes` in
  that same run (below). Each new run, including a `--redeliver` run, asks for a
  new confirmation: a confirmation never carries over to another run.
- Over SSH on a Mac, or with no session at the Mac's screen, it can't confirm
  that, so it shows no code (the table below). It says so before asking anyone to sign in. Over SSH on Linux, with a terminal, it works.
- An owner who isn't an instance admin sees the same states in the browser, and
  asks an instance admin to run the command.

**Shown once, and again only until it's confirmed**. The command shows the code
once. If the owner hasn't confirmed it's stored (the run was interrupted, or the
answer wasn't `yes`), `agentic-org recovery release --redeliver` shows **the
same code** again. That second showing is recorded with who ran it and when,
never the code, and the browser says so (below). Once the owner confirms it's
stored, the host erases its copy and the code can't be shown again, with or
without `--redeliver`.

**What HQ records** (2026-11-10). The recovery
state carries when the code was first shown, when it was last shown again and
how many times, and when it was confirmed as stored. Every showing is recorded
in the instance's journal and audit log with the account and the time, never
the code. The browser shows only the dates and the count (below). Who showed it
is visible only with `agentic-org recovery status` on the HQ machine (below).

**Every message that reports the code's state** (collected, waiting, shown but
not confirmed, being prepared, couldn't be prepared, not recorded), wherever it appears (the
notice, the Recovery row, Needs you items, setup's recovery block, a repeated
setup's reminder, `agentic-org service status`, `agentic-org recovery release` and
`agentic-org recovery status`), ends with this
sentence: "Recovering an organization after losing the HQ machine isn't supported
yet." (REC-1, §7.) The tables below leave it out for space. Input errors (no
`--org`, an unknown organization, a group that doesn't match, no interactive
terminal, an instance that isn't running, a failed sign-in, an account that isn't
an instance admin, a confirmation that was cancelled or can't be given over SSH)
don't report the state and don't need it.

**States in the browser.** Only the organization's owner sees them.

| State | Settings › Organization, Recovery row | Notice at the top of the organization's pages | Needs you |
| --- | --- | --- | --- |
| Being prepared (the package is still being created) | "Recovery code: being prepared. This takes a moment." | None | None |
| Waiting to be collected | "Recovery code: waiting to be collected on the HQ machine. Run `agentic-org recovery release --org '<organization>'` on <HQ machine>." **Copy command** (the name is shell-quoted: single quotes, with an embedded `'` written `'\''`, so names with `"`, `$` or backticks paste safely) | The notice below | "Collect the recovery code for <organization> on <HQ machine>." Opens the Recovery row. |
| Shown, not confirmed (shown on the HQ machine, not yet confirmed as stored) | "Recovery code: shown on <HQ machine> on <date>, but not confirmed as stored. To show it again and confirm, run `agentic-org recovery release --redeliver --org '<organization>'` on <HQ machine>." After a second showing: "Recovery code: shown on <HQ machine> on <date> (shown again 2 times, last on <date>), but not confirmed as stored. To show it again…" **Copy command** (quoted as above) | The notice below, in its "shown, not confirmed" form | "Confirm the recovery code for <organization> is stored, on <HQ machine>." Opens the Recovery row. |
| Collected | "Recovery code collected on <date>." After a second showing: "Recovery code shown on <date> (shown again 2 times, last on <date>) · collected on <date>." | None | None (the item clears) |
| Couldn't be prepared | "Recovery code: couldn't be prepared. <reason>. On <HQ machine>, run `agentic-org doctor`." **Copy command** | "Recovery material for <organization> couldn't be prepared. See Settings › Organization." | "Recovery material for <organization> couldn't be prepared." |
| Status unknown (HQ didn't say) | "Recovery code: status unknown." Never shown as collected. | None | None |

- The command includes `--name <instance>` when the instance isn't `default`.
- Dates are in the browser's date format, for example "10 Nov 2026": "shown on <date>" is when the code was first shown, "last on <date>"
  when it was last shown again, and "collected on <date>" when it was confirmed
  as stored. "shown again 2 times" is the number of second showings; one reads
  "shown again once, on <date>". When HQ doesn't give a date, the row leaves out
  that "on <date>", never guesses it.
- `<HQ machine>` is the name HQ reports for its machine (A93).
- When the signed-in owner isn't an instance admin, the Waiting and Shown, not
  confirmed rows add: "An instance admin of this HQ runs it."
- **Copy command** copies only the command. No state copies or shows the code.
- The browser state changes live when the code is shown, shown again or
  collected: the notice and Needs you item change, or go away once it's
  collected, without a reload. While live updates are paused, the
  change shows on the next load.
- If Home's Needs you isn't built yet, the notice and the Settings row carry the
  state alone. Nothing else depends on Needs you.

**The notice** (owner only, every page of that organization, no close button,
gone once collected):

```
 ┃ ⚠ Collect the recovery code for Lumen Labs
 ┃   It's waiting on the HQ machine and is never shown in the browser.
 ┃   On hq-mac, run:  agentic-org recovery release --org 'Lumen Labs'
 ┃   Then store it and the recovery package away from that machine.
 ┃   Recovering an organization after losing the HQ machine isn't supported yet.
 ┃                                                            [Copy command]
```

In the Shown, not confirmed state:

```
 ┃ ⚠ Confirm the recovery code for Lumen Labs is stored
 ┃   It was shown on hq-mac on 10 Nov 2026, but nobody confirmed it's stored.
 ┃   To show it again and confirm, on hq-mac run:
 ┃     agentic-org recovery release --redeliver --org 'Lumen Labs'
 ┃   Recovering an organization after losing the HQ machine isn't supported yet.
 ┃                                                            [Copy command]
```

After a second showing, its second line reads "It was shown on hq-mac on 10 Nov
2026 and shown again 2 times, last on 11 Nov 2026, but nobody confirmed it's
stored."

**Collecting it on the HQ machine:**

```
$ agentic-org recovery release --org 'Lumen Labs'
Recovery code for Lumen Labs (instance default)

Sign in as an instance admin of this instance.
Username: alex
Password (not shown as you type):

You'll see this code once, until you confirm it's stored. With the
recovery package, it's what will let you recover Lumen Labs if this
machine is lost. Store both away from this machine: for example, the
code in a password manager on another device, or on paper somewhere
safe.
Recovering after losing the HQ machine isn't supported yet, but
keeping the code now is what will make it possible.

Recovery package: <the file it was exported to, ADR-0011>

Showing the code is recorded, with your username and the time. The code
itself is never recorded.
Press Enter to show the code.
Confirm it's you in the system dialog (Touch ID or this Mac's login password).

Recovery code: <the code: ten groups of four characters separated by dashes, ADR-0007>

To confirm you stored it, type the last group of the code: 9FXM
Are the code and the encrypted recovery package stored away from this machine?
Type yes to confirm: yes

Recorded on 10 Nov 2026 at 14:02. This code won't be shown again.
Recovering an organization after losing the HQ machine isn't supported yet.
```

- The code is shown, and counts as shown, only after Enter and the confirmation.
  Stopping before that shows nothing and leaves the code waiting.
- The one confirmation, after Enter, covers showing the code and
  recording that it's stored: `yes` records it with no second dialog. The
  system dialog reads "show the recovery code for Lumen Labs and record it as
  stored".
- On Linux there's no system dialog: the password typed at sign-in confirms it's
  you, so the "Confirm it's you" line doesn't appear.

**Showing it again**, when it was shown but not confirmed as stored:

```
$ agentic-org recovery release --redeliver --org 'Lumen Labs'
Recovery code for Lumen Labs (instance default)
It was shown on 10 Nov 2026 at 14:02, but nobody confirmed it's stored.
Showing it again is recorded, with your username and the time. The code
itself is never recorded.

Sign in as an instance admin of this instance.
Username: alex
Password (not shown as you type):

Press Enter to show the code again.
Confirm it's you in the system dialog (Touch ID or this Mac's login password).

Recovery code: <the same code, in groups separated by dashes>

To confirm you stored it, type the last group of the code: 9FXM
Are the code and the encrypted recovery package stored away from this machine?
Type yes to confirm: yes

Recorded on 10 Nov 2026 at 15:10. This code won't be shown again.
Recovering an organization after losing the HQ machine isn't supported yet.
```

- It's the **same code**: showing it again never makes a new one.
- This run asks for its own confirmation, even if an earlier run was confirmed
 . The system dialog reads "show the recovery code for Lumen Labs again
  and record it as stored".
- `--redeliver` while the code is still waiting (never shown) is accepted as the
  first showing (2026-11-10): the run looks and reads
  like the first-showing transcript above, it's recorded as a first showing, not
  as shown again, and the browser's count of second showings doesn't change.
- Its failures are the same as the first showing's (below), except the rows
  marked `--redeliver`: a showing again that fails or is interrupted leaves the
  code **Shown, not confirmed**, never waiting, and its copy says so.
- A run that can't show the code (not an instance admin, run without
  `--redeliver` after a showing, already collected) is refused before the
  presence confirmation, so it never asks to confirm it's you.

| Case | CLI output | Exit |
| --- | --- | --- |
| Typed group doesn't match | "That isn't the last group of the code. Check what you stored, then type it again." Asks again. | — |
| Anything but `yes` at the last prompt | "Not recorded. Once the code and the recovery package are stored away from this machine, show the code again and confirm: agentic-org recovery release --redeliver --org 'Lumen Labs'. Recovering an organization after losing the HQ machine isn't supported yet." The code is now **Shown, not confirmed**. | Error |
| Interrupted before the code is shown (Ctrl-C at sign-in or at "Press Enter") | "Not shown. The code is still waiting; run this command again to see it. Recovering an organization after losing the HQ machine isn't supported yet." Nothing changes. | Error |
| `--redeliver`: interrupted before the code is shown again (Ctrl-C at sign-in or at "Press Enter") | "Not shown again. Nobody has confirmed the code is stored yet. To show it again and confirm, run: agentic-org recovery release --redeliver --org 'Lumen Labs'. Recovering an organization after losing the HQ machine isn't supported yet." Nothing changes: the code stays **Shown, not confirmed**. | Error |
| Interrupted after the code is shown, before `yes` (Ctrl-C, closed terminal) | On Ctrl-C: "Not recorded. To show the code again and confirm, run: agentic-org recovery release --redeliver --org 'Lumen Labs'. Recovering an organization after losing the HQ machine isn't supported yet." A closed terminal prints nothing. Either way the code is **Shown, not confirmed**. | Error |
| Run without `--redeliver` after the code was shown but not confirmed | "The recovery code for Lumen Labs was shown on 10 Nov 2026 at 14:02, but nobody confirmed it's stored. To show it again, run: agentic-org recovery release --redeliver --org 'Lumen Labs'. Recovering an organization after losing the HQ machine isn't supported yet." No code. | Error |
| Already collected, with or without `--redeliver` | "The recovery code for Lumen Labs was collected on 10 Nov 2026. It can't be shown again. Recovering an organization after losing the HQ machine isn't supported yet." No code. | Error |
| Wrong username or password at sign-in | "That username and password don't match. Try again." Asks again. Too many tries: "Too many sign-in attempts. Try again in a few minutes." No code. | — / Error |
| The account isn't an instance admin | "sam isn't an instance admin, so it can't show recovery codes. Sign in as an instance admin of instance default." No code. | Error |
| The confirmation is cancelled or fails (the system dialog) | "Not confirmed, so the code wasn't shown. Run this command again when you're at the machine." No code; nothing changes. It's the run's only confirmation, so there's none to cancel after the code is shown. | Error |
| `--redeliver`: the confirmation is cancelled or fails (the system dialog) | "Not confirmed, so the code wasn't shown again. Nobody has confirmed it's stored yet. When you're at the machine, run: agentic-org recovery release --redeliver --org 'Lumen Labs'. Recovering an organization after losing the HQ machine isn't supported yet." No code; nothing changes: the code stays **Shown, not confirmed**. | Error |
| Over SSH on a Mac, or no session at the Mac's screen | "Showing the recovery code needs you at this Mac, to confirm it's you with Touch ID or the login password, and that can't be done over SSH. Run this command at the Mac itself or through Screen Sharing." No code. | Error |
| Still being prepared | "Recovery material for Lumen Labs is still being prepared. Try again in a minute. Recovering an organization after losing the HQ machine isn't supported yet." | Error |
| Couldn't be prepared | "Recovery material for Lumen Labs couldn't be prepared: <reason>. Run agentic-org doctor. Recovering an organization after losing the HQ machine isn't supported yet." | Error |
| No `--org`, one organization on the instance | Collects that organization's code. | — |
| No `--org`, several organizations | "This instance has several organizations. Run again with --org and one of: Lumen Labs, Side Project." | Error |
| Unknown organization | "There's no organization called "Lumen Labs" on instance default." | Error |
| Not an interactive terminal | "The recovery code is shown only in an interactive terminal." No code. Automation never gets the code: noninteractive setup ends with it waiting (§4a.2). | Error |
| Instance not running, or the host can't be reached | "Instance default isn't running. Start it with agentic-org service start, then run this command again." For a named instance: "Instance lab isn't running. Start it with agentic-org service start --name lab, then run this command again." No code. | Error |

- `agentic-org service status` lists each organization's recovery state in the same
  words as the Settings row, never the code.
- An organization created in the browser starts as **Being prepared**, then
  **Waiting to be collected**. An instance admin collects it with the same
  command, naming that organization.

**Who showed it, and when** (2026-11-10).
`agentic-org recovery status` on the HQ machine is the only place that shows
who showed the code. It never shows the code, and it changes nothing:

```
$ agentic-org recovery status --org 'Lumen Labs'
Recovery code for Lumen Labs (instance default)

Sign in as an instance admin of this instance.
Username: alex
Password (not shown as you type):

State: shown, not confirmed as stored.
Showings (the code itself is never recorded):
  10 Nov 2026 14:02  shown         by alex
  10 Nov 2026 16:40  shown again   by alex
  11 Nov 2026 09:30  shown again   by alex
To show it again and confirm, run:
  agentic-org recovery release --redeliver --org 'Lumen Labs'
Recovering an organization after losing the HQ machine isn't supported yet.
```

- **State** uses the Settings row's words: "waiting to be collected", "shown,
  not confirmed as stored", "collected on 11 Nov 2026 at 09:31", "being
  prepared", "couldn't be prepared: <reason>".
- **Showings** lists every showing, oldest first, with its date, time and
  account. A code that was never shown reads "Showings: none yet.", and the
  last lines give the `agentic-org recovery release --org '<organization>'`
  command instead. Once it's collected, the list stays, and the command lines
  are replaced by "It can't be shown again."
- It needs an instance admin's sign-in, like `recovery release`, but no confirmation that it's you, so it works
  over SSH too. Its sign-in, not-an-admin, instance-not-running,
  unknown-organization and no-terminal-for-the-password failures use the
  `recovery release` and account tables' copy, except that a non-admin is
  refused with "<username> isn't an instance admin, so it can't see recovery
  status. Sign in as an instance admin of instance <instance>." and nothing is
  shown.
- Without `--org`, on an instance with several organizations, it lists each
  one's State line, then "Run again with --org to see who showed it and when."
  With one organization, it shows that one.

### 4a.7 Accessibility

- **Labels.** Every field has a visible label tied to it: Username, Password.
  The fields use `autocomplete="username"` and `"current-password"`, so password
  managers work. **Show password** is a button with `aria-pressed` and a fixed
  label.
- **Focus order** on the sign-in page: the error area (when shown), Username,
  Password, Show, Sign in, then the help text. In the sign-in-again dialog:
  Password, Show, Sign in as someone else, Not now, Sign in (the visual order). The dialog traps
  focus while open and returns focus to where it was when it closes, except
  after an unlink from the page's organization: then focus goes to the
  organization list's heading, and "You no longer own <organization>." is
  announced through the polite live region (§4a.4).
- **Errors.** A field's error sits under it, is linked with `aria-describedby`,
  and sets `aria-invalid`. The wrong-password message is linked to both fields.
  Errors are announced (`role="alert"`) when they appear. Focus moves as §4a.3
  says, never to a hidden element.
- **Status changes** (Signing in…, the session-ended bar, the recovery state
  changing) are announced through a polite live region. The rate-limit minutes
  don't tick in the announcement.
- **Not by colour alone.** Errors have an icon and words. The notice has an
  icon, a heading and words. Recovery states are words.
- **The CLI** writes plain words, with no meaning carried only by colour or
  symbols. It follows `NO_COLOR` and writes no colour when the output isn't a
  terminal. Prompts say what to type ("Type yes to confirm"). The code is shown
  as plain text in short groups, so a screen reader can read it group by group.

### 4a.8 Not in v1

Inviting people to sign in, account screens in the browser, password change or
reset in the browser, sign-in with another provider, two-step sign-in, showing
the recovery code in the browser, and recovering after losing HQ (REC-1).

### 4a.9 Open questions

- A81 · Username length and characters, and the password rule (at least 12 characters is proposed). Usernames are normalized (NFC, case-folded) and unique ignoring case.
- A82 · Sign-in rate limit: per username, per source address or both, the limits, and whether the refusal carries a retry time the page can show.
- A87 · The owner bar (offices §7.1) shows the owner's name. Whether setup asks for a display name, or the owner bar shows the username.
- A88 · Which read gives the browser "being prepared" and "couldn't be prepared" recovery states. `organization.list` gives linked organizations `recovery: pending | delivered | acknowledged` with the dates and the count of second showings.
- A93 · Which name for the HQ machine HQ reports for "<HQ machine>" in copy: the host name, or a configured label.

## 5. Lifecycle, updates and removal

All instance operations accept --name, defaulting to default: service start/stop/status, logs, doctor, upgrade and uninstall. Service registration is performed by setup automatically. `serve` runs the selected
instance in the foreground. These are local control operations; starting a
stopped daemon cannot require an HTTP response from that daemon.

Status distinguishes **Installed**, **Service running**, **HQ ready**, **Awaiting
approval**, **Synchronizing**, **Office ready** and runtime authentication failures.
It reports the actual browser URL only for an HQ endpoint and the connected HQ
address for a satellite. Logs and diagnostics redact secrets.

Stopping an instance stops admission of new work, drains durable writes and stops
its children within defined deadlines, preserving data. It does not affect another
instance. Closing a browser does not stop anything. Upgrades target the selected
instance and switch a complete matching server/helper/assets release. Another
instance may continue using its existing version. An upgrade fails recoverably.

Uninstall unregisters only the selected service and preserves its data by default.
Data deletion is a separate explicit, instance-scoped operation; active children
must be stopped first. Shared releases stay while any instance references them.
Cache deletion additionally requires verified protected-source coverage, rebindable
credentials, retained unresolved communication and process cleanup/fencing. It
must preserve local/unpushed code checkpoints. Deliberate deletion of an explicitly
disposable test instance is a different action from rebuilding a real office cache.
An office removed from an organization follows the existing revocation/reclaim
rules, which are distinct from uninstalling software on a machine.

## 6. Acceptance criteria

- SRV-1 · A supported macOS or Linux host installs and runs HQ without frontend build tools. Its browser loads the bundled application through the server.
- SRV-2 · A first setup without a name creates `default`; a repeated setup reuses its identity, data and port and starts at most one daemon.
- SRV-3 · A named setup creates or selects that name. Two instances have disjoint data, credential authority, sockets, services and listening endpoints. Browser cookies may be delivered across same-host ports; instance/origin-bound session proofs isolate authority as required by SRV-23.
- SRV-4 · Concurrent setup/start operations and aliased paths cannot open the same instance storage twice; conflicting ports produce an actionable error without changing saved addresses.
- SRV-5 · Interactive and noninteractive setup reach the same configuration, except the recovery code, which noninteractive setup leaves waiting (RCV-20). Missing inputs fail predictably; satellite configuration is refused before initial HQ provisioning; secrets do not appear in logs or command arguments.
- SRV-6 · Repeating setup cannot silently upgrade, re-enroll, rotate identity, reset configuration or restart a healthy daemon. Interrupted setup resumes without duplicate offices.
- SRV-7 · Initial setup creates HQ only and establishes owner authentication, with no satellite choice. Unauthenticated or unauthorized requests, including requests from another website to loopback, cannot perform administration.
- SRV-8 · Any authorized browser can use HQ without installing software or becoming a satellite; quitting it leaves offices and agents running.
- SRV-9 · A satellite is initiated later from HQ UI or an HQ-issued command and enrolls through a verified invitation and approval, reconnects using its stored identity and opens no public application listener.
- SRV-10 · Organization changes remain HQ-authoritative; locally owned work commits locally and synchronizes with deduplication and resumable positions.
- SRV-11 · HQ shows last-known data and freshness accurately. Live control of an unreachable satellite is unavailable or explicitly queued by that operation's contract, never falsely completed.
- SRV-12 · Browser reconnection recovers missed updates without duplicates or a snapshot/event gap; an expired cursor refreshes and a lost mutation response can be resolved by operation ID.
- SRV-13 · Instance and agent paths are server-managed and shown read-only where authorized. No browser path picker, import or storage relocation flow ships in v1.
- SRV-14 · Setup installs a matching `agent-os`, detects required coding tools and selected harnesses, and distinguishes daemon readiness from agent authentication/readiness.
- SRV-15 · `service start`, `service stop`, `service status`, `logs`, `doctor`, `upgrade` and `uninstall` operate on the selected instance. Service manager or credential-unlock limitations are surfaced explicitly.
- SRV-16 · Upgrading or uninstalling one instance cannot replace a running peer's helpers, remove a referenced release, delete peer data or sweep peer credentials.
- SRV-17 · Shutdown drains acknowledged writes and reaps owned descendants within documented deadlines. Restart reconciles recorded work; browser disconnect does not cancel durable work.
- SRV-18 · Production browser paths use the HTTP API, with an explicit unavailable state for missing features. An API failure never selects sample data. The unavailable pages' copy is in `app-shell-ux.md` §3 (SHELL-18 to SHELL-21).
- SRV-19 · Withdrawn: there is no desktop application to exclude.
- SRV-20 · Superseded code is deleted, not adapted, and nothing is imported. Domain rules, storage durability, office identity and supervision invariants stay covered.
- SRV-21 · A fresh HQ and satellite complete the cross-platform walkthrough in [dogfood-run.md](dogfood-run.md), including disconnect/reconnect and a real agent. Mock-only success is insufficient.

- SRV-22 · The retained browser application's source/package is `apps/agentic-org-ui` / `agentic-org-ui`; worker source/package/executable are apps/agentic-org-office / agentic-org-office. Browser server and agent CLI use apps/agentic-org-web / agentic-org-web and apps/agent-os / agent-os; host library crates/org-host is composed by agentic-org serve. Workspace, imports, tests, build, packaging and ownership references agree; the rename preserves the per-office worker and host credential boundaries.

- SRV-23 · Default/lab and a hostile same-host port cannot authenticate, read, mutate, stream, refresh, recover a proof or logout another origin using a copied/overwritten cookie alone. Every protected path requires the valid instance/origin-bound cookie+proof pair; proofs never enter URLs/logs/redirects.
- SRV-24 · Two HQ organizations share one web process listener. The web process holds no office key; the host keeps provisioning and the private launch. Instance-admin creation succeeds; organization-owner-only, employee and office-certificate callers cannot create organizations.
- SRV-25 · HQ creation idempotently provisions versioned encrypted CA escrow, protected one-time recovery-code delivery and off-HQ retention acknowledgement. A fixture recovers the same CA identity with the code and refuses wrong-code/damaged escrow; no code/usable CA key is replicated or logged. REC-1 remains an explicit production-recovery release decision.
- SRV-26 · No interface the product provides (browser, CLI, HTTP API) returns one employee's or instance's private data to another employee's agent or another instance. Isolation from other processes of the same OS user depends on the agent isolation mode ([ADR-0015](../adr/0015-agent-isolation-modes.md)); in `same-user` mode it is not claimed. Release credential tests on both platforms still refuse missing/wrong keys, unavailable/locked providers and development plaintext-store fallback: existing databases/identity are unchanged, no replacement key is created, and restoring the legitimate credential allows recovery; web/CLI helpers cannot obtain raw office keys through any interface the product provides.
- SRV-27 · Eight HQ tabs share at most one authenticated streaming fetch per browser origin, including across login/session changes, and commands still complete under HTTP/1.1. Leader loss reconnects by cursor; unsupported coordination uses bounded short polling and no per-tab long-lived stream.
- SRV-28 · HQ durably accepts Pause for a disconnected satellite as paused status with stop pending. Reconnect/restart reconciles the latest version before new work; a later Resume is not overwritten by stale stop delivery. Known-offline new goals are rejected without a write/queue; uncertain forwarded replies follow EMP-3.
- SRV-29 · Every home or path input on create or update, including absolute paths and traversal, is rejected before filesystem access. The launch working directory derives from managed IDs and the selected office, employee, task and repository workspace; response paths stay read-only under PROF-4.
- SRV-30 · Add agent record follows STAFF-4/HRL-3: neither UI nor direct HTTP calls provision an employee, home or messaging binding before approval. The explicit built-in bootstrap exception still requires valid home-team references and separate execution readiness; it cannot create a teamless runnable HR employee.
- SRV-31 · Person-started CLI context selects explicit immutable instance and office identities; a managed binding never falls back to another office.
- SRV-32 · The local API is version 1, changes in place until v1 ships and rejects unsupported peers before dispatch. HTTP, launch and sync contracts are independent. Cargo package agentic-org at apps/agentic-org owns the CLI, including setup dispatch; no separate setup package exists.

- SRV-33 · Initial HQ setup automatically verifies components and registers/starts one service. No install/service-install prerequisite or create-or-join question appears. Repeating/resuming setup preserves the same HQ and service. Satellite input in unattended initial configuration is refused before provisioning; an existing satellite is never converted into HQ.
- SRV-34 · setup --bundle <path> --offline verifies a complete local HQ release without any external request or enrollment, including repository fetch/auth probes, provider/model probes and update checks. Validate the mandatory remote organization URL/state branch and local credential references/configuration locally; retain them as remote, with connectivity/publication readiness unverified or pending until online operation is authorized. Background jobs honor offline policy. Untrusted/incomplete bundles and missing local platform prerequisites fail explicitly without fetching. HQ readiness does not assert remote or agent-provider readiness. Missing service-manager support reports foreground serve instructions.
- SRV-35 · Setup/create persists explicit organization slug, remote repository registration and state branch; rejects local-only organization Git, invalid/duplicate slug, invalid/missing branch and invalid local credential references/configuration with actionable safe errors. Offline setup validates these locally without contacting the repository, and reports remote connectivity/publication unverified or pending. Authorized online validation/publication reports remote credential/connectivity failures separately without fabricating readiness or recreating HQ. Lost replies and repeated setup keep the same plan and IDs.
- SRV-36 · Every committed bootstrap employee has one valid home team, including HR and an optional owner-person; team lead is a direct member. Owner authentication/admin rights remain independent from profile flags. Missing team blocks bootstrap completion without selecting an invented HR team.
- SRV-37 · Agent identity is independent from execution profile/model, connection order, credentials and placement. Missing config, stale entitlement/auth, absent shared permit or unverified prior cleanup produces explicit not-ready/recovery-required state. Restart does not launch a duplicate process or allocate replacement placement authority from a cache.
- SRV-38 · A disposable projection rebuild reproduces acknowledged domain/receipt/stream/allocation state from verified Git plus protected sources. Missing/corrupt source, wrong identity/hash/epoch or old high-water fails explicitly; unknown commands are reconciled before retry. Legitimate keys remain necessary; no fabricated usage, receipt, budget capacity or authority.
- SRV-39 · Restore preserves protected chat visibility. Message bodies, counts, previews, attachments and search do not leak to anyone outside a message's audience; shared projection access never grants author-private note access or reveals note existence in shared receipts.
- SRV-40 · Budget policy revisions, one-time extensions, notices and independent manual/security/budget holds survive restart/rebuild. Stop pending remains distinct from acknowledged held; releasing one reason never releases another. Release caps warn only; incomplete usage and cost/amount access scopes (including only the own-agent CLI exception) remain explicit. Runtime evaluation/pausing is unavailable until implemented.
- SRV-41 · The target runtime path layout proves sandbox denies, one office/repository cache, association-specific integration branch and retained worktrees/checkpoints across handoff/placement change. Deletion cannot erase sole acknowledged protected sources, private stores or local-only/unpushed code recovery.


**Sign in, session and first access (SIGN)** (§4a)
- SIGN-1 · Opening any HQ page without a session shows the sign-in page, with Username, Password and Sign in. The page and its responses contain no organization data.
- SIGN-2 · The sign-in page has no "forgot password" link, reset form or sign-up form. It shows `agentic-org account reset-password --name <instance> <username>`, its Linux form with `sudo` in front, and `agentic-org account create --name <instance>`.
- SIGN-3 · Submitting with an empty field sends no request and shows "Enter your username." or "Enter your password." under that field.
- SIGN-4 · A wrong password for an existing username and any password for an unknown username show the same message, "That username and password don't match. Try again.", and HQ answers both with the same status and body. Password is cleared and Username kept.
- SIGN-5 · A name that matches a person record (offices §7.4) but no account, with any password, is refused with SIGN-4's message.
- SIGN-6 · A rate-limited sign-in shows "Too many sign-in attempts." with the retry time when HQ gives one, and keeps Sign in disabled until then; without a time it shows "Wait a few minutes, then try again."
- SIGN-7 · When the loaded sign-in page can't reach HQ, it shows "Can't reach HQ…" with `agentic-org service status --name <instance>` and Try again, and keeps both fields.
- SIGN-8 · A correct sign-in opens the page the user was trying to open, if it is a page of this HQ that the account can open. A return target outside this HQ is ignored, and the last organization or the organization list opens (OPEN-1); a page of an organization the account can't open is ignored too, and the organization list opens.
- SIGN-9 · After sign-in, an account with no organizations sees "No organizations yet." with Sign out if it is an instance admin; add Create organization and "Create one to get started." only when the real flow is available, and "This account doesn't own any organizations on this HQ. Ask an instance admin of this HQ to link it to one." with Sign out if not.
- SIGN-10 · When a session ends while a page is open, the next refused request or stream opens the "Sign in again" dialog over that page; the page doesn't navigate away.
- SIGN-11 · Text typed into a page and not saved is still in its fields after the user signs in again from the dialog, or after Not now and then Sign in, except after an unlink from that page's organization (SIGN-43).
- SIGN-12 · A save refused because the session ended is marked "Not saved", and nothing is resent after signing in until the user saves again.
- SIGN-13 · After signing in again from the dialog, the user is on the same page with focus where it was (except after an unlink from that page's organization, SIGN-43), and live updates resume without a gap (SRV-12).
- SIGN-14 · With two tabs of the same HQ showing the dialog, signing in in one closes the dialog in the other, and the other stays on its page with its text, except when its page belongs to an organization the account no longer owns (SIGN-43).
- SIGN-15 · On a page without editable fields, Not now closes the dialog and shows "You're signed out." with Sign in. On an editable page it shows "You're signed out. Text you haven't saved is still here…" with Sign in (after an unlink from that page's organization, SIGN-43's bar instead); the page sends no changes while signed out.
- SIGN-16 · Sign in as someone else asks before discarding text that isn't saved, then opens the sign-in page.
- SIGN-17 · Withdrawn: a restart of HQ never ends a browser session (ADR-0013); while HQ is down an open page shows LIVE-8's paused line.
- SIGN-18 · The account menu in the top bar shows "Signed in as <username>", "Instance admin" only for an account with the grant, and Sign out. It has no password or account form.
- SIGN-19 · Sign out with text that isn't saved asks "Sign out? Text you haven't saved in any open tab of this HQ will be lost." first. Stay keeps the page.
- SIGN-20 · Sign out ends the session at HQ: every open tab of that HQ shows the sign-in page with "You signed out.", Back shows no organization data, and a session in another browser still works.
- SIGN-21 · First setup asks for a username and the password twice, without echoing it. Different entries show "The passwords don't match. Enter them again." and ask again; a password under the rule shows the rule.
- SIGN-22 · First setup creates one account that is an instance admin and owns the first organization, and ends by printing the browser address and username. Its output, logs and JSON status never contain the password.
- SIGN-23 · Signing in with the setup account at the printed address opens the first organization, with that account as its owner.
- SIGN-24 · A repeated setup asks for no account and prints the address and the existing username.
- SIGN-25 · `agentic-org account create` at the HQ machine asks an instance admin to sign in, then (on a Mac) to confirm it's them, and creates an account that can sign in at the printed address. The account has the admin grant only when the answer to "Make this account an instance admin?" is y, and owns exactly the organizations chosen; the last line says both.
- SIGN-26 · `agentic-org account set-password` asks for the username and current password, then the new one twice. After it, the old password is refused, the new one signs in, and every session of that account has ended: an open page of that account shows "Sign in again".
- SIGN-27 · The sign-in page and the dialog meet §4a.7: labelled fields with username and current-password autocomplete, the stated focus order, errors linked with `aria-describedby` and announced, and no meaning by colour alone.
- SIGN-28 · No browser screen lists, adds or edits accounts, and HQ's HTTP API refuses any request to create an account.
- SIGN-29 · Sign out shows Signing out… while HQ ends the session; when HQ can't be reached it shows "Couldn't sign out: HQ can't be reached…" with Try again, and the page stays signed in.
- SIGN-30 · Each row of §4a.2's account-command table (`account create`, `set-password`, `reset-password`, `link`, `unlink` and `revoke`) shows its copy and exit status. A refusal changes nothing, and a non-admin is refused `link`, `unlink` and `revoke` with "<login> isn't an instance admin, so it can't change accounts".
- SIGN-31 · When a different account signs in from one tab, every other open tab of that HQ shows "You're now signed in as <username> in another tab…" with Reload, and shows no data of the previous account.
- SIGN-32 · `agentic-org account reset-password <username>` at a Mac HQ machine asks for no sign-in, asks for the new password twice and for confirmation in the system dialog, then: the old password is refused, the new one signs in, every session of that account has ended (an open page shows "Sign in again"), a rate-limited sign-in for that account is allowed again, and the reset is recorded with its time and without the password.
- SIGN-33 · On Linux, `agentic-org account reset-password <username>` without root is refused with the `sudo` command and changes nothing. With `sudo`, it resets the password on the instance of the Linux user who ran `sudo` (`--name` picks among that user's instances), and its first line names that user; run as root without `sudo`, it needs `--user <user>`.
- SIGN-34 · `agentic-org account create` and `agentic-org account reset-password` over SSH on a Mac print the "needs you at this Mac" message before any prompt, and change nothing. Over SSH on Linux, with a terminal, both work.
- SIGN-35 · `agentic-org account link <username> --org <organization>` asks an instance admin to sign in, asks a y/N that names the account and the organization, then (on a Mac) asks for confirmation in the system dialog; after y, that account signs in and opens the organization as its owner.
- SIGN-36 · After y, `agentic-org account unlink <username> --org <organization>` leaves that account unable to open the organization, and every session of that account has ended: an open page of it shows "Sign in again".
- SIGN-37 · After y, `agentic-org account revoke <username>` ends every session of that account, and signing in with it is refused with SIGN-4's message.
- SIGN-38 · `agentic-org account create` lists every organization on the instance, and `account link` accepts any of them, including organizations the signed-in admin doesn't own.
- SIGN-39 · Unlinking or revoking an organization's only linked account is refused with "<username> is the only account linked to <organization>…", before the y/N question, and nothing changes.
- SIGN-40 · Revoking the instance's only instance admin is refused with "<username> is the only instance admin of instance <name>…", before the y/N question, and nothing changes.
- SIGN-41 · `agentic-org account link`, `unlink` and `revoke` over SSH on a Mac print the "needs you at this Mac" message before any prompt, and change nothing.
- SIGN-42 · Answering anything but y to `link`, `unlink` or `revoke` prints "Nothing changed." and changes nothing.
- SIGN-43 · With a page of an organization open, after `agentic-org account unlink <username> --org <organization>` for that page's account, the next sign-in from any tab of this HQ, from the "Sign in again" dialog or from a page showing the Not now bar, succeeds and opens the organization list (or §4a.5's no-organization page) with "You no longer own <organization>." above it, not that page, and no text from that page is kept. Focus is on the organization list's heading (or the no-organization page's), and "You no longer own <organization>." is announced through the polite live region. Every other open tab of this HQ on a page of an organization the account no longer owns also opens the organization list with that line, and keeps none of its text, even a tab that chose Not now; tabs on organizations it still owns behave as SIGN-14 says. The dialog shows "You no longer own <organization>, so you were signed out." and warns that signing in, here or in another tab, loses that page's unsaved text only when HQ gives that reason, and after Not now its bar reads "You're signed out. Text you haven't saved here is lost when you sign in, here or in another tab: copy it first."; otherwise it shows its usual copy. On a page without editable fields, omit the unsaved-text sentence and use the simple signed-out bar defined in §4a.4. After `agentic-org account revoke <username>`, signing in again from the dialog is refused with SIGN-4's message, and the page stays as it was.

**Recovery code (RCV)** (§4a.6)
- RCV-1 · The recovery code never passes through the web process or reaches the browser: a fixture code appears in no web request or response, HTTP response, event, page, notice or copied text.
- RCV-2 · While the code is waiting, the organization's owner sees the notice on every page of that organization, with `agentic-org recovery release --org '<organization>'` and Copy command, and no close button. Other accounts don't see it.
- RCV-3 · While the code is waiting, Needs you shows "Collect the recovery code for <organization> on <HQ machine>.", and the item opens the Recovery row. Without Needs you, the notice and the row still show.
- RCV-4 · The Recovery row in Settings › Organization shows the exact copy for each state: being prepared, waiting to be collected, shown but not confirmed, collected on <date>, couldn't be prepared with its reason, and status unknown.
- RCV-5 · No browser state is labelled collected or ready before the acknowledgement is recorded; status unknown is never shown as collected.
- RCV-6 · `agentic-org recovery release` records the acknowledgement only after the typed last group matches and the owner types `yes`.
- RCV-7 · A typed group that doesn't match is refused with "That isn't the last group of the code…" and nothing is recorded.
- RCV-8 · Interrupting the command after the code is shown and before `yes` records no acknowledgement; running it again without `--redeliver` shows no code and gives the `--redeliver` command, and the browser shows Shown, not confirmed.
- RCV-9 · After the acknowledgement of a code that was never shown again, the browser shows "Recovery code collected on <date>" (RCV-23 covers a code shown again) and the notice and Needs you item go away, without a reload while live updates run.
- RCV-10 · After the acknowledgement, `agentic-org recovery release`, with or without `--redeliver`, prints when it was collected and that it can't be shown again, prints no code and exits with an error status.
- RCV-11 · While the package is being created, the browser shows "being prepared" and the command says to try again in a minute, with no code.
- RCV-12 · When the package couldn't be created, the browser and the command show the reason and `agentic-org doctor`, with no code.
- RCV-13 · Interactive setup at the HQ machine (or over SSH on Linux) offers to collect the code once the HQ is created and before it starts the service; answering n leaves it waiting and prints `agentic-org recovery release --org '<organization>'`.
- RCV-14 · An organization created in the browser starts as being prepared, then waiting, and is collected with `--org` naming it.
- RCV-15 · `agentic-org recovery release` run without an interactive terminal prints no code.
- RCV-16 · Every message that reports the code's state (in the notice, the Recovery row, Needs you, setup, a repeated setup, `agentic-org service status`, `agentic-org recovery release` and `agentic-org recovery status`) says that recovering after losing the HQ machine isn't supported yet.
- RCV-17 · CLI output for setup, account and recovery commands carries no meaning by colour alone, and writes no colour codes with `NO_COLOR` set or when output isn't a terminal.
- RCV-18 · After the code is shown and before it's confirmed, the owner sees the Shown, not confirmed row, notice and Needs you item, each with `agentic-org recovery release --redeliver --org '<organization>'`.
- RCV-19 · `agentic-org recovery release --redeliver` before the acknowledgement shows the same code as the first showing, then asks for the same confirmation (last group, then `yes`).
- RCV-20 · Noninteractive setup, and setup over SSH on a Mac, finish with the HQ created, exit successfully, report the recovery code as waiting with no code in any output, and print `agentic-org recovery release --org '<organization>'` to run at the machine.
- RCV-21 · `agentic-org recovery release` over SSH on a Mac prints the "needs you at this Mac" message, shows no code and leaves the state unchanged.
- RCV-22 · `agentic-org recovery release` signed in with an account that isn't an instance admin is refused with "<username> isn't an instance admin…", shows no code and leaves the state unchanged.
- RCV-23 · After an acknowledgement that followed second showings, the Recovery row reads "Recovery code shown on <date> (shown again <n> times, last on <date>; with one second showing: "shown again once, on <date>") · collected on <date>.", with the first-showing, last-second-showing and acknowledgement dates, without a reload while live updates run.
- RCV-24 · `agentic-org recovery status --org '<organization>'`, after an instance admin signs in, shows the state and lists every showing with its date, time, account and whether it was the first showing or shown again; it shows no code and changes nothing.
- RCV-25 · One run of `agentic-org recovery release` asks for one confirmation, before the code is shown, and the system dialog names both showing the code and recording it as stored; after `yes`, the acknowledgement is recorded with no second dialog.
- RCV-26 · A new run of `agentic-org recovery release --redeliver` asks for the confirmation again, even when an earlier run was confirmed.
- RCV-27 · `agentic-org recovery release --redeliver` on a code that was never shown shows the code and records it as the first showing: `recovery status` lists it as shown, not shown again, and the browser's count of second showings stays at none.
- RCV-28 · In interactive setup, when the code was shown and the answer at the last prompt isn't `yes`, the code is Shown, not confirmed, setup prints `agentic-org recovery release --redeliver --org '<organization>'`, and the service still starts.
- RCV-29 · While the code is Shown, not confirmed after second showings, the Recovery row and the notice give the first-showing date, the number of second showings and the last one's date.
- RCV-30 · `agentic-org recovery status` signed in as an account without the instance-admin grant is refused with "<username> isn't an instance admin, so it can't see recovery status…", shows no state or showings, and exits with an error.


## 7. Decisions still requiring implementation evidence

**REC-1 remains open:** a reviewed manual HQ-loss recovery/re-certification/fencing
procedure and disaster-recovery evidence are required before production recovery
claims. V1 still provisions CA escrow and its owner recovery code under SRV-25.

The server and installer document the supported OS and architecture matrix,
minimum versions and Linux service prerequisites. The host's credential custody on
headless Linux, in a container and on signed macOS passes the design's platform
checks. TLS termination and bounded command and event handling need concrete
contracts. Cross-office coding also needs a supported repository transport.
