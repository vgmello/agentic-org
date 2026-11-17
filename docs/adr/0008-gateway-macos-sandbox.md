# ADR-0008: macOS web sandbox (GW-SBX-MAC-1), revision 1

2026-11-15. **ACTIVE for macOS adapter implementation after fresh independent architecture READY.** Canonical decision: docs/adr/0008-gateway-macos-sandbox.md. Lead owns this focused prerequisite; original architecture/fullstack author remains implementation owner. Parent ADR0006r5 and complete assignment contract remain unchanged.

## Requirement and boundary

GatewayFirstLine/ReadyForSecrets in crates/org-host-control/src/web.rs require the host to verify the suspended image, then the web process to restrict itself before rendezvous ready_for_secrets, before any secret handoff or untrusted browser input. No current org-socket adapter implements it. This is a missing prerequisite, not a disproved assignment/journal premise. Scope this decision to macOS; do not select a Linux minimum ABI, pretend Linux enforcement exists, or waive its existing Landlock+single-seccomp-filter contract. Unsupported platforms fail closed until their own concrete adapter is implemented and validated. Architectural READY here is only permission to implement the macOS path, not full platform/product readiness or acceptance.

## Selected mechanism

1. Keep narrow OS sandbox primitives in existing libc-only org-socket. No project-crate dependency or new external crate. Web remains forbid(unsafe_code); it composes this adapter and converts enforcement evidence into the existing ReadyForSecrets DTO. org-socket has no protocol, storage, host credential or domain knowledge. Current package dependency graph is unchanged.
2. On the web process's main thread before any concurrency: disable core dumps, preload only approved public static assets into bounded immutable memory, close all unintended inherited descriptors (before rendezvous; retain only stdio/launch input required by the real spawn contract), then verify pthread_main_np==1, pthread_is_threaded_np==0 AND proc_pidinfo(PROC_PIDTASKINFO) succeeds with the exact expected structure size and pti_threadnum==1. Any uncertain result aborts before readiness. No runtime bypass or debug-only sandbox skip. The trusted entry point must call this before application runtime/thread creation; checking one thread does not make it safe to call after untrusted input.
3. Apply this fixed Seatbelt profile with sandbox_init_with_parameters, flags0 and NULL-terminated name/value parameters binding the absolute NUL-free UTF8 rendezvous pathname. Do not interpolate path text into SBPL:

    (version 1)
    (deny default)
    (allow network-inbound)
    (deny network-bind)
    (allow network-outbound
      (remote unix-socket (path-literal (param "RENDEZVOUS"))))

No file/path/root-directory allowlist, Mach service allowlist, keychain access, process execution/fork allowance or general outbound permission is added. Inbound allows accepting the listener the host will pass, while explicit deny network-bind forbids new listeners. Approved static assets are already in memory. TLS certificate/key supplied by the protected host remain in memory under their existing custody contract; no CA/database key or credential files are made available.
4. On successful enforcement only, connect to that exact approved rendezvous, send ReadyForSecrets from the same sole thread and continue the existing handoff. Host binds acceptance to the verified child identity/audit token, rechecks image/liveness and independently queries its sandbox state through sandbox_check(pid,NULL,0), requiring a positive sandbox-state result and denial (exact result1, not errors) for known critical host/office path read/write operations via sandbox_check(...,SANDBOX_FILTER_PATH,...). These path probes are kernel permission queries, not secret-file reads. Any check failure kills/reaps the child before passing descriptors or secrets. Image checks plus fixed-code enforcement are the policy trust basis; a handful of path probes is not claimed to prove every possible sandbox right.
5. Subsequent web-created threads inherit the already-applied policy. Only approved received descriptors carry browser/control/office traffic. Preserve exact descriptor cardinality/type checks, host EOF/revocation/deadline handling, strict secret ingress, authorization and output sanitization. The sandbox is defense in depth; it does not replace these controls. No alternate socket tunnel, mock backend or sandbox-success constant is authorized.

## Evidence and limits

E1 VERIFIED source contract: web.rs handshake order and single-thread Linux report; org-socket Cargo/lib current libc-only leaf; web forbidunsafe existing library/source. Review exact current files and ADR0006r5, not a broad implementation diff.
E2 ACTUAL disposable macOS child probes: macos-parameter-probe.json applies the parameterized policy with normal and quote/backslash/newline/Unicode paths; approved exact Unix connection and inherited TCP accept succeed, while unrelated Unix/TCP connections, file read/write, new TCP bind/listen and process spawn return EPERM. A broad inbound profile without deny-bind incorrectly permitted new listeners; selected profile explicitly fixes this observed difference. Prior syntax/long-path failures are retained, not hidden.
E3 ACTUAL SDK C child: macos-thread-and-host-probe.json verifies initial main/thread count/never-threaded preconditions; creates a thread after sandbox application, and that thread cannot read an unrelated temporary file. Parent observes child sandbox-state1 and denied read/write1; unsandboxed parent reports0. Source/build diagnostics and cleanup are retained. No real secrets, services or organization roots used.
E4 LIMIT: Seatbelt custom-profile APIs are deprecated and the SDK explicitly says no longer supported. Current OS exposes and actually enforces them; this is observed feasibility, not an Apple support guarantee. Treat OS/SDK changes as recheck triggers; any missing symbol/profile/enforcement failure refuses startup without fallback. Keep the volatile adapter narrow. Product Rust/HTTP/TLS/assets/FD lifecycle remains unverified until actual executable tests. No universal macOS-version support claim or physical compromise isolation claim.
E5 LINUX NOT SELECTED: current official kernel docs give named-Unix resolution permission at ABI9, so an older filesystem-only Landlock domain must not be claimed to restrict arbitrary named Unix connects. This note does not introduce a new Linux minimum or reinterpret historical wire fixtures.

## Alternatives, recheck and acceptance

Rejected: remove sandbox readiness (violates existing custody contract); unsafe code in web (unnecessary boundary expansion); broad filesystem reads for runtime convenience (leaks custody); raw string SBPL interpolation (avoidable injection); new platform crate/dependencies (existing leaf suffices); broad inbound without deny-bind (observed additional listener creation); use App Sandbox entitlement alone as if it supplies this exact per-spawn Unix policy (not established and does not match the selected rendezvous contract).

Feasibility permits implementation only if independently READY. Product acceptance requires real Rust child tests for positive/negative thread preconditions, special path parameters, denied ambient file/process/socket behavior, inherited browser listener and approved transfer/office descriptor traffic, parent post-accept enforcement/image rechecks, failure cleanup/no secret handoff, and actual supervised static-UI/HTTP assignment with current full checks and fresh code/original-author/final reviews. Do not add broad allowances if a runtime call fails: return exact evidence and reassess policy. The same full task assignment contract remains binding. This decision neither implements nor completes release signing, Linux sandboxing, HQ recovery, setup registration or browser functionality.

## Independent review and activation

### Implementation binding: web signing identity

Lead clarification, 2026-11-15: use the separately compiled signing identifier
`com.ellosoft.agentic-org.org-gateway` for `agentic-org-web`. The existing office
worker identifier `com.ellosoft.agentic-org.org-server` remains unchanged and must
not satisfy web admission. This selects the missing literal for the already
required distinct-image check; it introduces no new trust root, entitlement,
dependency or protocol. The independent revision-1 review covered the distinct
verified web image boundary, not this subsequently selected identifier.

Release admission uses the existing Developer ID Application/team requirement,
hardened-runtime and rejected-debug/injection-entitlement policy, with no web
Keychain access-group entitlement. Preserve suspended-child admission, audit-token
continuity and the post-accept image/liveness checks before any handoff. A release
build without a valid compiled team identifier refuses admission. Explicit debug
signature-check behavior may reuse the existing development policy, but sandbox
enforcement and descriptor/authentication checks never become optional.

Validate wrong image/worker identity, missing team, non-hardened code, disallowed
entitlements and image replacement as refusal paths with no secret handoff.
Disposable unsigned debug fixtures establish only their actual process behavior;
they do not establish Developer ID release-signing or packaging acceptance. Do
not access the user's signing credentials or Keychain to manufacture that proof.

Fresh read-only reviewer `01a1e104-234a-7373-ab5f-b78c183a3f51` returned ARCHITECTURE **READY**, no findings, on exact candidate SHA-256 `7d09e440713ea85f1f2a926e85a6dbc94a8fdeaa7c9265472f00cc2e54569139`. Required assignment Sol/Xhigh; reviewer explicitly did not independently attest model/effort. Actual ordered five-source read ACK and current preflight verified; raw and lead-normalized ACK retained. The original author separately proposed the same policy boundary; this reviewed variant binds the pathname through parameters and uses probed proc_pidinfo plus main/never-threaded guards. Mechanism above is unchanged by activation. Evidence under `.agent-kit/logs/v1-assignment-integration/gateway-sandbox-scope/`. This activates only macOS implementation; Linux and full product/code acceptance remain unverified and required where applicable. Original fullstack author remains sole product writer.
