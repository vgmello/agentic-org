# Office integration validation on macOS and Linux

Owner direction, 2026-11-16: use disposable virtual machines and Linux containers
to validate HQ and remote offices, including their real communication and failure
behavior. Keep quality checks and security enforcement intact. This procedure
defines the environments and evidence for the existing
[dogfood acceptance run](../specs/dogfood-run.md); it does not establish that a
harness exists, that a platform is supported, or that any scenario has passed.

## Authority and current coverage

The [installation acceptance criteria](../specs/server-installation-and-access-ux.md#6-acceptance-criteria),
[host boundary](../design/office-host-boundary.md),
[documentation authority map](../design/documentation-authority.md) and selected
data contracts remain authoritative. Follow dogfood row IDs rather than creating
a second feature checklist. Delivery status remains in [FEATURES.md](../FEATURES.md).

Observed on 2026-11-16:

- [CI](../../.github/workflows/ci.yml) is configured for native macOS and Ubuntu
  Rust checks, with browser checks on Ubuntu. The Linux workspace job excludes
  broader `org-host`/`agentic-org` tests, runs selected CLI tests separately, and
  permits two existing host lints. Those exclusions are explicit coverage gaps,
  not full Linux acceptance. Configuration alone does not prove a green run.
- Local Linux cross-target Clippy checks compilation, not runtime behavior.
- [Platform and connectivity smoke checks](../verification/2026-11-16-platform-connectivity-check.md)
  verified native macOS compilation, Linux VM/container access and HTTP in all
  three directions: macOS to Linux, Linux to macOS, and between Linux containers.
  These are infrastructure observations, not real-office end-to-end acceptance.
- Disposable Podman probes ran Linux security primitives and a miniature Tokio
  workload. Their assessment is retained locally at
  `.agent-kit/logs/v1-assignment-integration/linux-feasibility/assessment-1.md`.
  They did not run the complete host, gateway, office or enrollment flow.
- [ADR-0008](../adr/0008-gateway-macos-sandbox.md) selects a macOS adapter only.
  Linux support remains separately gated; unsupported enforcement must fail
  closed. A passing container probe cannot select or complete that adapter.

There is no full cross-platform office acceptance receipt established by these
checks. The topology harness and missing platform/application prerequisites below
remain work to implement and independently review.

## Environment matrix

| Lane | HQ | Satellite | Required evidence and limits |
| --- | --- | --- | --- |
| Fast Linux integration | Disposable Linux container | One or more separate Linux containers | Real office processes, separate identities/storage, authenticated enrollment, transport, replay and failure handling. Run when Linux prerequisites exist; blocked prerequisites remain visible. |
| Cross-platform A | Native macOS host or macOS VM on Apple hardware | Linux VM; container supplementary | Real macOS host behavior and Linux satellite behavior across a network boundary. Required orientation from dogfood §0. |
| Cross-platform B | Linux VM or native Linux runner | Native macOS host or macOS VM on Apple hardware | Real Linux HQ and macOS satellite. Required reverse orientation from dogfood §0. |
| Native platform lifecycle/security | Each advertised supported OS, architecture and minimum kernel/OS profile | As needed | Actual service manager, credential provider, process admission/confinement, restart and reboot behavior. Record tested combinations; do not infer another architecture or kernel from them. |

Docker and Podman are alternatives for the Linux container lane. Containers can
start multiple remote offices cheaply. They share their Linux host/VM kernel;
an Ubuntu container does not select an Ubuntu kernel. Docker Desktop on macOS
runs Linux containers in a Linux VM, not macOS guests. Use native macOS execution
for launchd, Keychain, OS authentication, image admission and macOS confinement.
See [Docker's platform documentation](https://docs.docker.com/faqs/platform/) and
[Landlock kernel documentation](https://docs.kernel.org/userspace-api/landlock.html).

Container results apply to their recorded kernel, restrictions and filesystem.
They do not by themselves prove native installation, login/logout/reboot service
behavior, physical OS authentication, or power-loss durability. Exercise those
on the appropriate native runner/VM and retain their separate limitations.

## Disposable topology and safety

1. Use a unique run ID and exact built commit/artifact hashes. Give HQ, each
   satellite and a second isolated instance distinct test roots, identities,
   credentials, sockets, ports and service registrations. Use temporary
   directories or test-owned volumes; never a user's organization workspace.
2. Use a private test network and explicitly controlled cross-host endpoints.
   Satellites initiate the supported connection to HQ; assert that they expose
   no public application listener. A separate browser reaches HQ without
   becoming an office. Forward only required HQ/test endpoints.
3. Use separate office processes and storage. Never simulate two offices by
   sharing a database, identity directory or writable organization checkout.
   Supply a disposable remote Git service/repository for metadata where needed;
   an HQ-local path is not a satellite's remote repository.
4. Use the actual supported setup/enrollment/service entry points and built
   gateway, office, browser assets and agent CLI. Missing adapters are Blocked,
   not replaced with invented commands. Development fixtures and mocks stay
   explicitly scoped to lower-level tests and cannot pass production rows.
5. Generate test-only credentials through supported test/provisioning mechanisms;
   redact secrets from commands, logs, screenshots and receipts. Do not mount
   personal homes, host credential stores, container sockets or live spools.
   Do not use privileged containers or broad capabilities to bypass a refusal.
6. Inject failures only into resources owned by this run. Never change the
   developer's host firewall, stop shared services/VMs, or prune global container
   state. Record ownership before creation and retain diagnostics before exact
   resource cleanup, including on failure. Keep unfinished fixtures isolated.

The harness may need reviewed, narrowly scoped kernel-test settings to observe
the application's own security policy. Record the enclosing container policy and
actual application-enforced denials; a container engine's denial alone does not
prove that the application installed its required restrictions.

## Scenarios to exercise

Map every executed case to the current dogfood row and owning contract. These
groups describe coverage, not alternative pass conditions:

| Group | Existing acceptance | What the real topology must demonstrate |
| --- | --- | --- |
| HQ and independent instances | Dogfood §§1–2 | Setup/sign-in/reopen; stable identities on retry; separate roots, keys and service names; unauthorized/cross-origin and cross-instance requests refused. Offline setup makes no external requests and does not claim remote readiness. |
| Satellite enrollment | Dogfood §3, rows 3.1–3.4 | HQ-issued invitation, approval, pending restart/retry, certificate/identity reuse, initial snapshot and real readiness; no satellite web listener. Invalid/revoked identities and incompatible peers cannot connect. |
| Useful remote execution | Dogfood §3, rows 3.5–3.6 | One explicitly bounded task through a supported real coding-agent runtime, correct office/employee routing, retained result/usage and authorized control. A fake provider cannot pass the real-agent row. |
| Connection and retry failures | Dogfood §4 | Disconnect/reconnect, HQ and satellite restart, stale/fresh UI states, bounded slow consumers, lost replies and same-ID replay with one logical change; conflicting retries refused. Pause/resume reconciles current versions before new work. |
| Lifecycle and custody | Dogfood §5 | Owned children stop correctly; peer instances remain usable; unavailable/wrong credentials refuse startup without key replacement, data mutation or development-store fallback; legitimate credentials restore access. |
| Data and recovery | Dogfood §§6–6a | Preserve acknowledged records, task identity/history and pending receipts across supported restart/recovery operations. Rebuilding a cache does not discard protected sources or authorize duplicate execution. Kill/reopen is not proof of physical power-loss durability. |

Start with the team-scoped non-repository task specified in dogfood §3.5. Claim
cross-office coding only after real repository checkout, code change, commit and
transport are exercised using supported interfaces and a disposable repository.
Use a bounded, explicitly authorized provider run with test credentials and a
spend/turn limit; ordinary integration jobs must not silently spend provider quota.

Native security cases must exercise actual positive and negative boundaries:
allowed office traffic, denied ambient files/processes/sockets, identity and
image validation, required startup ordering, cleanup and no secret handoff on
failure. Missing kernel/security support must refuse the protected path. Linux
implementation first requires its separately reviewed profile and adapter;
this document does not select its minimum ABI or change ADR-0008.

## Evidence and quality gates

For each run retain a redacted manifest with:

- Exact source commit, binary/assets/image digests, lockfile/toolchain versions,
  OS/architecture/kernel, relevant security ABI/configuration, container engine
  and enclosing policy, native service/credential configuration and topology.
- Run-owned resource IDs, temporary roots, mapped ports, office/instance IDs and
  test inputs; never private keys, passwords, session proofs or invitations.
- Each dogfood row ID, timestamp, actual command, exit status, result and raw log
  location; browser evidence for UI assertions and before/after identities,
  revisions and durable receipts for persistence/retry assertions.
- Failure injection and recovery observations, cleanup outcome, prerequisites
  still missing and the exact scope the run does and does not establish.

Use the dogfood result rules: missing required capability/environment is Blocked;
an observed violation is Fail. Neither counts as Pass. Partial development runs
must retain omitted required rows and reasons rather than report a complete run.
Retain failed attempts as well as successful repairs.

Keep existing formatting, strict linting, tests, schema/catalog checks, meaningful
browser checks, original-author validation and independent/fresh final review.
Run the affected topology cases after relevant transport, storage, host, security
or service changes, and bind acceptance to the exact integrated artifacts. Do not
skip tests, relax assertions, add policy allowances or expand CI exclusions to
make genuine defects green. Existing Linux gaps must be resolved before claiming
the corresponding supported-platform acceptance.

### Select reruns from the changed behavior

Use this matrix during feature scoping and before acceptance. Trace indirect
effects through shared dependencies, configuration and generated artifacts; file
location alone is not enough. Record the affected dogfood rows, environments and
reason for running or omitting an additional topology check in the change's
verification receipt. A missing harness/capability is Blocked, not an omission
that counts as a pass.

| Change can affect | Additional validation to select |
| --- | --- |
| Documentation, visual styling or isolated pure logic with no communication/lifecycle/storage/security impact | Keep the normal required checks; no additional platform smoke or office topology run solely for that change. Record why there is no affected integration boundary. |
| Office protocol, serialization/version compatibility, routing, network/TLS configuration, invitation/enrollment, reconnect, stream handling or shared transport dependencies | Real HQ/satellite enrollment, authorized exchange and affected disconnect/retry/refusal cases. Exercise both macOS/Linux orientations where interoperability can change, plus relevant Linux container cases. Plain HTTP reachability is insufficient. |
| Authentication, certificates, authorization, credential custody, image admission, sandbox/FD handoff or privilege boundaries | Affected real-office positive and negative security cases on each affected native OS, including refusal before secret handoff, revocation and relevant cross-instance/cross-origin isolation. Retain separately gated genuine signing/OS-authentication evidence. |
| Persistence, journal/receipt formats, operation IDs, replication or recovery | Affected real-office commit/ack, lost-reply/same-ID replay, crash/reopen and reconnect consistency cases using disposable durable data; check preserved identities and no duplicate logical changes. |
| Setup, service registration, process supervision, endpoint binding or shutdown | Native lifecycle checks on the affected OS/role, peer-instance isolation and office reconnection after restart. Login/logout/reboot cases require a disposable native environment rather than the shared development VM. |
| Remote agent execution, placement, office control, IPC or result delivery | Affected real remote task/control/result journey and correct office/employee routing. Use the bounded real-runtime case when runtime behavior changes; mocks cannot establish that acceptance. |
| VM/container engine, OS/kernel/SDK, security profile, architecture, network/port-forwarding setup, or unexplained connectivity failure | Recheck the relevant infrastructure smoke directions, then affected native security/lifecycle and real-office cases. Record new environment versions; a prior kernel/architecture result does not cover the replacement. |
| Release candidate | Complete the required dogfood run in both cross-platform orientations and advertised platform acceptance. Perform authentic signed-release checks in the final signing phase; earlier selective/development runs do not replace them. |

Do not restart the infrastructure smoke checks on every edit, commit, status poll
or unrelated feature. Reuse their recorded prerequisite evidence while that
environment remains applicable; investigate when it changes or fails. Product
changes still need the selected real-office cases against their current artifacts.
After a repair, rerun affected cases on the repaired source; run broader checks
when shared effects or remaining uncertainty warrant them. Old artifact results
remain historical and must not be relabeled as current acceptance.

This selection governs the additional integration coverage; it removes none of
the existing mandatory CI, repository or independent review gates. It is a
documented execution rule, not a claim that automated change selection or the
missing topology harness is already installed.

Release-signing setup and authentic signed-release verification remain the last
phase, as requested by the owner. Meanwhile run available development/component
and native tests with their exact custody/admission limits recorded. An unsigned
artifact that a protected path correctly refuses is not a successful production
run. Do not disable enforcement to exercise it. After genuine signing inputs are
configured, repeat all signing-dependent installation, admission and custody rows
with authentic artifacts; earlier development evidence cannot satisfy those rows.

## Implementation and integration handoff

The main implementation lead owns the harness, dependency order and acceptance.
Reuse working native fixtures and targeted checks rather than duplicating them.
Add the disposable Linux topology runner when its required adapters can execute;
then automate both cross-platform orientations and native lifecycle/security
cases. Add exact invocation instructions only once those commands exist.

Attach receipts to the affected existing feature rows when cases become runnable
or pass, without changing their full-scope status on the strength of a partial
probe. This procedure is documentation only: it creates no VM/container, changes
no CI gate and claims no new passing integration or independent review.
