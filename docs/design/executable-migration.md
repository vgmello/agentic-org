# Executable migration from v1

This change migrates implemented executable responsibilities. Application directories, Cargo packages and executable names use the same names. Reusable domain/protocol libraries retain their existing org-* names. It does not complete the separate standalone installer, production web authentication or agent execution/pool designs.

| Executable | Source/package | Implementation status |
| --- | --- | --- |
| agentic-org | apps/agentic-org / agentic-org | Human CLI composes org-host. Selected HQ setup journals its original inputs/operations, admits a complete pinned release, invokes protected bootstrap/provisioning, reconciles per-user launchd registration and probes local readiness. Missing trusted build inputs or platform prerequisites refuse setup. Recovery collection remains unfinished; end-to-end production setup is not verified. |
| agentic-org-office | apps/agentic-org-office / agentic-org-office | Existing daemon and legacy protocols retained, with versioned selected writer admission, canonical bootstrap/adoption, durable task acceptance and escrow prerequisites. |
| agentic-org-web | apps/agentic-org-web / agentic-org-web | Supervised authenticated listener/control handoff, real account/session backend and selected-task routes are wired. Selected release startup independently authenticates the pinned manifest, kernel image and static assets before confinement and secrets. Production publisher/platform signing evidence remains required. |
| agent-os | apps/agent-os / agent-os | Existing employee CLI, preserving binding, environment variables and live Phase A spool behavior. |

The static browser application is `apps/agentic-org-ui`, frontend package
`agentic-org-ui`. CI, build scripts and licensed UI containment use that path. It
produces static assets served by `agentic-org-web`, not another executable or
JavaScript service.

## Host extraction

Host behavior lives in crates/org-host as a library, preserving its build guard. The human CLI in apps/agentic-org composes this library; it is not another executable, and there is no app-to-app dependency or extra host daemon. Production startup is explicit through agentic-org serve. Plain agentic-org displays help and never starts a host implicitly. Selected service registration invokes the exact pinned CLI with serve and an optional instance name; protected setup state supplies its root, listener and release identity.

Existing host integration tests and test-only fake/fixture binaries live in agentic-org alongside the composition they exercise. Test-support features keep fixtures out of a normal production build. Host unit tests remain beside the library. Tracked-source SQLCipher/build-opener and UID-ownership audits follow the moved files; they are not disabled.

The host's sibling executable discovery now selects agentic-org-office. Code-signing identifiers, entitlement requirements, private launch framing, environment restrictions and key-store namespaces remain stable independently of filenames. Signing/release tooling must sign the new filenames with the existing required identities. This branch does not relax platform validation or ship a plaintext release credential fallback.

## State and live tooling

No database migration, ID change, spool move, namespace reset, service installation or interaction with a running user's organization is performed by this source migration. The previously installed live CLI remains untouched until a separately verified cutover. New build/test invocations use agent-os. CLI help and generated messaging/process hints use the new name while wire field names, ORG_AGENT/ORG_OFFICE bindings and deduplication semantics stay unchanged.

Superseded application directories and package/executable aliases are removed. Cargo build/test commands select the same name as the application directory and production binary. org-host is library-only. Test-only fixture names and the legacy relay dispatch behavior are not new production service executables.

## Selected integration status

Initial agentic-org setup creates HQ only and exposes no satellite choice. Satellite onboarding remains a separate, unavailable HQ-initiated flow. The selected implementation consumes a complete local directory or tar.gz release with build-pinned publisher trust; it never supplies a generated production trust key or an unsigned fallback. Setup resumes its protected original operation IDs and exact installed pin, including when the original input bundle is no longer present. An existing pin without its setup intent requires explicit repair/adoption. The launchd adapter has isolated real bootstrap/replay/conflict/bootout evidence; this does not establish genuine signed-release setup or a deployed user service. See [standalone server](standalone-server.md) and [ADR-0009](../adr/0009-complete-release-verification.md).

The web executable receives the authenticated listener/control handoff and real browser authorization backend. Disposable Chromium evidence exercises protected bootstrap/adoption, supervised Rust services and the built static UI through the explicit debug image/custody path; it proves neither production signing nor normal setup/service deployment. WebKit verification remains unresolved. Installer downloads, recovery collection/export/ACK, authenticated production artifacts and Linux confinement/service support remain incomplete. Missing release trust or platform admission fails closed. No production mock backend is provided.

## Verification

Use the pinned Rust toolchain and locked dependencies. Run format, workspace clippy/tests and the existing platform-specific host checks. Linux retains v1 CI's host exclusions and lint exceptions pending platform bring-up, now covering the new host composition package; CLI-only migration tests still run there. macOS CI continues to exercise the complete host integration suite.

New tests verify the four production targets, absence of a standalone host binary, help/version without state creation, setup refusal without partial provisioning, and web startup refusal without opening a listener. Existing office, agent CLI, messaging and source-security tests follow the new executable paths. Tests use disposable roots and never install into or change a user's live organization.
