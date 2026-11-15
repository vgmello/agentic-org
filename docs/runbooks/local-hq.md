# Runbook: a real HQ office on this Mac (debug build)

Status: **source-checked development guidance, 2026-11-15; full HQ setup blocked**.
The current names are `agentic-org`, `agentic-org-office`, `agentic-org-web` and
`agent-os`. Host code is `crates/org-host`. Tauri is removed. This runbook can
inspect/build the current entry points and run a disposable foreground host; it
cannot complete organization creation, service installation or a production browser
session on this branch. [Executable migration](../design/executable-migration.md)
and [the authority map](../design/documentation-authority.md) separate current code
from the selected organization-data target.

The superseded 2026-11-08 Tauri/bundle walkthrough is available in
[Git history](https://github.com/vgmello/agentic-org/blob/c6e520a36c4dc7c738c68915fac5e7d5d1cbe804/docs/runbooks/local-hq.md).
Its `org-office-host`, `org-server`, `tauri dev`, sessionless create and app-token
instructions cannot be run against the current checkout. Do not recreate them by
bypassing the protected bootstrap gate.

## 0. Before you start: the tree must hold both halves of key custody

The office key is host-owned and supplied over launch v2; `office.json` remains
schema v1. Check `crates/org-office-launch/src/lib.rs`, `crates/org-host/src/keys/`
and `apps/agentic-org-office/` at the checkout being used. Preserve
`org.agentic.office-key`, required signing identifiers/entitlements and existing
`ORG_AGENT`/`ORG_OFFICE` bindings; executable renames do not rename credentials.
Never substitute a generated key for an existing office whose legitimate key is
missing. Debug keys are plaintext fixture files, not a release credential provider.

Current `apps/agentic-org/src/lib.rs` explicitly refuses `setup` before changing
setup state. `apps/agentic-org-web/src/main.rs` refuses production startup without
protected handoff. `crates/org-host/src/service/mod.rs` admits only registered-instance
`hello` and count-only `instance.status`; it refuses every call when the instance
is unregistered. Retired sessionless create/list/token calls are not a bootstrap
workaround. Do not edit registry/DB/marker files by hand to make them available.

## 1. One folder for everything dev, apart from a real install

Use an empty, short, owner-private disposable root, never a user's office home,
Keychain items, installed CLI or live Phase A spool. In a development terminal:

```sh
export HQ_REPO="$(pwd -P)"  # run at the repository root
export HQ_DEV="$(mktemp -d /tmp/ao-hq.XXXXXX)"
chmod 700 "$HQ_DEV"
mkdir -m 700 "$HQ_DEV/control" "$HQ_DEV/keys" "$HQ_DEV/logs"
export ORG_OFFICE_HOME="$HQ_DEV/control"
export ORG_OFFICE_DEV_KEY_DIR="$HQ_DEV/keys"
```

Keys must be outside the office home, with no symlink/alias to it. Use one key
folder per home. Keep the generated path for later terminals; do not rerun mktemp
to reconnect to an existing host. Socket paths, including the 12-character office
handle, must fit macOS's 103-byte limit. The development control root is current
implementation evidence, not the target `runtime/<office-id>` data layout.

## 2. Build (debug)

Use the repository's pinned toolchain and platform prerequisites. These are
commands for a prepared development Mac; this documentation edit does not claim
they were run or that Linux release key custody is complete.

```sh
cargo build --locked -p agentic-org -p agentic-org-office -p agentic-org-web -p agent-os
./target/debug/agentic-org --help
./target/debug/agentic-org --version
./target/debug/agentic-org serve --help
./target/debug/agentic-org-office --help
./target/debug/agentic-org-web --help
./target/debug/agent-os --help
```

There is no `org-host` executable to build or install. Test-only fake workers are
not release helpers. Do not copy the new `agent-os` over the team's installed live
CLI or change its spool/context. `ORG_DEV_APP_PATH` is a historical debug-only
signed-app fallback, not a way to authorize a browser or human CLI in this run.

## 3. Start the host

In the same terminal, run the debug host in the foreground against the disposable
root. This creates development host control state and logs, not an HQ organization
or registered production service:

```sh
./target/debug/agentic-org serve \
  --home "$ORG_OFFICE_HOME" \
  --server-bin "$HQ_REPO/target/debug/agentic-org-office"
```

The process remains attached until SIGINT/SIGTERM. Release builds reject these
trust-changing overrides and require packaged signing/key-provider validation.
The host owns locks and worker supervision; an agentic-org-web listener is not
started by this debug command. Do not mistake the existence of `host.sock` for
HQ readiness.

## 4. A command-line client for host.sock

Do not use the old raw `organization.create` example. The private host-control
protocol has its own versioned hello/range handshake and a registered-instance
requirement. Count-only status does not grant access to organization records or
provisioning. Use checked host-control clients/tests for diagnostics on a legitimately
registered development instance. A fresh root here has no such registration, so
`not_authorized` is expected, even for hello. Setup is the missing authorized
provisioner; inventing a registry entry is not part of this runbook.

## 5. Create "Agentic Consulting"

**Blocked on this branch.** The selected [create flow](../design/create-organization.md)
requires an instance administrator, explicit organization slug, remote organization
repository and state branch, and valid chosen home teams for bootstrap HR/optional
owner-person profiles. Employee identity, execution profile/model/connection bindings
and current office placement are separate. HR cannot become runnable from a
teamless profile or a missing execution configuration.

Initial `agentic-org setup` must eventually perform HQ-only provisioning, protected
recovery-code staging, component verification and automatic service registration.
Offline setup must validate the remote URL/branch and local credential references
without external fetch/auth/provider requests, keeping remote connectivity/publication
unverified until authorized online; local HQ readiness cannot claim remote success.
It currently reports that bootstrap/service integration is unavailable. Satellite
enrollment remains a separate HQ-issued flow. The current published name-only HTTP
request and launch-v2 create line do not carry the complete selected plan; their
reviewed adapters are required. No replacement wire fields are invented here.

## 6. Check it works

A successful debug host start proves only that its foreground composition can
open the chosen development control root. It is not an installer, browser-auth,
remote-sync or execution acceptance pass. The eventual HQ run must establish:

- Canonical remote organization Git/slug/branch and valid one-home-team employees;
  security-principal ownership remains independent of employee flags.
- Separate server and agent readiness, ordered model-profile routes, fresh local
  credential/entitlement evidence, shared permits and durable placement fencing.
- Authenticated browser snapshot/event continuity, one logical result after a
  lost reply, and no privileged key exposure through gateway/CLI.
- Durable sources and acknowledged recovery evidence surviving cache loss without
  broadening message/private-note/cost access or duplicating execution.

Use [dogfood §6a](../specs/dogfood-run.md#6a-selected-data-and-recovery-checks) for
concrete selected-model checks. Record missing capabilities as Blocked, not N/A or
passed because a static page loads.

## 7. Logs

Use the foreground stderr and the host's owner-private logs under the selected
control root. Worker logs are keyed by their current office handle when a worker
exists. Missing key/provider, malformed marker, refused create and recovery-required
state must stay distinct. Diagnostic evidence may include fixture IDs, release and
safe status; it must exclude key bytes, passwords, invitations, session proofs,
private notes and recovery codes. A normal status response is not a backup.

## 8. Stop and clean up

Send Ctrl-C to this foreground host or SIGTERM to its verified process. Wait for
it to finish draining and reap its owned descendants; do not kill all processes
with a shared executable name. Host EOF, office locks and process ownership remain
necessary after a restart. Never signal a restored PID without matching ownership
mark, generation and machine/boot/start identity.

Remove only the exact disposable root created in §1 after confirming its host and
children stopped and it contains no real data. Do not transplant a recursive-delete
command to an existing office or sweep its credentials. A real target office cache
can be rebuilt only after checking complete authorized durable sources: organization
Git plus unpublished journals, allocation/authority and execution records, protected
messages/attachments, receipts/streams/snapshots/cleanup, usage archive, budget
history/extensions/holds and the separate author-private notes store. Rebind
legitimate credentials and reconcile processes before starts. Missing coverage
must stop affected recovery; Git alone cannot fabricate continuity.

Task worktrees survive handoff until closure. Preserve local-only/unpushed code
checkpoints before any safe workspace migration or cleanup. Uninstalling a service
retains its data and is distinct from deleting this empty fixture root.

## 9. Known gaps for the switchover

Protected administrator/bootstrap, HQ setup/service registration, production web
handoff, reviewed target create adapter, selected source extraction/restore and
runtime-path sandbox migration remain implementation gates. Satellite networking
and selected profile/permit/session-runner integration need their own evidence.
Manual HQ-loss recovery remains REC-1; an escrow round-trip is not promotion.
Automatic promotion, scheduling/memory, Areas, Undo and process execution are not
selected by this runbook. Keep active Phase A communication intact until its
separately verified conditional transport cutover.
