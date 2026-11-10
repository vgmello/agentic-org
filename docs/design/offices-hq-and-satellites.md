# Design: offices, HQ and satellites

Status: **approved by the user** 2026-11-08 (decisions at the end); later amendments by ada (2026-11-08: adopting a stored create; the launch pipe, ephemeral `app_token`, name reservation, the dev-build fallback, the host-crash trade-off, structural launch stdio, `--discard`, where the app-only rule for org commands is enforced, phase 2 enrollment proof left open; key custody). To be recorded as an ADR. It settles the model, the rules and the phases. Wire formats and byte-level protocol details are written in each phase's own design before it's built. Owner: ada. Its decisions become an ADR, which amends ADR-0001 and ADR-0002, and the messaging ADR (0003, still a proposal).

## In one paragraph
An organization has offices. **The first office is HQ**. It holds the organization's authoritative data (members, agents, teams, projects, roles, org chart) and the **registry of the organization's offices**, with each one's address. Every other office is a **satellite**, on this Mac or another machine. A satellite keeps a **full copy** of the organization's data, so it reads locally and has a copy if HQ is lost. A satellite never changes that data itself. It **sends each change to HQ**, and HQ checks it, commits it and records the **result** as an event in its log. HQ then **propagates the event to every satellite**, the sender included. **Work data is different:** tasks and agent messages are **office-first**. An agent runs on its office's machine and talks to that office. The office saves the work locally and answers in milliseconds, then syncs it to HQ in the background, and HQ passes it on to the offices that need it. **Each office is its own `org-office-server` process**, with its own folder, encrypted databases and key.

## The model
- **Organization → offices.** An office belongs to exactly one organization. Creating an organization creates its HQ.
- **HQ** (`role: hq`):
  - It is the single writer of organization data and keeps the event log.
  - It provisions satellites and keeps the registry of offices: id, name, role, address, status, last event applied, last seen, app version.
- **Satellite** (`role: satellite`): local (a second office on the same Mac) or remote. It is provisioned by HQ and stores HQ's address. It holds a full copy of the organization's data and its own runtime state.
- **Organization data** (structure, which changes rarely) is owned by HQ and copied everywhere. It covers members and agents, teams, projects, roles, the org chart and the registry.
- **Work data** (tasks and messages, which change constantly) is **office-first**. See the next section.
- **Runtime summaries** (e.g. "3 agents running") are not logged. Each office sends HQ passing status updates over its connection. HQ keeps the latest one per office in memory and forwards them to the other offices, so the app can show the whole organization without growing the log or forcing a full flush per status change.
- **Office runtime** is owned by each office and never copied: running sessions, live status, the scheduler's run queue, caches. (Tasks themselves are work data and do sync.)
- **The app always talks to an office on its own Mac.** It prefers that organization's HQ if HQ runs here. A Mac with only a satellite reads from it and sends changes through it. "Open organization" in the app means joining an existing organization by enrolling a new satellite on this Mac (see Provisioning).

## Processes on one Mac
The boundary between the host and an office server (who writes what on disk, the pipe protocol, what the host does with each failure) is in [`office-host-boundary.md`](office-host-boundary.md).

- **One office = one process**: `org-office-server --office <office-id>` for an existing office (a new one starts with `--create` alone, and an unfinished one is removed with `--discard <office-id>`, see below). It owns `<home>/offices/<office-id>/`:
  - `office.json`: plaintext, no secrets. It holds the office id, the organization's id and name, the role, and HQ's address.
  - `org.db`: SQLCipher. It holds the organization data, HQ's original or a satellite's copy, and the copied command-result and revocation tables.
    - On a satellite it also holds the last applied position, committed in the same transaction as each apply.
    - On HQ it also holds the event log.
  - `office.db`: SQLCipher, the same key. It holds the office's own data: the work data it owns or received (tasks, messages), its outboxes, sessions and work queue.
    - On HQ it also holds **HQ-only secrets**, which are never copied: the organization CA's private key and the public keys of pending invitations.
  - `workspace/`: the office's files.

  Keeping them apart means replacing the copy (a snapshot) never touches the office's own data.
- **Keys** (amended 2026-11-08: key custody): **`org-office-host` creates and holds every office's key.** It makes the key for a create, before the office's first `--create`, and keeps it in the Data Protection keychain (service `org.agentic.office-key`, account = the office id, in the signed host's access group, this Mac only; debug builds and tests use a dev-keys file store). It sends the key to the server on the first pipe line of every create, start and discard. The server never creates, stores, loads or deletes a key, and never touches the Keychain. Why: a same-user process could stop the host and run the signed `org-office-server` itself with its own app token, and a Keychain item owned by the server would trust it. With the host holding the keys, a server started by anyone else gets no key and can't open `org.db`. The details are in [`office-host-boundary.md`](office-host-boundary.md) ("Key custody").
- **An office lock:** the server takes an exclusive `flock` on its folder at start. Two processes can never write one office.
- **`org-office-host` is the one login service.** macOS lets an app register a fixed set of login programs, not a variable number, so one supervisor covers them all.
  - It starts one `org-office-server` per office folder on this Mac that has a valid `office.json`, and restarts a crashed one with backoff. It looks only at folders named by a plain office id.
  - **A folder without an `office.json` is an unfinished create.** The host never starts it, and doesn't delete it itself: it runs `org-office-server --discard <office-id>`, handing it the office's key, or none if the host holds none (amended 2026-11-08: key custody; amended 2026-11-09: since launch version 3 the key goes on the key hand-off, not the first line, see `app_token` below). That server takes the office lock and refuses a folder that holds `office.json` or databases with a committed organization. It deletes the folder only when no database file exists, or when it opens them with that key and they hold no committed organization. A database it can't open or check (the key doesn't open it, or the host sent none) means it refuses too (`storage_error`). On any refusal the host logs it and leaves the folder and its key, so a real office is never destroyed automatically. The server never deletes a key: the host deletes that office's key item only after the server reports "discarded". `org-agent`'s scan ignores an unfinished folder too (ADR-0003 §2.1).
  - **A folder with an unreadable or invalid `office.json`** is left alone and logged: the host neither starts it nor runs `--discard`. **A folder whose `office.json` names another office id** is never started: the host moves it to `offices/.refused/<id>`, `<id>` being the folder's name.
  - For "create organization", it starts a new office server with `--create` as its only argument and the host passes every detail of the new office (its id, its role `hq`, the organization's id and name, and its key) to it through the pipe between them, never on the command line. That server creates the office and then goes on serving it. Once creation has finished (the office's `office.json` is written), later starts use `--office <id>`; a create that fails or crashes before that is never restarted. On a create that fails BEFORE `office.json` is in place, the host keeps the key it made and runs `--discard` for that office with it, deletes the key only after the server reports "discarded", and returns the error to the app (amended 2026-11-08: key custody). Once `office.json` is in place, the host never runs `--discard`: if the stored `office.json` matches the plan, the host adopts the office (the create succeeds, see office-host-boundary.md). A created office whose `office.json` doesn't match the create request (another office id, `org_id`, `org_name`, `office_name` or role) is refused: the host stops the server and moves the REQUESTED id's folder to `offices/.refused/<requested id>`, deleting nothing, so a person can inspect it. The host acts only on the folder of the office id it requested, never on an id the server reported, and never touches any other office. The pipe is long-lived and two-way: stdin carries the host's lines to the server (the first one at every start, `--discard` included), stdout carries the server's control lines to the host ("created", "ready", "discarded", "failed"), and the server exits when stdin closes. The server logs to stderr only. No child can read the host's lines or write a control line, and that's structural, not a convention. The guarantee is the fd swap: first thing at startup, before any thread or child starts, the server dups fds 0 and 1 to new close-on-exec (`CLOEXEC`) descriptors, which carry the pipe, and reopens 0 and 1 on `/dev/null`. So a child gets `/dev/null` on fds 0 and 1 unless it's given its own pipes, whoever spawns it. fd 2 (stderr, the logs) is inherited. Every other fd the server opens is close-on-exec, the `office.lock` fd above all: a child that inherited it would keep the lock after the server died, and the lock would no longer mean the server is running. The server does the swap itself; the spawn helper doesn't. One spawn helper, `org-supervise` (River's, not on `v1` yet), is the single spawn path: `posix_spawn` with `POSIX_SPAWN_CLOEXEC_DEFAULT`, so a child inherits only the fds it's given (it needs libc FFI; the `unsafe` stays confined to `org-supervise`). The pipe protocol and `office.json` will be defined in a new contract crate, `org-office-launch` (see "Where the code lives").
  - **Creates reserve their name:** the host's **catalog mutex** (in-process; the host also holds the **single-host lock**, `host.lock`, for its whole life) covers only the name check and the reservation: it checks the name against this Mac's organizations *and* the names reserved by creates still in flight, reserves it, and releases the mutex. The slow part (starting the server, waiting for "created") runs outside the mutex, so `organization.list` and `office.list` never wait on a create. A reservation ends when its create succeeds or fails.
  - **`app_token`:** the host makes each office's `app_token` and keeps it only in memory, and passes it to the server at every start (ADR-0003 §2.1). An office created while the host runs gets its token at create time. A host restart rotates every token. (amended 2026-11-09: the key hand-off, launch version 3, ada) Neither the token nor the key travels on the pipe: the first line carries only a key socket id, and the host hands both secrets only to the admitted server, over a one-shot per-spawn socket it binds after admitting the server and that the server connects to itself, after its own fd hygiene ([`office-host-boundary.md`](office-host-boundary.md), "Only the signed server gets a key"; ADR-0003 §2.1). Residual risks remain while a same-user process can resume the server before the host admits it (it chooses the server's launch context, stdio pipes and Mach special ports; a pre-`main` write to a socket on fd 2 is refused by the host's peer-fd check, amended 2026-11-09, but a fileport via the special ports could still reach the secrets, so these stay open until the gate), so before any signed build holds real data the host sets a spawn-time launch requirement on the server's spawn (public API only; Mach messages with the audit trailer as the fallback), and release builds require macOS 14.4 or later.
  - **Dev builds:** an unsigned dev build of the app can't pass the host's code-signature check, so it gets `app_token` through a debug-only fallback in the host: the caller must run as the same uid and from the app's exact executable path (`Contents/MacOS/<exe>` inside the dev bundle, as the OS reports the process's executable), the fallback exists only under `cfg(debug_assertions)`, and every use is logged loudly (the log line never contains the token). Same-uid access is an accepted dev-only risk: any same-user process that can run from the dev app's executable path, or inject into an unsigned build, can get the token. That executable path is a compile-time constant of the dev build (`ORG_DEV_APP_PATH`), never read from config or the environment; if it doesn't exist when the host starts (the app isn't built yet), the host logs that and runs with the fallback off. Release builds never have it: the release profile keeps `debug-assertions = false`, and a CI check confirms the release host binary has no fallback symbol. (amended 2026-11-08: host review) There is no CI yet, so that check waits until CI exists; until then it's run by hand on each release build (`strings` and `nm` on the release host binary show no fallback text and no `proc_pidpath`).
  - Its `office.list` tells the app which offices run on this Mac and how to reach each one. `org-agent` finds its office from `office.json` (ADR-0003 §2.1).
  - It holds no organization data.
- **Local addresses:** a Unix socket at `<home>/run/<handle>.sock`. The handle is the last 12 characters of the office id (its random part), checked for uniqueness when the office is created. macOS caps a socket path at 103 bytes, so the host checks the length and fails with a clear message if it would be too long.
- **If the host dies, its office servers stop too.** They run in the host's process group and also exit when the pipe to the host closes, so no office is ever left running unsupervised. When launchd restarts the host, the host starts them all again. The office lock means a slow-exiting server and its replacement never overlap: the new one waits for the lock.

## Office-first: tasks and messages
Agents run on their office's machine and talk only to that office, through the local CLI (`org-agent`). Work data must be fast, so it doesn't go through HQ first:
- **Each record has one owning office,** which is the only one that changes it:
  - **A message** is created by the sender's office and never changes. Only its read state changes, and the **recipient's office** owns that.
  - **A task** is owned by one office at a time *(decided by the user, 2026-11-08)*:
    - **The assignee's office owns it.** The assignee is an agent, which belongs to an office, or a person.
    - **Whoever creates a task can assign it to someone elsewhere.** A task created in the app is created at **HQ**. A task created by an agent is created at **that agent's office**.
    - When the creator's office isn't the assignee's office, the creator's office creates the task and hands it over to the assignee's office straight away (see Handover).
    - A task assigned to a person is owned by **HQ**, which is where people work through the app.
    - An unassigned task stays with the office that created it.

    The owner applies changes instantly.
- **Changes from other offices** to a task are **change requests**, handled like organization commands but by the task's owner:
  - Each request carries a change id and the task version it expects. It travels to the owner through HQ.
  - The owner accepts it (a new task version, streamed out as usual) or rejects it (`conflict`, `not_owner`). The result goes back to the requesting office.
  - **The owner keeps a `change id → result` table** for 30 days, with the same rules as HQ's command results, and passes it on with the task at a handover. So a retry after a commit gets its original answer, not a false conflict.
- **Handover:** ownership moves **only through the current owner**. The current owner emits a handover record naming the new owner, for example when the task is reassigned to an agent in another office, or when an agent moves office (an organization change, after which the old office hands over that agent's tasks).
  - Change requests in flight are forwarded to the new owner.
  - Until it receives the handover, the new office can't act as owner, so a handover never puts two owners in charge. A reclaim can, briefly (see below).
- **Reclaim, when the owner can't hand over:** an owning office may be removed, lost, or offline too long.
  - HQ can then **reclaim** its tasks, as an organization event: on office removal, in the promotion's epoch record, or by the owner's explicit choice in the app.
  - Ownership moves to HQ or to the assignee's current office, at the latest task version HQ holds.
  - Every task version carries an **ownership generation** that a handover or reclaim increases.
  - **A reclaim can split ownership for a while.** If the old owner was only offline, it still believes it owns the task and keeps confirming changes to its agents until it hears of the reclaim. So the warning before a reclaim says so.
  - When the old owner comes back, its stale changes are **not dropped**. It may resend them before it hears of the reclaim. So **HQ alone** turns each task version from an older generation, numbered above the reclaim's base version, into a change request to the current owner.
    - Its change id is built from the version's (origin, seq), so it happens once. Its age for the 30-day rule is the version's creation time.
    - The first such request expects the reclaim's base version, and each later one the version its predecessor produces, as in the outbox.
    - Each is accepted or rejected like any other request, and rejected ones stay visible, flagged, for the owner to resolve.
    - HQ **never forwards** older-generation versions. Applying a reclaim resets the task to (new generation, base version) at every office, so an office that receives the reclaim late can't keep a diverged copy.
  - Change requests already in flight at a handover are forwarded, as above. The generation applies to task versions, not to requests.

  Every change has exactly one office in charge, so offices don't conflict over work data. The one exception is a reclaim from an office that was only offline, handled above.
- **Locally first:** the owning office commits the record (a durable commit, ADR-0002) and answers the agent in milliseconds. A message between two agents of the same office is delivered right there, and waiting agents wake immediately.
- **Then to HQ, in the background:**
  - Each office numbers every **work change** it makes (a new message, a read mark, a task version, a handover) with its own counter (origin office, origin seq), and streams them to HQ over its connection.
    - The number is allocated **in the same transaction** as the change.
    - An origin id is never reused: it includes an **incarnation id**, new whenever an office is re-enrolled or restored from a backup. HQ stores them, deduplicating by (origin, seq), and acknowledges up to a number. Later changes to the same task are separate changes, never dropped as duplicates.
  - HQ forwards each record to the offices that need it: a message to the recipient's office, a task change to the task's owning office and to the offices that follow that project.
  - The same works in reverse. Offices track, per origin, the last number they have, so a reconnect resends only what's missing.
  - **A new satellite, or an office that starts following a project,** gets a **filtered backfill** from HQ: the current work records for that scope.
    - An office keeps its stream positions **per scope**, so a backfill for a new project never moves the positions of scopes it already follows.
    - Because a backfill, the live stream and changes from a new owner after a handover can arrive in any order, a task version is applied **only if it's newer** than the one held.
    - **"Newer" means (generation, version), compared in that order,** everywhere, HQ included. So a stale owner's higher version number from an older generation never overwrites a reclaimed task.
- **References to structure** (a task's project, a message's agents): each work change carries the **organization position (epoch, seq)** its origin had applied.
  - A receiving office checks the references only after it has applied that position itself, so a message never arrives "before" its agent exists.
  - If the structure has since moved on (e.g. the project was deleted), the record is kept and flagged, not lost. Flags are checked again on later applies, so a reference that becomes valid clears.
  - A work change may refer only to structure the origin has already applied, not to its own still-queued organization commands. The origin rejects such references at once.
- **Across offices:** a message or task change takes two network hops (office → HQ → office) over connections that are already open, with no polling (HQ relays; satellites don't connect to each other, decision 7). An `ask` to an agent in another office waits at the asking agent's own office, and the reply arrives the same way. The ask carries its deadline, so a request delivered after it expired is marked expired, not acted on.
- **Storage:** messages are stored for heavy traffic: written once, grouped by conversation thread, bodies compressed and kept apart from small headers, unread messages kept in a small list, writes batched. The messaging ADR (0003) defines the exact structure.
- **HQ keeps all work data**, and each satellite keeps what concerns its agents and projects (decision 3).

## How organization changes flow
1. A client (the app; an agent only where ADR-0003's table allows) asks its local office for an organization change: a **command**, e.g. "create project X in team Y". The command carries:
   - a client-made **command id**
   - **client-made ids (ULIDs) for anything it creates**, so a later command, even one still queued, can refer to it
   - when it edits something, the **version it last saw**
2. **The office passes it to HQ.** At HQ itself this is a function call. A satellite forwards it over its connection to HQ.
   - **The receiving office enforces who may send it:** the office the client is bound to. An org command from a connection that isn't the app's is refused there with `not_authorized`, unless a row of ADR-0003's call-by-call table allows it. Today every org command is app-only (teams design).
   - HQ accepts org commands only from its own office's connections, checked as above (today, the local app's), or from enrolled offices relaying them (mTLS from phase 3). Never from an arbitrary client socket.
   - **Open for phase 2:** how a local satellite proves it's enrolled. Same-user socket permissions aren't enough here, because agents run as the same user.
   - A relaying satellite has already enforced the app-only rule on its side.
3. **HQ validates the command** against its current state. If it's valid, HQ commits the **result as an event**, fully resolved: the timestamps and normalised names it chose, plus the client's ids. The event is appended to the log at position **(epoch, seq)**, carrying the command id.
   - A command whose expected version is stale is **rejected** (`conflict`), not applied over newer data.
   - **Rejections are events too.** So the `command id → result` table is part of the copied data, and a retried or replayed command gets the same answer at any office, including after a promotion, for results that office had received.
   - **Dedup window, by HQ's own clock:** command ids are ULIDs, which carry their creation time.
     - HQ rejects as `expired` any command dated more than 29 days ago, and as `clock_skew` any dated more than 5 minutes ahead of HQ's clock.
     - HQ keeps each result until its command's ULID time + 30 days.

     So a satellite's clock, fast or slow, can never make HQ apply a command twice.
   - HQ looks up a stored result **before** the age checks, and keeps a monotonic high-water mark of its clock, so a clock that steps back can't reopen the window.
   - Before showing an outbox entry as blocked, the satellite looks its command id up in its copied result table, in case it was committed after all.
   - **Versions** are per entity (a team, a project, …). An entity's version goes up by exactly one for each event that changes it, including events from commands that change several entities. Rejection events change no entity's version.
   - Retrying a blocked command uses a **new** command id, because the old id would return the old rejection.
4. **Propagation:** every satellite keeps one connection open **to HQ**, asking for "events after position P". HQ sends each event in order, and the satellite applies it with deterministic code. **Satellites never re-validate events.** HQ updates its own current state with the **same apply code, in the same transaction** as the event, so copies can't drift. The sender sees its own change arrive the same way.
5. **Reads** at any office come from its copy, tagged with the position it reflects. A client that just wrote can ask its office to wait until it has applied that position: read-your-writes.
6. **Snapshots:** a satellite that's new, or too far behind (HQ trims old events), gets a snapshot.
   - HQ exports the **copied tables** of its `org.db` into a temporary **SQLCipher database, encrypted with a one-time key** (never plaintext on disk, as ADR-0002 requires), in one short read transaction (so checkpoints aren't held up while a slow satellite downloads), tagged with its position and schema version. The export is a storage operation in `store.rs`. It includes the command-result and revocation tables, and never HQ-only data, which lives in HQ's `office.db`.
   - HQ streams the file in chunks over the authenticated connection, since a frame is 1 MiB, together with the one-time key. The satellite re-encrypts the copy with its own key.
   - The satellite builds a new `org.db` from it and swaps it in only when complete. Its `office.db` is untouched. Then the stream resumes.
7. **Deletions** are events too. Every copy that applies a delete event removes the thing. Trimming the log later doesn't bring it back.

**Connections:** each satellite keeps one outbound connection to HQ, and everything between them travels over it, in both directions. So a satellite behind a home router or on a sleeping laptop needs no open port.
- HQ records where each satellite last connected from (for display; satellites don't listen, decision 7), and **HQ's own addresses**, as the owner asked. HQ can relay requests to a satellite's office data (e.g. its live sessions) over that same connection.
- Satellites never listen for connections, and HQ relays everything between them (decision 7).

## Versions
- Events, commands and snapshots carry a **schema version**.
- When a satellite connects, both sides exchange the range they support.
- **A satellite too old** for HQ's events stops applying and shows "update needed". It never applies a half-understood event.
- **HQ** refuses commands it doesn't understand (`unsupported_version`).
- **An organization-wide protocol level**, recorded as an event, sets what everyone uses.
  - HQ raises it only once the registry shows that every **active** office supports the new level. Active means seen in the last 30 days. An office away longer must update before it syncs again.
  - No office sends commands, or HQ emits events, above the current level.
  - So updating any office, in any order, never stops the others. A new feature switches on once every office has updated, and the app can show which offices are holding it back.

## Provisioning a satellite
1. In the app, the owner adds an office: a name, plus local or remote.
2. HQ adds a `pending` registry entry.
   - **Local** (a second office on this Mac, phase 2): the host has the office created directly: it starts a new office server with `--create` and passes it its details through the pipe, never on the command line (the `org-office-launch` protocol, as for HQ). No link, TLS, certificate or approval is involved: trust rests on both offices running as the same macOS user. Steps 3–4 don't apply, and the satellite goes straight to step 5.
   - **Remote** (phase 3): HQ makes a **one-time invitation link** (see "The invitation link" below).
3. **Remote:** the owner copies the link to the other machine and pastes it into Agentic Org at "Open organization" (recommended), or opens it, which pre-fills the same screen.
4. **Remote: the satellite makes its own key pair** and keeps the private key in its Keychain. It sends HQ a certificate request together with its **proof of the invitation** (see "The invitation link"). The invitation secret itself never crosses the network.
   - HQ checks the proof, waits for the owner's approval, signs a client certificate with the organization's CA, and marks the invitation used.
   - **Certificates say who and what they are** (from phase 3): office id, role, and client or server use.
     - A satellite's certificate is client-only.
     - The satellite pins HQ's office id and accepts only a server certificate with role HQ for that id. So no satellite can pose as HQ.
   - Certificates last one year and renew automatically over the authenticated connection.
5. HQ sends a snapshot, the stream starts, and the entry becomes `active`.
6. **Removing an office** revokes its certificate (the revocation list is copied data), so it can't sync again. HQ reclaims its tasks (see Reclaim), and its agents become **unplaced**: the owner places each one in another office, or offboards it. An offline machine keeps whatever copy it already had until it next connects, when it's told to delete it. HQ can't force that.

## The invitation link
*(The user, 2026-11-08: a link that HQ generates, which the owner copies and opens on the other machine. This section's details are **proposed**, awaiting the user's confirmation.)*
- **Shape:** `agenticorg://join#<token>`.
  - Agentic Org registers the `agenticorg` URL scheme. Opening the link opens Agentic Org at **Open organization → Join with a link** (the other path there opens an organization this Mac already has), with the details **filled in, never joined automatically**. The owner sees the organization's name and HQ's address and must confirm, so a hostile link can't quietly enroll a Mac into someone else's organization.
    - The confirmation says plainly that **joining lets that organization run its agents on this Mac**: until v2, agents run as you, so they can read and change your files, use your apps and network, and act on your behalf. Only join organizations you own.
    - It flags a name that matches an organization this Mac already knows under a different id.
  - The token is in the fragment (after `#`), so it isn't sent to a server if the link passes through a browser, and the app never logs it. A browser may still keep it in history, so **pasting it into the app is the recommended way**.
  - The same token can be pasted into the app as text.
- **The token** is versioned and base64url-encoded. It holds:
  - the organization's name and id
  - the new office's id and name
  - HQ's office id
  - HQ's current addresses, e.g. `hq-mac.local:7443` and its LAN IP (decision 6: direct network access only)
  - the fingerprint of the organization's CA
  - the expiry: 24 hours by default, shorter if the owner chooses
  - a **one-time invitation key**: the private half of an Ed25519 key pair made for this invitation alone

  Anyone who sees the link can read these, including the organization's name and HQ's network address. Treat the link like a password.
- **How it's protected.** This follows the standard for invite links: the link is the key until it's used, cancelled or expired, so the protection comes from limiting what it can do and checking who uses it.
  - **HQ keeps only the public half** of the invitation key, in its encrypted `office.db`. A leaked HQ database can't be used to join.
  - **The secret never crosses the network.** The joining office connects with TLS, pins HQ by **HQ's office id and the CA fingerprint** from the link, and signs the TLS session's exporter value (channel binding, RFC 9266) with the invitation key. Someone watching the network learns nothing, and a fake HQ can't reuse the proof.
  - **The owner approves every join at HQ, by typing a check code.**
    - The joining machine shows a short code, derived from **the invitation id and its public key**, so it stays the same if the connection restarts. It's 8 characters from an alphabet without look-alike letters (Crockford base32), shown as two groups of four. The proof itself is still bound to the TLS session (above).
    - HQ shows the joining machine's name and IP, and the owner **types that code** at HQ to approve, rather than just clicking. A thief holding a stolen link can't produce a code the owner sees on their own machine.
    - Rejecting a join cancels the invitation, and so do **three wrong codes** in a row.
    - In phase 3, adding an office and approving a join happen in the app on **HQ's Mac**.
  - **Single use, short expiry, cancellable:** the owner can cancel a pending invitation in the app. Failed attempts are rate-limited per source address, without using up the invitation.
  - **The joining office makes its own key pair first.** Its certificate request travels in the same TLS session as the proof, and the check code covers that request's public key. So what the owner approves is exactly the key that gets certified (Provisioning step 4).
- **"Internally encrypted", honestly:** everything the joiner needs travels in the link, so encrypting the token with a key in that same link would add nothing, and signing it wouldn't help either, because the joiner gets the checking key from the same link. The link's integrity rests on how the owner sends it: a private message, not a public channel. The real protections are the one-time key that never crosses the wire, the pinned HQ identity, and the owner's check-code approval.
- **Local joins** (a second office on the same Mac, phase 2) don't use the link or TLS. The host creates the office directly, and trust rests on both running as the same macOS user (socket permissions). That covers creating the office. How its connection to HQ proves it's the enrolled office, and not an agent running as the same user, is open for phase 2 (see "How organization changes flow").
- **When HQ's address changes** (a laptop on another network): HQ records its current addresses as a registry event, so every satellite learns them.
  - A satellite that can't reach HQ tries every address it knows, then looks for HQ on the local network (Bonjour) by HQ's office id.
  - Failing that, the owner types the new address on that machine. That's safe, because HQ is pinned by identity, not by address.

## When things are unreachable
- **A satellite can't reach HQ:**
  - Reads work from the copy, labelled "last synced …", and office data keeps working.
  - Organization changes wait in the satellite's **outbox**, in order. A queued edit to something already edited in the outbox expects the version that earlier edit will produce, so two queued edits of one thing don't conflict with each other. When HQ is back, they're sent **one at a time, in order**.
  - If HQ rejects one, the commands queued after it are held, not sent. The app shows them as **blocked** for the owner to retry or discard, because they may depend on the rejected one.
  - Until they're committed, queued changes show in the app as **pending**. They aren't in the copy.
- **Messages between agents** (ADR-0003, proposed):
  - A message between two agents **in the same office** is delivered by that office directly, even with HQ unreachable.
  - A message to an agent **in another office** goes through HQ, which delivers it to the recipient's office. While HQ is unreachable it waits in the sender office's **message queue**, which is separate from the command outbox, so a blocked command never holds messages back.
- **The app on a Mac with only a satellite:**
  - A task created in the app is still created **at HQ**: the satellite relays it there, and queues it in its command outbox while HQ is unreachable (the one work change that travels there). It shows as pending until HQ confirms it. The create carries a client-made task id and change id, so a retry can't create it twice. HQ always returns the create's result to the relaying office (as change-request results go back to the requester), and keeps that result for 30 days even after it hands the task over, so a retried create gets its original answer, so the satellite knows the outcome even when the task is handed to another office it doesn't follow.
  - Changes to person-owned tasks (owned by HQ) are change requests, queued while HQ is unreachable, like any other.
- **"Backup", honestly:** satellites follow HQ immediately, so a deletion or bad change reaches them too, and a local satellite shares HQ's disk. Satellites protect against losing HQ's machine, not against mistakes. Real backups (point-in-time snapshots) are a separate feature.

## Losing HQ: promotion (phase 4, designed in full before it's built)
If HQ is gone for good, the owner can **promote** a satellite to HQ. It's a deliberate act in the app, never automatic. Promotion is the hardest part of this design, so here are the requirements its own design must meet, not a finished protocol:
- **A signed epoch record.** Promotion starts a new epoch. The owner authorizes it, and it produces a record naming the new HQ's **office id** and address, and the last position it holds. Satellites re-pin to that office id.
  - Promotion issues the new HQ a **server certificate** (role HQ, its office id, the new epoch).
  - A satellite accepts it in place of its pinned HQ only when it's presented together with the CA-signed epoch record naming that office id.
  - **The new HQ must be reachable by the other offices.** Satellites only dial out (decision 7), and there's no relay service (decision 6), so the new HQ needs a direct network address the others can reach.
  - An office learns of the new epoch when it connects to the new HQ, at an address it is given. The owner gives it in the app on that machine.
  - An office that applied events beyond that position takes a fresh snapshot. Those events, which only the old HQ had, may be lost, and the app warns before promoting.
  - **An office the record can't reach**, including the old HQ while it's asleep or offline, has to be told on its own machine. Until then it can keep following the old HQ, so the design must say plainly that a split is possible until every office has heard.
- **Certificates also carry the epoch** (role and office id are there from phase 3). An office rejects any HQ below the highest epoch it knows. A former HQ that learns of a higher epoch becomes read-only and re-joins as a satellite with a fresh snapshot.
- **The organization's CA key never sits usable on a satellite.** It's escrowed, encrypted under a **recovery code the owner keeps**, and unlocked only to promote.
  - The code is generated by the app (at least 128 bits, shown once) and stretched with a memory-hard key derivation (Argon2id).
  - So a stolen Mac's copy can't be brute-forced. A satellite or a stolen Mac can then neither impersonate HQ nor re-certify itself.
- **Revocations are part of the copied data**, so every office refuses a removed office's certificate.
- **Dedup across promotion** works for every result the promoted office had received, because command results, rejections included, are copied data. Results only the old HQ had are lost, as above.
- **Work data across promotion:**
  - The new HQ held only part of the work data, so **each office republishes the current records it owns**, and their earlier changes, from the new HQ's per-origin positions. To make that possible, each origin keeps its own change log for the retention period.
  - Dedup by (origin, seq) makes repeats safe.
  - Work data that only the old HQ held (its own tasks and messages) may be lost. The warning before promoting says so, and the reclaim rule above applies to the old HQ's tasks.
- **The escrowed CA key** is created in phase 3, together with the owner's recovery code, as copied data, so organizations created then can be promoted later.

## Where the code lives (architecture rules)
- **`org-sync`** (new library): the **replication logic**, independent of storage and transport:
  - positions and epochs, the order in which events are applied, the outboxes and their ordering, per-origin work streams, version levels
  - what an event *means* stays in `org-office`: `org-sync` calls an apply trait that `org-office` implements, so the domain never leaks into `org-sync` and there's no cycle
  - the sync session (stream, fan-out, relay, status updates) as a state machine over an abstract connection

  It works over a small **storage trait** that `org-office`'s `store.rs` implements, so all SQL, snapshot export included, stays in `store.rs` (ADR-0002's swap point). The server only supplies the transport.
- **`org-office`:** the domain. It validates commands, produces events and uses `org-sync`. It knows nothing about networks or certificates.
- **`org-pki`** (new adapter crate): enrollment, the organization's CA, certificates, TLS. Only the server binaries use it.
- **`org-office-launch`** (new contract crate): the pipe protocol between the host and an office server it starts, and the `office.json` file the server writes and the host reads. The host and the office server use it, and `org-agent` reads `office.json` through it (ADR-0003 §2.1). It stays out of `org-proto`, so the crate every client depends on doesn't grow.
- **`org-sync-proto`** (new contract crate): the office-to-office protocol (stream, snapshot chunks, enrollment, status updates). It stays out of `org-proto`, so the crate every client depends on doesn't grow.
- **`org-office-server` and `org-office-host`** stay thin, each with a library target for its logic.
- **Owners (AGENTS.md):**
  - **River** (`ac-software-engineer-river`) owns `org-office-host`, `org-sync` and `org-pki` (this design's crates); AGENTS.md is the full ownership list.
  - **server-dev** owns `org-office` (incl. `store.rs` and its implementations of the storage and apply traits) and `org-office-server`.
  - `org-sync-proto` and `org-office-launch` are contracts, so ada owns them and reviews changes, like `org-proto`. River writes `org-office-launch`'s first version.

## Phases
1. **Now: HQ only.** Create organization = start a new HQ office.
   - HQ stores organization data as events + log + current state from the start. Commands and results are versioned and the log has positions, so later phases add satellites on top. Schema changes will still need migrations, as always.
   - Introduce `org-office-host` with one office.
2. **A local satellite** on the same Mac: provisioning, the stream, snapshots, versions and the outbox, without networking. Authentication between two local offices rests on both running as the same macOS user (socket permissions). Certificates come in phase 3. How a local satellite's connection proves it's enrolled is open (see "How organization changes flow").
3. **Remote satellites:** TCP + TLS, the organization's CA and its escrow with the owner's recovery code, invitation links, direct network access to HQ (decision 6).
4. **Promotion:** epochs, re-joining a former HQ, and CA escrow in use.

## What this changes in earlier decisions
- **ADR-0001:**
  - An office belongs to an organization.
  - `org-office-server` runs one office, and `org-office-host` is the login service.
  - Offices keep running when the app quits (ADR-0001), but they run under the host: a host crash or restart stops every office on this Mac (and agents' messaging) until the host restarts them, and rotates all app tokens. This is the phase-1 trade-off.
  - The app talks to an office on its own Mac, and its menu lists organizations, each with its offices.
- **ADR-0002:**
  - **Each office has its own two databases:** `org.db`, the organization data or its copy, and `office.db`, its own data. They share one key, kept under the office id. (amended 2026-11-08: key custody) The host creates and holds it, in the Data Protection keychain on this Mac only, and sends it on the launch pipe; the server never stores one. (amended 2026-11-09: launch version 3) It now goes on the key hand-off, never on the launch pipe ([`office-host-boundary.md`](office-host-boundary.md)).
  - A key is created only for a create, by the host. The host never makes one for an existing office: a key item it doesn't have is `key_missing`, set by the host without starting the server, and a key that doesn't open the databases is `key_missing` (fatal), a `failed` code on the launch pipe.
  - Layout `<home>/offices/<office-id>/`.
  - HQ's `org.db` holds the event log. SQLCipher, one writer per database, durability and the swap point hold per office.
- **ADR-0003 (proposed):**
  - `org-agent` talks to **its own office**, so binding needs only the agent.
  - Messages are office-first: committed and delivered locally, then synced to HQ, and through HQ to other offices. The storage structure for heavy traffic and the cross-office `ask` are defined there.
- **`org-proto`** (client ↔ office):
  - The host gains `hello`, `organization.create`, `organization.list`, `office.list` and `office.app_token` (ADR-0003 §2.1).
  - `not_authorized` and `unavailable` are org-proto error codes (`office.app_token`'s). `key_missing` isn't: it's a `failed` code on the launch pipe (`crates/org-office-launch`, which River is writing; it isn't on `v1` yet).
  - An office's `hello` reports its organization and role.
  - Organizations list their offices from the registry.
  - The exact shapes are drafted after this design is agreed.

## Decisions (the user, 2026-11-08)
1. **Promotion** is manual, by the owner, with the recovery code.
2. **While HQ is unreachable,** satellites queue organization changes, as designed.
3. **Work-data copies:** HQ keeps every task and message, and each satellite keeps what concerns its own agents and projects.
4. **Project files are git repositories, connected to GitHub first.** See `projects-git-github.md`. The earlier "git service at HQ" idea is replaced: GitHub hosts the repositories, and each office clones from it.
5. **Remote setup:** an invitation link that HQ generates, copied and opened on the other machine (see "The invitation link").
6. **Reaching HQ:** direct network access only. There's no relay service for now. HQ's addresses travel in the link and the registry.
7. **Satellites don't listen.** They connect out to HQ and keep a full-duplex connection open. **HQ is the relay:** anything between satellites goes through HQ.
8. **All offices are the owner's.** There are no permissions between offices for now. Permissions between employees of different offices may come later and stay an open topic.
9. *(Assumption, not a decision yet)* Every Mac that opens an organization is a full satellite. A lighter "connect without a copy" mode can come later.
10. **Task ownership:** the assignee's office. A task can be created anywhere (in the app, at HQ; by an agent, at its office) and is handed to the assignee's office.
11. **One recovery code per organization**, kept by the owner.
