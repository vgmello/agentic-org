# First five feature verification audit

Audited **2026-11-16**, against source and requirements at `ec4a8f0cda85c18abc9d90f607e77129282b4761` on `ada/migrate-eden-emery-docs`. Selected the first five **Needs verification** rows in checklist order: FC-005, FC-006, FC-007, FC-017 and FC-020. “Top five” is order, not a priority ranking.

The previous inventory reconciled requirements and documented evidence; it had not traced these capabilities through their actual implementation. This audit reads their source, checks composition and runs relevant existing tests. No application code was changed. Outcomes use the [checklist definitions](../FEATURES.md#reading-the-status): In progress means partial implementation, not an assertion that someone is actively working on it.

| Feature | Previous status | Audit outcome | Finding |
| --- | --- | --- | --- |
| [FC-005](#fc-005) · Shared forms/feedback/visual rules | Needs verification | In progress | Shared primitives and selected interaction tests pass; concrete form, loading, focus and motion gaps remain. |
| [FC-006](#fc-006) · Search/New/scoped lookup | Needs verification | Planned | Required global controls, search route and live scoped result composition are absent from the inspected shell. |
| [FC-007](#fc-007) · Device preferences/keyboard settings | Needs verification | Planned | No preferences route, keyboard switch or required device-storage key in the inspected UI. |
| [FC-017](#fc-017) · TLS/CA custody/privilege separation | Needs verification | In progress | TLS and custody/admission primitives exist and tests pass; production handoff, CA escrow and platform acceptance are incomplete. |
| [FC-020](#fc-020) · Satellite invitation/approval/repair | Needs verification | In progress | Status/repair-instruction UI exists; Add office is disabled and worker creation explicitly accepts HQ only. |

All five can leave Needs verification. **None is complete.** This audit left 58 Needs verification rows. The subsequent [remaining-58 audit](2026-11-16-remaining-feature-audit.md) classified those as well; live totals remain in the checklist and [area guide](../FEATURE-AREAS.md).

## FC-005

**Requirements:** [Shell UX](../specs/app-shell-ux.md), §6a and SHELL-29–39. Scope includes shared controls, status/feedback, focus, reflow, menus/overlays and reduced motion. **Outcome: In progress.**

Implemented source includes shared [Button](../../apps/agentic-org-ui/src/lib/components/ui/button/button.svelte), [Callout](../../apps/agentic-org-ui/src/lib/components/Callout.svelte), [StatusPill](../../apps/agentic-org-ui/src/lib/components/StatusPill.svelte), [theme tokens](../../apps/agentic-org-ui/src/app.css) and [request form](../../apps/agentic-org-ui/src/lib/components/NewRequestDialog.svelte). The selected menu/focus, text-scale, mobile-header and request-form tests below passed under explicit browser test composition.

| Criteria | Evidence found | Gap before completion |
| --- | --- | --- |
| SHELL-29–30 · Visual rules/reflow | Dark theme tokens, neutral primitives, selected 320px/200% and 40px-root menu cases. | Not an all-screen contrast, typography, state and reflow audit. |
| SHELL-31–32 · Status/icon semantics | Textual status labels, shared SVG navigation and passing no-emoji guards. | look-primitives still expects Paused amber; dialog/LiveBanner still use Lucide. Reconcile revised semantic/icon rules per screen. |
| SHELL-33 · Forms | Labels, invalid state, ordinary confirmation and retained draft on rejected send. | Empty request submission returns without focusing the field; ordinary textError is not associated with the textarea. Pending submit lacks busy/verb state; mobile target/text-size rules are incomplete. |
| SHELL-34 · Controls/tabs | Disabled Add office has an associated visible reason; request primary/outline actions differ. | All-screen tab semantics/primary-action reconciliation remains. |
| SHELL-35 · Loading/feedback | Named busy list skeleton and office-check busy state; successful request follows awaited send. | No “Still loading” implementation found. Request and retry actions lack complete pending feedback; real acknowledgement and sanitized owning-flow error behavior need integration. |
| SHELL-36 · Focus | Shared focus outline, destination-focus unit tests and native-action focus case passed. | No Skip to content implementation found; deletion focus is not exhaustively verified. |
| SHELL-37 · Menus | Selected Chromium cases pass keyboard focus, Escape return and outside-click behavior. | Not every menu or required browser engine was tested. |
| SHELL-38 · Overlays | Bounded request dialog scrolling/footer and draft-cancel behavior. | Dialogs retain 1rem mobile margins rather than the required full width; complete short-height/text-scale matrix is outstanding. |
| SHELL-39 · Motion | Office skeleton has reduced-motion handling. | Button translates on press; work skeleton and generic dialog animations lack complete reduced-motion handling. |

Concrete locations in the audited snapshot: NewRequestDialog lines 234–264, 280–303, 375–379; Button lines 7/18; generic dialog-content line 31; work page lines 142–143; LiveBanner line 25. Official Svelte analysis found no compiler issues in ShellTopbar/NewRequestDialog; advisory reactivity suggestions were not counted as demonstrated product failures.

**Completion evidence still needed:** fix the concrete gaps, then run the full applicable SHELL-29–39 matrix across supported screens/browsers, with real persistence and authorization where the screen requires them. Selected fake-backed tests establish component behavior only.

## FC-006

**Requirements:** [Shell UX](../specs/app-shell-ux.md), AC-IA13-1–2, AC-IA19-1–4 and Search/New/scoped lookup rules. **Outcome: Planned.**

[ShellTopbar](../../apps/agentic-org-ui/src/lib/platform/shell/ShellTopbar.svelte) takes a heading/children and renders those only. Its [shell composition](../../apps/agentic-org-ui/src/routes/(shell)/+layout.svelte) supplies MobileNavigation, with no Search/New controls. The [topbar test](../../apps/agentic-org-ui/e2e/shell-topbar.spec.ts) explicitly expects the older header to have no inputs/links. The route inventory has no search route; [section matcher](../../apps/agentic-org-ui/src/params/section.ts) admits sessions/activity/settings, not a search implementation.

Targeted route/source searches found no `/search?q=`, `aria-keyshortcuts`, preference-governed N/slash handler or grouped global search/New composition. Existing feature-local chat/similarity search is narrower. Existing request capture is not global New: [requests-view](../../apps/agentic-org-ui/src/lib/work/requests-view.ts) still formats REQ-, while selected [identity](../design/organization-data/06-identity-and-numbering.md) requires typed T- references.

**Remaining work:** grouped authorized Projects/Work/Members and permitted Conversation results; route/query/selection behavior and scoped references; selected New actions and narrow controls; T- mapping and preference-governed shortcuts. The [production work adapter](../../apps/agentic-org-ui/src/lib/work/index.ts) is unavailable, so live authorization/result tests cannot pass through the current product composition. Planned describes the absent selected shell implementation; production backend dependencies remain explicit in FC-013.

## FC-007

**Requirements:** [Shell UX](../specs/app-shell-ux.md), Preferences/Keyboard and shortcut coupling. **Outcome: Planned.**

The requirement calls for `/preferences` → Keyboard, Single-key shortcuts on by default, device-local `agentic-org.singleKeyShortcuts` values 1/0, default-on behavior when storage is unreadable, and coordinated shortcuts/hints without overriding browser modifier shortcuts.

The inspected route/source inventory contains no preferences route, account-menu preferences entry, Single-key switch or required storage key; no corresponding tests were found. The [sidebar footer](../../apps/agentic-org-ui/src/lib/components/Sidebar.svelte) still links to `/welcome` for Switch organization. The [settings route](../../apps/agentic-org-ui/src/routes/(shell)/[section=section]/+page.svelte) is a generic placeholder. Existing tab/filter/draft local storage does not implement this setting.

**Remaining work:** implement and verify the device-local control, failure fallback, shortcut enable/disable behavior and matching accessible hints. Account-zone persistence is separately gated and tracked by FC-008; it does not block implementing the selected keyboard setting.

## FC-017

**Requirements:** [Host/office boundary](../design/office-host-boundary.md), [standalone design](../design/standalone-server.md) and [installation/access UX](../specs/server-installation-and-access-ux.md), especially SRV-23–26 and CA escrow/recovery boundaries. **Outcome: In progress.**

| Boundary | Implementation evidence | Remaining gap / verification limit |
| --- | --- | --- |
| Browser TLS transport | [tls.rs](../../apps/agentic-org-web/src/tls.rs) validates matching certificate/key pairs, uses TLS 1.2/1.3, disables early data/session resumption and atomically replaces identities. [Transport tests](../../apps/agentic-org-web/src/transport/server/tests.rs) exercise actual local HTTPS handshakes, rotation, origin mismatches, bounded handshakes and refusal of non-loopback plaintext. | This is an implemented transport library exercised with fixture certificates, not a provisioned production HQ listener. |
| Key custody separation | [Office keys](../../crates/org-host/src/keys/mod.rs) separates development files from release Keychain selection. [Host secrets](../../crates/org-host/src/host_secrets.rs) and its [Keychain adapter](../../crates/org-host/src/host_secrets/keychain.rs) implement scoped, bounded/redacted custody and add-only TLS identities. [Worker](../../apps/agentic-org-office/src/lib.rs) receives keys through private launch; [web dependencies](../../apps/agentic-org-web/Cargo.toml) contain no office DB/host key-store dependency. | Keychain tests use pure attributes/fake calls; this audit did not run a signed release with real entitled custody or Linux credential-provider acceptance. Development stores are not release credential evidence. |
| Worker admission | [server_check.rs](../../crates/org-host/src/server_check.rs) admits signed/hardened worker identities and rechecks tokens before key handoff; release key-store selection checks signing/entitlements. Existing targeted admission/key tests passed. | Synthetic/unsigned test results do not establish the signed packaged-host path or both-platform release behavior. Preserve documented current-launch limitations. |
| Production handoff/authorization | [web main](../../apps/agentic-org-web/src/main.rs) explicitly refuses listener startup. [Host dispatcher](../../crates/org-host/src/service/mod.rs) admits only hello/instance.status and refuses session.grant, recovery.release and tls.fetch; its negative-dispatch test passed. | Protected bootstrap, authenticated browser composition, grant/TLS handoff and actual end-to-end privilege separation remain unfinished. |
| CA escrow and recovery | [Host registry](../../crates/org-host/src/hostdb.rs) has private TLS/bootstrap record primitives and explicitly no production bootstrap caller. Standalone §6 requires encrypted CA escrow and one-time protected recovery delivery. | No executable provisioning/recovery path was found: setup explicitly returns unavailable. No fresh fixture demonstrated same-CA recovery, wrong-code/damaged-escrow refusal or protected recovery delivery. Do not equate the TLS private-key store with CA escrow. |

Rust semantic navigation traced TlsCertificates into transport and its tests, and HostSecrets into hostdb/custody/tests. It did not find invitation/approval service symbols or a certificate-issuing PKI module in the current workspace; absence of a symbol alone was not used as completion evidence. Actual entry-point/dispatcher refusals establish the integration gap.

**Completion evidence still needed:** production bootstrap and authenticated listener/control composition; CA escrow/release/recovery implementation and its fixtures; real signed macOS and supported Linux credential/privilege/isolation acceptance. REC-1 remains a separate production HQ-loss-recovery release decision.

## FC-020

**Requirements:** [Office/team UX](../specs/offices-and-teams-ux.md), JOIN-1–20, applicable OFC criteria and §5c/§6 join, approval and repair flows. **Outcome: In progress.**

Partial browser presentation exists. The [Offices route](../../apps/agentic-org-ui/src/routes/(shell)/offices/+page.svelte) lines 235–237 explicitly says phase 1 is HQ-only and disables Add office with an explanation. [OfficeDetailsSheet](../../apps/agentic-org-ui/src/lib/components/OfficeDetailsSheet.svelte) implements expandable local doctor instructions, copy command, reported hostname and optional instance selector. These are instructions, not a working repair command. The [office tests](../../apps/agentic-org-ui/e2e/offices.spec.ts) cover disabled Add office and fixture-driven status/repair presentation; those tests were inspected, not rerun in this audit.

The [OfficeApi](../../apps/agentic-org-ui/src/lib/office/api.ts) has no invitation/approval methods and its production implementation returns unavailable. The [human CLI](../../apps/agentic-org/src/lib.rs) refuses satellite setup arguments and exposes no implemented enrollment/repair commands. The [worker creation path](../../apps/agentic-org-office/src/lib.rs), lines 165–172, explicitly rejects every role except HQ before creating an office. The [host dispatcher](../../crates/org-host/src/service/mod.rs) has only hello/status operations, and production web startup remains disabled. Lower-level replication fixtures that bind a satellite directly are not enrollment, invitation validation or authorized approval.

**Remaining work:** real invitation generation/validation, identity/expiry/check-code binding, owner/admin authorization, persisted approval/join state, safe idle/drain handling, initial copy/readiness and actual cancel/remove-pending/repair commands. Re-run JOIN acceptance against real HQ/satellite services on both supported platforms after those paths exist. Existing UI means partial implementation, while the production enrollment flow is absent.

## Executed verification

Host environment: **macOS arm64**, pinned Rust **1.99.1**, locked dependencies. Rust tests use temporary roots, fixture keys/certificates and test custody. No installed CLI, user organization, service or live Keychain was altered. Existing unit/transport tests do not substitute for signed release or Linux acceptance.

| Command (repository root unless noted) | Result | What it establishes |
| --- | --- | --- |
| `cargo test -p agentic-org-web --locked` | 77 passed: 63 library, 1 executable refusal, 1 CSP harness CLI, 12 HTTP integration; no failures | Fixture-backed transport/HTTPS/rotation/admission/HTTP security behavior and production startup refusal. |
| `cargo test -p org-host --lib --locked host_secrets::` | 44 passed | Custody policy, fake Keychain/development filesystem conformance, isolation/redaction/wiping. |
| `cargo test -p org-host --lib --locked service::tests::` | 2 passed | Restricted open-method inventory and pre-handshake ordering policy. |
| `cargo test -p agentic-org --test cli --locked` | 3 passed | Human entry-point help/layout and fail-closed setup with no partial state. |
| `cargo test -p org-host --lib --locked keys::` | 39 passed | Office key-store selection, attributes, refusal and sweep policies under test custody. |
| `cargo test -p org-host --lib --locked server_check::` | 6 passed | Worker admission/signature/token policy, including unsigned-child refusal; not a signed-release acceptance run. |
| UI: `bun install --frozen-lockfile` | Passed | Locked test/build dependency setup. |
| UI: `bun run test src/lib/platform/shell/capabilities.test.ts src/lib/platform/feature-factory.test.ts src/lib/platform/shell/desktop-navigation.test.ts src/lib/platform/tooling/no-emoji.test.ts` | 38 tests passed in 4 files | Unavailable/fake selection boundary, navigation/focus and emoji guards. |
| UI: `bun run build` | Passed, including asset guard (5 CSS / 75 JS) | Static build only; existing fake/sample import warnings remain in report mode. |
| UI: targeted Playwright command below | 6 Chromium tests passed | Selected menu/native-action/40px-root/mobile-header and request draft/form behavior under explicit fakes. |

The targeted browser command, run from `apps/agentic-org-ui`:

```sh
PW_PORT=4393 bunx playwright test e2e/focus-primitives.spec.ts e2e/shell-topbar.spec.ts e2e/requests.spec.ts --project=chromium --workers=2 --grep 'menu keyboard and pointer contract at 320/200|Button six variants and native actions at 960/100|explicit 40px root menu at 320|current mobile Menu|Just save queues it unrouted|Esc with words asks'
```

Official Svelte documentation/autofixer and Rust analyzer symbol/reference navigation supported source analysis. No Firefox/WebKit, exhaustive shared-control matrix, real HQ/satellite onboarding, signed Keychain release or Linux run is claimed. The [shell capability gate](../../apps/agentic-org-ui/src/lib/platform/shell/capabilities.ts) remains unavailable; [e2e composition](../../apps/agentic-org-ui/e2e/vite-composition.ts) explicitly replaces it with test-owned fakes. Passing those tests does not make the production feature complete.
