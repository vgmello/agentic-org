---
id: doc-63
title: 'Design: Executables and Entry Points'
type: guide
created_date: '2026-11-21T00:48:00Z'
updated_date: '2026-11-21T00:48:00Z'
tags:
  - design
  - architecture
  - binaries
  - status
---
> **Review note (2026-11-21):** Synchronized in `TASK-19` (reflects live `agentic-org-web` 9 host routes, `agentic-org setup`, and B1b/B2.1 `org-store` state). **Remaining drift & open work:** (a) the `agentic-org` row says "On macOS" while Linux setup/systemd/container foreground mode landed in `X5` (`decision-17`); (b) browser-to-office domain routes remain pending (`TASK-5`, B2 Q8); (c) `agent-os` still references `org-spool` pending deletion, and thinning `agentic-org-office`/`agent-os` plus wiring `org-command` are tracked in `TASK-15`; (d) the "Libraries that exist and are not yet reachable" paragraph still lists `org-release` and `org-socket` confinement primitives even though `org-release` is wired into `org-host/src/release/` (`decision-9`) and confinement lives in `org-platform`/`org-platform-host` (`decision-8`, `decision-17`).

# Executables and entry points

Agentic Org ships four executables. Application directories, Cargo packages and
executable names are the same. Reusable domain and protocol libraries keep their
`org-*` names. What each program is responsible for is fixed by the
[architecture non-negotiables](../architecture-non-negotiables.md) (rule 7).

| Executable | Source | Job |
| --- | --- | --- |
| `agentic-org` | `apps/agentic-org` | The person's command line. `setup` creates the instance and prints a one-time registration link; `serve` runs the host, which owns accounts, sessions and secrets and starts the offices and the web process. |
| `agentic-org-office` | `apps/agentic-org-office` | One office worker: every domain rule, the only writer of organization data, and the supervisor of the office's agents. |
| `agentic-org-web` | `apps/agentic-org-web` | Serves the static UI and `/api/v1` to browsers. Holds no keys and opens no database. |
| `agent-os` | `apps/agent-os` | The only way an agent reaches the product. |

The static browser application is `apps/agentic-org-ui` (package `agentic-org-ui`).
It builds to static assets that `agentic-org-web` serves; it is not another
executable or JavaScript service.

## Host composition

Host behaviour lives in the library `crates/org-host`. `agentic-org serve` composes
it; there is no separate host executable and no app-to-app dependency. Plain
`agentic-org` prints help and never starts a host. Service registration invokes the
pinned `agentic-org` with `serve` and an optional instance name; the saved setup
state supplies its root, listener and release identity.

Host integration tests and test-only fixture binaries live in `apps/agentic-org`
beside the composition they exercise. Test-support features keep fixtures out of a
production build. The host finds the worker as the sibling executable
`agentic-org-office`.

Code-signing identifiers, entitlement requirements, launch framing and key-store
namespaces are persisted identities. They do not follow file names, and release
tooling signs the four executables with those identities.

## Where the entry points stand (2026-11-21)

This section is what the code does today. Each statement was checked against the
source; recheck it against the commit you work on.

| Executable | What works |
| --- | --- |
| `agentic-org` | On macOS: `setup [--name N] [--port P]` creates the instance, registers and starts its launchd user service and prints one registration link when the output is a terminal ([`crates/org-host/src/setup_flow.rs`](../../crates/org-host/src/setup_flow.rs)); `admin reset [--name N]` issues a new registration link after local presence and the system dialog (its set-new-password branch answers `unavailable` until A3); `service start\|stop\|status [--name N]`; `serve --name N` runs a registered instance from its frozen setup record and creates the host registry at first start. Help and version. Setup on unsupported systems or with `--satellite` refuses before any change. A release build of `setup` and `serve --name` refuses until it carries a pinned runtime release (SRV-1). Debug builds keep the legacy `serve --home` entry for integration tests. The host socket answers `hello`, `instance.status` (with `administrator`), `admin.link.initial` and `admin.reset` (`crates/org-host/src/service/mod.rs`, `crates/org-host/src/admin.rs`). |
| `agentic-org-office` | Runs when the host starts it, on macOS and GNU/Linux (`org_platform_host::admission`: on macOS the child is checked by its code signature, on Linux it runs from a sealed copy of the bytes the host hashed; a system with neither adapter starts none). Uses `org-store` and verbatim `schema.sql` (`B1b` + `B2.1` landed: organization family tables actively projected/queried in `crates/org-office/src/store/sqlite/organization.rs`; legacy `org.db`/`office.db` and out-of-store SQL removed). The host launches it only over the registered instance's verified rendezvous; a host with no registered instance starts no office. Serves the local socket for `agent-os`. |
| `agentic-org-web` | Help and version. `--serve` runs only after a verified host handoff and otherwise refuses without opening a listener ([`apps/agentic-org-web/src/main.rs`](../../apps/agentic-org-web/src/main.rs)). Serves static assets and 9 live host-backed API routes (`apps/agentic-org-web/src/api.rs`): `GET /api/v1/version`, `GET /api/v1/instance`, `POST /api/v1/links/register`, `POST /api/v1/session/sign-in`, `GET /api/v1/session`, `POST /api/v1/session/sign-out`, `GET /api/v1/organizations`, `POST /api/v1/organizations`, and `GET\|HEAD /api/v1/operations/host/{id}`. Browser-to-office domain routes (`B2 Q8` / `TASK-5`) remain pending. |
| `agent-os` | Directory calls (`who`) against a running office when one is named; the office answers `msg send`, `msg inbox`, `msg read` and `msg wait` with `unsupported` until B6 rebuilds messaging on the new store. With no office named it writes to a file queue, which is deleted (implementation plan B6). |

Libraries that exist and are not yet reachable from any command: release
verification (`org-release`), socket admission and confinement primitives
(`org-socket`), the work-file codec and the office's acceptance journal, recovery
escrow (`org-pki`), protected HQ creation and recovery delivery in `org-host`.

The order in which these become usable is the
[implementation plan](../implementation-plan.md).

## Verification

Run format, workspace Clippy and tests with the pinned toolchain. Tests check the
four production targets, that no standalone host binary exists, that help and
version create no state, that `setup` refuses satellite flags and unknown arguments
without partial provisioning, that `setup` run twice changes nothing (with a
child-process service manager; no test touches launchd unless opted in), and
that web startup refuses without opening a listener until the handoff is wired.
Tests use temporary roots and never touch a real organization.
