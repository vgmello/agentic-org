---
kind: spec
title: "Standalone server: installation, instances and browser access (UX)"
status: for-review
revision: 4
owner: ac-product-designer-eden
created: "2026-11-09"
updated: "2026-11-10"
requested_by: owner
reviewers: [ac-staff-engineer-ada, owner, app-frontend-dev, server-dev, ac-software-engineer-river, ac-software-engineer-remy]
builds_on: [offices-and-teams-ux, agent-run-modes-ux, employee-supervision-ux, processes-ux, chat-ux]
---

# Spec · Standalone server: installation, instances and browser access

## Changelog

**Revision 4 (2026-11-10): sign in, first access and recovery code.** New §4a,
built on the owner's decisions O-1 (accounts) and O-2 (recovery-code delivery):
who can sign in, the setup transcript for the first account, the sign-in page
and its errors, session end and signing in again, sign out, what opens after
sign-in, and the recovery-code states in the browser and the CLI, with their
accessibility. Open questions A80 to A90 and A93 for ada (§4a.9). §4 points to
§4a. **Criteria added:** SIGN-1 to SIGN-28, RCV-1 to RCV-17. No existing
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
   CA escrow/recovery-code provisioning (collecting the code and acknowledging its
   off-HQ retention happens after HQ is ready, and can be deferred: §4a.6); or the
   satellite invitation, destination identity and enrollment confirmation.
4. Runtime selection, dependency checks and authentication readiness.
5. Service registration, start and a summary with separate readiness states.

Automation uses the same validation and state transitions. Missing mandatory
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
and **O-2** (recovery-code delivery). Commands are proposed spellings, like the
rest of this spec (A80). Placeholders in angle brackets, such as `<HQ machine>`,
are filled from HQ's data. Questions for ada are in §4a.9.

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
- In v1.0 another account is added only on the HQ machine, with
  `agentic-org account add`. No browser screen lists, adds or edits accounts.
  If a later screen lists accounts, it shows that command and has no form.
- A password is reset only on the HQ machine, with
  `agentic-org account password <username>`. The browser has no "forgot
  password" flow and no form to change a password.
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
Service running.
HQ ready.

Recovery code for Lumen Labs
It's shown once, here on the HQ machine, and never in the browser.
Collect it now? [Y/n]: n
Not collected. It's waiting on this machine. Collect it later with:
  agentic-org recovery-code collect --org "Lumen Labs"

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
- **Collect it now? Y** runs the same steps as `recovery-code collect` (§4a.6).
  **n**, or setup ending before the acknowledgement, leaves the code waiting.
- **A repeated setup** (SRV-2) asks for no account. It ends with "HQ is running.
  Open <address> and sign in as alex." While the code is waiting, it also prints
  the collect command.
- **The first browser visit** is an ordinary sign-in (§4a.3). After it, the first
  organization opens (OPEN-1), with the recovery notice while the code is waiting.

**Other account commands (HQ machine only):**

```
$ agentic-org account add
Username: sam
Password (at least 12 characters, not shown as you type):
Password again:
Account "sam" created. sam can sign in at http://127.0.0.1:<port>.
sam doesn't own any organization yet.

$ agentic-org account password alex
New password for alex (at least 12 characters, not shown as you type):
Password again:
Password changed for alex.
```

Whether `account add` can give the admin grant, and whether a password change
ends that account's open sessions, are A86 and A85.

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
 │  Forgot your password? On the HQ machine, run       │
 │  agentic-org --name <instance> account password ... │
 │  Need an account? Accounts are added on the HQ      │
 │  machine: agentic-org --name <instance> account add │
 └────────────────────────────────────────────────────┘
```

- "Sign in to HQ at <host:port>" uses the address in the browser's own address
  bar, so two instances on one machine (default and lab) are told apart without
  HQ revealing anything before sign-in.
- **Show** reveals the password and becomes **Hide**.
- Enter in either field submits.

| State | What shows |
| --- | --- |
| Loading | "Loading…" while the page checks for a session that is still valid. No form, no organization data. |
| Ready | The form. Focus is in Username, or in Password when Username is already filled. |
| Signing in | **Signing in…**, disabled. Both fields are read-only. |
| Empty field | No request is sent. Under the field: "Enter your username." or "Enter your password." Focus moves to the first empty field. |
| Wrong username or password | "That username and password don't match. Try again." The same words for both cases. Password is cleared, Username kept, focus moves to Password. |
| Too many attempts, with a retry time | "Too many sign-in attempts. Try again in 5 minutes." **Sign in** stays disabled until then. The minutes round up and update once a minute. |
| Too many attempts, no retry time | "Too many sign-in attempts. Wait a few minutes, then try again." Limits are A82. |
| HQ unreachable | "Can't reach HQ. Check that the instance is running on the HQ machine (`agentic-org status`), then try again." **Try again**. Both fields are kept. |
| HQ error | "HQ couldn't sign you in because of a problem on the HQ machine. Try again. If it keeps happening, run `agentic-org doctor` there." **Try again**. |
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

**When a session ends.** It expires (A83), the account's password is changed (A85), or HQ
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
 │ Signed in as alex                                   │
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
- **Other tabs of this HQ** show the same dialog. Signing in in any one of them
  closes the dialog in the others, because they share one session.
- **Not now** (or Esc) closes the dialog. The page stays visible but changes
  nothing, with a bar at the top: "You're signed out. Text you haven't saved is
  still here: copy it, or sign in to save it." **Sign in** reopens the dialog.
- **Sign in as someone else** goes to the sign-in page. If the page has text
  that isn't saved, it asks first: "Discard text you haven't saved on this
  page?" **Keep editing** · **Discard and sign in**.
- Reloading or closing the tab loses text that isn't saved, as §4 says. A reload
  while signed out goes to the sign-in page. After signing in, the user lands on
  that same page.

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

### 4a.5 After sign-in: what opens

1. The page the user was trying to open, if it is a page of this HQ. Any other
   return target is ignored.
2. Otherwise the last authorized organization, or the organization list (OPEN-1).
3. An account with no organizations sees:
   - Instance admin: "No organizations yet. Create one to get started."
     **Create organization** (offices §5a).
   - Not an admin: "This account doesn't own any organizations on this HQ."
     **Sign out**. (How such an account gets one is A86.)

### 4a.6 Recovery code

The recovery code, with the organization's encrypted recovery package, will let
the owner recover the organization's certificate authority if the HQ machine is
lost. Under O-2 the host generates the code, the worker creates the authority and
the encrypted package, and the host keeps the code until it is collected with
`agentic-org` on the HQ machine. This is the same for every organization: the
first one from setup, and each one created in the browser. **The browser never
shows the code.** It shows only its state.

The notice, every Recovery row state and the collect command end with this
sentence (the table below leaves it out for space): "Recovering an organization after losing the
HQ machine isn't supported yet." (REC-1, §7.)

**States in the browser.** Only the organization's owner sees them.

| State | Settings › Organization, Recovery row | Notice at the top of the organization's pages | Needs you |
| --- | --- | --- | --- |
| Being prepared (the package is still being created) | "Recovery code: being prepared. This takes a moment." | None | None |
| Waiting to be collected | "Recovery code: waiting to be collected on the HQ machine. Run `agentic-org recovery-code collect --org "<organization>"` on <HQ machine>." **Copy command** | The notice below | "Collect the recovery code for <organization> on <HQ machine>." Opens the Recovery row. |
| Collected | "Recovery code collected on <date>." | None | None (the item clears) |
| Couldn't be prepared | "Recovery code: couldn't be prepared. <reason>. On <HQ machine>, run `agentic-org doctor`." **Copy command** | "Recovery material for <organization> couldn't be prepared. See Settings › Organization." | "Recovery material for <organization> couldn't be prepared." |
| Status unknown (HQ didn't say) | "Recovery code: status unknown." Never shown as collected. | None | None |

- The command includes `--name <instance>` when the instance isn't `default`.
- `<date>` is the acknowledgement date in the browser's date format, for example
  "10 Nov 2026". `<HQ machine>` is the name HQ reports for its machine (A93).
- **Copy command** copies only the command. No state copies or shows the code.
- The browser state changes live when the code is collected: the notice and the
  Needs you item go away without a reload. While live updates are paused, the
  change shows on the next load.
- If Home's Needs you isn't built yet, the notice and the Settings row carry the
  state alone. Nothing else depends on Needs you.

**The notice** (owner only, every page of that organization, no close button,
gone once collected):

```
 ┃ ⚠ Collect the recovery code for Lumen Labs
 ┃   It's waiting on the HQ machine and is never shown in the browser.
 ┃   On hq-mac, run:  agentic-org recovery-code collect --org "Lumen Labs"
 ┃   Then store it and the recovery package away from that machine.
 ┃   Recovering an organization after losing the HQ machine isn't supported yet.
 ┃                                                            [Copy command]
```

**Collecting it on the HQ machine:**

```
$ agentic-org recovery-code collect --org "Lumen Labs"
Recovery code for Lumen Labs (instance default)

You'll see this code once. With the recovery package, it's what will
let you recover Lumen Labs if this machine is lost. Store both away
from this machine: for example, the code in a password manager on
another device, or on paper somewhere safe.
Recovering after losing the HQ machine isn't supported yet, but
keeping the code now is what will make it possible.

Recovery package: <where it is, or how to copy it off this machine, A90>

Press Enter to show the code.

Recovery code: <the code, in groups separated by dashes, A90>

To confirm you stored it, type the last group of the code: 9FXM
Are the code and the encrypted recovery package stored away from this machine?
Type yes to confirm: yes

Recorded on 10 Nov 2026 at 14:02. This code won't be shown again.
Recovering an organization after losing the HQ machine isn't supported yet.
```

| Case | CLI output | Exit |
| --- | --- | --- |
| Typed group doesn't match | "That isn't the last group of the code. Check what you stored, then type it again." Asks again. | — |
| Anything but `yes` at the last prompt | "Not recorded. Run this command again once the code and the recovery package are stored away from this machine." The code stays waiting. | Error |
| Interrupted before `yes` (Ctrl-C, closed terminal) | "Not recorded. The code is still waiting; run this command again to see it." Running it again shows the **same** code. | Error |
| Already collected | "The recovery code for Lumen Labs was collected on 10 Nov 2026. It can't be shown again. Recovering after losing the HQ machine isn't supported yet." No code. | Error |
| Still being prepared | "Recovery material for Lumen Labs is still being prepared. Try again in a minute." | Error |
| Couldn't be prepared | "Recovery material for Lumen Labs couldn't be prepared: <reason>. Run agentic-org doctor." | Error |
| No `--org`, one organization on the instance | Collects that organization's code. | — |
| No `--org`, several organizations | "This instance has several organizations. Run again with --org and one of: Lumen Labs, Side Project." | Error |
| Unknown organization | "There's no organization called "Lumen Labs" on instance default." | Error |
| Not an interactive terminal | "The recovery code is shown only in an interactive terminal." No code. (Automation's protected output is A80.) | Error |

- `agentic-org status` lists each organization's recovery state in the same
  words as the Settings row, never the code.
- An organization created in the browser starts as **Being prepared**, then
  **Waiting to be collected**. Its owner collects it with the same command,
  naming that organization.

### 4a.7 Accessibility

- **Labels.** Every field has a visible label tied to it: Username, Password.
  The fields use `autocomplete="username"` and `"current-password"`, so password
  managers work. **Show** is a button with `aria-pressed` and the label "Show
  password".
- **Focus order** on the sign-in page: the error area (when shown), Username,
  Password, Show, Sign in, then the help text. In the sign-in-again dialog:
  Password, Show, Sign in, Not now, Sign in as someone else. The dialog traps
  focus while open and returns focus to where it was when it closes.
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

- A80 · Command spelling: `agentic-org account add`, `agentic-org account password <username>`
  and `agentic-org recovery-code collect --org <organization>` are my proposals.
  What does noninteractive setup use as the code's protected output (§2)?
- A81 · Username rules (length, characters, whether uniqueness ignores case) and
  the password rule. I've proposed at least 12 characters and no other rule.
- A82 · Sign-in rate limit: per username, per source address or both, the limits,
  and whether the refusal carries a retry time the page can show.
- A83 · Session lifetime: idle and absolute expiry, whether activity extends it,
  and whether an idle expiry should also hide what the page shows (§4a.4 keeps
  the page visible).
- A84 · Does an HQ restart end every session (the plan's starting point for B1-06),
  or do sessions survive it? When HQ refuses a request because the session ended,
  does it say why (expired, signed out, HQ restarted, password changed)? Without a reason the
  page uses the generic copy. Is that reason safe to show before sign-in?
- A85 · Does `account password` end that account's open sessions?
- A86 · Can `account add` give the admin grant? Can a second account be linked
  to an existing organization in v1.0? Does the admin grant alone open
  organizations the account doesn't own?
- A87 · The owner bar (offices §7.1) shows the owner's name. Does setup ask for a
  display name, or does the owner bar show the username?
- A88 · Which read gives the browser the recovery state (being prepared, waiting,
  collected with its date, couldn't be prepared with a reason), and is it
  limited to the owner?
- A89 · If the recovery package can't be created, does the create fail
  (offices §5a), or is the organization created with recovery "couldn't be
  prepared"? What repairs it: a host restart, `doctor`, or a command?
- A90 · The acknowledgement covers keeping the encrypted package off HQ too. How
  does the owner get a copy of the package? And what does the code look like
  (length, groups), so the CLI can ask for its last group?
- A93 · Which name for the HQ machine does HQ report for "<HQ machine>" in copy:
  the host name, or a configured label?

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
- SRV-5 · Interactive and noninteractive setup reach the same configuration. Missing inputs fail predictably; pending approval is not reported as enrollment success; secrets do not appear in logs or command arguments.
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
- SRV-26 · Release credential tests on both platforms refuse missing/wrong keys, unavailable/locked providers and development plaintext-store fallback. Existing databases/identity are unchanged, no replacement key is created, and restoring the legitimate credential allows recovery; gateway/CLI helpers cannot obtain raw office keys.
- SRV-27 · Eight HQ tabs share at most one authenticated streaming fetch per browser origin, including across login/session changes, and commands still complete under HTTP/1.1. Leader loss reconnects by cursor; unsupported coordination uses bounded short polling and no per-tab long-lived stream.
- SRV-28 · HQ durably accepts Pause for a disconnected satellite as paused status with stop pending. Reconnect/restart reconciles the latest version before new work; a later Resume is not overwritten by stale stop delivery. Known-offline new goals are rejected without a write/queue; uncertain forwarded replies follow EMP-3.
- SRV-29 · All create/update home overrides, including home/clear_home, absolute paths and traversal, are rejected before filesystem access. Launch cwd derives from managed instance/office/member IDs and matches the read-only profile path under PROF-4.
- SRV-30 · Add agent record follows STAFF-4/HRL-3: neither UI nor direct HTTP calls provision a member, home or messaging binding before approval. The built-in organization bootstrap exception is explicit.
- SRV-31 · A fixture-based live Phase A cutover preserves unresolved asks/replies, deadlines and read/triage state across interruption/retry. Person-started CLI context selects explicit immutable instance/office identities; managed binding cannot fall back to spool or another office. Fresh organizations need no spool import.
- SRV-32 · The first standalone local API negotiates version 2 and rejects unsupported peers before dispatch; version-1 statements remain historical/additive. HTTP/launch/sync version spaces are independent. New setup Cargo package org-setup is distinct, and the retired Tauri member cannot overwrite the staged agentic-org CLI artifact.

**Sign in, session and first access (SIGN)** *(new, rev 4, §4a)*
- SIGN-1 · Opening any HQ page without a session shows the sign-in page, with Username, Password and Sign in. The page and its responses contain no organization data.
- SIGN-2 · The sign-in page has no "forgot password" link, reset form or sign-up form. It shows the HQ-machine commands to reset a password and to add an account.
- SIGN-3 · Submitting with an empty field sends no request and shows "Enter your username." or "Enter your password." under that field.
- SIGN-4 · A wrong password for an existing username and any password for an unknown username show the same message, "That username and password don't match. Try again.", and HQ answers both with the same status and body. Password is cleared and Username kept.
- SIGN-5 · A person record's name (offices §7.4), with any password, is refused with SIGN-4's message.
- SIGN-6 · A rate-limited sign-in shows "Too many sign-in attempts." with the retry time when HQ gives one, and keeps Sign in disabled until then; without a time it shows "Wait a few minutes, then try again."
- SIGN-7 · When the loaded sign-in page can't reach HQ, it shows "Can't reach HQ…" with `agentic-org status` and Try again, and keeps both fields.
- SIGN-8 · A correct sign-in opens the page the user was trying to open, if it is a page of this HQ. A return target outside this HQ is ignored, and the last organization or the organization list opens (OPEN-1).
- SIGN-9 · After sign-in, an account with no organizations sees "No organizations yet." with Create organization if it is an instance admin, and "This account doesn't own any organizations on this HQ." with Sign out if not.
- SIGN-10 · When a session ends while a page is open, the next refused request or stream opens the "Sign in again" dialog over that page; the page doesn't navigate away.
- SIGN-11 · Text typed into a page and not saved is still in its fields after the user signs in again from the dialog, and after Not now.
- SIGN-12 · A save refused because the session ended is marked "Not saved", and nothing is resent after signing in until the user saves again.
- SIGN-13 · After signing in again from the dialog, the user is on the same page with focus where it was, and live updates resume without a gap (SRV-12).
- SIGN-14 · With two tabs of the same HQ showing the dialog, signing in in one closes the dialog in the other.
- SIGN-15 · Not now closes the dialog and shows "You're signed out. Text you haven't saved is still here…" with Sign in; the page sends no changes while signed out.
- SIGN-16 · Sign in as someone else asks before discarding text that isn't saved, then opens the sign-in page.
- SIGN-17 · When an HQ restart ends the session (A84), an open page first shows LIVE-8's paused line, then the dialog. When HQ reports the restart as the reason, the dialog reads "HQ restarted, so you were signed out."; otherwise it shows the generic copy.
- SIGN-18 · The account menu in the top bar shows "Signed in as <username>", "Instance admin" only for an account with the grant, and Sign out. It has no password or account form.
- SIGN-19 · Sign out with text that isn't saved asks "Sign out? Text you haven't saved in any open tab of this HQ will be lost." first. Stay keeps the page.
- SIGN-20 · Sign out ends the session at HQ: every open tab of that HQ shows the sign-in page with "You signed out.", Back shows no organization data, and a session in another browser still works.
- SIGN-21 · First setup asks for a username and the password twice, without echoing it. Different entries show "The passwords don't match. Enter them again." and ask again; a password under the rule shows the rule.
- SIGN-22 · First setup creates one account that is an instance admin and owns the first organization, and ends by printing the browser address and username. Its output, logs and JSON status never contain the password.
- SIGN-23 · Signing in with the setup account at the printed address opens the first organization, with that account as its owner.
- SIGN-24 · A repeated setup asks for no account and prints the address and the existing username.
- SIGN-25 · `agentic-org account add` on the HQ machine creates an account that can sign in at the printed address.
- SIGN-26 · `agentic-org account password <username>` on the HQ machine changes the password: the old one is refused and the new one signs in.
- SIGN-27 · The sign-in page and the dialog meet §4a.7: labelled fields with username and current-password autocomplete, the stated focus order, errors linked with `aria-describedby` and announced, and no meaning by colour alone.
- SIGN-28 · No browser screen lists, adds or edits accounts, and HQ's HTTP API refuses any request to create an account.

**Recovery code (RCV)** *(new, rev 4, §4a.6)*
- RCV-1 · The recovery code never reaches the browser: a fixture code appears in no HTTP response, event, page, notice or copied text.
- RCV-2 · While the code is waiting, the organization's owner sees the notice on every page of that organization, with the collect command and Copy command, and no close button. Other accounts don't see it.
- RCV-3 · While the code is waiting, Needs you shows "Collect the recovery code for <organization> on <HQ machine>.", and the item opens the Recovery row. Without Needs you, the notice and the row still show.
- RCV-4 · The Recovery row in Settings › Organization shows the exact copy for each state: being prepared, waiting to be collected, collected on <date>, couldn't be prepared with its reason, and status unknown.
- RCV-5 · No browser state is labelled collected or ready before the acknowledgement is recorded; status unknown is never shown as collected.
- RCV-6 · `agentic-org recovery-code collect` shows the code once and records the acknowledgement only after the typed last group matches and the owner types `yes`.
- RCV-7 · A typed group that doesn't match is refused with "That isn't the last group of the code…" and nothing is recorded.
- RCV-8 · Interrupting the command before `yes` records nothing, and running it again shows the same code. The browser still says waiting.
- RCV-9 · After the acknowledgement, the browser shows "Recovery code collected on <date>" and the notice and Needs you item go away, without a reload while live updates run.
- RCV-10 · After the acknowledgement, the collect command prints when it was collected and that it can't be shown again, prints no code and exits with an error status.
- RCV-11 · While the package is being created, the browser shows "being prepared" and the command says to try again in a minute, with no code.
- RCV-12 · When the package couldn't be created, the browser and the command show the reason and `agentic-org doctor`, with no code.
- RCV-13 · Interactive setup offers to collect the code at its end; answering n leaves it waiting and prints the collect command.
- RCV-14 · An organization created in the browser starts as being prepared, then waiting, and is collected with `--org` naming it.
- RCV-15 · The collect command run without an interactive terminal prints no code.
- RCV-16 · The notice, the Recovery row and the collect command each say that recovering after losing the HQ machine isn't supported yet.
- RCV-17 · CLI output for setup, account and recovery commands carries no meaning by colour alone, and writes no colour codes with `NO_COLOR` set or when output isn't a terminal.


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
