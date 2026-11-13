# P4 charter: org-process fingerprint, render, checklist merge (Rory)

Scope (docs/design/processes.md §3.3, §3.4, Appendix C "the fingerprint, the render cap and
order, the checklist merge"). Pure functions in crates/org-process; no new dependencies
(org-docs, serde_json only); Tier A; slices <= 250 non-test code lines.

1. `fingerprint(handbook_commit, integration_commit: Option, &Work) -> Fingerprint`: the set
   cache key (§3.3). Plain canonical data, not a hash (the caller may hash it): handbook commit,
   integration commit or none, and the team chains in a canonical order, so equal inputs give
   equal keys whatever order the chains arrive in, and any change to one input changes the key.
2. `checklist(default: &[Step], &EffectiveSet) -> Vec<Step>` (§3.4 Checklists, PROC-12): the
   project's default checklist first, then the steps of the winning O, T and J processes (never
   G: agent-level winners are skipped), in a fixed order (by level, then key); a step whose text
   matches an earlier one is merged into it, and `required` wins (required if any copy is).
3. `render(&EffectiveSet, &RenderInput) -> String` (§3.4 "Your processes"): RenderInput carries
   what the office knows: the blobs the agent has read per key, the winning file's blob and full
   text per key, which moments are On with each moment's guidance text. Content: the set (name,
   level, version, summary), turned-off processes and conflicts in the spec §3 wording; changed
   since last read (winning blob not among the read blobs for that key) as full text; each
   process's moment prompt after that moment's guidance text, only while the moment is On.
   Cap 24 KiB including prompts: summaries first (past the cap "and N more: run
   `org-agent process list`"), then changed texts, organization first; the rest named with
   "run `org-agent process show <key>`". Also returns what was delivered (keys and blobs), so
   the office can mark them read when the runtime accepts the start or wake.
4. The P3 review corner (an unreadable outermost-team file plus a real seal lower in the same
   chain) gets its test.

Slices: P4a = fingerprint + checklist + the corner test; P4b = render. One request to Tomas each,
with evidence and a guard-to-test table. Ambiguities are read fail-closed or smallest-surprise,
listed in the request, and pinned by tests.
