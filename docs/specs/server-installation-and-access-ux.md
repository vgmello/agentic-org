---
kind: spec
title: "Standalone server: installation, instances and browser access (UX)"
status: for-review
revision: 5
owner: ac-product-designer-eden
created: "2026-11-09"
updated: "2026-11-10"
requested_by: owner
reviewers: [ac-staff-engineer-ada, owner, app-frontend-dev, server-dev, ac-software-engineer-river, ac-software-engineer-remy]
builds_on: [offices-and-teams-ux, agent-run-modes-ux, employee-supervision-ux, processes-ux, chat-ux]
---

# Spec · Standalone server: installation, instances and browser access

## Changelog

**Revision 5 (2026-11-10): recovery release, setup without a local session, and
instance isolation.** From Rowan's X-02 (`org-proto` v2, rev 13, approved by
ada), §6.2 and §6.4, and its Q8 (accepted by ada):
- The recovery command is `agentic-org recovery release` everywhere (was the
  placeholder `recovery-code collect`). A80 resolved.
- Who can show the code: an instance admin, signed in on the HQ machine, who
  confirms it's them (Touch ID or the login password on a Mac, the typed password
  on Linux) (§4a.6).
- The code is shown once. If it was shown but not confirmed as stored,
  `agentic-org recovery release --redeliver` shows the same code again, and
  that second showing is recorded without the code. After the owner confirms it's
  stored, it can't be shown again. New browser state **Shown, not confirmed**,
  and the Collected row names a second showing (§4a.6).
- Noninteractive setup and setup over SSH on a Mac finish with the HQ created
  and the code waiting: a stated exception to §2's "same state transitions"
  (§2, §4a.2, §4a.6).
- SRV-26 states what v1.0 isolates and what it doesn't (the owner via ada, wording
  agreed by Rowan and Eden).
- Setup offers the recovery code once the HQ is created and before it starts the
  service, in X-02 §6.2's bootstrap order (§2, §4a.2, RCV-13). Automation's
  waiting end also departs from `standalone-server.md` §6's protected outputs
  (A104).
Also new: RCV-30 (recovery status refuses a non-admin, with its own copy).
SIGN-8, SIGN-11, SIGN-13, SIGN-14 and SIGN-15 are rewritten (except after an unlink, SIGN-43).

- The account commands follow X-02 §6.2 *(rev 5)*: `agentic-org account add`
  is now `account create`, which an instance admin runs after signing in, and
  which can give the admin grant and link the organizations the account owns.
  `account password <username>` splits in two: `account set-password` changes
  your own password, and `account reset-password <username>` resets a forgotten
  one, with confirmation at the Mac or `sudo` on Linux. Both end every session
  of that account. The sign-in page's help, the commands' failure table and
  §4a.5's no-organization copy follow (§4a.1 to §4a.5).
- *(Review of 07a8a4b)* Setup's recovery block says what each ending leaves: n,
  or no showing, leaves the code waiting; shown without `yes` leaves it Shown,
  not confirmed, and setup still starts the service and prints the
  `--redeliver` command (§4a.2, RCV-28). The sign-in page's mock-up shows the
  full `sudo agentic-org account reset-password …` command. The `--redeliver`
  cancelled-confirmation row ends with the REC-1 sentence.
- *(Rowan's answers, 2026-11-10; X-02 Q9)* A99: the Recovery row and notice give
  the first showing's date, how many times it was shown again and the last one's
  date, and the new `agentic-org recovery status` lists who showed it and when
  (§4a.6). A100: one confirmation per run, naming both showing and recording,
  replaces the two prompts; `--redeliver` on a code never shown is its first
  showing (§4a.6). A103: new `agentic-org account link`, `unlink` and `revoke`,
  with their prompts and the refusals that keep an instance admin and a linked
  account per organization; `account create` lists every organization on the
  instance (§4a.1, §4a.2). A104: Rowan updates `standalone-server.md` §6.
- *(Review of 0802754)* Unlinking and revoking are among the causes of a session
  ending (§4a.4). Signing in again after being unlinked from the page's
  organization opens the organization list with "You no longer own Side
  Project." instead of the page, and text that wasn't saved there is gone; the
  dialog warns first only when HQ gives that reason (A84). A revoked account
  can't sign in again: the dialog shows SIGN-4's message (§4a.2, §4a.4, §4a.5;
  SIGN-43). The revoke example's sam owns only Lumen Labs, as after the unlink
  example. Focus then goes to the organization list's heading, and "You no
  longer own Side Project." is announced (§4a.4, §4a.7). Every other open tab
  on a page of an organization the account no longer owns goes to the list
  too, and its unsaved text is gone. A reload while signed out on such a page
  opens the list, as §4a.5 says (§4a.4; SIGN-8, SIGN-11, SIGN-13, SIGN-14,
  SIGN-43).

**Criteria rewritten (same ID):** SRV-5, SRV-26, SIGN-2, SIGN-8, SIGN-9,
SIGN-11, SIGN-13, SIGN-14, SIGN-15, SIGN-25, SIGN-26, SIGN-30, RCV-2, RCV-4, RCV-6,
RCV-8, RCV-9, RCV-10, RCV-13, RCV-15,
RCV-16, RCV-23 *(rev 5)*. **New:** SIGN-32 to SIGN-43, RCV-18 to RCV-30. Open questions
A99, A100, A103 and A104; A80, A85 and A86 resolved, then A99, A100 and A103
resolved by Rowan's answers; A81, A88 and A94 updated.

**Revision 4 (2026-11-10): sign in, first access and recovery code.** New §4a,
built on the owner's decisions O-1 (accounts) and O-2 (recovery-code delivery):
who can sign in, the setup transcript for the first account, the sign-in page
and its errors, session end and signing in again, sign out, what opens after
sign-in, and the recovery-code states in the browser and the CLI, with their
accessibility. Open questions A80 to A90, A93 and A94 for ada (A91, A92 and A95 are in offices) (§4a.9). §4 points to
§4a. **Criteria added:** SIGN-1 to SIGN-31, RCV-1 to RCV-17. No existing
criterion changed.

Revisions 1 to 3 came from PR #5 and have no entries here.

## 0. Scope and decisions

The owner changed the v1 direction on 2026-11-09: Agentic Org has never been
deployed. Build the new architecture directly and delete obsolete desktop paths.
There is no requirement to migrate the old app, import its data, preserve its IPC
API, or maintain two application transports. This does not authorize installers
or tests to erase existing user directories.

V1 is a standalone Rust service on macOS and Linux. HQ serves the static SvelteKit
application and its HTTP API. Satellites run their own offices and agents and
connect outward to HQ. Opening the website never installs or enrolls an office.
Tauri, the tray, native notifications, global shortcuts and an embedded desktop
window are v2 features. Any future desktop window uses the same web application
and HTTP API; it adds no second implementation of organization behavior.

The implementation proposal is [standalone-server.md](../design/standalone-server.md),
the decision is [ADR-0005](../adr/0005-standalone-server-v1.md), and the
[removal inventory](../design/standalone-server-removal-inventory.md) names the
existing code to delete, retain or replace. CLI spelling below is proposed,
not a claim that these commands exist on the inspected branch.

The owner's naming amendment uses `org-ui` / `apps/org-ui/` for the browser
application and `org-server` / `apps/org-server/` for the Rust worker app, package,
and binary. See [project structure](../design/project-structure.md) for the proposed
folder layout and reorganization guidelines. The setup/release command remains
provisionally `agentic-org`; office roles and the per-office process model remain.
These source renames are implementation acceptance, not changes performed by this PR.

## 1. Install once; select an instance

A small bootstrap command selects the platform release, downloads and verifies
one release executable, then hands off to that executable's setup command.
The executable contains the server payload, frontend assets and Agentic Org
helpers; setup may unpack them locally. Users do not build the application or
install a JavaScript server to run it.

| Invocation | New instance | Existing instance |
| --- | --- | --- |
| `agentic-org setup` | Configure `default`, then start it | Reuse `default`; start if stopped; otherwise show status and URL |
| `agentic-org setup --name lab` | Configure and start `lab` | Reuse `lab` with the same identity, configuration and data |
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
2. **Create an organization (HQ)** or **Join an organization (satellite)** for its
   first office. An instance may later host more than one office. Each office's
   role is persisted independently.
3. HQ instance-admin/owner access, gateway listening/TLS configuration, and protected
   CA escrow/recovery-code provisioning (setup offers the code once the HQ is
   created, before step 5 starts the service, as in X-02 §6.2; collecting it and
   acknowledging its off-HQ retention can be deferred: §4a.2, A94); or the
   satellite invitation, destination identity and enrollment confirmation.
4. Runtime selection, dependency checks and authentication readiness.
5. Service registration, start and a summary with separate readiness states.

Automation uses the same validation and state transitions, with one stated
exception *(rev 5, X-02 Q8)*: noninteractive setup, and setup over SSH on a Mac,
finish with the HQ created and its recovery code waiting, because showing the
code needs someone at the HQ machine (§4a.2, §4a.6). This also departs from
`standalone-server.md` §6 ("CA escrow and the explicit recovery gate"), which
says "Automation uses separate protected outputs": there is no protected output
for the code, and automation never receives it. Updating the design to match is
A104. Missing mandatory
input causes a structured failure instead of waiting for stdin. Secrets enter
through a protected descriptor/file or a dedicated stdin option, not arguments
or ordinary configuration fields. Machine-readable output contains stable error
codes and status fields, never credentials. An unattended enrollment can remain
`awaiting_approval`; setup must distinguish that from an active satellite. Owner
approval is still required by the enrollment policy, even without a wizard.

The default service runs as its configured user, not root. macOS uses a per-user
launchd service; Linux uses an appropriate systemd user service, with foreground
execution available when a user service manager is absent. Logout/reboot behavior
and credential availability are reported honestly. Running after logout on Linux
may require explicitly configured lingering; setup does not silently enable it.
Before-login macOS execution is outside this v1 default.

## 3. Files, identity and tools

The instance owns its configuration, office folders, encrypted databases, runtime
state and agent homes. Executables live separately from mutable data. Each agent
has a stable identity and server-provisioned configuration and workspaces. Local
paths are resolved on the office machine; no absolute path from another machine
becomes an agent's identity or a browser-selected storage root.

The web UI may display an authorized user's server paths read-only. There is no
folder picker, directory relocation setting, old-team scanner or automatic file
upload in v1. The server writes its own data and agents write only their permitted
workspaces. Instance credentials and another employee's private files are not
part of an agent's writable configuration. The [employee supervision requirements](employee-supervision-ux.md),
including bounded delegates and process-driven behavior, apply at the owning office;
moving access to HTTP does not turn those into browser responsibilities.

Setup installs `org-agent` from the same release. It locates or installs only the
selected supported harnesses, checks their versions and reports their sign-in
state. Git is a prerequisite for coding workflows. GitHub CLI is conditional on
the GitHub feature, which remains v2 in the existing project design. The old
`org-relay` alias is not required for a fresh install. Server readiness and runtime
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

Satellite enrollment reuses the invitation, HQ identity verification, owner
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

## 4a. Sign in, first access and recovery code *(rev 4)*

This section builds on the owner's decisions of 2026-11-10: **O-1** (accounts)
and **O-2** (recovery-code delivery). The recovery and account commands follow
X-02 §6.2 and §6.4 (A80, resolved); other commands are proposed spellings, like
the rest of this spec. Placeholders in angle brackets, such as `<HQ machine>`,
are filled from HQ's data. Questions for ada are in §4a.9.

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
- Whoever creates an organization owns it ([offices §5a](offices-and-teams-ux.md#5a-create-organization-instance-admin-only-rev-14)).
- **Only accounts sign in.** People in an organization's People view
  ([offices §7.4](offices-and-teams-ux.md#74-add-edit-and-remove-a-person)) are
  records that agents can reach. They never sign in, and nothing invites them.
  Agents never sign in to the browser either.
- In v1.0 another account is added only on the HQ machine, by an instance
  admin, with `agentic-org account create` *(rev 5)*. It can make the new
  account an instance admin, and it links the account to the organizations it
  owns. No browser screen lists, adds or edits accounts. If a later screen lists
  accounts, it shows that command and has no form.
- Later, on the HQ machine, an instance admin links an account to an
  organization (`agentic-org account link`), unlinks it (`account unlink`), or
  revokes the account (`account revoke`), which signs it out everywhere and
  disables it (§4a.2) *(A103, Rowan, 2026-11-10; X-02 Q9)*. These, and `account
  create`, can name any organization on this instance, not only the admin's own.
- Two rules always hold, and a command that would break one is refused: the
  instance keeps at least one instance admin, and every organization keeps at
  least one linked account (X-02 §6.2).
- Passwords are changed only on the HQ machine *(rev 5)*:
  - `agentic-org account set-password` changes your own password. It asks for
    the current one.
  - `agentic-org account reset-password <username>` is the way back from a
    forgotten password. It needs no sign-in. On a Mac it needs someone at the
    HQ machine to confirm it's them (Touch ID or the Mac's login password); on
    Linux it needs root (`sudo agentic-org account reset-password <username>`).
  - Either one ends every session of that account, in every browser (X-02 §4.1,
    §6.2).

  The browser has no "forgot password" flow and no form to change a password.
- The password is checked with a maintained library (O-1). The page never
  says whether the username or the password was wrong.

### 4a.2 First owner access

Setup creates the account in its step 3 (§2). The transcript shows only the
lines that matter here; `…` stands for the other steps.

```
$ agentic-org setup
Setting up instance "default".
…
Create or join an organization?
  1) Create an organization. This machine becomes its HQ.
  2) Join an organization with an invitation.
Choose 1 or 2: 1
Organization name: Lumen Labs

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
  "at least 12 characters" (proposed, A81). Too short: "Use at least 12
  characters." If the two entries differ: "The passwords don't match. Enter
  them again." Setup then asks for both again.
- Setup never prints the password, and never puts it in a command argument,
  a log or JSON status (SRV-5). Noninteractive setup takes it through the
  protected input in §2.
- The last line prints the configured browser address and the username.
- **Order** *(rev 5, X-02 §6.2 bootstrap steps 4 and 5)*: setup creates the
  account and the HQ, then offers the recovery code, and only after that
  registers and starts the service. So the recovery block comes before
  "Service running", and HQ isn't open in a browser until the code has been
  collected, left waiting, or shown but not confirmed.
- **Collect it now? Y** runs the same steps as `recovery release` (§4a.6),
  using the account just created, so it doesn't ask to sign in again. It still
  asks the person to confirm it's them, once (§4a.6).
- **What setup leaves behind** *(rev 5, review of 07a8a4b)*. In every case below
  setup goes on, registers and starts the service, and ends with "HQ ready": X-02
  §6.2 doesn't make starting the service depend on the acknowledgement.
  - **n**, or the confirmation cancelled or failed, so the code wasn't shown:
    the code is **Waiting to be collected**, and setup prints the
    `agentic-org recovery release --org '<organization>'` command, as in the
    transcript above.
  - **The code was shown, and the answer at the last prompt wasn't `yes`:** the
    code is **Shown, not confirmed** (§4a.6, RCV-8, X-02 §6.4). Setup prints the
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

  Finishing setup without the acknowledgement is a **deliberate departure** from
  `standalone-server.md` §6, which says
  setup "requires acknowledgement": a first-time owner shouldn't be blocked from
  reaching the browser, and the code stays waiting with a notice until collected.
  ada decides whether to accept it and update the design, or to make the
  acknowledgement part of setup (A94).
- **A repeated setup** (SRV-2) asks for no account. It ends with "HQ is running.
  Open <address> and sign in as alex." While the code is waiting, it also prints
  the `agentic-org recovery release` command. While it was shown but not
  confirmed (§4a.6), it prints the same command with `--redeliver`.
- **Setup without a local session** *(rev 5, X-02 Q8)*. Noninteractive setup,
  and setup run over SSH on a Mac, can't show the code: showing it needs someone
  at the HQ machine (§4a.6). They don't ask "Collect it now?". They finish with
  the HQ created and the code **waiting**, which isn't a failure: setup exits
  successfully, and its status output reports HQ ready and the recovery code
  waiting (`pending`), never the code. This is a stated exception to §2's "same
  state transitions", and a departure from `standalone-server.md` §6, where
  automation gets the code through a separate protected output: here it never
  gets the code (A104). The owner collects it later with `agentic-org recovery
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

**Other account commands (HQ machine only)** *(rev 5, X-02 §6.2)*. They match
X-02's host operations: `account create` is `account.create`, `account
set-password` is `account.set_password`, and `account reset-password` is
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
  signed-in admin doesn't own *(A103, Rowan, 2026-11-10; X-02 Q9)*. With no organizations to list, it
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
  (X-02 §4.1). An open page shows "Sign in again" (§4a.4).

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
  "Too many sign-in attempts" no longer applies (X-02 §6.2).
- On a Mac the system dialog reads "reset the password for alex". Over SSH, or
  with no session at the Mac's screen, it stops before asking for a password
  (the table below).
- **On Linux,** run it with `sudo` as the Linux user who runs the instance. It
  finds that user's instance; `--name` picks among that user's instances, after
  the command as everywhere: `sudo agentic-org account reset-password --name lab
  alex`. Signed in as root without `sudo`, name that user with `--user <user>`.
  The first line names the Linux user, so a reset never lands on another user's
  instance unnoticed.
- Without `sudo` on Linux, a forgotten password can't be reset. X-02's only
  other way back is removing the instance and its data (§5). No copy suggests
  that; the not-root error says to use `sudo`.

**Link, unlink and revoke an account** *(rev 5; A103, Rowan, 2026-11-10; X-02
§6.2 and Q9)*. They match X-02's `account.link`, `account.unlink` and
`account.revoke`. Each one asks an instance admin to sign in, then asks y/N in
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
  the signed-in admin doesn't own (X-02 Q9). `--org` takes the organization's
  name, quoted for the shell as in §4a.6.
- `revoke`'s question names the account and every organization it owns; an
  account that owns none reads "sam doesn't own any organization." The dialog
  reads "revoke the account sam".
- On a Mac the dialogs read "link the account sam to Side Project" and "unlink
  the account sam from Side Project". On Linux there's no dialog: the password
  typed at sign-in confirms it's you, so the "Confirm it's you" line doesn't
  appear. Over SSH on a Mac they stop before any prompt (the table below).
- Unlinking and revoking end every session of that account (X-02 §4.1), so an
  open page of that account shows "Sign in again" (§4a.4). Revoking the account
  you signed in with ends your own sessions too. After an unlink, signing in
  again on a page of the unlinked organization opens the organization list
  instead of that page; a revoked account can't sign in again (§4a.4,
  SIGN-43) *(review of 0802754)*.
- Signing in with a revoked account shows the sign-in page's usual "That
  username and password don't match. Try again." (§4a.3), so the page never
  says an account exists or was revoked.
- **The two rules** (§4a.1). A command that would leave the instance with no
  instance admin, or an organization with no linked account, is refused before
  the y/N question, and nothing changes (the table below).

**When account commands fail** (nothing is changed in any case) *(rev 5)*:

| Case | CLI output | Exit |
| --- | --- | --- |
| `create`, `set-password`, `link`, `unlink`, `revoke`: wrong username or password at sign-in | "That username and password don't match. Try again." Asks again. Too many tries: "Too many sign-in attempts. Try again in a few minutes." | — / Error |
| `create`: the account that signed in isn't an instance admin | "sam isn't an instance admin, so it can't add accounts. Sign in as an instance admin of instance default." | Error |
| `link`, `unlink`, `revoke`: the account that signed in isn't an instance admin *(A103)* | "sam isn't an instance admin, so it can't change accounts. Sign in as an instance admin of instance default." | Error |
| `link`, `unlink`, `revoke`: anything but y at the question *(A103)* | "Nothing changed." | Error |
| `link`, `unlink`, `revoke`: unknown username *(A103)* | "There's no account called sam on instance default." | Error |
| `link`, `unlink`: unknown organization *(A103)* | "There's no organization called "Side Project" on instance default." | Error |
| `link`, `unlink`: no `--org` *(A103)* | "Name the organization: agentic-org account link <username> --org <organization>" (`unlink` in the unlink command) | Error |
| `link`: already linked *(A103)* | "sam is already linked to Side Project. Nothing changed." Asks nothing else. | Success |
| `unlink`: not linked *(A103)* | "sam isn't linked to Side Project. Nothing changed." | Error |
| `unlink`: the organization's only linked account *(A103)* | "sam is the only account linked to Side Project, so it can't be unlinked: every organization keeps at least one. Link another account first: agentic-org account link <username> --org 'Side Project'" | Error |
| `revoke`: the only linked account of an organization *(A103)* | "sam is the only account linked to Side Project, so it can't be revoked: every organization keeps at least one. Link another account to Side Project first: agentic-org account link <username> --org 'Side Project'" (it names every such organization) | Error |
| `revoke`: the instance's only instance admin *(A103)* | "sam is the only instance admin of instance default, so it can't be revoked: the instance keeps at least one. Add another instance admin first with agentic-org account create." | Error |
| `revoke`: already revoked *(A103)* | "sam is already revoked. Nothing changed." | Error |
| `create`: username already used | "There's already an account called sam. Choose another username." Asks again. Usernames that differ only in capital letters count as the same (X-02 §6.2). | — |
| `create`: empty username | "Enter a username." Asks again. | — |
| `create`: anything but y, n or Enter at the admin question | "Type y or n." Asks again. | — |
| `create`: an organization number that isn't in the list | "Type numbers from the list, separated by commas, or press Enter for none." Asks again. | — |
| `create`, `reset-password`, `link`, `unlink`, `revoke` on a Mac: the confirmation is cancelled or fails (the system dialog) | "Not confirmed, so nothing was changed. Run this command again when you're at the machine." | Error |
| `create` over SSH on a Mac, or no session at the Mac's screen | "Adding an account needs you at this Mac, to confirm it's you with Touch ID or the login password, and that can't be done over SSH. Run this command at the Mac itself or through Screen Sharing." Shown before any prompt. | Error |
| `link`, `unlink`, `revoke` over SSH on a Mac, or no session at the Mac's screen *(A103)* | "Changing an account needs you at this Mac, to confirm it's you with Touch ID or the login password, and that can't be done over SSH. Run this command at the Mac itself or through Screen Sharing." Shown before any prompt. | Error |
| `reset-password` over SSH on a Mac, or no session at the Mac's screen | "Resetting a password needs you at this Mac, to confirm it's you with Touch ID or the login password, and that can't be done over SSH. Run this command at the Mac itself or through Screen Sharing." Shown before any prompt. | Error |
| `reset-password` on Linux without root | "Resetting a password on Linux needs root. Run it with sudo: sudo agentic-org account reset-password alex" (with `--name lab` for a named instance). | Error |
| `reset-password` as root, not through `sudo`, without `--user` | "Name the Linux user who runs the instance: agentic-org account reset-password --user <user> alex" | Error |
| `reset-password` on Linux: that user has no instance by that name | "Linux user ops has no instance called default." | Error |
| `reset-password` on Linux: the instance's files aren't owned by that user | "Instance default's files aren't owned by ops, so nothing was changed. As ops, run agentic-org doctor." | Error |
| `reset-password` without a username | "Name the account: agentic-org account reset-password <username>" | Error |
| `reset-password`: unknown username | "There's no account called sam on instance default." | Error |
| Password too short (any command) | "Use at least 12 characters." Asks again. | — |
| The two password entries differ | "The passwords don't match. Enter them again." Asks for both again. | — |
| Instance not running, or the host can't be reached | "Instance default isn't running. Start it with agentic-org start, then run this command again." For a named instance: "Instance lab isn't running. Start it with agentic-org start --name lab, then run this command again." Under `sudo`: "Instance default of Linux user ops isn't running. As ops, start it with agentic-org start, then run this command again." | Error |
| Not an interactive terminal, and no protected password input given | "Run this in a terminal, or pass the password through the protected input (§2)." | Error |

`agentic-org start` is §5's. A86 (the admin grant) and A85 (sessions after a
password change) are resolved by X-02 (§4a.9).

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
  for width. *(rev 5)* The reset help names the Linux form too, because the
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
| HQ unreachable | "Can't reach HQ. Check that the instance is running on the HQ machine (`agentic-org status --name <instance>`), then try again." **Try again**. Both fields are kept. |
| HQ error | "HQ couldn't sign you in because of a problem on the HQ machine. Try again. If it keeps happening, run `agentic-org doctor --name <instance>` there." **Try again**. |
| Page out of date | "This page is out of date. Reload to get the current version." **Reload**. (API and page versions differ, standalone design §5.) |
| Signed in | Opens what §4a.5 says. |

A note above the form says why the visitor is here, when the page knows:

| Arrived because | Note |
| --- | --- |
| They signed out | "You signed out." |
| HQ restarted (when HQ says so, A84) | "HQ restarted, so everyone was signed out. Sign in again." |
| Their session ended for any other reason, or the reason is unknown | "Your session ended. Sign in again." |

When HQ is stopped, the browser can't load this page at all and shows its own
error. The table covers a page that loaded and then can't reach HQ.

### 4a.4 Session end, signing in again, and sign out

**When a session ends.** It expires (A83), the account's password is changed or
reset *(rev 5: X-02 §4.1, §6.2; A85 resolved)*, the account is unlinked from
an organization or revoked (`agentic-org account unlink` or `revoke`, §4a.2;
X-02 §4.1) *(review of 0802754)*, or HQ
restarts, if the gateway ends sessions when its listener stops (the plan's
starting point; not yet decided in `standalone-server.md` §6, A84). Signing out
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
  request or the event stream because the session ended. Its first line depends
  on the reason, when HQ gives one (A84):
  - HQ restarted: "HQ restarted, so you were signed out. Sign in again to carry on."
  - Any other reason, or unknown: "Your session ended. Sign in again to carry on."
- While HQ is stopped or restarting, the page shows LIVE-8's "Live updates
  paused" line, as for any lost connection. The dialog appears once HQ answers
  again.
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
    because the account was unlinked from this page's organization (A84). Its
    first line then reads "You no longer own Side Project, so you were signed
    out." and its second "Signing in, here or in another tab, opens your list of
    organizations. Text you haven't saved on this page will be lost: choose Not
    now to copy it first." After Not now, the bar reads "You're signed out. Text
    you haven't saved here is lost when you sign in, here or in another tab:
    copy it first." *(review of 18d3f56)*
  - Without that reason, the dialog can't know, so it doesn't warn: it shows its
    usual copy.
- **A revoked account can't sign in again.** The dialog shows SIGN-4's message,
  "That username and password don't match. Try again." (§4a.3), as for any wrong
  password, so it never says the account was revoked (§4a.2; X-02 §4.1).
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
  the session stays valid until HQ ends it or it expires (A83).

### 4a.5 After sign-in: what opens

1. The page the user was trying to open, if it is a page of this HQ. Any other
   return target is ignored. A page of an organization the account can't open
   is ignored too, and the organization list opens instead. When the "Sign in
   again" dialog was open over that page, the list shows "You no longer own
   <organization>." above it, with the name the page was showing (§4a.4)
   *(review of 0802754)*.
2. Otherwise the last authorized organization, or the organization list (OPEN-1).
3. An account with no organizations sees:
   - Instance admin: "No organizations yet. Create one to get started."
     **Create organization** (offices §5a).
   - Not an admin: "This account doesn't own any organizations on this HQ.
     Ask an instance admin of this HQ to link it to one." **Sign out**.
     *(rev 5)* The admin grant alone opens no organization (X-02 §6.1; A86
     resolved). An admin links an existing account with `agentic-org account link
     <username> --org <organization>` (§4a.2; A103 resolved).

### 4a.6 Recovery code

The recovery code, with the organization's encrypted recovery package, will let
the owner recover the organization's certificate authority if the HQ machine is
lost. Under O-2 the host generates the code, the worker creates the authority and
the encrypted package, and the host keeps the code until it is collected with
`agentic-org recovery release` on the HQ machine. This is the same for every
organization: the first one from setup, and each one created in the browser.
**The code never passes through the gateway or the browser.** It shows only its
state.

**Who can show the code** *(rev 5, X-02 §6.2, §6.4)*. Only an **instance admin**,
on the HQ machine, in a session at that machine:
- `agentic-org recovery release` first asks the admin to sign in with their
  username and password. Setup's own offer (§4a.2) uses the account it just made.
- Then it asks them to confirm it's them, **once per run**, before it shows the
  code *(rev 5; A100, Rowan, 2026-11-10; X-02 Q9)*. That one confirmation covers
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

**Shown once, and again only until it's confirmed** *(rev 5)*. The command shows the code
once. If the owner hasn't confirmed it's stored (the run was interrupted, or the
answer wasn't `yes`), `agentic-org recovery release --redeliver` shows **the
same code** again. That second showing is recorded with who ran it and when,
never the code, and the browser says so (below). Once the owner confirms it's
stored, the host erases its copy and the code can't be shown again, with or
without `--redeliver`.

**What HQ records** *(rev 5; A99, Rowan, 2026-11-10; X-02 Q9)*. The recovery
state carries when the code was first shown, when it was last shown again and
how many times, and when it was confirmed as stored. Every showing is recorded
in the instance's journal and audit log with the account and the time, never
the code. The browser shows only the dates and the count (below). Who showed it
is visible only with `agentic-org recovery status` on the HQ machine (below).

**Every message that reports the code's state** (collected, waiting, shown but
not confirmed, being prepared, couldn't be prepared, not recorded), wherever it appears (the
notice, the Recovery row, Needs you items, setup's recovery block, a repeated
setup's reminder, `agentic-org status`, `agentic-org recovery release` and
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
| Shown, not confirmed *(rev 5)* (shown on the HQ machine, not yet confirmed as stored) | "Recovery code: shown on <HQ machine> on <date>, but not confirmed as stored. To show it again and confirm, run `agentic-org recovery release --redeliver --org '<organization>'` on <HQ machine>." After a second showing *(A99)*: "Recovery code: shown on <HQ machine> on <date> (shown again 2 times, last on <date>), but not confirmed as stored. To show it again…" **Copy command** (quoted as above) | The notice below, in its "shown, not confirmed" form | "Confirm the recovery code for <organization> is stored, on <HQ machine>." Opens the Recovery row. |
| Collected | "Recovery code collected on <date>." After a second showing *(rev 5, A99)*: "Recovery code shown on <date> (shown again 2 times, last on <date>) · collected on <date>." | None | None (the item clears) |
| Couldn't be prepared | "Recovery code: couldn't be prepared. <reason>. On <HQ machine>, run `agentic-org doctor`." **Copy command** | "Recovery material for <organization> couldn't be prepared. See Settings › Organization." | "Recovery material for <organization> couldn't be prepared." |
| Status unknown (HQ didn't say) | "Recovery code: status unknown." Never shown as collected. | None | None |

- The command includes `--name <instance>` when the instance isn't `default`.
- Dates are in the browser's date format, for example "10 Nov 2026" *(rev 5,
  A99)*: "shown on <date>" is when the code was first shown, "last on <date>"
  when it was last shown again, and "collected on <date>" when it was confirmed
  as stored. "shown again 2 times" is the number of second showings; one reads
  "shown again once, on <date>". When HQ doesn't give a date, the row leaves out
  that "on <date>", never guesses it.
- `<HQ machine>` is the name HQ reports for its machine (A93).
- When the signed-in owner isn't an instance admin, the Waiting and Shown, not
  confirmed rows add: "An instance admin of this HQ runs it." *(rev 5)*
- **Copy command** copies only the command. No state copies or shows the code.
- The browser state changes live when the code is shown, shown again or
  collected *(rev 5)*: the notice and Needs you item change, or go away once it's
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

In the Shown, not confirmed state *(rev 5)*:

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
stored." *(A99)*

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

Recovery package: <where it is, or how to copy it off this machine, A90>

Showing the code is recorded, with your username and the time. The code
itself is never recorded.
Press Enter to show the code.
Confirm it's you in the system dialog (Touch ID or this Mac's login password).

Recovery code: <the code, in groups separated by dashes, A90>

To confirm you stored it, type the last group of the code: 9FXM
Are the code and the encrypted recovery package stored away from this machine?
Type yes to confirm: yes

Recorded on 10 Nov 2026 at 14:02. This code won't be shown again.
Recovering an organization after losing the HQ machine isn't supported yet.
```

- The code is shown, and counts as shown, only after Enter and the confirmation.
  Stopping before that shows nothing and leaves the code waiting.
- *(A100)* The one confirmation, after Enter, covers showing the code and
  recording that it's stored: `yes` records it with no second dialog. The
  system dialog reads "show the recovery code for Lumen Labs and record it as
  stored".
- On Linux there's no system dialog: the password typed at sign-in confirms it's
  you, so the "Confirm it's you" line doesn't appear.

**Showing it again** *(rev 5)*, when it was shown but not confirmed as stored:

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
  *(A100)*. The system dialog reads "show the recovery code for Lumen Labs again
  and record it as stored".
- `--redeliver` while the code is still waiting (never shown) is accepted as the
  first showing *(A100, Rowan, 2026-11-10; X-02 Q9)*: the run looks and reads
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
| Anything but `yes` at the last prompt *(rev 5)* | "Not recorded. Once the code and the recovery package are stored away from this machine, show the code again and confirm: agentic-org recovery release --redeliver --org 'Lumen Labs'. Recovering an organization after losing the HQ machine isn't supported yet." The code is now **Shown, not confirmed**. | Error |
| Interrupted before the code is shown (Ctrl-C at sign-in or at "Press Enter") *(rev 5)* | "Not shown. The code is still waiting; run this command again to see it. Recovering an organization after losing the HQ machine isn't supported yet." Nothing changes. | Error |
| `--redeliver`: interrupted before the code is shown again (Ctrl-C at sign-in or at "Press Enter") *(rev 5)* | "Not shown again. Nobody has confirmed the code is stored yet. To show it again and confirm, run: agentic-org recovery release --redeliver --org 'Lumen Labs'. Recovering an organization after losing the HQ machine isn't supported yet." Nothing changes: the code stays **Shown, not confirmed**. | Error |
| Interrupted after the code is shown, before `yes` (Ctrl-C, closed terminal) *(rev 5)* | On Ctrl-C: "Not recorded. To show the code again and confirm, run: agentic-org recovery release --redeliver --org 'Lumen Labs'. Recovering an organization after losing the HQ machine isn't supported yet." A closed terminal prints nothing. Either way the code is **Shown, not confirmed**. | Error |
| Run without `--redeliver` after the code was shown but not confirmed *(rev 5)* | "The recovery code for Lumen Labs was shown on 10 Nov 2026 at 14:02, but nobody confirmed it's stored. To show it again, run: agentic-org recovery release --redeliver --org 'Lumen Labs'. Recovering an organization after losing the HQ machine isn't supported yet." No code. | Error |
| Already collected, with or without `--redeliver` *(rev 5)* | "The recovery code for Lumen Labs was collected on 10 Nov 2026. It can't be shown again. Recovering an organization after losing the HQ machine isn't supported yet." No code. | Error |
| Wrong username or password at sign-in *(rev 5)* | "That username and password don't match. Try again." Asks again. Too many tries: "Too many sign-in attempts. Try again in a few minutes." No code. | — / Error |
| The account isn't an instance admin *(rev 5)* | "sam isn't an instance admin, so it can't show recovery codes. Sign in as an instance admin of instance default." No code. | Error |
| The confirmation is cancelled or fails (the system dialog) *(rev 5)* | "Not confirmed, so the code wasn't shown. Run this command again when you're at the machine." No code; nothing changes. It's the run's only confirmation, so there's none to cancel after the code is shown *(A100)*. | Error |
| `--redeliver`: the confirmation is cancelled or fails (the system dialog) *(rev 5)* | "Not confirmed, so the code wasn't shown again. Nobody has confirmed it's stored yet. When you're at the machine, run: agentic-org recovery release --redeliver --org 'Lumen Labs'. Recovering an organization after losing the HQ machine isn't supported yet." No code; nothing changes: the code stays **Shown, not confirmed**. | Error |
| Over SSH on a Mac, or no session at the Mac's screen *(rev 5)* | "Showing the recovery code needs you at this Mac, to confirm it's you with Touch ID or the login password, and that can't be done over SSH. Run this command at the Mac itself or through Screen Sharing." No code. | Error |
| Still being prepared | "Recovery material for Lumen Labs is still being prepared. Try again in a minute. Recovering an organization after losing the HQ machine isn't supported yet." | Error |
| Couldn't be prepared | "Recovery material for Lumen Labs couldn't be prepared: <reason>. Run agentic-org doctor. Recovering an organization after losing the HQ machine isn't supported yet." | Error |
| No `--org`, one organization on the instance | Collects that organization's code. | — |
| No `--org`, several organizations | "This instance has several organizations. Run again with --org and one of: Lumen Labs, Side Project." | Error |
| Unknown organization | "There's no organization called "Lumen Labs" on instance default." | Error |
| Not an interactive terminal | "The recovery code is shown only in an interactive terminal." No code. Automation never gets the code: noninteractive setup ends with it waiting (§4a.2, X-02 Q8). | Error |
| Instance not running, or the host can't be reached | "Instance default isn't running. Start it with agentic-org start, then run this command again." For a named instance: "Instance lab isn't running. Start it with agentic-org start --name lab, then run this command again." No code. | Error |

- `agentic-org status` lists each organization's recovery state in the same
  words as the Settings row, never the code.
- An organization created in the browser starts as **Being prepared**, then
  **Waiting to be collected**. An instance admin collects it with the same
  command, naming that organization.

**Who showed it, and when** *(rev 5; A99, Rowan, 2026-11-10; X-02 Q9)*.
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
- It needs an instance admin's sign-in, like `recovery release` (X-02 §6.2's
  `recovery.*` are admin only), but no confirmation that it's you, so it works
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

### 4a.8 Out of scope in v1.0

Inviting people to sign in, account screens in the browser, password change or
reset in the browser, sign-in with another provider, two-step sign-in, showing
the recovery code in the browser, and recovering after losing HQ (REC-1).

### 4a.9 Open questions for ada

- ~~A80~~ · *Resolved (Rowan, X-02 §6.2 and §6.4, approved by ada; rev 5):* the
  recovery command is `agentic-org recovery release --org <organization>`, with
  `--redeliver` for a second showing before the code is confirmed. Noninteractive
  setup has no protected output for the code: it ends with the code waiting, and
  the owner collects it later at the machine (X-02 Q8, §4a.2). The account
  commands follow X-02 §6.2 too: `agentic-org account create`, `account
  set-password` and `account reset-password <username>` (§4a.2).
- A81 · Username rules (length, characters, whether uniqueness ignores case) and
  the password rule. I've proposed at least 12 characters and no other rule.
  *(rev 5)* Partly answered: X-02 §6.2 normalizes usernames (NFC, case-folded)
  and keeps them unique, so uniqueness ignores case. Length, characters and the
  password rule are still open.
- A82 · Sign-in rate limit: per username, per source address or both, the limits,
  and whether the refusal carries a retry time the page can show.
- A83 · Session lifetime: idle and absolute expiry, whether activity extends it,
  and whether an idle expiry should also hide what the page shows (§4a.4 keeps
  the page visible).
- A84 · Does an HQ restart end every session (the plan's starting point for B1-06),
  or do sessions survive it? When HQ refuses a request because the session ended,
  does it say why (expired, signed out, HQ restarted, password changed, and,
  *(review of 0802754)*, unlinked from which organization)? Without a reason the
  page uses the generic copy. Is that reason safe to show before sign-in?
- ~~A85~~ · *Resolved (Rowan, X-02 §4.1 and §6.2, approved by ada; rev 5):* yes.
  `account reset-password` ends every session of that account, and so does
  `account set-password`: X-02 ends a session on any password change (§4a.2,
  §4a.4).
- ~~A86~~ · *Resolved (Rowan, X-02 §6.1 and §6.2, approved by ada; rev 5):*
  `account create` can give the admin grant (`admin`), and links the new account
  to the organizations it owns (`owner_of`). A second account can be linked to
  an existing organization: at creation, or later with X-02's admin-only
  `account.link`, the command `agentic-org account link` (A103, resolved). Several accounts can link to one organization. The
  admin grant alone opens no organization: the reads and grants cover linked
  organizations only (§4a.5).
- A87 · The owner bar (offices §7.1) shows the owner's name. Does setup ask for a
  display name, or does the owner bar show the username?
- A88 · Which read gives the browser the recovery state (being prepared, waiting,
  collected with its date, couldn't be prepared with a reason), and is it
  limited to the owner? *(rev 5)* Partly answered: X-02 §6.1's
  `organization.list` gives linked organizations `recovery: pending | delivered |
  acknowledged`. Being prepared and couldn't be prepared are still open. The
  dates and the count of second showings are answered (A99, resolved).
- A89 · If the recovery package can't be created, does the create fail
  (offices §5a), or is the organization created with recovery "couldn't be
  prepared"? What repairs it: a host restart, `doctor`, or a command?
- A90 · The acknowledgement covers keeping the encrypted package off HQ too. How
  does the owner get a copy of the package? And what does the code look like
  (length, groups), so the CLI can ask for its last group?
- A93 · Which name for the HQ machine does HQ report for "<HQ machine>" in copy:
  the host name, or a configured label?
- A94 · Setup can finish without the recovery-code acknowledgement (§4a.2), a
  deliberate departure from `standalone-server.md` §6. Accept it and update the
  design, or make the acknowledgement part of setup? *(rev 5)* X-02 Q8 accepts
  the waiting end for noninteractive setup and setup over SSH on a Mac. Answering
  n in an interactive setup is still this question.
- ~~A99~~ · *Resolved (Rowan, 2026-11-10; X-02 Q9):* the recovery state carries
  when the code was first shown (`released_at`), when it was last shown again
  (`redelivered_at`) with how many times (`redelivery_count`), and when it was
  confirmed (`acknowledged_at`). Every showing is journaled and audited with the
  account and the time. Who showed it is visible only through `agentic-org
  recovery status`; the browser shows the dates and the count (§4a.6; RCV-23,
  RCV-24, RCV-29).
- ~~A100~~ · *Resolved (Rowan, 2026-11-10; X-02 Q9):* one confirmation per run,
  whose prompt names both actions, covers showing the code and recording it as
  stored; the acknowledgement is still recorded only after `yes` in that run, and
  a second run asks again. `--redeliver` on a code that was never shown is
  accepted and recorded as the first showing (§4a.6; RCV-25 to RCV-27).
- ~~A103~~ · *Resolved (Rowan, 2026-11-10; X-02 Q9):* `agentic-org account link
  <username> --org <organization>`, `account unlink <username> --org
  <organization>` and `account revoke <username>`. Each asks for an instance
  admin's sign-in, a y/N naming the account and the organization, and the
  confirmation that it's you (Touch ID or the login password on a Mac, the
  password at a terminal on Linux). `account create` and `account link` can name
  any organization on the instance. A command that would leave no instance admin,
  or an organization with no linked account, is refused (§4a.1, §4a.2; SIGN-35
  to SIGN-42).
- A104 · *(rev 5)* X-02 Q8 (accepted) ends noninteractive setup, and setup over
  SSH on a Mac, with the recovery code waiting: automation never receives the
  code. `standalone-server.md` §6 ("CA escrow and the explicit recovery gate")
  still says "Automation uses separate protected outputs". Can you update that
  design to match the decision (§2, §4a.2)? *(Rowan, 2026-11-10)* Rowan updates
  standalone-server.md §6 (agreed), on his next docs branch. Open until it's
  merged.

## 5. Lifecycle, updates and removal

All instance operations accept `--name`, defaulting to `default`: `start`, `stop`,
`status`, `logs`, `doctor`, `upgrade` and `uninstall`. `serve` runs the selected
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
instance may continue using its existing version. Future upgrades of this new
format still require supported schema handling and recoverable failure; this is
distinct from migrating the undeployed desktop architecture.

Uninstall unregisters only the selected service and preserves its data by default.
Data deletion is a separate explicit, instance-scoped operation; active children
must be stopped first. Shared releases stay while any instance references them.
An office removed from an organization follows the existing revocation/reclaim
rules, which are distinct from uninstalling software on a machine.

## 6. Acceptance criteria

- SRV-1 · A supported macOS or Linux host installs and runs HQ without Tauri or frontend build tools. Its browser loads the bundled application through the server.
- SRV-2 · A first setup without a name creates `default`; a repeated setup reuses its identity, data and port and starts at most one daemon.
- SRV-3 · A named setup creates or selects that name. Two instances have disjoint data, credential authority, sockets, services and listening endpoints. Browser cookies may be delivered across same-host ports; instance/origin-bound session proofs isolate authority as required by SRV-23.
- SRV-4 · Concurrent setup/start operations and aliased paths cannot open the same instance storage twice; conflicting ports produce an actionable error without changing saved addresses.
- SRV-5 · *(rev 5)* Interactive and noninteractive setup reach the same configuration, except the recovery code, which noninteractive setup leaves waiting (RCV-20). Missing inputs fail predictably; pending approval is not reported as enrollment success; secrets do not appear in logs or command arguments.
- SRV-6 · Repeating setup cannot silently upgrade, re-enroll, rotate identity, reset configuration or restart a healthy daemon. Interrupted setup resumes without duplicate offices.
- SRV-7 · HQ setup establishes owner authentication. Unauthenticated or unauthorized requests, including requests from another website to loopback, cannot perform administration.
- SRV-8 · Any authorized browser can use HQ without installing software or becoming a satellite; quitting it leaves offices and agents running.
- SRV-9 · A satellite enrolls through a verified invitation and approval, reconnects using its stored identity and opens no public application listener.
- SRV-10 · Organization changes remain HQ-authoritative; locally owned work commits locally and synchronizes with deduplication and resumable positions.
- SRV-11 · HQ shows last-known data and freshness accurately. Live control of an unreachable satellite is unavailable or explicitly queued by that operation's contract, never falsely completed.
- SRV-12 · Browser reconnection recovers missed updates without duplicates or a snapshot/event gap; an expired cursor refreshes and a lost mutation response can be resolved by operation ID.
- SRV-13 · Instance and agent paths are server-managed and shown read-only where authorized. No browser path picker, desktop import or storage relocation flow ships in v1.
- SRV-14 · Setup installs a matching `org-agent`, detects required coding tools and selected harnesses, and distinguishes daemon readiness from agent authentication/readiness.
- SRV-15 · `start`, `stop`, `status`, `logs`, `doctor`, `upgrade` and `uninstall` operate on the selected instance. Service manager or credential-unlock limitations are surfaced explicitly.
- SRV-16 · Upgrading or uninstalling one instance cannot replace a running peer's helpers, remove a referenced release, delete peer data or sweep peer credentials.
- SRV-17 · Shutdown drains acknowledged writes and reaps owned descendants within documented deadlines. Restart reconciles recorded work; browser disconnect does not cancel durable work.
- SRV-18 · Production browser paths use the HTTP API, with an explicit unavailable state for missing features. Missing Tauri globals or API failure never select sample data.
- SRV-19 · The v1 build, runtime and acceptance suite require no Tauri packages, desktop command bridge, tray, native notification or global shortcut.
- SRV-20 · The implementation deletes superseded paths without preserving old desktop contracts or importing undeployed app data. Domain rules, storage durability, office identity and supervision invariants remain covered.
- SRV-21 · A fresh HQ and satellite complete the cross-platform walkthrough in [dogfood-run.md](dogfood-run.md), including disconnect/reconnect and a real agent. Mock-only success is insufficient.

- SRV-22 · The retained browser application's source/package is `apps/org-ui` / `org-ui`; the server application's source/Cargo package/worker executable is `apps/org-server` / `org-server`. Workspace, imports, tests, build, packaging and ownership references agree; the rename preserves the per-office worker and host credential boundaries.

- SRV-23 · Default/lab and a hostile same-host port cannot authenticate, read, mutate, stream, refresh, recover a proof or logout another origin using a copied/overwritten cookie alone. Every protected path requires the valid instance/origin-bound cookie+proof pair; proofs never enter URLs/logs/redirects.
- SRV-24 · Two HQ organizations share one instance gateway listener. The gateway has no office DB key/key entitlement; the host retains provisioning/private launch. Instance-admin creation succeeds; organization-owner-only, employee and office-certificate callers cannot create organizations.
- SRV-25 · HQ creation idempotently provisions versioned encrypted CA escrow, protected one-time recovery-code delivery and off-HQ retention acknowledgement. A fixture recovers the same CA identity with the code and refuses wrong-code/damaged escrow; no code/usable CA key is replicated or logged. REC-1 remains an explicit production-recovery release decision.
- SRV-26 · *(rev 5)* No interface the product provides (browser, CLI, HTTP API) returns one employee's or instance's private data to another employee's agent or another instance. Isolation from other processes of the same OS user is not claimed in v1.0; a dedicated OS user per instance is a Wave C hardened mode. Release credential tests on both platforms still refuse missing/wrong keys, unavailable/locked providers and development plaintext-store fallback: existing databases/identity are unchanged, no replacement key is created, and restoring the legitimate credential allows recovery; gateway/CLI helpers cannot obtain raw office keys through any interface the product provides.
- SRV-27 · Eight HQ tabs share at most one authenticated streaming fetch per browser origin, including across login/session changes, and commands still complete under HTTP/1.1. Leader loss reconnects by cursor; unsupported coordination uses bounded short polling and no per-tab long-lived stream.
- SRV-28 · HQ durably accepts Pause for a disconnected satellite as paused status with stop pending. Reconnect/restart reconciles the latest version before new work; a later Resume is not overwritten by stale stop delivery. Known-offline new goals are rejected without a write/queue; uncertain forwarded replies follow EMP-3.
- SRV-29 · All create/update home overrides, including home/clear_home, absolute paths and traversal, are rejected before filesystem access. Launch cwd derives from managed instance/office/member IDs and matches the read-only profile path under PROF-4.
- SRV-30 · Add agent record follows STAFF-4/HRL-3: neither UI nor direct HTTP calls provision a member, home or messaging binding before approval. The built-in organization bootstrap exception is explicit.
- SRV-31 · A fixture-based live Phase A cutover preserves unresolved asks/replies, deadlines and read/triage state across interruption/retry. Person-started CLI context selects explicit immutable instance/office identities; managed binding cannot fall back to spool or another office. Fresh organizations need no spool import.
- SRV-32 · The first standalone local API negotiates version 2 and rejects unsupported peers before dispatch; version-1 statements remain historical/additive. HTTP/launch/sync version spaces are independent. New setup Cargo package org-setup is distinct, and the retired Tauri member cannot overwrite the staged agentic-org CLI artifact.

**Sign in, session and first access (SIGN)** *(new, rev 4, §4a)*
- SIGN-1 · Opening any HQ page without a session shows the sign-in page, with Username, Password and Sign in. The page and its responses contain no organization data.
- SIGN-2 · *(rev 5)* The sign-in page has no "forgot password" link, reset form or sign-up form. It shows `agentic-org account reset-password --name <instance> <username>`, its Linux form with `sudo` in front, and `agentic-org account create --name <instance>`.
- SIGN-3 · Submitting with an empty field sends no request and shows "Enter your username." or "Enter your password." under that field.
- SIGN-4 · A wrong password for an existing username and any password for an unknown username show the same message, "That username and password don't match. Try again.", and HQ answers both with the same status and body. Password is cleared and Username kept.
- SIGN-5 · A name that matches a person record (offices §7.4) but no account, with any password, is refused with SIGN-4's message.
- SIGN-6 · A rate-limited sign-in shows "Too many sign-in attempts." with the retry time when HQ gives one, and keeps Sign in disabled until then; without a time it shows "Wait a few minutes, then try again."
- SIGN-7 · When the loaded sign-in page can't reach HQ, it shows "Can't reach HQ…" with `agentic-org status --name <instance>` and Try again, and keeps both fields.
- SIGN-8 · *(rev 5)* A correct sign-in opens the page the user was trying to open, if it is a page of this HQ that the account can open. A return target outside this HQ is ignored, and the last organization or the organization list opens (OPEN-1); a page of an organization the account can't open is ignored too, and the organization list opens.
- SIGN-9 · *(rev 5)* After sign-in, an account with no organizations sees "No organizations yet." with Create organization if it is an instance admin, and "This account doesn't own any organizations on this HQ. Ask an instance admin of this HQ to link it to one." with Sign out if not.
- SIGN-10 · When a session ends while a page is open, the next refused request or stream opens the "Sign in again" dialog over that page; the page doesn't navigate away.
- SIGN-11 · *(rev 5)* Text typed into a page and not saved is still in its fields after the user signs in again from the dialog, or after Not now and then Sign in, except after an unlink from that page's organization (SIGN-43).
- SIGN-12 · A save refused because the session ended is marked "Not saved", and nothing is resent after signing in until the user saves again.
- SIGN-13 · *(rev 5)* After signing in again from the dialog, the user is on the same page with focus where it was (except after an unlink from that page's organization, SIGN-43), and live updates resume without a gap (SRV-12).
- SIGN-14 · *(rev 5)* With two tabs of the same HQ showing the dialog, signing in in one closes the dialog in the other, and the other stays on its page with its text, except when its page belongs to an organization the account no longer owns (SIGN-43).
- SIGN-15 · *(rev 5)* Not now closes the dialog and shows "You're signed out. Text you haven't saved is still here…" with Sign in (after an unlink from that page's organization, SIGN-43's bar instead); the page sends no changes while signed out.
- SIGN-16 · Sign in as someone else asks before discarding text that isn't saved, then opens the sign-in page.
- SIGN-17 · When an HQ restart ends the session (A84), an open page first shows LIVE-8's paused line, then the dialog. When HQ reports the restart as the reason, the dialog reads "HQ restarted, so you were signed out."; otherwise it shows the generic copy.
- SIGN-18 · The account menu in the top bar shows "Signed in as <username>", "Instance admin" only for an account with the grant, and Sign out. It has no password or account form.
- SIGN-19 · Sign out with text that isn't saved asks "Sign out? Text you haven't saved in any open tab of this HQ will be lost." first. Stay keeps the page.
- SIGN-20 · Sign out ends the session at HQ: every open tab of that HQ shows the sign-in page with "You signed out.", Back shows no organization data, and a session in another browser still works.
- SIGN-21 · First setup asks for a username and the password twice, without echoing it. Different entries show "The passwords don't match. Enter them again." and ask again; a password under the rule shows the rule.
- SIGN-22 · First setup creates one account that is an instance admin and owns the first organization, and ends by printing the browser address and username. Its output, logs and JSON status never contain the password.
- SIGN-23 · Signing in with the setup account at the printed address opens the first organization, with that account as its owner.
- SIGN-24 · A repeated setup asks for no account and prints the address and the existing username.
- SIGN-25 · *(rev 5)* `agentic-org account create` at the HQ machine asks an instance admin to sign in, then (on a Mac) to confirm it's them, and creates an account that can sign in at the printed address. The account has the admin grant only when the answer to "Make this account an instance admin?" is y, and owns exactly the organizations chosen; the last line says both.
- SIGN-26 · *(rev 5)* `agentic-org account set-password` asks for the username and current password, then the new one twice. After it, the old password is refused, the new one signs in, and every session of that account has ended: an open page of that account shows "Sign in again".
- SIGN-27 · The sign-in page and the dialog meet §4a.7: labelled fields with username and current-password autocomplete, the stated focus order, errors linked with `aria-describedby` and announced, and no meaning by colour alone.
- SIGN-28 · No browser screen lists, adds or edits accounts, and HQ's HTTP API refuses any request to create an account.
- SIGN-29 · Sign out shows Signing out… while HQ ends the session; when HQ can't be reached it shows "Couldn't sign out: HQ can't be reached…" with Try again, and the page stays signed in.
- SIGN-30 · *(rev 5)* Each row of §4a.2's account-command table (`account create`, `set-password`, `reset-password`, `link`, `unlink` and `revoke`) shows its copy and exit status. A refusal changes nothing, and a non-admin is refused `link`, `unlink` and `revoke` with "<login> isn't an instance admin, so it can't change accounts".
- SIGN-31 · When a different account signs in from one tab, every other open tab of that HQ shows "You're now signed in as <username> in another tab…" with Reload, and shows no data of the previous account.
- SIGN-32 · *(new, rev 5)* `agentic-org account reset-password <username>` at a Mac HQ machine asks for no sign-in, asks for the new password twice and for confirmation in the system dialog, then: the old password is refused, the new one signs in, every session of that account has ended (an open page shows "Sign in again"), a rate-limited sign-in for that account is allowed again, and the reset is recorded with its time and without the password.
- SIGN-33 · *(new, rev 5)* On Linux, `agentic-org account reset-password <username>` without root is refused with the `sudo` command and changes nothing. With `sudo`, it resets the password on the instance of the Linux user who ran `sudo` (`--name` picks among that user's instances), and its first line names that user; run as root without `sudo`, it needs `--user <user>`.
- SIGN-34 · *(new, rev 5)* `agentic-org account create` and `agentic-org account reset-password` over SSH on a Mac print the "needs you at this Mac" message before any prompt, and change nothing. Over SSH on Linux, with a terminal, both work.
- SIGN-35 · *(new, rev 5, A103)* `agentic-org account link <username> --org <organization>` asks an instance admin to sign in, asks a y/N that names the account and the organization, then (on a Mac) asks for confirmation in the system dialog; after y, that account signs in and opens the organization as its owner.
- SIGN-36 · *(new, rev 5, A103)* After y, `agentic-org account unlink <username> --org <organization>` leaves that account unable to open the organization, and every session of that account has ended: an open page of it shows "Sign in again".
- SIGN-37 · *(new, rev 5, A103)* After y, `agentic-org account revoke <username>` ends every session of that account, and signing in with it is refused with SIGN-4's message.
- SIGN-38 · *(new, rev 5, A103)* `agentic-org account create` lists every organization on the instance, and `account link` accepts any of them, including organizations the signed-in admin doesn't own.
- SIGN-39 · *(new, rev 5, A103)* Unlinking or revoking an organization's only linked account is refused with "<username> is the only account linked to <organization>…", before the y/N question, and nothing changes.
- SIGN-40 · *(new, rev 5, A103)* Revoking the instance's only instance admin is refused with "<username> is the only instance admin of instance <name>…", before the y/N question, and nothing changes.
- SIGN-41 · *(new, rev 5, A103)* `agentic-org account link`, `unlink` and `revoke` over SSH on a Mac print the "needs you at this Mac" message before any prompt, and change nothing.
- SIGN-42 · *(new, rev 5, A103)* Answering anything but y to `link`, `unlink` or `revoke` prints "Nothing changed." and changes nothing.
- SIGN-43 · *(new, rev 5, review of 0802754)* With a page of an organization open, after `agentic-org account unlink <username> --org <organization>` for that page's account, signing in again from the "Sign in again" dialog succeeds and opens the organization list (or §4a.5's no-organization page) with "You no longer own <organization>." above it, not that page, and no text from that page is kept. Focus is on the organization list's heading (or the no-organization page's), and "You no longer own <organization>." is announced through the polite live region. Every other open tab of this HQ on a page of an organization the account no longer owns also opens the organization list with that line, and keeps none of its text, even a tab that chose Not now; tabs on organizations it still owns behave as SIGN-14 says. The dialog shows "You no longer own <organization>, so you were signed out." and warns that signing in, here or in another tab, loses that page's unsaved text only when HQ gives that reason (A84), and after Not now its bar reads "You're signed out. Text you haven't saved here is lost when you sign in, here or in another tab: copy it first."; otherwise it shows its usual copy. After `agentic-org account revoke <username>`, signing in again from the dialog is refused with SIGN-4's message, and the page stays as it was.

**Recovery code (RCV)** *(new, rev 4, §4a.6)*
- RCV-1 · The recovery code never passes through the gateway or reaches the browser: a fixture code appears in no gateway request or response, HTTP response, event, page, notice or copied text.
- RCV-2 · *(rev 5)* While the code is waiting, the organization's owner sees the notice on every page of that organization, with `agentic-org recovery release --org '<organization>'` and Copy command, and no close button. Other accounts don't see it.
- RCV-3 · While the code is waiting, Needs you shows "Collect the recovery code for <organization> on <HQ machine>.", and the item opens the Recovery row. Without Needs you, the notice and the row still show.
- RCV-4 · *(rev 5)* The Recovery row in Settings › Organization shows the exact copy for each state: being prepared, waiting to be collected, shown but not confirmed, collected on <date>, couldn't be prepared with its reason, and status unknown.
- RCV-5 · No browser state is labelled collected or ready before the acknowledgement is recorded; status unknown is never shown as collected.
- RCV-6 · *(rev 5)* `agentic-org recovery release` records the acknowledgement only after the typed last group matches and the owner types `yes`.
- RCV-7 · A typed group that doesn't match is refused with "That isn't the last group of the code…" and nothing is recorded.
- RCV-8 · *(rev 5)* Interrupting the command after the code is shown and before `yes` records no acknowledgement; running it again without `--redeliver` shows no code and gives the `--redeliver` command, and the browser shows Shown, not confirmed.
- RCV-9 · *(rev 5)* After the acknowledgement of a code that was never shown again, the browser shows "Recovery code collected on <date>" (RCV-23 covers a code shown again) and the notice and Needs you item go away, without a reload while live updates run.
- RCV-10 · *(rev 5)* After the acknowledgement, `agentic-org recovery release`, with or without `--redeliver`, prints when it was collected and that it can't be shown again, prints no code and exits with an error status.
- RCV-11 · While the package is being created, the browser shows "being prepared" and the command says to try again in a minute, with no code.
- RCV-12 · When the package couldn't be created, the browser and the command show the reason and `agentic-org doctor`, with no code.
- RCV-13 · *(rev 5)* Interactive setup at the HQ machine (or over SSH on Linux) offers to collect the code once the HQ is created and before it starts the service; answering n leaves it waiting and prints `agentic-org recovery release --org '<organization>'`.
- RCV-14 · An organization created in the browser starts as being prepared, then waiting, and is collected with `--org` naming it.
- RCV-15 · *(rev 5)* `agentic-org recovery release` run without an interactive terminal prints no code.
- RCV-16 · *(rev 5)* Every message that reports the code's state (in the notice, the Recovery row, Needs you, setup, a repeated setup, `agentic-org status`, `agentic-org recovery release` and `agentic-org recovery status`) says that recovering after losing the HQ machine isn't supported yet.
- RCV-17 · CLI output for setup, account and recovery commands carries no meaning by colour alone, and writes no colour codes with `NO_COLOR` set or when output isn't a terminal.
- RCV-18 · *(new, rev 5)* After the code is shown and before it's confirmed, the owner sees the Shown, not confirmed row, notice and Needs you item, each with `agentic-org recovery release --redeliver --org '<organization>'`.
- RCV-19 · *(new, rev 5)* `agentic-org recovery release --redeliver` before the acknowledgement shows the same code as the first showing, then asks for the same confirmation (last group, then `yes`).
- RCV-20 · *(new, rev 5)* Noninteractive setup, and setup over SSH on a Mac, finish with the HQ created, exit successfully, report the recovery code as waiting with no code in any output, and print `agentic-org recovery release --org '<organization>'` to run at the machine.
- RCV-21 · *(new, rev 5)* `agentic-org recovery release` over SSH on a Mac prints the "needs you at this Mac" message, shows no code and leaves the state unchanged.
- RCV-22 · *(new, rev 5)* `agentic-org recovery release` signed in with an account that isn't an instance admin is refused with "<username> isn't an instance admin…", shows no code and leaves the state unchanged.
- RCV-23 · *(new, rev 5; rewritten for A99)* After an acknowledgement that followed second showings, the Recovery row reads "Recovery code shown on <date> (shown again <n> times, last on <date>; with one second showing: "shown again once, on <date>") · collected on <date>.", with the first-showing, last-second-showing and acknowledgement dates, without a reload while live updates run.
- RCV-24 · *(new, rev 5, A99)* `agentic-org recovery status --org '<organization>'`, after an instance admin signs in, shows the state and lists every showing with its date, time, account and whether it was the first showing or shown again; it shows no code and changes nothing.
- RCV-25 · *(new, rev 5, A100)* One run of `agentic-org recovery release` asks for one confirmation, before the code is shown, and the system dialog names both showing the code and recording it as stored; after `yes`, the acknowledgement is recorded with no second dialog.
- RCV-26 · *(new, rev 5, A100)* A new run of `agentic-org recovery release --redeliver` asks for the confirmation again, even when an earlier run was confirmed.
- RCV-27 · *(new, rev 5, A100)* `agentic-org recovery release --redeliver` on a code that was never shown shows the code and records it as the first showing: `recovery status` lists it as shown, not shown again, and the browser's count of second showings stays at none.
- RCV-28 · *(new, rev 5)* In interactive setup, when the code was shown and the answer at the last prompt isn't `yes`, the code is Shown, not confirmed, setup prints `agentic-org recovery release --redeliver --org '<organization>'`, and the service still starts.
- RCV-29 · *(new, rev 5, A99)* While the code is Shown, not confirmed after second showings, the Recovery row and the notice give the first-showing date, the number of second showings and the last one's date.
- RCV-30 · *(new, rev 5)* `agentic-org recovery status` signed in as an account without the instance-admin grant is refused with "<username> isn't an instance admin, so it can't see recovery status…", shows no state or showings, and exits with an error.


## 7. Decisions still requiring implementation evidence

**REC-1 remains open:** a reviewed manual HQ-loss recovery/re-certification/fencing
procedure and disaster-recovery evidence are required before production recovery
claims. V1 still provisions CA escrow and its owner recovery code under SRV-25.
See [review decisions](../design/standalone-v1-review-decisions.md).

The server/installer must document the supported OS/architecture matrix, minimum
versions and Linux service prerequisites. Production key custody on headless Linux
and standalone signed macOS must pass the design's platform checks. TLS termination,
version negotiation and bounded command/event handling need concrete contracts.
These are engineering gates for release, not reasons to restore Tauri or an old-app
compatibility layer. Cross-office coding also needs a supported repository transport;
the pivot does not silently bring v2 GitHub integration into v1.
