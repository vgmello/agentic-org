**BLOCKED / INCOMPLETE for the full task.** The protocol adapter is feasible as missing implementation, but the current launch boundary cannot express and carry the required connection-specific authentication context. Ambient configuration isolation and model-tool security also remain unverified. Detailed planning should wait for the contract prerequisite and fresh architecture review.

Inspected branch `codex/v1-codex-runtime` at the exact base/HEAD `4dd45f5ccd616cb5c54900196bd88b56ebe30011`. Rust LSP symbol, hover, reference and definition queries confirmed the relevant types and environment mapping.

The intent remains the complete [registered task](/Users/vgmello/projects/agentic-org/.agent-kit/tasks/v1-codex-runtime-contract.md): a Codex-owned, sans-I/O adapter behind `AgentRuntime`/`Session`, with one app-server process and one native thread per session, faithful policy replay, normalized lifecycle/events and verified capability limits. All seven acceptance criteria remain unchanged.

Numbered premises, assessed against this checkout on 2026-11-15:

1. **VERIFIED — selected execution/provider contracts govern isolation.** Document 11 revision 6 requires office-local connection bindings, launch authority in the office supervisor, fresh connection evidence and refusal of unsupported policy. Document 14 defines the corresponding private `auth_context_ref` and binding revision. The historical “normal Codex home” observation does not override these requirements. Recheck when either owning document changes. Sources: [execution contract](/Users/vgmello/projects/agentic-org/docs/design/organization-data/11-model-profiles-and-agent-execution.md:128), [provider/binding contract](/Users/vgmello/projects/agentic-org/docs/design/organization-data/14-model-providers-and-connection-pools.md:98).

2. **VERIFIED — existing fields carry most requested session settings.** `SessionSpec` supplies model, effort, cwd, instructions, policy, sandbox roots/denies/sockets, office MCP definitions and tools. `LaunchContext` supplies executable/version, generated-file directory, scratch directory, environment **names**, and native conversation identity. These support pure planning and protocol translation without credential values. Recheck on port changes. Sources: [SessionSpec](/Users/vgmello/projects/agentic-org/crates/org-runtime/src/spec.rs:46), [LaunchContext](/Users/vgmello/projects/agentic-org/crates/org-runtime/src/spec.rs:408).

3. **VERIFIED — the connection auth-context prerequisite is absent.** `LaunchContext` has no selected binding/auth-directory input; `runtime_dir` means generated adapter files, not connection authentication. `ProbeContext` likewise contains only executable, version and optional runtime directory. Environment names cannot identify their values or prove the selected account. Consequently, the adapter cannot itself select the connection’s isolated auth directory from the current documented inputs. Sources: [LaunchContext](/Users/vgmello/projects/agentic-org/crates/org-runtime/src/spec.rs:408), [ProbeContext](/Users/vgmello/projects/agentic-org/crates/org-runtime/src/probe.rs:21).

4. **VERIFIED — `env_add` does not resolve that absence.** The supervisor’s captured/context allowlists reject `CODEX_HOME`; runtime additions admit only three reviewed Claude switches. Launch and probe preparation enforce that list. This is an intentional boundary, and document 11 explicitly identifies an adapter/environment-contract extension as unfinished work. Recheck after Ada/River-owned resolution. Sources: [environment construction](/Users/vgmello/projects/agentic-org/crates/org-supervise/src/environment.rs:62), [reviewed switches](/Users/vgmello/projects/agentic-org/crates/org-supervise/src/runtime.rs:136), [owning specification’s prerequisite](/Users/vgmello/projects/agentic-org/docs/design/organization-data/11-model-profiles-and-agent-execution.md:944).

5. **UNVERIFIED — complete ambient configuration isolation.** The current inputs provide the desired MCP set, but neither an ambient configuration inventory nor a guaranteed office-controlled configuration namespace. Disabling known server names cannot cover arbitrary unknown names. A blanket replacement/suppression mechanism could avoid needing an inventory, but its startup and merge semantics require pinned evidence. Generated `config` schemas accepting arbitrary properties do not establish those semantics. Hostile workspace settings/hooks must also remain ineffective; accepted task-local instruction steering is a separate, documented behavior. Sources: [security requirements](/Users/vgmello/projects/agentic-org/docs/design/agent-sessions.md:530), [Codex S0 gaps](/Users/vgmello/projects/agentic-org/spikes/codex/README.md:113).

6. **VERIFIED, narrowly — 0.169.0 offline evidence is available.** The completed [artifact](/Users/vgmello/projects/agentic-org/.agent-kit/logs/v1-codex-runtime/offline-0.169.0.json) contains 22 results and no `verified:false` result. It records standalone command sandbox/Git/socket checks and thread startup. It sent **zero `turn/start` requests**. Resume of the unused thread returned `no rollout found`. Its profile inherits temporary-directory defaults, so it does not establish exact writable roots. Recheck after probe or binary changes.

7. **UNVERIFIED — production native-tool enforcement and lifecycle behavior.** The generated 0.169.0 schemas establish request shapes for start/resume, permission profiles, steering, interruption, approvals and usage. They do not prove live behavior, suppression of risky tools, effect visibility or descendant cleanup. The [probe source](/Users/vgmello/projects/agentic-org/spikes/codex/probe.mjs:247) deliberately exercises no model turn.

8. **VERIFIED — the existing session port/testkit supports deterministic lifecycle implementation.** It checks busy handling, unchanged state on rejected controls, approvals/steering, usage ordering and `Ended` exactly once, last, only on process exit. Adapter fixtures must additionally establish native correlation, malformed-input behavior and shutdown deadlines. Passing these checks would establish protocol conformance, not office readiness. Sources: [Session](/Users/vgmello/projects/agentic-org/crates/org-runtime/src/session.rs:40), [testkit](/Users/vgmello/projects/agentic-org/crates/org-runtime/src/testkit.rs:93).

The minimal dependency direction remains `org-runtime-codex → org-runtime`, with private native codecs, policy translation and session state. Excluding standard-library, external and development dependencies, the new crate has proposed `Ce=1`, initial `Ca=0`, `I=1`. Serde/JSON add serialization dependencies without adding domain coupling. The adapter gains no dependency on office storage, supervision or another adapter; later composition chooses it. This preserves the [architecture rules](/Users/vgmello/projects/agentic-org/docs/architecture-rules.md) and keeps volatile native protocol details outside the shared port.

The contract prerequisite belongs to Ada, with River involved in supervisor mapping and affected implementer agreement. **Minimal question for Ada, unsent:**

> Which reviewed launch/probe contract will select the connection-specific auth context and office-only configuration namespace? Can it supply a non-secret canonical auth-directory reference, or an explicitly guaranteed equivalent, while the supervisor alone resolves credentials and keeps them out of tool environments?

Connection identity/revision evidence must remain associated with admission and launch-digest checking; it need not become plaintext credential data or force unnecessary adapter knowledge.

The preferred alternative is that narrow, owner-reviewed boundary plus a version-proven configuration-isolation mechanism. An office-supplied environment mechanism could be equivalent if its guarantees and supervisor support are explicitly established. Reusing personal authentication/configuration, placing credentials in plans, changing global `HOME`, hiding bindings in adapter state, or using unrestricted environment maps does not satisfy the task. A universally unsupported stub would preserve refusal behavior but would not deliver the adapter.

Before architectural **READY**, the remaining bounded evidence must cover:

| Evidence | Required distinction |
|---|---|
| Two isolated connection contexts | Launches and probes use the intended binding; no credential values enter plans/events or ordinary tools. |
| Hostile synthetic configuration | Personal/parent/workspace hooks, MCP, plugins and settings cannot alter admitted execution, including startup effects. |
| Actual model shell and file tools | Exact roots plus scratch, complete fixed denies, searches, symlink aliases, Git carveouts and the exact office socket hold together. |
| Real start/resume/control | Instructions and policy replay; busy, steer, interrupt, approvals, cancellation and graceful stop match recorded behavior. |
| Effects, usage and cleanup | Native counters and unknowns remain distinct; missing usage never becomes fabricated zero; tool visibility and descendant cleanup support only proven claims. |
| Deterministic adapter fixtures | Success, malformed/truncated protocol, approval/escalation denial, shutdown and terminal silence pass the shared testkit. |

These are evidence requirements, not newly executed probes. Unsupported specs/versions must remain unavailable rather than dropping restrictions. No checkpoint portability, safe replay or billing claim follows from native conversation resume.

Affected work is connection-bound launch/auth planning, configuration-isolation qualification and any production-ready claim. Useful independent work remains schema/provenance inventory, sanitized offline fixture preparation and protocol correlation/failure analysis. Those activities do not complete this task or authorize dependent launch implementation. Supervisor, persistence, API and UI integration remain separate parts of the overall v1 goal.

Ordered packet read acknowledgment: **all six sources read; every actual SHA-256 matches the packet**.

```text
1 .agent-kit/project-roles/staff-engineer.md
  57eb9af5ef20b7c1a1c9dfd840f171622a44bf2cf921d0acadfc0aaa364dcfba
2 .agent-kit/project-skills/kit-maintenance/SKILL.md
  a04eea1920dca5518c9a41390028fe4ee77a371f1b3dfcdaeb3571244da90fab
3 .agent-kit/project-skills/rust-workspace/SKILL.md
  34b74996eb8c441136490163ce3521ce7c43b7c0e8d641af1545e98ed0c55eaa
4 .agent-kit/project-skills/security-boundaries/SKILL.md
  c21e731d48e2b57f60c17dd3c326fb91ad239f348c84f508091b391fc030167f
5 AGENTS.md
  cc2dd7a13948e5ee836acbe072dfd4c0f7e18ad91973c710b5efad6861f22f19
6 docs/processes/branch-and-merge.md
  91a460fcead6a524729d10ef6fd377bcf1019248bd556515b9f0b6ce08ebfbc3
```

No implementation, credential/personal-config inspection, delegation, additional model turns or team messages were performed. The lead should save this proposal and obtain fresh independent architecture review before detailed breakdown; requested reviewer routing remains `gpt-6.1-sol`, effort `xhigh`.