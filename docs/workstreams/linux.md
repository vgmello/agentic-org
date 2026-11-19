# Linux stream (Track X)

Last updated: 2026-11-19 (linux-dev, v1 a31248ca)

Scope: Track X of `docs/implementation-plan.md`: Linux and container hosting behind platform
contracts (`crates/org-platform`, `crates/org-platform-host`), the Linux service and container
(`deploy/container/`), HTTPS for the container, both systems in CI, and the Track X rows of
`docs/FEATURES.md` (FC-164; the HTTPS part of FC-017). Signing the release image is out of scope.

**The owner asked on 2026-11-19 to finish only work in flight; nothing new starts without the owner.**

## Status

| Increment | Status | Evidence | Next / remaining |
| --- | --- | --- | --- |
| X1a · the platform crate and the check | Done | v1 (`crates/org-platform`; LNX-5 check `crates/org-platform/tests/platform_conditionals.rs`) | — (the allowlist's last entries are X6's) |
| X1b · files, locations, processes and sockets | Done | v1 (`org_platform::process`, `socket`) | — |
| X1c · admission, confinement and presence | Done | v1 (`org_platform::confinement`; macOS CI fix cf49f0f0) | Not yet run on a signed macOS build (needs the release pipeline) |
| X1d · credentials | Done | v1 (`org-platform-host` credentials) | — |
| X2 · the first office worker on Linux (LNX-1) | Done | v1 54f0a404 | — |
| X3 · host secrets on Linux (LNX-4) | Done | v1 54f0a404; custody rounds in de960b2c | — |
| X4 · the web process confined on Linux (LNX-3) | Done | v1 54f0a404 | — |
| X5 · service and container (LNX-2) | Done (its HTTPS criterion moved to X7 by the owner's decision of 2026-11-18) | v1 de960b2c, c60bcada, 58a44b2b; restart-race fix in a31248ca | — |
| X6 · both systems in CI (LNX-5, LNX-6) | In progress | v1 701d8460 (CI `container` job with the profiles, Linux `agent-os` timings, platform facts for test code) | The last 11 predicates, in `apps/agentic-org-office/src/startup/tests.rs` (8) and `tests/launch/activation/transfer.rs` (3), to empty the allowlist; then FC-164 Done; `session attach` timings after B6 |
| X7 · HTTPS for a container, with the instance's own certificate | In progress | v1 a31248ca (self-made or supplied pair, `tls.set`, renewal, instance view `tls`) | The container's switch to https (`deploy/container/setup.json`, README, LNX-2 https steps); a Linux and container route for `admin tls set` (owner decision below); P3s: re-announce `tls.renew` after a failed web fetch; the gate cache's doc wording |
| Release image | Not started | — | Needs the release pipeline |

## Next up

When the owner resumes Track X: X7's container switch to https (setup.json, README, LNX-2's https
steps), then X6's last 11 predicates to empty the allowlist (LNX-5), which closes FC-164.

## In flight

Nothing on origin. One local branch `x/x5-restart-race-diag` (bdfe1a28) in the linux-dev checkout:
diagnostic logging only, never to merge; delete when the owner agrees.

## Open decisions (owner)

- `agentic-org admin tls set` refuses on Linux and in containers: no password is sent to a host
  known only by its user. Route: command-line sign-in approved in the browser (FC-173), or a contract
  change (a root-local `tls.set`, as SIGN-33 does for the reset).

## Detailed history

`.agent-kit/tasks/v1-track-x.md`: decisions, review rounds, the model of each phase, evidence. Its
own status table is superseded by this file.

## For the other streams

- backend: `docs/design/office-host-boundary.md` still says HTTPS is unavailable (X7 is in v1,
  a31248ca). The last 11 platform predicates are in your office test files; Track X takes them when
  the owner resumes it, or you fold them in. Every `Registry` call now waits up to 5 s for the
  locator lock (a31248ca); the instance root lock stays try-only. B1b's rewritten LNX-2, step 10
  included, passed in the CI `container` job on GitHub's Ubuntu runner (run 44552811792).
  `supervisor::tests::an_admission_that_could_not_be_built_is_asked_for_again` failed once under
  load on Linux (30 of 30 alone).
- ui: `wire.ts` accepts `listen_scheme: http_published`, and https instances send
  `tls: ok|missing|expiring` (a31248ca); nothing in the UI shows TLS yet.
- all: the CI `container` job runs `deploy/container/lnx2.sh` on every push, with the seccomp and
  AppArmor profiles on GitHub's runner; a change that breaks LNX-2 fails CI. macOS CI still fails 7
  existing `org-host` tests (`release::store` ×6, `unsupported_bootstrap_*`), not Track X's.
