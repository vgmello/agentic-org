# Native Codex runtime for selected v1 execution

The owner's overall request is to complete the selected v1 organization-data
model feature by feature across UI/backend/database, proceeding to independent
work when blocked. Finished features receive local independent reviews and merge
into `v1-integration` without waiting for a PR. This task does not represent all
v1 execution or completion of v1.

Deliver the Codex-owned native adapter in `crates/org-runtime-codex` against the
existing `org-runtime::AgentRuntime`/`Session` port. The adapter plans and translates
the installed native protocol; it does not spawn processes, perform I/O, keep
credentials, introduce a supervisor daemon, or silently weaken admitted policy.
The selected document 11 and document 14 control execution and credential
isolation; compatible requirements in `docs/design/agent-sessions.md` and the
existing port apply. Preserve those requirements if feasibility fails.

Acceptance:

1. Establish version-pinned local protocol and capability evidence for the actual
   installed Codex binary. Do not mark a version runnable from schema acceptance
   alone. Reuse and extend the existing S0 research with temporary synthetic
   workspaces; no tests write to a user's organization workspace.
2. Plan one app-server process per session over stdio. Preserve the office's
   explicit environment and isolated connection binding; no ambient personal
   hooks/MCP/plugins, global environment changes, secret values in plans/events,
   or credential forwarding to tools. Replay admitted settings on start/resume.
3. Enforce fixed denies, exact writable roots, office socket access, tool policy
   and sandbox invariants, or fail closed with a defined unsupported result.
   Unknown or unverified versions/capabilities are unavailable. A successful
   standalone command is not evidence that a model's native tools are isolated.
4. Implement the existing sans-I/O protocol lifecycle: initialize, native thread
   start/resume, normal send/busy/steer/interrupt, approval mapping and denial,
   output/tool/turn/usage events, bounded protocol failures and graceful stop.
   Unsupported operations fail without mutating session state. Validate native
   identifiers and encode all output through the shared line encoder.
5. Ended is emitted once, last and only on process exit; terminal sessions produce
   no further output/deadlines. Run the shared port testkit and deterministic
   versioned fixtures for success, protocol failure, approval and shutdown.
   Usage distinguishes unknown values and actual native counters; no fabricated
   cost, bill, checkpoint portability or side-effect replay guarantees.
6. Do not change another owner's contract or composition area. If the current
   port or installed runtime cannot satisfy an essential requirement, stop the
   affected implementation, record exact evidence and request its owner's
   resolution; do not replace this contract with a smaller stub or claim success.
7. Before integration, run required workspace and kit checks, obtain fresh local
   independent review, original-author validation and a separate fresh configured
   final review for the exact commit. Record capability and support limits
   honestly. Full supervisor, persistence, API and UI integration remain separate
   required parts of the overall selected v1 goal.
