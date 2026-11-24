---
id: TASK-11
title: >-
  Verify provider OAuth env scrubbing, separate CA/token wrapping keys, and stop
  copying private chat into turn/input.json
status: Done
assignee:
  - '@lead'
created_date: '2026-11-20T23:24:00Z'
updated_date: '2026-11-21T01:09:00Z'
labels:
  - security
  - runtime
  - p1
milestone: m-3
dependencies:
  - TASK-2
documentation:
  - docs/architecture/architecture-and-data-audit.html
  - docs/adr/0032-browser-driven-cli-signin-and-remote-token-distribution.md
  - docs/adr/0016-no-application-level-data-encryption.md
  - docs/design/organization-data/11-model-profiles-and-agent-execution.md
priority: high
type: enhancement
ordinal: 11000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Audit finding H-07: ADR-0032 passes a 1-year CLAUDE_CODE_OAUTH_TOKEN via child environment variables relying on unverified CLAUDE_CODE_SUBPROCESS_ENV_SCRUB=1 behavior (and Codex refresh relies on unverified experimentalApi), Org CA keys and provider tokens share a single per-office wrapping key, and supervision/turns/<id>/input.json copies private chat in plaintext outside durable/messaging/ with no retention or purge policy.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Empirical verification (automated test or documented spike on target Claude Code and Codex CLI builds) proves child tool subprocesses cannot read provider OAuth credentials, or authentication is moved to an out-of-process proxy before C2 wiring.
- [x] #2 Org CA key wrapping and provider OAuth token wrapping use distinct wrapping keys.
- [x] #3 Turn execution inputs reference protected chat messages or enforce explicit redaction/purge rather than retaining permanent unredacted plaintext shadow copies outside durable/messaging/.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Inspect CLAUDE_CODE_SUBPROCESS_ENV_SCRUB=1 and CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR behavior in installed Claude Code 2.1.311 and document/test empirical findings (plus fail-closed proxy/FD requirement if scrubbing is incomplete).
2. Add cryptographically distinct wrapping-key derivation (HKDF-SHA256) and provider-credential wrapping API in crates/org-pki/src/wrap.rs (separate subkeys for office private keys vs provider OAuth credentials, plus distinct AAD domains) with unit tests, and update ADR-0016 and ADR-0032.
3. Update Doc 11 §3/§5 so supervision/turns/<turn-id>/input.json references protected chat messages by ID/watermark/content_ref under durable/messaging/ (never permanently copying plaintext private chat bodies outside durable/messaging/) and enforces explicit post-turn purge of rendered runtime inputs.
4. Verify cargo test -p org-pki -p org-agent-runtime-claude, cargo fmt/clippy, test_schema.py, finalize TASK-11, and commit.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Empirically audited Claude Code 2.1.311 (CLAUDE_CODE_SUBPROCESS_ENV_SCRUB=1 + CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR one-shot FD handoff) and Codex 0.169.0 (app-server JSON schema confirms chatgptAuthTokens over stdio JSON-RPC, experimentalApi gate on account/chatgptAuthTokens/refresh, and shell_environment_policy env restriction). Added HKDF-SHA256 KeyClass separation (OfficePrivateKey vs ProviderCredential) and wrap_provider_credential/unwrap_provider_credential with distinct AAD domain in crates/org-pki/src/wrap.rs. Updated Doc 11 §5 so supervision/turns/<turn-id>/input.json references protected durable/messaging/ records by ID/content_sha256/watermark rather than retaining permanent plaintext copies and purges ephemeral rendered runtime inputs post-turn.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Verified Claude Code 2.1.311 & Codex 0.169.0 credential handoff/scrubbing (preferring FD/JSON-RPC handoff + fail-closed proxy requirement), added HKDF-SHA256 KeyClass key separation & provider-credential wrapping API in org-pki, and updated ADR-0016, ADR-0032, and Doc 11 §5 to stop copying private chat into turn input.json.
<!-- SECTION:FINAL_SUMMARY:END -->
