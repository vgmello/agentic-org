# Design: offices, HQ and satellites

How an organization runs in several offices: what each office holds, how changes and
messages flow between them, how a satellite joins, and what happens when HQ is
unreachable or lost. The relationships and sources are owned by the
[data contract](organization-data/README.md); message routing by
[ADR-0003](../adr/0003-agent-messaging-office-first.md); how a worker is started by the
[host boundary](office-host-boundary.md).

## In one paragraph

An organization has one HQ and any number of satellites. Offices place agents; the
team tree, employees, projects, repository registrations and HQ's counters belong to the
organization. HQ creates teams, projects, releases and milestones and is the
organization authority: it numbers the records only HQ creates, registers feature slugs,
and issues turn permits and placement generations. Any office creates tasks and
features durably, even disconnected; a task's reference is minted by its creating
office at once, and a feature's number waits for HQ. Organization Git is remote and
holds portable definitions and open work; chat, accounting, budgets, execution and
recovery evidence have their own durable sources. The database indexes these sources.
A copied projection grants neither access to private data nor authority to run a
process.

## The model

- **Organization and offices.** Creating an organization creates its HQ and records
  its stable ID, slug, remote repository registration and `state_branch`. Office
  definitions (`id`, slug, name, role, revision, provenance) live at
  `offices/<office-id>.yaml` in organization Git; addresses, enrollment state,
  credentials and connectivity are local operational state.
- **One home team per employee.** A lead is a direct member who manages the subtree.
  Moving an agent to another office does not change its team. The owner's authority is
  a host account, never an employee flag.
- **Projects and code.** A project has one owner team, optional contributing teams and
  zero or more repository associations, each with its own `integration_branch`. An
  office keeps one bare cache per repository. Code repositories may be local-only; the
  organization repository may not.
- **Accountability and assignment.** A task has an accountable `team_id` (the project's
  owner for a project task) and an independent assignment pair changed atomically at an
  expected revision. The reporter is the immutable creator. A reference such as `T-A42`
  is complete on its own.
- **Placement.** Each office supervises its agents' turns, attempts and processes
  ([employee supervision](employee-supervision.md)). A runnable agent needs its
  execution configuration, fresh local connection evidence and a permit from HQ.
  Placement needs the durable placement generation and cleanup or fencing of any prior
  process.
- **Access.** The browser authenticates to HQ; no local office is needed. Initial setup
  creates HQ only; a satellite is created by enrollment started from HQ. Replication
  scope never overrides chat visibility, private-note author-only access or cost and
  budget authorization. Presence is ephemeral and grants nothing.

## Processes in one instance

The host inside `agentic-org serve` supervises one `agentic-org-office` per office and,
for an instance with an HQ, one `agentic-org-web`. It holds each office's secrets key,
the instance and catalog locks and the private launch capability; the web process has
none of them. The worker owns domain operations, its `office.json` marker and its office
lock. Launch, admission, quarantine, restart and error rules are in the
[host boundary](office-host-boundary.md).

Each office's data follows the
[storage contract](organization-data/README.md#shared-storage-contract): sources under
`durable/`, rebuildable caches under `runtime/<office-id>/`. Private task notes are a
separate author-only database at HQ. Task worktrees survive handoff until closure;
local-only and unpushed edits are checkpointed before any cleanup.

When the host dies, its workers and their processes stop. A restart needs lock
exclusion, valid credentials and process reconciliation. A restored PID is never
signalled without a matching ownership mark, generation and machine, boot and start
identity. Rebuilding a cache never allocates placement authority; missing evidence
means recovery is required, not permission to launch.

## Office first: tasks and messages

An office acknowledges a permitted local write once it is durable locally, before any
network publication ([ADR-0014](../adr/0014-acknowledgement-classes-and-agent-os-latency.md)).
Office first does not mean an office owns teams or projects or allocates HQ's counters;
the one sequence an office keeps is for the tasks it creates
([document 06](organization-data/06-identity-and-numbering.md)). Stable IDs, revisions,
the domain source and the journal and receipt evidence are stored before the
acknowledgement. Hourly Git publication is never the only copy of an accepted change. A
feature may have `number: null` until HQ registers it. Every acknowledged allocation
survives a restore, and a replica never increments an HQ counter.

**Who writes a task.** Each task has one office that accepts writes to it at a time, its
transport writer. This is a serialization role, separate from the task's `team_id`,
assigned team and assignee. The rules:

- A cross-office edit carries its operation ID and expected revision to the current
  writer through HQ; a replay returns the original actor-bound outcome.
- The current writer emits the handover. Until it arrives the destination cannot write,
  and requests in flight are forwarded to the new writer. Handover carries retry and
  result evidence with the task state.
- An explicit reclaim while a writer is disconnected can split perceived ownership. The
  product warns about stale local confirmations, increases the transport generation and
  compares `(generation, revision)` before accepting state. HQ turns retained stale
  changes into deterministic reconciliation requests and never discards them silently.
  This generation is neither a placement generation nor a new task identity.
- Acknowledged origin sequences, incarnations and high-water marks survive restore.
  Repeated frames are deduplicated and a restore never reuses a sequence. Messaging has
  one allocator for content and state; work has its own stream.
- Filtered backfill keeps per-scope positions and applies only newer versions. A
  referenced organization position is applied before references are checked.
  Unresolved references are kept with a diagnostic, never dropped.

Changing an agent's office or assignee never deletes its task workspace or closes the
task or turn.

**Messages** have immutable content and separately revisioned recipient state;
conversation membership is independent of teams and offices. The protected envelopes
and attachments of [document 15](organization-data/15-messaging-and-chat.md) are stored
before the send is acknowledged. Bodies, counts, previews and search are filtered by
message visibility. A message to another office travels office → HQ → destination over
the existing outbound connections; local delivery works without HQ, and cross-office
delivery waits in its own durable queue, keeping deadlines (ADR-0003 decisions 1 to 9).
HQ's routing copy and a satellite's copy stay access-controlled.

## How organization changes flow

1. The actor is authenticated at the browser or the agent's office. The request is an
   explicit typed operation with an organization scope. An office certificate or the
   same OS user is never owner authority. HQ creates teams, projects, releases and
   milestones.
2. Required fields, references and the expected revision are validated. The remote
   repository, state branch, home teams and owner teams round-trip at canonical
   stable-ID paths. Every ID is a UUIDv7.
3. The accepting writer commits the change, its journal evidence and the bound receipt
   at one recoverable boundary. The same operation ID with another actor or fingerprint
   conflicts. Unknown side effects are reconciled before a replay. Private-note
   existence never enters shared receipts.
4. Changes replicate as typed events with positions and deterministic apply.
   Organization files are published to the remote state branch separately.
5. A reconnecting office resumes from verified checkpoints or takes a verified
   snapshot. It checks the organization identity, CA and HQ pins, codec, contiguous
   sequences and digests and the highest epoch, and hash-checks staged snapshot bytes
   before installing them. A partial or stale snapshot confers no authority. A snapshot
   file travels only over the office TLS connection and is verified by its content
   digest (ADR-0016).
6. A projection is rebuilt only from complete authorized Git plus the protected
   sources. Credentials are bound again, receipt, stream and allocation high-water marks
   kept, and process cleanup or fencing verified before anything runs. Lost usage,
   budget history, messages or authority are never synthesized from Git; affected work
   stops if source coverage is lost.

Operation IDs are admitted for 29 days, up to five minutes in the future, and results
kept 30 days, with lookup before the age check. Receipts are never purged by a cache
rebuild. A rejection never authorizes an automatic retry with a fresh ID.

Deletions keep tombstones, historical references and archived identities. Satellite
connections are outbound and full-duplex; HQ relays between them and records when each
was last seen. Satellites open no public listener.

## Versions

- Events, commands and snapshots carry a schema version, and offices exchange the range
  they support when they connect.
- A satellite too old for HQ's events stops applying them and shows "update needed"; it
  never applies a half-understood event. HQ refuses commands it does not understand
  (`unsupported_version`).
- An organization-wide protocol level, recorded as an event, sets what everyone uses.
  HQ raises it only once every active office (seen in the last 30 days) supports the new
  level; an office away longer updates before it syncs again. No office sends commands,
  and HQ emits no events, above the current level. So offices can be updated in any
  order, and the app shows which offices hold a new level back.

## Provisioning a satellite

1. An owner creates an invitation in the HQ web application. HQ reserves the office
   identity and a pending registry entry.
2. On the target machine, the enrollment flow issued by HQ selects a named instance and
   joins the organization; initial `agentic-org setup` stays HQ-only. It shows the
   organization and the pinned HQ identity before the owner confirms that the office
   may run agents there. Automation supplies that consent explicitly.
3. The satellite's worker, not setup or the host, generates and holds the office's TLS
   identity key, wrapped with its office secrets key.
   - **Hand-off.** Enrollment gives the invitation to the host with
     `office.join { operation_id, invitation }` on `host.sock`, journaled; the
     invitation is a secret type. Its authority is the admitted `agentic-org` peer plus
     the destination consent (JOIN-2). OS user presence is not required, so unattended,
     SSH and container joins work. On an instance that already has accounts,
     `office.join` also needs an instance administrator, like Add office. The host
     creates the pending office and starts the worker with a satellite `create`; the
     first line carries only the invitation's non-secret reference, and the invitation
     secret goes over the verified rendezvous, never in argv or a log. The host keeps it
     in memory only until `office.json` exists.
   - **Checks.** `office.join` refuses with `already_exists` (JOIN-8) an office ID this
     instance already has or whose socket handle collides with another office's,
     ignoring case. It does not apply create's organization-name check: a satellite may
     join an HQ on the same instance.
   - **Pending lifecycle.** The worker generates the TLS identity key and the request,
     stores them with the invitation key, then writes `office.json` with
     `state: pending`, all **before** it sends anything to HQ. From then on the office
     is created: the host never discards it automatically, later starts use
     `--office <id>` and resume the same request (same request ID and check code). A
     directory without `office.json` is an unfinished create and is discarded; the
     invitation is never persisted by enrollment, so a resume asks for it again.
   - **Status.** The worker reports the join state, check code and any error over the
     rendezvous, and `joined` once approval and the initial synchronization complete.
     The host shows a pending join as `starting` with `join_pending`, with these
     details in the instance status read, which `host.sock` serves without a session.
   - **Terminal states.** An expired, cancelled (three wrong codes cancel the
     invitation) or rejected request is written to `office.json`, the invitation key is
     erased and the worker exits. `agentic-org office remove-pending <id>`
     (`office.remove_pending { operation_id, office_id }`, with `office.join`'s
     authority) removes it; for a still-pending join the host stops the worker first.
     If HQ had already approved, the owner removes that office at HQ (OFC-7). A failed
     join is never reported as an active office (JOIN-7).
   - The worker verifies HQ's identity and proves possession of the invitation. The
     invitation key never goes to HQ and is erased when the join ends.
4. An owner approves the joining key at HQ with the check-code flow, from any
   authorized browser. Approval certifies exactly the request's key; the certificate
   names the office ID, role and purpose. A satellite certificate is client-only and
   cannot impersonate HQ.
5. HQ sends the snapshot, the satellite applies it and resumes the streams. The office
   becomes active only after this initial synchronization.
6. Later restarts reuse the enrolled identity and saved positions. Certificates last one
   year with authenticated renewal. Removing an office revokes its certificate and
   applies the reclaim and unplaced-agent rules. HQ cannot erase a disconnected
   machine's copy remotely.

An office on the same machine as HQ uses the same identity model; the same OS user is
not proof of enrollment.

## The invitation and connection

The invitation is a versioned token holding the organization and office identity, HQ's
addresses, the CA fingerprint, an expiry and a one-time invitation key; HQ stores the
public half. The satellite pins HQ's office ID and CA identity, then signs the TLS
exporter with the invitation key; the certificate request travels in that same
connection. The check code covers the invitation ID and the joining public key and stays
stable across reconnects of the same request: 8 Crockford base32 characters shown as two
groups of four. Three wrong approvals cancel the invitation. Invitations expire
(24 hours by default), can be cancelled, are single-use and rate-limit failures. Claim
and certificate issuance are atomic and recoverable by the same request ID after a lost
response.

The CLI takes an invitation through protected stdin or file input. HQ offers a copyable
enrollment instruction; the token is secret and may disclose organization and address
metadata. Owner approval and the destination's consent to run agents are distinct.
Noninteractive enrollment can report pending approval without claiming success.

Each satellite keeps one outbound full-duplex connection to HQ. Browser HTTPS and office
authentication are configured separately, because the TLS-exporter proof must terminate
at the verifier. HQ must be directly reachable; there is no relay service. HQ can
advertise changed addresses, but its pinned identity stays the trust anchor, and an
administrator can give a disconnected satellite a new address without it accepting a
different HQ.

## When things are unreachable

- **A satellite cannot reach HQ:**
  - Reads work from its copy, labelled "last synced …", and office data keeps working.
  - Organization changes wait in the satellite's **outbox**, in order. A queued edit to
    something already edited in the outbox expects the revision that earlier edit will
    produce. When HQ is back they are sent **one at a time, in order**.
  - If HQ rejects one, the changes queued after it are held. The app shows them as
    **blocked** for the owner to retry or discard.
  - Queued changes show as **pending** until committed; they are not in the copy.
- **Messages between agents:** within one office they are delivered directly, even
  with HQ unreachable. To another office they go through HQ and wait meanwhile in the
  sender office's message queue, which is separate from the command outbox, so a blocked
  command never holds messages back.
- **The browser when HQ is unavailable:** HQ's application cannot be reached. An open
  page shows the lost connection and labels its data stale; it keeps no offline edit
  queue. A browser command is saved only after HQ durably acknowledges it.
- **Backup:** satellites follow HQ immediately, so a deletion or a bad change reaches
  them too, and a satellite on HQ's machine shares its disk. Satellites protect against
  losing HQ's machine, not against mistakes. Point-in-time backups are a separate
  feature.

## Losing HQ: promotion

Promotion is not part of v1. Before it is built it gets its own design, which meets
these requirements; until then the manual HQ-loss procedure is the open gate REC-1 of
the [standalone design](standalone-server.md#ca-escrow-and-the-explicit-recovery-gate).

- **Manual and deliberate.** The owner promotes a satellite in the app, with the
  recovery code; it is never automatic.
- **A signed epoch record** names the new HQ's office ID and address and the last
  position it holds. The new HQ gets a server certificate for its role, office ID and
  the new epoch, and a satellite accepts it in place of its pinned HQ only with the
  CA-signed epoch record. The new HQ must be directly reachable. An office learns of the
  epoch when it connects to the new HQ at an address its owner gives it. An office that
  applied events beyond the recorded position takes a fresh snapshot, and those events
  may be lost; the app warns before promoting. An office the record cannot reach,
  including a sleeping old HQ, keeps following the old HQ until told on its own machine,
  so a split is possible until every office has heard.
- **Certificates carry the epoch.** An office refuses any HQ below the highest epoch it
  knows. A former HQ that learns of a higher epoch becomes read-only and rejoins as a
  satellite with a fresh snapshot.
- **The CA key is never usable on a satellite.** It is escrowed under the owner's
  recovery code (at least 128 bits, shown once through a protected channel, stretched
  with Argon2id) and unlocked only to promote. A stolen machine can neither impersonate
  HQ nor re-certify itself. The escrow is created with the organization
  ([standalone design](standalone-server.md#ca-escrow-and-the-explicit-recovery-gate)).
- **Revocations are replicated,** so every office refuses a removed office's
  certificate.
- **Deduplication across promotion** covers every result the promoted office received,
  because results, rejections included, are replicated. Results only the old HQ had are
  lost.
- **Work data.** Each office republishes the current records it writes, and their
  earlier changes, from the new HQ's per-origin positions; each origin therefore keeps
  its change log for the retention period, and deduplication by origin and sequence
  makes repeats safe. Work only the old HQ held may be lost, and the warning says so.

## Where the code lives

- **`org-sync`:** replication logic, independent of storage and transport: positions
  and epochs, apply order, outboxes, per-origin work streams, version levels, and the
  sync session (stream, fan-out, relay, status) as a state machine over an abstract
  connection. What an event means stays in `org-office`, behind an apply trait it
  implements, so there is no cycle. It works over a storage trait that the office's
  store implements, so all SQL stays in the store.
- **`org-office`:** the domain. It validates commands, produces events and uses
  `org-sync`. It knows nothing about networks or certificates.
- **`org-pki`:** enrollment, the organization's CA, certificates, TLS and private-key
  wrapping. Only office workers use it; setup and the host never hold an office's TLS
  identity key.
- **`org-office-launch`:** the host-to-worker launch contract and `office.json`.
- **`org-sync-proto`:** the office-to-office protocol (stream, snapshot chunks,
  enrollment, status). It stays out of `org-proto`, so the crate every client depends on
  stays small.
- **`agentic-org-office` and `agentic-org`** stay thin; `crates/org-host` owns the host
  lifecycle. `agentic-org-web` handles browser HTTP and holds no keys.

## Decisions

1. **Promotion** is manual, by the owner, with the recovery code.
2. **While HQ is unreachable,** satellites queue organization changes.
3. **Copies of work data:** HQ keeps every task and message; each satellite keeps what
   concerns its own agents and projects.
4. **Repositories:** organization Git is remote; code repositories may be remote or
   local-only, several per project, each association with its integration branch, one
   office cache per repository.
5. **Joining:** HQ generates a one-time invitation (**Copy invitation**), supplied to the
   destination's enrollment through protected input. The owner checks the code on the
   destination or through a trusted session to it, then approves at HQ.
6. **Reaching HQ:** direct network access only, with no relay service. HQ's addresses
   travel in the invitation and the registry.
7. **Satellites do not listen.** They connect out to HQ and keep a full-duplex
   connection; HQ relays everything between satellites.
8. **An office is not an authority over people's data.** Message visibility, author-only
   notes, cost scopes and actor grants apply in every replica, and an office certificate
   is never owner authority.
9. **A browser never becomes a satellite.** Only enrollment creates an office.
10. **The transport writer is not accountability.** Handover never closes work or
    deletes its workspace.
11. **One recovery code per organization,** kept by the owner.

## What the code does today

`crates/org-sync`, `crates/org-sync-proto` and `crates/org-pki` exist. `org-sync` is
linked into the office, but no satellite can be enrolled: there is no enrollment
listener, invitation or `office.join` call, and the launch contract refuses a satellite
create in its older form. Replication reads the office's older tables; it is parked
until the store writes the canonical files (implementation plan, later increment L5).
