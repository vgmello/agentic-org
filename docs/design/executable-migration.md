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

## Where the entry points stand (2026-11-17)

This section is what the code does today. Each statement was checked against the
source; recheck it against the commit you work on.

| Executable | What works |
| --- | --- |
| `agentic-org` | `serve`, help and version. `setup` refuses without changing state ([`apps/agentic-org/src/lib.rs`](../../apps/agentic-org/src/lib.rs)). The host socket answers only `hello` and `instance.status` (`crates/org-host/src/service/mod.rs`). |
| `agentic-org-office` | Runs when the host starts it, on macOS only (`crates/org-host/src/spawn.rs`). Serves the local socket for `agent-os`. Contains the HQ creation provider, which nothing calls yet. |
| `agentic-org-web` | Help and version. Startup refuses to open a listener ([`apps/agentic-org-web/src/main.rs`](../../apps/agentic-org-web/src/main.rs)). |
| `agent-os` | Messaging, directory and work calls against a running office when one is named; with none it writes to a file queue, which is deleted (implementation plan B6). |

Libraries that exist and are not yet reachable from any command: release
verification (`org-release`), socket admission and confinement primitives
(`org-socket`), the work-file codec and the office's acceptance journal, recovery
escrow (`org-pki`), protected HQ creation and recovery delivery in `org-host`.

The order in which these become usable is the
[implementation plan](../implementation-plan.md).

## Verification

Run format, workspace Clippy and tests with the pinned toolchain. Tests check the
four production targets, that no standalone host binary exists, that help and
version create no state, that `setup` refuses without partial provisioning, and
that web startup refuses without opening a listener until the handoff is wired.
Tests use temporary roots and never touch a real organization.
