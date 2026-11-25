# Codex-only adoption

Kit 0.10.0 supports `hybrid` and `codex-only`. Both install the same runtime,
Repowise index, reviewed memory, checks, review evidence and task metrics into
`.agents/agent-kit`. Native skills are in `.agents/skills`; Codex roles/configuration/hooks
are in `.codex`. Hybrid additionally installs the Claude adapter.

For agent-led setup, point the agent to ADOPT.md in the starter-kit branch. It
detects the application from the workspace and asks you to choose Codex only or
Claude + Codex, using the client's choice UI when available. It asks for a target
only when the workspace is ambiguous; you never need to edit the adoption document.
If your adoption request already explicitly selects Codex-only, the agent uses it
without asking again.

```bash
bash install.sh --project /absolute/path/to/project --mode codex-only
```

Follow ADOPT.md to merge existing project conventions, prepare the project skill
catalog/role briefs through project-capabilities, configure real checks and
run `.agents/agent-kit/runtime/kit_context.py setup` with a real symbol/source file, then
`kit_memory.py setup` for Hindsight. Run the setup-health skill with live checks.
Both `lookup` and `recall` use the shared MCP connection, so restricted review
shells need no network exception. Memory uses embedded pg0 and local CPU models;
it does not require a model API key or consume the Codex subscription.
Open Codex in that project, trust the project and inspect `/hooks` to trust the
exact installed definitions. Hooks are native local CLI integrations; cloud
orchestrators may not support them. Explicit verification/review commands remain
required on every host. Native role instructions do not create separate sandboxes;
the review wrapper launches fresh read-only exec processes and explicit-session
writable repair turns.

The default model is `gpt-6.2-sol` (owner switch from `gpt-6.1-sol` on 2026-11-18, after a live call
confirmed access; the effort levels below were set for 6.1 and are kept): Low for bounded extraction only, Medium for
fully evidenced familiar implementation, High for lead planning and other bounded
implementation/review, Xhigh for ambiguous/high-risk work and architecture review.
The review/fix rounds run on the routing policy's `review.model` (`gpt-6.1-sol` or
`gpt-6.2-sol`). Final review uses High, or Xhigh when required by risk. These are starting defaults,
not measured proof of optimal efficiency. Max is not a default or automatic retry.

Keep the original author context. Follow the installed review-cycle skill: fresh
Sol review, repairs in that exact session, a fresh recheck after repairs, original
author intent validation, then `kit_codex.py final-review --run RUN_FOLDER` for a
separate read-only Sol code/output review. Hybrid uses its fresh Opus final review
instead. Model/profile gates and current source/check evidence are required.

The native roles are full-stack and stack-independent. Each assignment gets a
project role brief and selected skills from the local catalog; do not load every
skill into the lead context. Independent pieces may use separate instances with
bounded ownership and a task-wide maximum of two writers. Critical-area guidance
specializes the existing reviewers; extra review sessions require a concrete need.
See [project capabilities and maintenance](PROJECT_CAPABILITIES.md).

The standalone installer defaults to hybrid unless selected explicitly; reinstalls
without a mode retain their recorded mode. Agent-led adoption gathers the user's
choice and always passes `--mode` explicitly. Profile selection is project-local;
it does not change global user configuration. Existing customized settings/guidance are preserved for adoption
review. Overrides can change effective models/efforts; verify them in the client.

Upgrading a 0.7 project copies durable task/decision/knowledge/config files into
`.agents/agent-kit`, with conflict checks and backups. Legacy originals, ignored review
folders and receipts remain at their historical paths. Memory references to copied
decisions follow the new canonical files. Index ownership metadata is copied;
completed-refresh/check receipts are not reused, so setup refreshes and validates
again. Inactive managed native roles/skills/rules are retired with backups.
Customized inactive adapters block a mode switch for reconciliation. Historical accepted evidence is not
rewritten or treated as current after an upgrade. Finish active model turns first,
rerun checks/setup and use new review runs. Keep private evidence referenced by
exported metrics before deleting old worktrees.

A 0.8 upgrade also requires the new capability assessment before readiness/review.
Later reinstalls retain the project catalog and local briefs. Changes to reviewed
stack/architecture evidence or guidance require reassessment before dispatch.

The installer and diagnostics never make model calls. Live validation requires
working Codex authentication and access to the configured model/effort. Missing
access must remain an explicit blocker, never a fake ACCEPTED outcome.

Sources checked November 14, 2026 (then `gpt-6.1-sol`): [Sol](https://developers.openai.com/api/docs/models/gpt-6.1-sol),
[native agents](https://learn.chatgpt.com/docs/agent-configuration/subagents),
[skills](https://learn.chatgpt.com/docs/build-skills),
[hooks](https://learn.chatgpt.com/docs/hooks).
