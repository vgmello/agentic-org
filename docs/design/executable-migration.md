# Executable migration from v1

This change migrates implemented executable responsibilities. Application directories, Cargo packages and executable names use the same names. Reusable domain/protocol libraries retain their existing org-* names. It does not complete the separate standalone installer, production web authentication or agent execution/pool designs.

| Executable | Source/package | Implementation status |
| --- | --- | --- |
| agentic-org | apps/agentic-org / agentic-org | Human CLI dispatch and serve run the existing host library. setup is reserved for HQ-only onboarding but explicitly unavailable until protected bootstrap and service registration land. |
| agentic-org-office | apps/agentic-org-office / agentic-org-office | Existing office daemon, unchanged launch/state protocol and storage. |
| agentic-org-web | apps/agentic-org-web / agentic-org-web | Existing HTTP/assets/TLS library retained; new executable supports help/version and refuses production startup until authenticated host handoff and backend composition land. |
| agent-os | apps/agent-os / agent-os | Existing employee CLI, preserving binding, environment variables and live Phase A spool behavior. |

The static browser application is `apps/agentic-org-ui`, frontend package
`agentic-org-ui`. CI, build scripts and licensed UI containment use that path. It
produces static assets served by `agentic-org-web`, not another executable or
JavaScript service.

## Host extraction

Host behavior lives in crates/org-host as a library, preserving its build guard. The human CLI in apps/agentic-org composes this library; it is not another executable, and there is no app-to-app dependency or extra host daemon. Production startup is explicit through agentic-org serve. Plain agentic-org displays help and never starts a host implicitly. Service managers must use serve when their registration implementation lands.

Existing host integration tests and test-only fake/fixture binaries live in agentic-org alongside the composition they exercise. Test-support features keep fixtures out of a normal production build. Host unit tests remain beside the library. Tracked-source SQLCipher/build-opener and UID-ownership audits follow the moved files; they are not disabled.

The host's sibling executable discovery now selects agentic-org-office. Code-signing identifiers, entitlement requirements, private launch framing, environment restrictions and key-store namespaces remain stable independently of filenames. Signing/release tooling must sign the new filenames with the existing required identities. This branch does not relax platform validation or ship a plaintext release credential fallback.

## State and live tooling

No database migration, ID change, spool move, namespace reset, service installation or interaction with a running user's organization is performed by this source migration. The previously installed live CLI remains untouched until a separately verified cutover. New build/test invocations use agent-os. CLI help and generated messaging/process hints use the new name while wire field names, ORG_AGENT/ORG_OFFICE bindings and deduplication semantics stay unchanged.

Superseded application directories and package/executable aliases are removed. Cargo build/test commands select the same name as the application directory and production binary. org-host is library-only. Test-only fixture names and the legacy relay dispatch behavior are not new production service executables.

## Pending standalone integration

Initial agentic-org setup must create HQ only, automatically install/register/start its service and expose no satellite choice. Satellite onboarding is a separate HQ-initiated flow. Neither workflow is fabricated here: v1 has no wired protected bootstrap/service registration to migrate. The command returns failure before any provisioning and rejects satellite join flags with HQ guidance.

Likewise, the web executable must receive the authenticated listener/control handoff and real browser authorization backend before production startup. A test-only loopback harness or UnavailableApi cannot establish that boundary. The new executable fails closed instead of exposing that harness as production. Installer downloads, signed offline bundles, update activation and production macOS/Linux service registration remain integration work, not claimed working functionality.

## Verification

Use the pinned Rust toolchain and locked dependencies. Run format, workspace clippy/tests and the existing platform-specific host checks. Linux retains v1 CI's host exclusions and lint exceptions pending platform bring-up, now covering the new host composition package; CLI-only migration tests still run there. macOS CI continues to exercise the complete host integration suite.

New tests verify the four production targets, absence of a standalone host binary, help/version without state creation, setup refusal without partial provisioning, and web startup refusal without opening a listener. Existing office, agent CLI, messaging and source-security tests follow the new executable paths. Tests use disposable roots and never install into or change a user's live organization.
