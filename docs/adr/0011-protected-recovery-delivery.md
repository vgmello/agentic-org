# ADR-0011 · Protected recovery delivery (REC-DEL-1)

Status: accepted. Implementation pending. Builds on
[ADR-0007](0007-hq-creation-escrow.md), whose code, KDF, envelope, CA identity,
worker-only key custody, public and private exclusions and retry rules are
unchanged. This is not full recovery after losing HQ (REC-1).

## Requirement

From the [standalone design](../design/standalone-server.md) and the
[installation UX](../specs/server-installation-and-access-ux.md) (RCV-15, 19, 21,
22 and 25 to 27): the recovery code is released only to the signed native CLI, run
by the instance's administrator for the local HQ, signed in by approving the run in
a browser (no password in the terminal), with the OS user present at the
machine, and shown only on a terminal. One confirmation covers showing and
recording in the same invocation; every redelivery needs a fresh confirmation. The
audit is durable and code-free, and the code is erased after the owner acknowledges
keeping it off HQ. A non-interactive invocation or an SSH session on macOS receives
no code. There is no browser or web surface for the recovery secret. A waiting or
shown code does not block an otherwise ready HQ; acknowledged material is never
redisplayed or regenerated.

## Presence adapter and connection authority

`recovery.release` (with its `redeliver` parameter) and `recovery.acknowledge` are
dispatched only through the native `host.sock` service to the verified signed
`agentic-org` client. Strict raw request decoding, the peer's kernel identity and
loaded-image revalidation apply before and after the asynchronous OS confirmation
and again immediately before the response and before deletion is authorized. No
caller-supplied presence flag, SSH environment variable, terminal string, bare PID
or account cookie proves presence. Web and agent peers cannot invoke these methods,
including package export.

**macOS.** A private `org-host` adapter, backed by a small Objective-C (ARC, blocks)
translation unit compiled by `cc` only on macOS, links Foundation,
LocalAuthentication, Security, SystemConfiguration and the system `libbsm` (for
`audit_token_to_asid`, `audit_token_to_pid` and `audit_token_to_euid`). It uses
`SessionGetInfo` with the ASID from the peer's kernel-authenticated audit token, the
current console UID from `SCDynamicStore`, and exact-size
`proc_pidinfo(PROC_PIDTBSDINFO)` for the still-live peer. It requires a non-root
peer with the same effective UID as the host and the current console user, a
graphic, non-remote session, exact token PID and UID identity with exec-swap
fencing, a controlling terminal (`e_tdev != NODEV`) and the foreground process group
(`pbi_pgid == e_tpgid`). The host verifies that it and the peer belong to the same
local console audit session, so a remote session cannot borrow the console UID. Any
query failure denies. The CLI also checks that stdin and stdout are a terminal and
runs its local screen and Enter preflight before sign-in. These facts are rechecked
after the OS reply and before release. A user switch, session end, peer exit or
exec, revoked account or disconnected channel invalidates the request.

**Linux.** Linux needs its own presence adapter under ADR-0017. Until it exists,
`recovery.release` refuses on Linux; it never releases a code without a presence
check.

Each release or redelivery allocates a fresh `LAContext` with
`touchIDAuthenticationAllowableReuseDuration = 0` and evaluates
`LAPolicyDeviceOwnerAuthentication` once, with a fixed sanitized reason that names
both showing the code and recording it as stored. Untrusted organization or account
strings are never interpolated. Success requires `success == YES` with no error, a
still-active request and every postcheck. `canEvaluatePolicy` alone is never
success. Cancellation, inability, any error, a timeout, a nil context and a late or
duplicate callback all deny, with no code, no change to the shown state and no
acknowledgement authority. Success is OS device-owner authentication, not
biometric-only identity.

One presence operation is in flight per host, with a monotonic 90-second deadline;
a concurrent request fails as busy rather than queueing prompts. No controller,
database, writer, session-map or custody lock is held while waiting: a short
reservation is taken under lock, the lock is released, the evaluation runs, and the
frozen account, session, organization, HQ, delivery ordinal and public-package
identity are rechecked after reacquiring. A reservation does not mark the code
shown. Shutdown or EOF invalidates the OS context and the reservation. The bridge
owns the callback's lifetime through ARC heap state; no Rust stack reference
escapes. The terminal state is synchronized and assigned once, so a late callback
cannot release a code or touch freed memory. No C exception, `NSError` or user text
crosses into Rust logs; only fixed status categories do.

A successful release creates one private, non-serializable acknowledgement
permission tied to the exact native connection, current session and account,
instance and organization, escrow and provisioning identity and the committed
delivery ordinal. It is never returned to the caller or stored. It ends with the
session or connection and is discarded on EOF, session end or revocation, another
release on that connection, or an acknowledgement attempt. A different connection or
a fresh CLI run must release again and authenticate again. Acknowledging the
delivered code needs that permission plus live authorization and every binding
check, with no second OS dialog in the same invocation. A lost acknowledgement
response after durable completion may be retried as an already-acknowledged,
non-secret result under live native administrator authority; it cannot redisplay
the code or create a new permission.

## Durable state, audit and custody cleanup

A protected, versioned table in the host database is bound to the immutable
provisioning identity and the opaque custody handle. There is no public or
replicated recovery-code table, and the pending secret is never copied into SQL. The
record holds version, instance, organization, creation operation and HQ office,
public escrow digest and public-key identity, state (`pending`, `delivered`,
`ack_pending`, `acknowledged`), a monotonic delivery ordinal, first shown time, last
redelivery time and count, and acknowledgement time and account. Audit entries hold
action, actor, time, organization, ordinal and result, never code, key, handle or
raw request. Audit and status report stored dates and counts, never an inferred
terminal outcome. Source, identity and version consistency is validated on open and
before each change; a partial, missing or mismatched identity needs forward repair,
never a reset or rotation.

The delivery record is created only for a completed provisioning whose frozen
public identity verifies. A healthy acknowledged record needs no pending code, and
acknowledgement is never inferred from missing custody. A pending code before HQ is
complete stays "preparation incomplete", not ready.

**Release:** reserve → verify native peer, administrator, HQ, public source and state
→ OS presence outside locks → revalidate → read the same pending code into the
existing non-`Clone`, zeroizing secret wrapper → atomically commit `delivered`, the
ordinal and a code-free audit entry → install the connection's acknowledgement
permission → send through the secret response path. A failure after the commit but
before a complete response leaves the code shown and requires an explicit
redelivery in a new invocation. `redeliver=true` on a pending code records the first
showing. A delivered code without `redeliver` is refused, reporting the stored dates
without a code. Redelivery reuses the same code and package and records exactly one
new redelivery after successful presence. An ordinary retry never rotates.
Exhausting the ordinal fails closed. Cancellation before the commit changes nothing.

**Acknowledge:** verify the live same-run permission and binding and the current
ordinal, then atomically commit `ack_pending` with the account, time and audit
identity, and consume the permission. Only after that durable intent is the pending
custody item removed, idempotently. Its absence is verified, then the record is
atomically marked `acknowledged` and the code-free `organization.changed` recovery
state is emitted. No other custody, writer or key is touched. A failed deletion
leaves `ack_pending`, with no secret response or redisplay and a resumable explicit
failure. Startup finishes a frozen `ack_pending` deletion idempotently before HQ is
ready; a crash at any deletion boundary cannot resurrect the code or invent a new
acknowledging actor or time. An exact acknowledgement retry completes the same
intent without new presence. An already-acknowledged retry returns success with no
extra audit, generation or rotation. Missing custody before `ack_pending` is a
recovery error, except for an evidence-backed acknowledged record.

## Public escrow source and package export

A dedicated transfer record, `RecoveryPublicRead { version: 1, office_id, channel_id }`,
carries exactly one host-created connected Unix stream descriptor over the
authenticated launch rendezvous. The transfer header stays ≤ 1 KiB and the normal and
grant frames are unchanged. The host's supervisor creates and transfers it only
after authenticating the worker and checking the frozen office and instance binding;
close, restart or revocation invalidates it. It is handled once with bounded read and
write deadlines and then closed. The worker passes its actual office and immutable
instance and office binding; a caller cannot supply another database or path. This
exchange is not multiplexed into writer-control ordering.

Strict version-1 public DTOs live in `org-office-launch::recovery`, with no domain
or PKI import. The request binds `instance_id`, `office_id`, `organization_id` and
the creation `operation_id`. The response repeats them and adds the escrow version,
public key, `package_sha256` and `package_hex`, the original encrypted bytes as
lowercase hex (≤ 8950 characters for ≤ 4475 bytes). Frames are newline JSON capped at
16 KiB with `deny_unknown_fields` and exact EOF and newline rules. An unsupported
version, unknown or duplicate keys, an oversized, truncated, trailing or extra frame,
and any mismatch of binding, digest, public header or key are refused before bytes
are exposed or exported. The package is 1 to 4475 bytes and keeps ADR-0007's exact
envelope structure, profile, organization and key. The host may parse the bounded
public header for equality and hashing; it never derives a key or decrypts, and
does not depend on `org-pki`.

The worker's only response source is `Office::hq_creation_public_record()`, which
verifies the committed public record and its copied projection. It never
serializes the local record or reads the private DER into a response. The worker
checks its HQ role and the matching creation operation; the host compares against
the frozen provisioning identity, the live worker admission and the delivery
ledger. A stale, missing or conflicting copied package is a recovery failure and is
never rebuilt from caller bytes or a path hint.

`recovery.package` is a native-only typed export: session and organization only, no
server filename, no office selector. It returns the same public record, with no
code, handle or key. It requires a live signed native instance administrator on the
local HQ with the local console and terminal context; it does not grant
acknowledgement permission and needs no separate OS dialog, because it reveals only
the already-public encrypted package. Browser, web and agent transports refuse it.
The CLI writes the exact bytes to an explicitly chosen new local file
(`O_CREAT | O_EXCL | O_NOFOLLOW`, mode `0600`, no overwrite, file and parent flushed)
and reports an export failure before the final acknowledgement. It never writes into
an organization directory. Export does not claim an off-HQ backup.

## CLI

`agentic-org recovery` release and status, with an explicit local package
destination, are implemented in the `agentic-org` CLI library. It checks the
terminal and local context before collecting credentials, then uses the saved
instance context or an explicit instance, the administrator's sign-in approved in
a signed-in browser, the exact
organization, the current recovery state, Enter, one OS confirmation, the code on
the terminal only, package export, the code's final group and a typed `yes` that
covers both code and package kept off HQ. The code never appears in arguments,
standard output JSON, logs, error strings, screenshots, history or the package, and
never goes to redirected output. There is no force or non-interactive override. A
wrong group, cancel or `no` leaves the code shown and unconfirmed. Status is
non-secret and states that recovery after losing HQ (REC-1) is not supported.
Setup creates no organization and runs no part of this flow. An organization is
created in the browser; its code starts Waiting, its HQ is ready without it, and the
browser shows the exact command to run on the HQ machine.

## Dependencies

The only new direct edge is `org-host` build → `cc`, plus the listed macOS SDK
frameworks and `libbsm`. Existing edges stay acyclic: no host → office or host → pki,
and no web → host.

## Acceptance

1. Strict DTO version, size, duplicate, trailing and unknown checks; a 4475-byte
   package and one over; digest, organization, office, instance, operation and key
   mismatches; and ordinary, web, agent and unsigned peers refused before any
   secret or custody use. The authenticated supervisor exchange returns the exact
   committed bytes, and the private DER cannot be serialized into it.
2. With a temporary protected database and custody: first show, redelivery
   (including a pending code with the flag), cancellation, timeout, query failure,
   an absent or changed console or terminal, wrong PID, token or exec, account
   revocation during the prompt, EOF, a stale generation and concurrent requests.
   No global lock is held while blocked, at most one prompt is pending, and no
   caller input stands in for presence. A test adapter exists only under
   `cfg(test)`.
3. Objective-C lifetime, late callback, timeout, cancellation and FFI status tests
   without real credentials. A signed native client's real positive and cancelled OS
   authentication, local-console acceptance and remote or no-terminal denial are
   separate platform evidence.
4. SIGKILL and reopen, and faults before and after the delivery commit and send and
   around the acknowledgement intent, deletion and commit: exact audit ordinal,
   count, actor and time, code reuse, an absent or corrupt handle, a deletion
   error, no redisplay in `ack_pending` or `acknowledged`, and no secret in status,
   audit or export. Other instance or user keys are never touched.
5. The real CLI against an organization created through the browser on the normal
   pinned service: collect, defer, redelivery, package export and off-HQ
   acknowledgement. The
   same CA public identity is recovered from the exported package and the code; a
   wrong code or package fails. REC-1 is reported unsupported.

Before the delivery ledger exists, secret release is disabled and the code, CA,
provisioning and Waiting readiness are preserved. After `delivered`, `ack_pending`
or `acknowledged` is stored, repair only moves forward.
