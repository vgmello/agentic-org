# Slice F4-CONFORM: merge only the architecture-stable part of F4

Owner direction (2026-11-17): bring F4 in line with the new architecture (origin af79d274: ADR-0013..0020, web-api-v1,
v1 kickoff), merge everything that will not change, leave the rest for Track A, then stop. Where the documents are silent,
do not invent: leave it out and list it.

- Worktree: /Users/vgmello/projects/agentic-org-claude-worktrees/slot-1, branch worktree-f4-sessions, base ef388baf
  (F4 + af79d274 merged; gates-27 PASS 3300/0/9). Full F4 preserved at local branch f4-full-reference (ef388baf).
- Plan evidence: conformance plan (gaps G1-G14, questions Q1-Q9), summarized below.

## In scope (one writer, Opus High)
1. Remove the browser session layer that ADR-0013 replaces (G1-G7): apps/agentic-org-web/src/sessions.rs and
   sessions/{control,http,ingress,time}.rs with their tests, the session routes and their wiring in service.rs; F4's host
   changes for browser session capacity (org-host-control ops GATEWAY_SESSIONS; the web-connection cap in
   crates/org-host/src/service/authentication.rs restored to the base 67860707 behavior for that part); session legs of
   the real-process test. Keep: the web process serving /api/v1/version (and whatever base served), host supervision,
   admission, sandbox-verified handoff, serving notification, restart/backoff, release guards, dependency fence,
   per-socket sign-in lockout only if it does not depend on removed code. Native sessions unchanged (SES-8).
   The web process must not accept sign-in until FC-159 lands: session routes absent or an explicit unavailable reply
   per web-api-v1 (cite the rule you follow).
2. Rename F4-added "gateway" identifiers, files, logs, test names, doc comments and the F4-added wire method
   `gateway.serving` to "web" (G9, kickoff "browser-facing process is web"). Hold-outs (pre-existing, owner questions
   Q5-Q7): org_proto::gateway module, test-vectors/v2/gateway*, instance view `gateway_status`, status field `gateway`,
   ChannelKind::Gateway / launch v3 names, GatewayFirstLine/ReadyForSecrets/Handoff, the signing identifier, UI files that
   predate F4. List every hold-out left.
3. G12: rendezvous suffix from a neutral random function (not generate_db_key); drop "legacy" wording from
   org-host-control/src/gateway.rs:25-27 and apps/agentic-org-web/src/release.rs:94 (debug-only unpinned path stays).
4. UI: keep F4's session-controller/fake/e2e changes (browser contract unchanged, ADR-0013 d2); keep hq_restarted (Q1 open).
5. Docs (writer drafts, lead owns): FEATURES FC-013/014/017/159 and FEATURE-AREAS note, executable-migration F4 row,
   GW-01 status note: state exactly what merged (no browser sign-in yet; sessions deferred to FC-159).

## Out of scope (open questions or later slices)
Stored sessions FC-159 (Q2 caps, Q3 view freshness, Q4 refresh rotation), hq_restarted removal (Q1), wire/module renames
(Q5-Q7), ADR-0017 platform contracts (Q8, needs Linux design), ID formats (Q9), U6 branch.

## Acceptance
cargo fmt --check; cargo clippy --offline --locked --workspace --all-targets -- -D warnings; cargo test --offline --locked
--workspace; native real-process test exactly 1 passed (renamed); UI check/test/build/lint/format + session e2e chromium
and webkit; kit check; dependency closure test. `git grep -il gateway -- crates apps` lists only hold-outs. Lead runs
SHA-bound gates, Sol review and fresh final review before merging --no-ff into v1-integration.
