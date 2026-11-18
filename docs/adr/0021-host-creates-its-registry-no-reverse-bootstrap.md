# ADR-0021 · The service host creates its own registry; setup has no reverse bootstrap

Status: **accepted** (A2 decision D1). Partly implemented: the registry is created
with no account and the reverse-bootstrap wire is removed; the first-start creation
in `serve` and `admin.link.initial` are A2 increment B3. The
[standalone design](../design/standalone-server.md#setup-executable-and-release-store)
follows it.

## Context

Setup used to create the instance through a typed reverse connection. Setup opened a
private listener, `agentic-org serve` in bootstrap mode connected to it, and the
`bootstrap` and `exit` methods carried the first administrator's login and password,
and optionally a TLS key pair, into the host. That host created the registry and the
first account, then had to hand its instance, root and host locks to the normal
runtime without an unlocked restart.

The installation design has since changed. Setup asks for no username or password
and creates no account: the first administrator registers in the browser through a
one-time link. Setup refuses a supplied TLS pair until the host certificate verifier
exists. Nothing secret is left for the reverse connection to carry.

## Decision

1. **There is no reverse bootstrap.** The `bootstrap` and `exit` methods, the
   bootstrap TLS pair and the bootstrap journal fingerprint are removed from the
   host-control protocol.
2. **The service host creates its own registry at first start.** `agentic-org serve
   --name <name>`, started by the service manager, reads the frozen setup record and
   creates the registry under the instance and host locks it holds for its whole run.
   No lock hand-off and no unlocked window exist. The registry starts with no account
   and no organization.
3. **Setup gets the initial link over `host.sock`.** After the service is ready, setup
   calls `admin.link.initial` with the frozen setup operation ID. The host issues a
   registration link only if no link was ever issued for that operation and no
   administrator exists. A replay answers `already_issued` with no secret. The call is
   admitted like every `host.sock` client: hello, the registered instance, a peer
   effective UID equal to the host's and the signing check.

## Why

- The secrets the reverse connection existed to carry are gone, so it would be a
  second privileged channel with nothing to protect.
- Creating the registry in the process that serves it removes the lock hand-off,
  which was the hardest part to make safe.
- One admission path (`host.sock`) is easier to review than two.

## Rejected alternative

Keep the reverse connection with a spawned bootstrap host. The bootstrap host would
create the registry and then hand its locks to the host that launchd starts. That
hand-off is the unlocked restart the design forbids, or it needs a custody protocol
between two processes that launchd does not coordinate.

## Consequences

- Setup is a client of the running service. A setup interrupted after the service
  starts resumes by asking the same host for the link; the host's rule, not setup's
  record alone, guarantees at most one initial link.
- Noninteractive setup does not call `admin.link.initial`; the status reports
  `administrator: none` until `agentic-org admin reset`.
- The release-gate and recovery-custody tests of the old bootstrap module move to the
  setup-flow tests (A2 B4).
