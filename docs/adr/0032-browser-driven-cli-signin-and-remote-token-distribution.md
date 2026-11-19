# ADR-0032 · Browser-driven CLI sign-in enrollment and HQ-to-satellite token distribution

Status: **accepted by the owner 2026-11-19**. Implementation pending.
Builds on [ADR-0004](0004-headless-agent-sessions.md),
[ADR-0015](0015-agent-isolation-modes.md) (decisions 10 and 11),
[ADR-0016](0016-no-application-level-data-encryption.md),
[standalone-server.md](../design/standalone-server.md) (§506 office mTLS),
[14-model-providers-and-connection-pools.md](../design/organization-data/14-model-providers-and-connection-pools.md),
and the [C1 sign-in spike](../research/c1-sign-in-spike.md).

## Context

- Under [ADR-0004](0004-headless-agent-sessions.md), agent sessions run strictly headless over structured protocols, and there is no terminal in the browser.
- Operators deploying Agentic Org on headless Linux hosts or managing remote satellite offices still need a simple, browser-based way to connect Claude and Codex subscriptions to a pool connection (`provider_connections` / `office_connection_bindings`) without SSHing into every machine.
- [ADR-0015](0015-agent-isolation-modes.md) decision 11 left two open questions for spike C1:
  1. How each vendor CLI stores and locates a relocated sign-in on macOS vs. headless Linux.
  2. What happens when multiple agents or multiple offices share one subscription connection whose OAuth refresh token rotates on use.
- The [C1 sign-in spike](../research/c1-sign-in-spike.md) and [`pocs/console-attach`](../../pocs/console-attach/README.md) proved:
  - Both `claude setup-token` / `claude auth login --claudeai` and `codex login --device-auth` can be driven headlessly in a wide PTY without launching a server browser, yielding a standard OAuth URL (plus paste-back code for Claude, or a one-time device code for Codex).
  - `claude setup-token` produces a portable 1-year `CLAUDE_CODE_OAUTH_TOKEN` (`scope=user:inference`) that works on macOS and headless Linux without Keychain or `HOME` conflicts.
  - Codex OAuth refresh tokens are 30-day single-use rotating tokens: copying one `auth.json` across offices causes `refresh_token_already_used` revocations. Conversely, `codex app-server` `chatgptAuthTokens` mode + `account/chatgptAuthTokens/refresh` was verified live on Codex `0.169.0` with **zero `auth.json` written to disk**.

## Decision

### 1. Browser-driven one-time enrollment (harmonized with ADR-0004)

1. **Bounded enrollment PTY helper — never an agent terminal.** The host/office may spawn a short-lived, bounded Unix PTY strictly to execute a vendor's one-time sign-in CLI (`claude setup-token`, `claude auth login --claudeai`, or `codex login --device-auth`) during connection enrollment. It is never exposed as a terminal to the browser, carries no arbitrary keystrokes, and exits or is killed on timeout/cancel. All agent sessions remain strictly headless per ADR-0004.
2. **Structured enrollment API through `agentic-org-web`.** An authorized operator (owner) clicks sign-in in `apps/agentic-org-ui`, calling authenticated `agentic-org-web` endpoints backed by the host/office enrollment service:
   - **Claude (`paste_code` flow):** starts `claude setup-token` (preferred for multi-office/headless pools) or `claude auth login --claudeai` with `BROWSER=/usr/bin/true`, `NO_BROWSER=1`, and `SSH_TTY=/dev/pts/0` in a wide (`512x24`) PTY; returns only `{ login_id, flow: "paste_code", url, state }`. The user authorizes in a browser tab and submits the returned code to the API, which writes `<code\r>` to the PTY.
   - **Codex (`device_code` flow):** starts `codex login --device-auth` in a PTY; returns `{ login_id, flow: "device_code", url, user_code, state }`. The user completes device authorization in their browser while the UI polls status until authenticated.
3. **Secret hygiene during enrollment.** Extracted tokens (e.g., `sk-ant-oat01-...`) are immediately wrapped with `org-pki::wrap` under the office/host secrets key (ADR-0016) into `auth/<connection-id>/`. Raw tokens are **never** returned in debug previews, written to logs, stored in Git/SQLite in plaintext, or passed in child `argv` / `SessionSpec`.

### 2. HQ-to-satellite credential distribution over Organization-CA mTLS

4. **Transport and peer identity.** Credentials never travel over plain HTTP or through browser-facing listeners. All HQ <-> satellite token distribution uses the existing **Organization-CA mTLS full-duplex outbound channel** ([standalone-server.md](../design/standalone-server.md) §506):
   - Each satellite generates its own key pair locally; its private key never leaves the satellite and is wrapped under the satellite host's secrets key (ADR-0016).
   - HQ trusts each satellite by its Organization-CA-signed certificate bound to `office_id` and can revoke an individual office without rotating other offices.
   - If any relay is ever introduced in a future release, payloads are additionally AEAD envelope-encrypted to the target satellite's public key.
5. **Claude remote distribution (`CLAUDE_CODE_OAUTH_TOKEN`).**
   - HQ stores the wrapped 1-year `CLAUDE_CODE_OAUTH_TOKEN` in the connection's secret record (`auth/<connection-id>/`).
   - HQ delivers the token to authorized satellites over Organization-CA mTLS; each satellite wraps it locally under its own host secrets key and injects it into `claude` via process environment (never `argv`), allowing both `HOME` and `CLAUDE_CONFIG_DIR` to point cleanly into the agent's private home and allowing the satellite to keep working when HQ is offline ("Works alone").
6. **Codex centralized refresh (`chatgptAuthTokens`) vs. offline-independent offices.**
   - **Default multi-office mode (HQ-centralized refresh):** HQ owns the **single** rotating 30-day `refresh_token`, guarded by a single refresh lock at HQ. Satellites never receive the `refresh_token` and write no `auth.json`. Each satellite passes a short-lived (~1-hour) `access_token` (+ `idToken` / `chatgptAccountId`) to `codex app-server` via `account/login/start` (`type: "chatgptAuthTokens"`). On `account/chatgptAuthTokens/refresh`, the satellite requests a fresh `access_token` from HQ over the mTLS channel and replies to `app-server` without dropping the live session.
   - **Offline trade-off ("Works alone"):** Under centralized refresh, a satellite disconnected from HQ can keep running Codex until its current ~1-hour `access_token` expires. An office that must run Codex disconnected from HQ for days at a time uses its own independent per-office `codex login --device-auth` binding instead.
   - **Never copy rotating `auth.json`:** Two offices or two concurrent processes must never independently refresh copies of the same Codex `refresh_token`.

### 3. Friendly Advanced Settings presets (preserving `org-runtime` invariants)

7. **Secure by default, no bypass mode in product code.** The UI exposes friendly permission and sandbox presets under Advanced Settings (defaulting to workspace OS sandbox enabled + interactive approval routing) mapped strictly onto `org-runtime::PermissionMode`:
   - `safe_ask` *(default)* — workspace sandbox on; tool/command approvals routed to the owner/policy bridge.
   - `read_only` — workspace sandbox on; planning/inspection only, no mutating edits or shell writes.
   - `workspace_edits` — workspace sandbox on; workspace file edits allowed, shell commands routed for approval.
   - `autonomous_sandboxed` — workspace sandbox on; workspace edits and sandboxed commands allowed automatically within policy rules.
   - **No `unrestricted` / bypass preset in product code.** The `org-runtime` invariant prohibiting skip-permissions / bypass modes (`crates/org-runtime/src/lib.rs`) remains unchanged.
8. **Persistent sessions with background tasks and sub-agents enabled.** `org-runtime-claude` keeps the bidirectional `stream-json` process alive across turns (with `stdin` open) and does **not** set `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1` by default, so an agent can run background commands and background sub-agents while remaining responsive to new user/agent messages ([ADR-0025](0025-agents-start-sub-agents.md) decision 6, amended 2026-11-19). The office tracks active background work via `task_started` / `task_notification` / `background_tasks_changed`, treats reactive `system/init` wakeups as attributed turns under the same employee/run, and stops and reaps all remaining background tasks and sub-agents on Pause, Stop, Offboard, or stall timeout.

## Acceptance criteria

| ID | Passing evidence |
| --- | --- |
| ENROLL-1 | An owner completes Claude subscription enrollment (`claude setup-token`) from the browser without a terminal; no local browser opens on the server, and the minted token is stored only wrapped via `org-pki::wrap` under `auth/<connection-id>/`. |
| ENROLL-2 | An owner completes Codex subscription enrollment (`codex login --device-auth`) from the browser using the displayed URL and one-time code; cancelling or timing out terminates the helper PTY with no orphan process. |
| ENROLL-3 | No raw OAuth token, refresh token, or pasted authorization code appears in HTTP error replies, debug previews, logs, Git, SQLite tables, or child `argv`. |
| ENROLL-4 | A satellite office runs a Claude turn using a `CLAUDE_CODE_OAUTH_TOKEN` delivered by HQ over Organization-CA mTLS and continues working after HQ is disconnected. |
| ENROLL-5 | A satellite office runs a Codex turn using HQ-delivered `chatgptAuthTokens` with no `auth.json` written to disk, handles an `account/chatgptAuthTokens/refresh` callback via HQ's single-lock refresher, and rejects concurrent refresh races cleanly. |
| ENROLL-6 | Revoking a satellite's Organization-CA certificate immediately causes HQ to refuse subsequent token fetch/refresh requests from that office. |
| ENROLL-7 | Advanced Settings exposes only the four `org-runtime`-backed presets, defaults to `safe_ask` with OS sandbox enabled, and rejects any bypass/unrestricted request. |

## Consequences

- Headless Linux hosts and remote offices can be authenticated from the web UI with zero SSH and without violating ADR-0004's no-browser-terminal rule for agent sessions.
- Claude remote offices gain full offline resilience ("Works alone") using a single 1-year `CLAUDE_CODE_OAUTH_TOKEN` wrapped per office.
- Codex multi-office pools avoid `refresh_token_already_used` revocations by keeping the single rotating refresh token at HQ, at the documented cost of a ~1-hour HQ-offline window unless the office uses an independent per-office device login.
- Codex `chatgptAuthTokens` requires `experimentalApi: true` on Codex `0.169.0`; upgrades to Codex CLI must re-run the C1 external-auth probe before release.
