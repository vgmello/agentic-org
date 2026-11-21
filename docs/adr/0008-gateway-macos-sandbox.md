# ADR-0008 · macOS confinement of the web process (GW-SBX-MAC-1)

Status: accepted; **implemented** in `crates/org-platform-host/src/confinement/` and
composed by `apps/agentic-org-web`. This is the macOS implementation of the
confinement contract of [ADR-0017](0017-linux-and-container-hosting.md); the Linux
implementation (Landlock and seccomp) is a separate v1 adapter in the same module.
"Web" means the `agentic-org-web` process.

## Requirement and boundary

The host verifies the suspended web image. The web process then confines itself,
and only then connects to the launch rendezvous and reports ready for secrets (the
Linux adapter connects first and confines before reading or reporting; see the
[Linux host design](../design/linux-host.md#33-confinement-of-the-web-process)), so before any secret
handoff and before any untrusted browser input. A platform without a working
adapter refuses to start the web process; it never runs unconfined.

## Mechanism

1. The sandbox primitives live in `org_platform::confinement`, the macOS adapter of
   the confinement contract of [ADR-0017](0017-linux-and-container-hosting.md)
   (moved there from `org-socket` in increment X1c). That crate depends on the C
   library only, plus the system's `libsandbox`, and adds no external dependency. The
   web process keeps `forbid(unsafe_code)`; it composes this adapter and turns
   enforcement evidence into its ready-for-secrets message. `org-platform` has no
   protocol, storage, host credential or domain knowledge.
2. On the main thread, before any concurrency, the web process disables core
   dumps, preloads only approved public static assets into bounded immutable
   memory, and closes every unintended inherited descriptor, keeping only standard
   I/O and the launch input. It then verifies `pthread_main_np == 1`,
   `pthread_is_threaded_np == 0`, and that `proc_pidinfo(PROC_PIDTASKINFO)`
   succeeds with the exact expected structure size and `pti_threadnum == 1`. Any
   uncertain result aborts before readiness. There is no runtime bypass and no
   debug-only skip. The entry point calls this before creating any runtime thread.
3. It applies this fixed Seatbelt profile with `sandbox_init_with_parameters`,
   flags 0, and NULL-terminated name/value parameters that bind the absolute,
   NUL-free UTF-8 rendezvous path. Path text is never interpolated into the profile.

       (version 1)
       (deny default)
       (allow network-inbound)
       (deny network-bind)
       (allow network-outbound
         (remote unix-socket (path-literal (param "RENDEZVOUS"))))

   No file, path or root-directory allowance, Mach service allowance, keychain
   access, process execution or fork, or general outbound permission is added.
   Inbound allows accepting the listener the host passes; the explicit
   `deny network-bind` forbids new listeners. Static assets are already in memory.
   The TLS certificate and key the host supplies stay in memory; no CA key or
   credential file is reachable.
4. Only after enforcement succeeds does the process connect to the approved
   rendezvous and send ready-for-secrets from the same sole thread. The host binds
   acceptance to the verified child identity and audit token, rechecks image and
   liveness, and queries the child's sandbox state with `sandbox_check(pid, NULL, 0)`,
   requiring a positive result, and denial (exactly 1, not an error) for known
   critical host and office paths through `sandbox_check(..., SANDBOX_FILTER_PATH, ...)`.
   These are kernel permission queries, not file reads. Any failed check kills and
   reaps the child before descriptors or secrets are passed. The trust basis is the
   image checks plus fixed-code enforcement; the path probes do not claim to prove
   every right.
5. Threads created later inherit the policy. Only approved received descriptors
   carry browser, control and office traffic. Exact descriptor count and type
   checks, host EOF, revocation and deadline handling, strict secret ingress,
   authorization and output sanitization all remain; the sandbox is defence in
   depth. There is no alternate socket tunnel, mock backend or constant that reports
   sandbox success.

## Signing identity

`agentic-org-web` is signed with its own identifier,
`com.vgmello.agentic-org.web` (owner decision 2026-11-17: there is no gateway; nothing
has shipped, so the identity is renamed in place). The office worker's identifier,
`com.vgmello.agentic-org.office`, never satisfies web admission. Release admission
uses the Developer ID Application team requirement, the hardened runtime, and
rejects debug and injection entitlements; the web process has no keychain
access-group entitlement. A release build without a valid compiled team identifier
refuses admission. A debug build may relax the signature check only; sandbox
enforcement and descriptor and authentication checks are never optional.

## Limits

Seatbelt custom-profile APIs are deprecated in the SDK. The current OS exposes and
enforces them; this is observed behaviour, not an Apple support guarantee. An OS or
SDK change is a trigger to recheck. A missing symbol, profile or enforcement result
refuses startup, with no fallback.

## Rejected alternatives

Dropping sandbox readiness; unsafe code in the web process; broad filesystem reads
for convenience; interpolating paths into the profile; a platform crate for this
adapter alone (ADR-0017 later created `org-platform` for every system difference); broad
inbound without `deny network-bind`, which permitted new listeners; and the App
Sandbox entitlement alone, which does not give this per-spawn Unix-socket policy.
If a runtime call fails under the profile, the policy is reassessed with evidence;
no broad allowance is added.

## Acceptance

- Real Rust child tests for positive and negative thread preconditions, special
  characters in the path parameter, denied ambient file, process and socket access,
  the inherited browser listener, approved descriptor transfer and office traffic,
  the host's post-accept enforcement and image rechecks, and cleanup with no secret
  handoff on failure.
- Wrong image or worker identity, a missing team, non-hardened code, disallowed
  entitlements and image replacement are each refused with no secret handoff.
  Unsigned debug fixtures prove process behaviour only, not release signing.
- The supervised static UI and HTTP path runs under the profile.
