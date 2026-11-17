# Design: offices, HQ and satellites

Status: **selected office/data target reconciled 2026-11-15; networking and
recovery implementation remain gated**. The [authority map](documentation-authority.md)
and organization-data documents own relationships/canonical sources. The 2026-11-08
transport/launch design supplies transport and failure requirements, not proof
that remote enrollment, restore or promotion is implemented. Current executables
are `agentic-org`, `agentic-org-office`, `agentic-org-web` and `agent-os`; host
lifecycle is `crates/org-host`. See [availability](executable-migration.md).

## In one paragraph

An organization has one HQ and any number of satellites. Offices place agents;
the team forest, employees, projects, repository registrations and counters belong
to the organization. HQ creates teams/projects/releases/milestones and coordinates
shared numbering and execution admission. Any office may durably save tasks or
features, even disconnected, with final readable references pending authority
allocation. Organization Git is remote and holds portable definitions/open work;
protected chat, accounting, budget, execution and recovery evidence have their own
durable sources. SQLite indexes these sources. A copied projection alone grants
neither access to private data nor authority to run another process.

## The model

- **Organization → offices.** Creating an organization creates its HQ and records
  stable ID, explicit organization slug, remote repository registration and
  `state_branch`. Offline HQ setup validates that remote URL/branch and local
  credential configuration without contacting any external service; remote
  connectivity/publication remains unverified until online operation is authorized.
  Office definitions (`id`, slug, name, role, revision/provenance)
  live at `offices/<office-id>.yaml` in organization Git; network endpoints,
  enrollment status, credentials and connectivity belong to local operations.
- **One home team per employee.** Agents and people require `home_team_id`;
  optional role and direct reporting are separate. A lead is a direct employee
  appointed to manage the subtree. Office movement does not change membership.
  Owner login authority is a host principal, never a teamless employee flag.
- **Projects and code.** Each project requires one `owner_team_id`, optional
  contributing teams and zero or more organization repository associations.
  Each association has its own required `integration_branch`. An office uses
  one bare cache per repository, even if several projects share it. Code repos
  may be local-only; the organization repository cannot be local-only.
- **Accountability and assignment.** A task requires accountable `team_id`, equal
  to its project's owner when project-scoped. Projectless tasks belong to a team.
  Nullable `assigned_team_id` and `assignee_id` are independent from that owner
  and changed atomically at an expected revision. Reporter is the immutable
  creator. All task types use T-; a reference such as `T-A42` is complete on its own.
- **Runtime placement.** One office worker supervises logical agent controllers,
  turns/attempts and owned processes. A runnable agent needs separate execution
  config, model profile, fresh local connection evidence and a shared permit.
  Placement requires the durable generation and prior-process cleanup/fence.
- **Access.** The browser authenticates to HQ; no local office is needed. Initial
  setup creates HQ only; separate HQ-issued enrollment creates satellites.
  Replication scope never overrides chat visibility, private-note author-only
  access or cost/budget authorization. Presence is ephemeral, not authority.

## Processes in one instance

The host inside `agentic-org serve` supervises one `agentic-org-office` per office
and a separate `agentic-org-web` for the instance browser listener. It owns office
DB keys, instance/catalog locks and the private launch capability; the browser
server has no DB-key entitlement. The worker owns domain operations, the stored
`office.json` marker and its office lock. The full current launch-v2 framing,
fd isolation, key namespace/signing identities, adoption/quarantine/discard rules,
restart limits and error mapping remain in [office-host-boundary.md](office-host-boundary.md).
They must be preserved through any target adapter. Filenames do not rename
`org.agentic.office-key` or signing identifiers.

The selected target separates data by durability:

```text
<data-root>/<organization-id>/
├── durable/                  # sources: back up all of it; never removed by cleanup
│   ├── state/                # ordinary organization Git files
│   ├── journal/              # acknowledged unpublished changes
│   ├── authority/            # numbering, placement generations, permits (06, 11)
│   ├── accounting/           # durable usage streams/checkpoints (10)
│   ├── messaging/            # protected envelopes and attachments (15)
│   ├── recovery/             # receipts, streams, journals, snapshots, cleanup (16)
│   ├── budgets/              # policies, periods, events, extensions/holds (17)
│   ├── private/              # HQ only: author-private task notes (05)
│   ├── local/offices/        # private local bindings, never Git
│   └── checkpoints/          # local/unpushed code recovery
└── runtime/<office-id>/      # caches: rebuildable
    ├── office.db             # projections and explicitly transient state
    ├── repositories/<repository-id>.git/
    └── teams/<root-team-id>/teams/<child-team-id>/.../agents/<employee-id>/
        ├── projects/<project-id>/tasks/<task-id>/repositories/<repository-id>/
        └── tasks/<task-id>/repositories/<repository-id>/
```

The [organization-data index](organization-data/README.md#shared-storage-contract) owns this layout. Provision and protect `durable/` independently of the disposable `runtime/<office-id>/` caches.
Private task notes use a separate durable author-private DB; host credentials/CA
pins and execution journals retain their protected custody. The current runtime
still uses encrypted `org.db`/`office.db`, `office-state/` markers and legacy
workspaces. In the code today those databases are the only copy of what they
hold, so they are not caches until the store writes the selected durable sources.
Re-derive sandbox denies before switching paths. Handoff preserves task worktrees until closure; local-only and
unpushed edits require protected checkpoint recovery before cleanup.

Host death stops workers and owned processes; restart requires lock exclusion,
valid credentials and process reconciliation. Never signal a restored PID without
ownership mark, generation and machine/boot/start identity checks. Cache rebuild
cannot allocate placement authority. Missing evidence returns recovery-required,
not permission to launch. Initial setup/service registration and production web
handoff remain unavailable; launch v3/rendezvous and satellite enrollment remain
separate contract/implementation gates.

## Office-first: tasks and messages

Office-first means a permitted local write can be durable before network
publication. It does not mean offices own accountable teams/projects or allocate
HQ's counters; the one sequence an office keeps is for the tasks it creates
([document 06](organization-data/06-identity-and-numbering.md)). Persist stable IDs, revisions, domain source and required
journal/receipt evidence before acknowledging. Hourly Git publication cannot be
the only copy of an accepted change. Features may have `number: null` while
the shared authority registers their scope and allocates the final reference.
Preserve every acknowledged allocation across restore; never increment a replica
counter.

The existing work protocol's **owning office** is a serialization/transport role,
separate from task `team_id`, assigned team and individual. Current routing to an
agent assignee's office, HQ for a person, or the creator office when unassigned
remains implementation evidence; it cannot supply missing target accountability.
A target assignment operation atomically changes the pair with an expected
revision and a durable task event. Changing employee office or assignee must not
delete its task workspace or close the task/turn implicitly.

Retain these current transport safety requirements when adapting that model:

- Cross-office edits carry change ID and expected version to the current writer
  through HQ; replay returns the original actor/fingerprint-bound outcome.
- Handover is emitted by the current writer. Before it arrives the destination
  cannot write; in-flight requests forward to the new writer. Handover transfers
  retry/result evidence as well as task state.
- Explicit reclaim while a writer is disconnected can split perceived ownership.
  Warn about stale local confirmations; increase the transport ownership
  generation and compare `(generation, version)` before accepting state. HQ
  turns retained stale changes into deterministic requests for reconciliation;
  never discard them silently. That transport generation is not an execution
  placement generation or a new task identity.
- Acknowledged source sequences/incarnations and high-water records survive
  restore. Repeated frames are deduplicated; restoration never reuses an old
  sequence. Current messaging content/state share their original allocator;
  do not split it into overlapping counters. Work has its separate stream.
- Filtered backfill retains per-scope positions and applies only newer versions.
  A referenced organization position must be applied before checking references.
  Retain unresolved historical references with explicit diagnostics rather than
  dropping records or reading unapplied queued structure as authoritative.

Messages have immutable content plus separately revisioned recipient/card state;
conversation participation is independent of team/office placement. Persist
protected JSON envelopes and verified attachments under document 15 before
acknowledgement. Retain delivery identities. Filter message
bodies, counts, reply previews and search by message visibility. An owner
requires the existing authenticated chat policy; private task notes have no
owner exception.

Cross-office messages travel office → HQ → destination on existing outbound
connections. Local messages may work without HQ; cross-office delivery waits in
its distinct durable queue and preserves deadlines. HQ's routing copy and a
satellite's relevant-work copy remain access controlled. No new purge, wake or
automatic scheduling engine is selected.

## How organization changes flow

1. Authenticate the actor at the browser or local agent boundary. Select an
   explicit typed operation and organization scope; an enrolled office certificate
   or same UID is not owner authority. HQ creates teams, projects, releases and
   milestones. Office-local task/feature persistence uses the allocation path
   above, rather than becoming a second HQ writer.
2. Validate required target fields/references and expected revision. The remote
   organization repository, state branch, home teams and accountable owner teams
   must round-trip at canonical stable-ID paths. Every ID is a UUIDv7, command and
   operation IDs included (identity contract); the code admits ULID command/change
   IDs today.
3. The admitting writer commits domain changes, required source journal evidence
   and the bound receipt/outcome at one recoverable boundary. One operation ID
   with another actor or fingerprint conflicts. Unknown side effects are
   reconciled before replay. Preserve ordered task events after closure/archive.
   Host/private commands retain their separate receipt stores; private-note
   existence never enters shared receipts.
4. Replicate using negotiated typed events/positions and deterministic apply;
   publish organization files through the selected remote Git branch separately.
   Neither Git publication nor a projection transaction alone proves the combined
   acknowledgement boundary. Define/test the adapter before claiming migration.
5. Reconnect from verified checkpoints or obtain a verified snapshot. Validate
   org identity, CA/HQ pins, codec, contiguous sequence/digests and highest epoch;
   hash-check staged snapshot bytes before install. Partial/stale snapshots confer
   no authority. The existing encrypted `org.db` snapshot/swap isolates `office.db`
   and keeps one-time transport keys private; the target importer must preserve
   those guarantees (except that under ADR-0016 the snapshot file is no longer
   encrypted with a transport key: it travels only over the office TLS connection
   and is verified by its content digest) and protected pending/native/base payloads losslessly.
6. Rebuild SQLite only from complete authorized Git plus protected sources.
   Credentials are rebound, acknowledged receipt/stream/allocation high-waters
   retained, and marked-process cleanup/fencing verified before execution. Lost
   usage, budget policy/extension/hold history, protected messages or authority
   cannot be synthesized from Git. Stop affected work if source coverage is lost.

Current command admission retains ULID age/skew windows (29-day admission,
30-day result retention; future skew at most five minutes) and lookup-before-age
behavior with monotonic clock high-water. Those are existing wire semantics,
not the stable-ID rules. Retention/expiry needs equivalent durable retry evidence;
do not silently purge receipts during cache rebuild. Hidden/unauthorized outcomes
remain scoped, and rejection never authorizes an automatic fresh-ID retry.

Deletions preserve selected tombstones, historical references and archived
identities rather than erasing the evidence needed by tasks, messages or budgets.
Satellite connections remain outbound/full-duplex; HQ relays between them and
records last seen. Satellites open no public app listener.

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

1. The owner creates an invitation through the authenticated HQ web application.
   HQ reserves the office identity and a pending registry entry.
2. On the target machine, the separate HQ-issued enrollment flow selects a named
   instance and joins the organization; initial `agentic-org setup` remains HQ-only. It displays the organization and pinned HQ identity before the
   owner confirms that the office may run agents there. Automation supplies that
   intent explicitly; opening a browser does not enroll anything.
3. The satellite's **`agentic-org-office` worker** enrolls and holds the office's **TLS
   identity key** (distinct from the office database key the host holds; see "Keys").
   Setup (`agentic-org`) never generates or holds it.
   - **Hand-off.** Setup hands the invitation to the host with `office.join
     { operation_id, invitation }` on `host.sock`, journaled; the invitation is a
     secret type. Its authority is the signed `agentic-org` peer (macOS; on Linux the
     same-UID residual of X-02 §2, a draft not yet in this repo; see office-host-boundary.md §3, "Pending: launch v3") plus the destination consent setup captured, in
     the wizard or as the explicit automation input (JOIN-2). OS user presence isn't
     required, so unattended, SSH and Linux joins work. The trust anchors are the
     one-time invitation and the owner's check-code approval at HQ. On an instance
     that already has accounts, `office.join` also needs an instance-administrator
     session, like Add office (standalone §2). `office.join`, `office.remove_pending`
     and the invitation as a secret type are an open point for X-02's next revision
     (X-02 §12 Q9; a draft, see the launch v3 note in office-host-boundary.md §3). The host creates
     the pending office and starts the worker with launch **v3** `create` (role satellite).
     The first line carries only the invitation's non-secret reference; the
     invitation secret goes over the verified rendezvous (X-02 §5.4; a draft, see the launch v3 note in office-host-boundary.md §3), never in argv,
     the first line or a log. The host keeps it only in memory until `office.json`
     exists.
   - **Checks.** `office.join` refuses, with `already_exists` (JOIN-8), an `office_id`
     this instance already has or whose 12-character handle collides with another
     office's ignoring case (the socket and log file are keyed on it); an id that isn't
     a plain ID accepted by the current enrollment contract gets `invalid_request`. It doesn't apply create's
     organization-name check: the same `org_id` as an HQ office on this instance is
     allowed, because v1's local satellites (B2-10) join their own instance's HQ.
   - **Pending lifecycle.** The worker generates the TLS identity key and the
     request, persists them with the invitation key in its `office.db` (encrypted
     with the office database key; under ADR-0016 they become records wrapped by
     the host-held office secrets key), then writes `office.json` with `state: pending`,
     all **before** it sends anything to HQ. From then on the folder is a created
     office under the usual adopt rule (it's keyed on `office.json`): the host never
     runs `--discard` on it automatically (only `office.remove_pending` does, below), and later starts use `--office <id>` and resume the same
     request (same request ID and check code). A folder without `office.json` is an
     unfinished create and is discarded as usual; setup never persists the invitation, so its resume asks for it again and re-supplies the
     invitation, which never reached HQ.
   - **Status.** The worker reports the join's state, check code and any error over
     its rendezvous connection (and `joined` once approval and the initial
     synchronization complete); the host shows a pending join as `starting` with
     `join_pending` (office-host-boundary §4) and these details in the instance status read,
     which `host.sock` serves without a session (X-02 §6.2; a draft, see the launch v3 note in office-host-boundary.md §3) and which setup polls.
   - **Terminal states.** An expired, cancelled (three wrong codes cancel the
     invitation, below) or rejected request is written to `office.json` as that state, the invitation key
     is erased, and the worker exits cleanly. The host stops restarting it and
     reports it in status; `agentic-org office remove-pending <id>` removes it
     (`office.remove_pending { operation_id, office_id }` on `host.sock`, journaled,
     with `office.join`'s authority). For a still-`pending` join the host stops the
     worker first; HQ's reservation expires, or, if HQ had already approved, the owner
     removes that office at HQ (OFC-7) like any office that never connects. It then runs `--discard`,
     which accepts this one case of a folder with `office.json`: `role: satellite`,
     a `pending` or terminal `state`, and an `org.db` with no applied snapshot
     (checked with the key). It refuses an `active` office or any applied copy. The
     key is deleted after `discarded`, as usual. A failed join is never reported as an active
     office (JOIN-7). The worker sets `state: active` once approval and the required
     initial synchronization (step 5) complete.
   - The worker verifies HQ's identity and proves possession of the invitation. The
     invitation key is never sent to HQ and is erased when the join ends: on approval or a terminal state.
   (2026-11-10, post-merge review M1 and its follow-up review: process, key,
   pending lifecycle, `host.sock` authority.)
4. The owner approves the joining key at HQ using the existing check-code flow,
   from any authorized browser. Approval certifies exactly the request's key;
   certificate identity includes office ID, role and client/server purpose. A
   satellite certificate is client-only and cannot impersonate HQ.
5. HQ sends the snapshot, the satellite applies it and resumes the streams. The
   office becomes active only after the required initial synchronization completes.
6. Later restarts use the enrolled identity and saved positions. Certificates
   retain the original proposed one-year lifetime and authenticated renewal.
   Removal revokes the certificate and applies the existing reclaim/unplaced-agent
   rules. HQ cannot remotely erase a disconnected machine's retained copy.

Local offices use the same office-identity model in v1 (plan B2-10); a reviewed
private-capability equivalent is post-v1. Same UID is not enrollment proof. A local setup may orchestrate consent
directly but cannot bypass office identity validation.

## The invitation and connection

The invitation details remain a protocol proposal requiring implementation review.
Retain the versioned token containing organization/office identity, HQ addresses,
CA fingerprint, expiry and a one-time invitation key. HQ stores the public half.
The satellite pins HQ's office ID and CA identity, then signs the TLS exporter
with the invitation key; the certificate request travels in that same connection.
The check code covers invitation ID and the joining public key. It remains stable
across reconnects of the same pending request. The proposed code is 8 Crockford
base32 characters, displayed as two groups of four; three wrong approvals cancel
the invitation. Invitations expire (24 hours by default), are cancellable and
single-use, with rate-limited failures. Claim/certificate issuance must be atomic
and recoverable by the same request ID after a lost response.

The CLI accepts an invitation through protected stdin/file input rather than
requiring an `agenticorg://` desktop handler. HQ offers a copyable enrollment
instruction; the token remains secret and may disclose organization/address metadata.
Owner approval and the destination machine's execution consent are distinct.
Noninteractive setup can report pending approval without claiming success.

Each satellite keeps one outbound full-duplex connection to HQ. Browser HTTPS and
office authentication are configured separately: TLS-exporter proof must terminate
at the verifier, not at an unaware reverse proxy. Direct reachability is required;
there is no new relay service. HQ can advertise changed addresses, but its pinned
identity remains the trust anchor. A disconnected satellite can accept an explicit
new address from its administrator without accepting a different HQ identity.

## When things are unreachable
- **A satellite can't reach HQ:**
  - Reads work from the copy, labelled "last synced …", and office data keeps working.
  - Organization changes wait in the satellite's **outbox**, in order. A queued edit to something already edited in the outbox expects the version that earlier edit will produce, so two queued edits of one thing don't conflict with each other. When HQ is back, they're sent **one at a time, in order**.
  - If HQ rejects one, the commands queued after it are held, not sent. The app shows them as **blocked** for the owner to retry or discard, because they may depend on the rejected one.
  - Until they're committed, queued changes show in the app as **pending**. They aren't in the copy.
- **Messages between agents** (ADR-0003; routing and ownership in §2.2 and §§3.1–3.4 accepted 2026-11-17):
  - A message between two agents **in the same office** is delivered by that office directly, even with HQ unreachable.
  - A message to an agent **in another office** goes through HQ, which delivers it to the recipient's office. While HQ is unreachable it waits in the sender office's **message queue**, which is separate from the command outbox, so a blocked command never holds messages back.
- **The browser when HQ is unavailable:** HQ's application cannot be reached.
  An already open page shows lost connection and labels its data stale; it does
  not become a satellite or promise a durable offline edit queue. Office-local
  agents continue under the existing outbox and work-ownership rules. A browser
  command is saved only after HQ durably acknowledges it.

- **"Backup", honestly:** satellites follow HQ immediately, so a deletion or bad change reaches them too, and a local satellite shares HQ's disk. Satellites protect against losing HQ's machine, not against mistakes. Real backups (point-in-time snapshots) are a separate feature.

## Losing HQ: promotion (phase 4, designed in full before it's built)
These are recovery design requirements, not an implemented promotion protocol.
REC-1 in the standalone design keeps the manual replacement/fencing procedure an
explicit release decision; v1 CA escrow provisioning does not resolve it.

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
  - The code is generated by the server (at least 128 bits, shown once through a protected owner channel) and stretched with a memory-hard key derivation (Argon2id).
  - So a stolen Mac's copy can't be brute-forced. A satellite or a stolen Mac can then neither impersonate HQ nor re-certify itself.
- **Revocations are part of the copied data**, so every office refuses a removed office's certificate.
- **Dedup across promotion** works for every result the promoted office had received, because command results, rejections included, are copied data. Results only the old HQ had are lost, as above.
- **Work data across promotion:**
  - The new HQ held only part of the work data, so **each office republishes the current records it owns**, and their earlier changes, from the new HQ's per-origin positions. To make that possible, each origin keeps its own change log for the retention period.
  - Dedup by (origin, seq) makes repeats safe.
  - Work data that only the old HQ held (its own tasks and messages) may be lost. The warning before promoting says so, and the reclaim rule above applies to the old HQ's tasks.
- **V1 creates the escrowed CA key and owner recovery code at organization creation.**
  Encrypted escrow is copied organization data; the usable CA key and code are not.
  Setup provides protected one-time delivery, off-HQ retention acknowledgement and
  idempotent retry as specified in [standalone-server.md](standalone-server.md#ca-escrow-and-the-explicit-recovery-gate).
  REC-1 explicitly gates the supported manual HQ-loss recovery/fencing procedure;
  this escrow requirement does not claim promotion is already implemented.

## Where the code lives (architecture rules)
- **`org-sync`** (new library): the **replication logic**, independent of storage and transport:
  - positions and epochs, the order in which events are applied, the outboxes and their ordering, per-origin work streams, version levels
  - what an event *means* stays in `org-office`: `org-sync` calls an apply trait that `org-office` implements, so the domain never leaks into `org-sync` and there's no cycle
  - the sync session (stream, fan-out, relay, status updates) as a state machine over an abstract connection

  It works over a small **storage trait** that `org-office`'s `store.rs` implements, so all SQL, snapshot export included, stays in `store.rs` (ADR-0002's swap point). The server only supplies the transport.
- **`org-office`:** the domain. It validates commands, produces events and uses `org-sync`. It knows nothing about networks or certificates.
- **`org-pki`** (new adapter crate): enrollment, the organization's CA, certificates, TLS. Only the server binaries use it: the HQ worker (CA, certificates, and the enrollment listener where X-05 places it) and the satellite worker (its TLS identity key, enrollment, client TLS). Setup and the host never hold an office's TLS identity key.
- **`org-office-launch`** (new contract crate): the pipe protocol between the host and an office server it starts, and the `office.json` file the server writes and the host reads. The host and the office server use it, and `agent-os` reads `office.json` through it (ADR-0003 §2.1). It stays out of `org-proto`, so the crate every client depends on doesn't grow.
- **`org-sync-proto`** (new contract crate): the office-to-office protocol (stream, snapshot chunks, enrollment, status updates). It stays out of `org-proto`, so the crate every client depends on doesn't grow.
- **`agentic-org-office` and `agentic-org`** stay thin; `crates/org-host` owns reusable host lifecycle. `agentic-org-web` parses browser HTTP without key custody.
- **Owners:** the lead owns all of these crates under the current solo workflow (`AGENTS.md`). `org-sync-proto` and `org-office-launch` are contracts and change with the same care as `org-proto`.

## Implementation scope after the standalone-v1 pivot

1. Instance setup, macOS/Linux service and credential adapters, and per-office
   lifecycle/storage invariants.
2. Authenticated HQ HTTP API and static frontend, removing the desktop transport.
3. Local and remote satellite identity, enrollment, snapshots, outboxes, resume,
   relay and version negotiation, plus CA escrow/recovery-code creation and protected
   owner delivery during HQ setup. These are required for new v1 acceptance.
4. Automatic promotion remains deferred. REC-1 requires a separately reviewed manual
   HQ-loss recovery/fencing procedure before claiming production recovery support;
   retaining escrow in v1 is mandatory even while that procedure is unresolved.

These are dependency-ordered implementation steps, not a migration of a deployed
desktop release. The original phase-2/phase-3 labels elsewhere are historical
references; remote satellites are no longer outside v1.

## What this changes in earlier decisions

The following is the **historical 2026-11-08 ADR delta**, preserving original
protocol/security namespaces and storage evidence. Its process names and two-DB
layout are not the selected target filesystem or current executable spelling;
use the model/process sections above and the current launch contract.

- **ADR-0001:**
  - An office belongs to an organization.
  - `agentic-org-office` runs one office, and `org-office-host` is the login service.
  - Offices keep running when the app quits (ADR-0001), but they run under the host: a host crash or restart stops every office on this Mac (and agents' messaging) until the host restarts them, and rotates all app tokens. This is the phase-1 trade-off.
  - The web application talks to HQ; the CLI manages named instances and enrollment (ADR-0005).
- **ADR-0002:**
  - *(Amended by [ADR-0016](../adr/0016-no-application-level-data-encryption.md), 2026-11-17: database encryption is to be removed. The per-office key stays only to wrap an office's private keys, and a missing key no longer prevents the databases from opening. The bullets below describe the current implementation.)*
  - **Each office has its own two databases:** `org.db`, the organization data or its copy, and `office.db`, its own data. They share one key, kept under the office id. (amended 2026-11-08: key custody) The host creates and holds it, in the Data Protection keychain on this Mac only, and sends it on the launch pipe; the server never stores one.
  - A key is created only for a create, by the host. The host never makes one for an existing office: a key item it doesn't have is `key_missing`, set by the host without starting the server, and a key that doesn't open the databases is `key_missing` (fatal), a `failed` code on the launch pipe.
  - Layout `<home>/offices/<office-id>/`.
  - HQ's `org.db` holds the event log. SQLCipher, one writer per database, durability and the swap point hold per office.
- **ADR-0003 (routing and ownership accepted 2026-11-17; the rest is earlier design evidence):**
  - `agent-os` talks to **its own office**, so binding needs only the agent.
  - Messages are office-first: committed and delivered locally, then synced to HQ, and through HQ to other offices. The storage structure for heavy traffic and the cross-office `ask` are defined there.
- **`org-proto`** (client ↔ office):
  - The host gains `hello`, `organization.create`, `organization.list`, `office.list` and `office.app_token` (ADR-0003 §2.1).
  - `not_authorized` and `unavailable` are org-proto error codes (`office.app_token`'s). `key_missing` isn't: it's a `failed` code on the launch pipe (`crates/org-office-launch`, which River is writing; it isn't on `v1` yet).
  - An office's `hello` reports its organization and role.
  - Organizations list their offices from the registry.
  - The exact shapes are drafted after this design is agreed.

## Decisions (the user, 2026-11-08)

Recorded decisions with later selected scope made explicit:

1. **Promotion** is manual, by the owner, with the recovery code.
2. **While HQ is unreachable,** satellites queue organization changes, as designed.
3. **Work-data copies:** HQ keeps every task and message, and each satellite keeps what concerns its own agents and projects.
4. **Updated selected repository model:** organization Git is remote; code registrations can be remote or local-only, many per project, with required integration branch per association and one office cache per repository. GitHub transport support remains an implementation gate rather than the sole allowed repository kind.
5. **Remote setup (standalone amendment):** HQ generates a one-time invitation, copied with **Copy invitation** and supplied through the destination CLI's protected setup input. The owner verifies its check code on the destination terminal or through a trusted authenticated session to that machine, then approves through HQ (see "The invitation and connection").
6. **Reaching HQ:** direct network access only. There's no relay service for now. HQ's addresses travel in the invitation and the registry.
7. **Satellites don't listen.** They connect out to HQ and keep a full-duplex connection open. **HQ is the relay:** anything between satellites goes through HQ.
8. **Office ownership is not employee authorization.** All offices may be the owner's, but protected message visibility, author-only notes, cost scopes and trusted actor grants still apply across every replica. An office certificate is never owner authority.
9. **Superseded 2026-11-09:** a browser client does not become a satellite. Explicit setup/enrollment creates an office.
10. **Transport writer, not accountable ownership:** retained assignee-office routing must be adapted to required task team and independent assignment; durable handover never closes work or deletes its workspace.
11. **One recovery code per organization**, kept by the owner.
