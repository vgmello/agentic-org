> **Earlier models-policy design.** This imported source predates the newer [model profiles and agent execution contract](../design/organization-data/11-model-profiles-and-agent-execution.md) and [provider connections and pools](../design/organization-data/14-model-providers-and-connection-pools.md). Use those newer designs where their decisions differ from this source.

# Spec · Models: which model each part of an agent's work runs on (UX)

Owner: Eden · Status: **revision 1, for ada and the user** · 2026-11-09 · Builds on `roles-ux.md` (roles and per-agent settings), `our-org-in-the-app.md` (the agent record §5, Hire §7), `processes-ux.md` (the "why" panel, §4a autonomy) and `costs-and-insights-ux.md` (costs at API prices).

## Changelog

**Revision 1 (2026-11-09):** first version, designed with ada from the user's request and ada's runtime research (`knowledge/research/2026-11-09-runtime-model-delegation.md` in ada's home).

## The request (the user, via ada, 2026-11-09)

> An agent works as a lead and does the heavy work through helpers (subagents). The main session runs a strong model (for example Opus); helpers default to a cheaper strong model (for example Sonnet, high effort); the agent escalates a helper to the stronger model when a piece is too complex; independent reviews and security analysis run on the strong model. Build it into the product generically, across runtimes (Claude Code, Codex, Antigravity and others).

## 0. Decisions

1. **Four slots, in plain words:** **Main session** · **Helpers** (the subagents an agent starts) · **When it's hard** (the model a helper is escalated to) · **Reviews and security** (independent reviews and security analysis). Each slot is a **model** and an **effort**.
2. **A model is a tier or an exact model.** Tiers: **Fast** · **Balanced** · **Frontier**, mapped by the office to each runtime's current models, so they survive vendor renames and keep presets runtime-neutral. **Exact model…** chooses an exact vendor model instead (for example `claude-opus-5-5`), shown with its runtime.
3. **Effort is a neutral level:** **Low** · **Medium** · **High** · **Max**, or **Inherit** (from the level above). The office maps it per model and runtime; where a model can't run at that level, the slot says "Runs as High on this model". Max maps to each runtime's highest standard level (on Codex, `max`; its extra `ultra` isn't offered).
4. **Three levels, each overriding the one before** (the organization level is always complete: Inherit isn't offered there, and it starts from the Balanced preset): the **organization** default (Settings › Models), the **role** (Agents › Roles › <role> › Models; office data keyed by the role, not the role doc, like its permission policy, `roles-ux.md` decision 2), and the **agent** (Profile › Settings › Models). Only the owner changes them (the owner edits an agent's Models in its Settings, `our-org-in-the-app.md` PROF-4 rev 18). Agents can't change their own.
5. **What's enforced, what's guidance.** The office's launcher enforces every slot's model and effort on every supported runtime (it passes the main model and generates named helper types, each fixed to its slot; for office-run Claude Code agents, helpers first need the `Task` tool allowed, A77). **When** to escalate is the agent's judgment, so the When it's hard row reads "Model enforced · when to use it is the agent's judgment" (word plus icon); it comes with a short guideline the office composes into the agent's instructions (default: "Use the When it's hard model when a piece spans many files, the design is unclear, or a helper failed twice."), editable at each level.
6. **Presets** fill all four slots at once: **Balanced** (Main: Frontier, high · Helpers: Balanced, high · When it's hard: Frontier, high · Reviews: Frontier, high) · **Economical** (Main: Balanced, high · Helpers: Balanced, medium · When it's hard: Frontier, high · Reviews: Frontier, high) · **Max quality** (Frontier, max, everywhere). Changing any slot makes it "Custom (from Balanced)". Balanced is the organization's default.
7. **Costs are shown where the choice is made:** each preset and slot shows its API price per million tokens and an estimate, "≈ $X per active day at last week's activity" (for an agent; per agent on average at the organization and role levels; "No activity yet" before there's a week). Insights rolls helpers' sessions up under their lead, split as Main · Helpers · When it's hard · Reviews: new here, extending `costs-and-insights-ux.md` (which has neither yet).

8. **The agent record's one `model` is the agent's Main session model.** The profile header and Hire's Review show the effective Main session model and the Models summary; the record's other slots live in this policy (A77). A role no longer carries a `model_default`: its level here replaces it (`roles-ux.md` rev 2), and the organization's preset applies when the role sets nothing.

## 1. Flow 1 · The Models table (the same at every level)

```
 Models · ada (agent)                                      Preset: Custom (from Balanced) ▾
 ─────────────────────────────────────────────────────────────────────────────────────────
 Slot                 Model                Effort   From                        Cost hint
 Main session         Frontier             High     Role: Staff Engineer [Override…]  $15 / M in
 Helpers              Balanced             High     Organization [Override…]    $3 / M in
 When it's hard       claude-opus-5-5 📌    Max      This agent   [Reset to inherited]  $15 / M in
                      ⓘ Model enforced · when to use it is the agent's judgment
 Reviews and security Frontier             High     Organization [Override…]    $15 / M in
 ─────────────────────────────────────────────────────────────────────────────────────────
 When to escalate: "Use the When it's hard model when a piece spans many files…"  [Edit…]
 ≈ $38 per active day at last week's activity                          [Why]  [Save]
```

- **From** says where each value comes from: **Organization** · **Role: <name>** · **This agent** (or **This role** on a role). Inherited rows are read-only with **Override…**; overridden rows have **Reset to inherited**. **Why** shows the chain for each slot (organization → role → agent), as the Processes "why" panel does.
- **Model** is a picker: the three tiers first (each with its current mapping on this agent's runtime, "Frontier · claude-opus-5-5 on Claude Code"), then **Exact model…** (vendor models for this agent's runtime). An exact model shows a pin icon and its name.
- **Effort** is a picker (Low, Medium, High, Max, Inherit). When clamped: "Runs as High on this model" under the value.
- **On the role and organization levels** there's no single runtime, so tiers show their mapping per runtime on hover ("Claude Code: claude-opus-5-5 · Codex: <model>"), and an exact model shows which runtimes it applies to.
- **The agent's quick view and roster** show a short line, "Models: Balanced (from Staff Engineer)" or "Models: Custom".
- **Hire's Review** shows the agent's Models line with where it comes from, and **Change…** opens this table for the new agent (owner only).

## 2. States and edge cases

| Case | Behavior |
| --- | --- |
| Loading | Skeleton rows. |
| The agent's runtime isn't set | The table shows the values with "Set the runtime to see the models that apply." and the cost estimate is hidden. |
| An exact model isn't available on the agent's runtime | The row says "claude-opus-5-5 isn't available on Codex. Choose another model or a tier." and the agent's sessions use the slot's inherited value until it's fixed (at the organization level, the Balanced preset's tier for that slot); a Needs you item names the agent. |
| Helpers aren't available (for office-run Claude Code agents until `Task` is allowed, A77; any runtime without helpers; whether Antigravity has them is A77) | The Helpers, When it's hard and Reviews rows say "Helpers aren't available for this agent yet: this work runs in the main session." (word plus icon), and only Main session applies. |
| Effort clamped | "Runs as High on this model" under the value. |
| A tier's mapping changes (a vendor rename) | The tier keeps its name; the hover and the agent's runs show the new model. Exact models don't change. |
| Change while the agent is working | "Changes take effect at the agent's next session." (as the record's other settings). |
| The office is stopped or can't be reached | The table shows the last values read-only, with "The office isn't running, so models can't be changed now." and Save off. |
| Can't save (the office refused, or a newer change) | "Couldn't save the models: <reason>." with **Try again**; a newer change reloads the table and keeps what was typed. |

## 3. For agents

- The composed instructions name the agent's helper types and what each is for, and include the escalation guideline (§0 decision 5).
- `org-agent models show` prints the effective table with where each value comes from. Agents can't change it.
- Each escalation is recorded on the run (which helper, which model) and counted in Insights.

## 4. UX acceptance criteria (MOD)

- MOD-1 · Settings › Models, Agents › Roles › <role> › Models and Profile › Settings › Models show the same four-slot table (Main session, Helpers, When it's hard, Reviews and security), each with a model (a tier or an exact model), an effort and where it comes from (Organization, Role, This agent).
- MOD-2 · Inherited rows are read-only with Override…; overridden rows have Reset to inherited; Why shows each slot's chain. Only the owner can change any level; agents can only read theirs (`org-agent models show`).
- MOD-3 · The model picker offers Fast, Balanced and Frontier (with their current mapping on the agent's runtime) and Exact model… (the runtime's models). Efforts are Low, Medium, High, Max and Inherit (Max is each runtime's highest standard level; Codex's `ultra` isn't offered); a clamped effort says "Runs as <level> on this model". Inherit isn't offered at the organization level, which is always complete (it starts from Balanced).
- MOD-4 · Presets Balanced (the organization default), Economical and Max quality fill all four slots; changing a slot shows "Custom (from <preset>)".
- MOD-5 · The When it's hard slot reads "Model enforced · when to use it is the agent's judgment" and has an editable guideline, composed into the agent's instructions; each escalation is recorded on the run and counted in Insights.
- MOD-6 · Each slot shows its API price, and the table shows "≈ $X per active day at last week's activity" (or "No activity yet"); Insights splits an agent's cost into Main, Helpers, When it's hard and Reviews, with helpers rolled up under their lead.
- MOD-7 · The table covers loading, a missing runtime, an exact model the runtime doesn't have (with the inherited fallback and a Needs you item), helpers not available (office-run Claude Code agents until `Task` is allowed, or a runtime without them), a clamped effort, a tier remapped, changes taking effect at the next session, the office stopped or unreachable (read-only, Save off, with the reason), and a failed save.
- MOD-8 · The quick view, roster and Hire's Review show a one-line Models summary with where it comes from; Hire's Change… opens the table for the owner.

## 5. Questions

**For ada**
- A77 · The contract (ada is writing `docs/design/model-policy.md` and additive org-proto types with these slot names): the policy at three levels (organization, role as office data keyed by the role, agent on the record); tiers and their per-runtime mapping (who maintains it, and how a remap is announced); neutral efforts and clamping; the launcher's named helper types; recording escalations on runs; and the Insights split. Helpers for office-run Claude Code agents: `agent-sessions.md` puts `Task` (agent spawning) on the disallowed list (rev 3.3) and fails a start whose `init.tools` contains it, so helpers need `Task` allowed for office-run agents (a deliberate change to that design, yours); until then they show the "Helpers aren't available" state. Whether Antigravity runs helpers with their own models (your research covered Gemini CLI, which isn't one of the product's runtimes). How the record's single `model` maps to the agent level's Main session, and where the other slots are stored. An organization-wide allow or deny list of models stays for later, as you said.
