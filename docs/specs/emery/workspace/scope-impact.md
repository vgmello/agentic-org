# Scope impact: Insights to v1.1, CU7 to v1.1, Copilot capture to v1.1, Codex in v1.0 (2026-11-13)

Sources: Rowan `ac-fullstack-engineer-rowan/knowledge/designs/v1-prd-sizing.md`; ada STATUS 2026-11-13 09:55 (v1.0 ≈ 75 points, net ≈ +70; Insights, CU7, Copilot capture, most of Away, TK-5 and cuttable cost items to v1.1; Codex capture and account zones + Z2 stay in v1.0). Read-only scan of H/*.md. Paths below are relative to H.

## A. Pure "mark as v1.1" (or say v1.0 without Insights)

| File | Section / ID | Current text (short) | Conflict | Minimal amendment (product wording) |
|---|---|---|---|---|
| costs-and-budgets-v1 | §3 Goals (l.61) | "a release's or project's cost in **two clicks** from Home (Home → project → Insights)" | Insights tab is v1.1; Rowan: "Project cost is CLI-only in v1.0" | "In v1.0 a release's cost is two clicks from Home (Home → project → Releases, release header); a project's cost is in `org-agent cost show`. From v1.1, Home → project → Insights." |
| costs-and-budgets-v1 | §6 UI "Project › Insights › Cost" (l.325) | "Cost section, after Progress and Speed" | CU7 → v1.1 | Prefix: "(v1.1, with Insights)". |
| costs-and-budgets-v1 | CB-9, AC-CB9-1/2 (l.447-449) | "Project › Insights shows a Cost section…" | CU7 → v1.1 | Mark story "CB-9 (v1.1, ships with project-insights IN-1)". |
| costs-and-budgets-v1 | §9 Views row (l.480) | "Task, release, Insights Cost, agent profile, CLI, minimal org Cost page (CB-13)" | Insights Cost and CU6 org page are v1.1 | v1.0: "Task, release header, agent profile, CLI (incl. `cost show --org`)". New v1.1 note: "Insights Cost (CB-9), org Cost page (CB-13)". |
| costs-and-budgets-v1 | §9 "ada sequences. Depends on" (l.482) | "…the Insights tab (IN-1)…" | v1.0 costs can't depend on a v1.1 tab | "Depends on: adapters emitting `Usage`, PL-1..7, TK statuses. CB-9 alone depends on IN-1 (v1.1)." |
| costs-and-budgets-v1 | Related (l.21) | "This PRD adds its Cost section (replacing IN-11 'Next')" | Section ships v1.1 | Append "(v1.1, with Insights)". |
| costs-and-budgets-v1 | Req 13 (l.141) | "same query set serves the task page, Insights, the release header…" | Insights not in v1.0 | Keep; add "(Insights from v1.1)". |
| costs-and-budgets-v1 | C17 v1 surfaces (l.576) | "task Cost section, Insights Cost section, org Cost page (C30)…" | Same as §9 | Split: v1.0 surfaces / v1.1 (Insights Cost, org Cost page). |
| costs-and-budgets-v1 | CB-13, req 31, C30 (org Cost page) | "Organization › Cost shows, for the selected period…" | Rowan/ada: CU6 → v1.1 (cuttable); `cost show --org` stays | Mark CB-13 UI "v1.1"; AC-CB13-4's CLI half stays v1.0. AC-CB13-2's Insights comparison becomes v1.1. |
| costs-and-budgets-v1 | CB-8 release cap, C15 | "The owner or the project manager can set a release cap" | Rowan/ada: cap → v1.1; C12 Spent at release + CU4 header stay v1.0 | Mark CB-8 "v1.1". Release header in v1.0 shows "Spent $X" (no cap) and "Spent at release". |
| costs-and-budgets-v1 | Req 15, C11 "estimate" (C16 in sizing) | "Anyone may propose an estimate when they create a task" | Rowan/ada: → v1.1 | Mark req 15 and its `--estimate-*` flags "v1.1". |
| information-architecture-v2 | D42 (l.272) | "Insights' Cost section and budgets… R&D/Build in v1"; "Organization group gains a Cost page" | CU7, CU6 → v1.1 | "Budgets and task/agent/release cost in v1.0; Insights' Cost section and the org Cost page in v1.1." |
| ceremonies-and-retros-v1 | §10 row 5 (l.627) | "v1.0 via `project-insights-v1` (throughput, cycle time)" | Insights is v1.1; ada: pace/estimates wait for Insights | "v1.1, via `project-insights-v1` when it ships; forecast stays Next (IN-9). In v1.0 an estimate routine's lead reports from release progress and task data by hand." |
| ceremonies-and-retros-v1 | §10 row 1 (l.625) | "(team check, machine health, hourly estimate, weekly audit, retro)" | The hourly estimate's figures arrive with Insights | Add "(the hourly estimate's pace figures come with Insights, v1.1)". |
| ceremonies-and-retros-v1 | Related (l.10) | "throughput, cycle time (v1) and forecast (IN-9, Next)" | "v1" reads as v1.0 | "(Insights v1, after v1.0)". |
| project-insights-v1 | §4.1 (l.68) | "then the cost sections of `costs-and-insights-ux.md` once costs exist" | Costs ship v1.0 but the Cost section doesn't | "then costs' CB-9 Cost section (v1.1, with this tab)". §7 already says "after v1.0": no change. |
| acceptance-tests-v1-prd-set | Rulings, "Later than v1.0" (l.31) | "PL-8, PL-9, PL-10 (moved to insights as IN-4), points, private notes…" | List silent on the new v1.1 set | Add: all of project-insights (IN-1..13), CB-8, CB-9, CB-13 UI, costs req 15, Copilot capture, TZ-3, TZ-4, AW-1..6. |
| acceptance-tests-v1-prd-set | Rulings, TK-5 (l.37) | "whether it is in v1.0 is ada's sequencing call" | ada: TK-5 → v1.1 | "TK-5 is v1.1 (ada, 11-13); D26 option (a) stands until then." |
| tasks-v1 | §8 v1.0 list (TK-5 line) | "TK-5 review, if ada can fit it" | ada: v1.1 | Move TK-5 to a "v1.1" bullet; keep the D26 option (a) sentence. |

## B. v1.0 features that need an Insights definition (definition stays normative in v1.0; only the Insights UI moves)

| File | Section / ID | Current text (short) | Dependency | Suggested amendment |
|---|---|---|---|---|
| project-insights-v1 | §4.2 Completion %, rounding (D32), AC-IN2-1, AC-IN2-7 | "Done / total top-level tasks and bugs, Cancelled and deleted items excluded" | v1.0 project header (AC-IA17-1, D15), Home project cards (AC-IA04-3), Projects list, release/milestone progress (AC-PL2-21; Rowan P9 "one number for header, Home and releases", v1.0) | Add to §7: "**Normative in v1.0:** Completion %, its rounding and the 'Nothing planned yet' rule, used by the header, Home, Projects list and releases (P9). The Insights tab is v1.1." |
| project-insights-v1 | AC-IN2-4 | "header…, Home's Projects card, the Projects list and the Insights project % are always equal" | Equality test runs in v1.0 without Insights | "In v1.0 the test compares header, Home card, Projects list and the release figures; the Insights % joins in v1.1." |
| information-architecture-v2 | D40 (l.270); D15 (l.231) | D40 "amends D15's progress definition… header and the Insights project % are always equal" | D40's tab is v1.1, its D15 amendment is needed in v1.0 | "D40's progress definition applies from v1.0; the Insights tab itself from v1.1." |
| project-insights-v1 | §5 req 3 | "One query set serves Insights, the project header's progress, the Overview Flow tile… One shared calculation returns the percent" | Shared percent calc is v1.0 (P9); Flow tile and Insights are v1.1 | "The shared percent calculation ships in v1.0 (header, Home, releases); Insights, the Flow tile and the digest reuse it in v1.1." |
| project-insights-v1 | §4.2 "One basis for all durations" | cycle/lead time "exclude spans spent in Done or Cancelled" | tasks-v1 T8 / AC-TK9-1 (v1.0, AT check l.275) cites §4.2 | Mark the shared duration rule "normative in v1.0 (tasks-v1 T8)"; or move it into tasks-v1 T8 and have Insights cite it. |
| costs-and-budgets-v1 | §4 "Time in progress (people)" (l.114); CB-11, AC-CB11-1 | "`project-insights-v1` §4.2 'Time in progress', attributed to… the assignee (AC-IN7-4)" | Rowan CU8 is v1.0; it needs IN-10's definition, AC-IN7-4 attribution and the duration formatter (AC-IN10-2) | Mark in insights §7: "Time in progress (§4.2, AC-IN7-4) and the duration formatter (§4.2, AC-IN10-2) are normative in v1.0 for costs CB-11; the Speed section is v1.1." |
| costs-and-budgets-v1 | §4 day/month boundaries (l.118); AC-CB7-3, C11 | "follow `project-insights-v1` §5 requirement 4 and D14"; "earliest-created owner account's zone" | v1.0 agent budgets and period spend (C5-C6, C11, X8) | Mark insights §5 req 4, D14 and AC-IN7-5 "normative in v1.0 (costs)"; or restate the rule in costs §4. AC-CB7-3's "HQ's zone before TZ-1" can go, since TZ-1 is now v1.0 (see D1). |
| costs-and-budgets-v1 | Req 12 (l.140) | "list sums to the figure (as `project-insights-v1` AC-IN7-3)" | v1.0 drill-down rule cites a v1.1 AC | Keep the rule; change "as" to "the same rule as". No scope change. |
| away-digest-and-timezones-v1 | AC-AW5-4, AC-AW9-1, §4.3 "Over its usual time", req 5 | "uses the definition in `project-insights-v1.md` §4.4"; median cycle time "with the shared formatter" | Not v1.0: AW-1..5 are v1.1, AW-9 Speed is Next, IN-13 is v1.1 | No v1.0 change. v1.1 ordering: IN-13 "usual" (Rowan I5) ships before or with AW-5 (Rowan A4-A5 depend on I5). |
| ia-v2-stories | AC-IA17-2 Flow tile, AC-IA17-3, Deferred "Insights" (l.176-208) | "plus a Flow tile once IN-1 ships"; "Insights is the project's last tab, after v1.0" | Already conditional | No change (consistent). |

## C. Copilot capture (no adapter) → v1.1, if Emery accepts

| File | Section / ID | Current text (short) | Conflict | Minimal amendment |
|---|---|---|---|---|
| costs-and-budgets-v1 | §5.1 req 1 (l.125) | "Copilot session runs are recorded too" | No adapter in v1.0 | "Copilot session runs are recorded from v1.1 (no adapter in v1.0)." Keep the rest: any run with tokens but no price is attributed and shows "—" (C26). |
| costs-and-budgets-v1 | AC-CB1-8 (l.391) | "A run with tokens but no price (a Copilot run)…" | Example is v1.1; the rule is still needed in v1.0 (e.g. a Codex model with no price-list rate) | Change "(a Copilot run)" to "(for example, a run on a model with no price-list rate)". Test unchanged. |
| costs-and-budgets-v1 | C26 (l.585) | "Session runs with tokens but no price (for example Copilot)…" | Same | "for example a model not on the price list; Copilot from v1.1". |
| costs-and-budgets-v1 | §9 Capture row (l.476) | "Every agent session run, Copilot" | Copilot v1.1 | v1: "Claude Code and Codex session runs"; add v1.1 note "+ Copilot". |
| costs-and-budgets-v1 | §12.1 COST-6 (l.527) | "Copilot (COST-6): attributed like any run…" | Eden's spec mapping implies v1 | Append "(from v1.1)". Eden hand-off. |
| costs-and-budgets-v1 | §1 facts (l.44) | "Copilot runs return input and output tokens (F10-F11)" | Fact stays true | No change; optionally add "no adapter emits them yet". |

## D. Other placements stated by Rowan/ada that touch the PRDs

| File | Section / ID | Current text (short) | Conflict | Minimal amendment |
|---|---|---|---|---|
| away-digest-and-timezones-v1 | §7 Scope (l.255) | "v1.0-next (after v1.0): TZ-1 to TZ-4, AW-1 to AW-5…" | ada/Rowan: account zones (Z1, TZ-1 incl. picker and browser detection) and Z2 display (TZ-2) stay **v1.0** for ceremonies (AC-CER1-5, AC-CER2-5) | "v1.0: TZ-1 and TZ-2 (needed by routines). v1.1: TZ-3, TZ-4, AW-1..AW-5, AW-6's list. If ceremonies are cut, TZ-1/TZ-2 move to v1.1." |
| information-architecture-v2 | D41 (l.271) | "account menu's Your preferences gains a Time zone and a… threshold" | Zone v1.0, threshold v1.1 | "Time zone (v1.0); away threshold and the Home card (v1.1)." |

## Rowan's sizing items mapped to Emery's PRDs (as Rowan/ada place them)

- **Shared (v1.0):** X1-X8 (keys/depth, work_type, blocked_on, work_texts, erasure, replication, tz helpers). ada: X1-X5 already in the 249, so the net PRD add is ≈ +70.
- **tasks-v1:** v1.0 = T1-T13 and U1-U8 (§8 "recommended"). v1.1 = TK-5 review (T14, T15, U9); D26 option (a) until then.
- **projects-planning-v1:** v1.0 = P1-P11 (P9 = shared progress calc for header, Home and releases), PU1-PU5, and PU6 refs in search/chat (cut only if ada needs one).
- **project-insights-v1:** all to v1.1 (I1-I5 backend: queries/drill-down, burn-up, throughput, cycle/lead, time in status, "usual"; UI Progress, burn-up, Speed, Overview tile). Deps P9, T6, P6, X8.
- **away-digest-and-timezones-v1:** v1.0 = A1 account and person zones, Z2 display rule, zone picker with browser detection (for ceremonies). v1.1 = A2-A7 (away windows, privacy, problem capture, digest read) and their UI.
- **costs-and-budgets-v1 (D42):** v1.0 = C1-C11, C13, C15, CU1-CU3, CU5, CU8, plus C12 Spent at release, CU4 release header, C14 record move; Codex capture (~4 points, X-08) included by ada. v1.1 = release cap (CB-8), C16 estimates (req 15), CU6 org Cost page (`cost show --org` stays), CU7 Insights Cost; Copilot capture v1.1 unless Emery shows it is v1. Project cost is CLI-only in v1.0.
- **ceremonies-and-retros-v1 (Rev 17):** v1.0 = W1-W9, WU1-WU4 (≈ 15 points, provisional until #60). W9 import is a v1.1 candidate only if ada drops it. Pace/estimates (§10 row 5) wait for Insights (v1.1).
- **Totals:** v1.0 ≈ 75 points (66-90), v1.1 ≈ 23 (20-27). Not in points: ~5 new contract freezes (tasks, Following, planning, costs capture, budgets).
