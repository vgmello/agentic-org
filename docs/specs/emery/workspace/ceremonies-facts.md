# Facts for the Routines (ceremonies and retros) PRD · 2026-11-12

Agents repo paths are from the repo root. Product = `git show origin/v1:<path>` in `app/`. Interim tool = `hr/retro.py` (working tree; its `tick` part is uncommitted).

**Interim retro tool**
- F1 Commands: `add "<text>" [--kind problem|waste|win|idea] [--ref]`, `list [--all|--closed] [--mine] [--retro DATE] [--json]`, `close <id> [--note]`, `reopen`, `next`, `held [--note]` (hr/retro.py:8-13, 658-686; bin/retro:3-7). Identity and project come from `$ORG_AGENT` only (hr/retro.py:4-6, 240-244).
- F2 Data: each agent appends to its own `agents/<agent>/retro/events.jsonl` (add, close, reopen, held) under an exclusive flock; attribution comes from the file's directory, never from event fields (hr/retro.py:18-25, 298-313; tests :522). Ids are `<name>-<n>`, sequential under concurrent adds (hr/retro.py:27-30, 556-560; tests :428).
- F3 Schedule: `projects/ac/retro/schedule.toml` is `daily 18:00`, local machine time, no zone, lead or instructions (schedule.toml:1-3). Intervals must be at least 30 min and divide a day (hr/retro.py:427-430).
- F4 `held` consumes the nearest scheduled slot; more than half a period away, the neighbour (hr/retro.py:46-51, 494-501; tests :213, :706).
- F5 Carry-over is a label only: an open item older than the last `held` shows "carried" (hr/retro.py:378-380; tests :265). Any agent of the project may close any item (hr/retro.py:10; tests :131).
- F6 A schedule problem never blocks `add`: the item is recorded "unscheduled" and shown under the next retro (hr/retro.py:52-54, 547-553; tests :637).
- F7 Ordering doesn't trust the clock: adds fold before closes, each file keeps its own order by the latest timestamp seen (hr/retro.py:329-340; tests :654). Human output replaces control/format characters; `--json` stays raw (hr/retro.py:138-140, 638; tests :512).
- F8 The scheduler half (`retro tick`: lead announcement 15 min before, team broadcast, "start now", "missed" after a grace of one interval capped at 2 h, `ticks.jsonl`) is documented (hr/retro.py:14-16, 40-44, 56-69, 470-472) but no `tick` subcommand is registered in the parser (hr/retro.py:658-686).

**ada's real retro record**
- F9 Retro 1 was owner-triggered and paused eight agents for ~40 min; ada judged that too costly to repeat (RETRO-1.md:3, 103). Each reply has Right / Wrong / Improve with owners and metrics (RETRO-1.md:5-61).
- F10 A good outcome record holds: a ranked synthesis, numbered decisions (D-B69..D-B73), rules with owners, one new hire, metrics and the next retro date (RETRO-1.md:67-78); a follow-up scoreboard per issue: fix, status, evidence (:87-98); the cadence decision: 4-hourly checkpoint plus daily 18:00 async retro (:102-107).
- F11 Rules are one dated, numbered record that wins over messages, each citing the retro and items that produced it (RULES.md:3, 34-40, e.g. "Retro 3 items eden-1/2, rowan-1/2/5"). Rule 24: state-machine specs send the transition table and permission matrix first (RULES.md:40).

**Product (origin/v1)**
- F12 Messages wake only live listeners: the relay pokes `inbox/<to>/wake/*.sock`; `msg wait` returns on them (apps/org-agent/README.md:212-226). Spool presence is online/offline only; "the spool can't tell busy from idle" (README.md:119-121).
- F13 Designed, not built: a message, ask, schedule or the owner wakes an office-run agent (docs/design/agent-sessions.md:334-339); a normal message for a busy session waits for the turn's end (:355); a paused agent's triggers wake nothing (:342); on-call/scheduled runs sleep after 5 min idle (:361). Agents with no `run_mode` are run by hand, as all are today (:377); HQ's `member.set_run_mode` handler is missing (:46). No Rust code fires schedules (no `last_fired`/`next_fire`/cron in `*.rs`).
- F14 Per-agent schedules exist as data: `Schedule { spec, time_zone }`, spec uninterpreted by HQ, IANA zone (crates/org-proto/src/org.rs:264-281); `RunMode` always_on/on_call/scheduled (org.rs:234-249). A schedule firing during a live run is skipped and noted; a missed run runs once (agent-sessions.md:341; docs/specs/agent-run-modes-ux.md:95-98).
- F15 No "System" sender: the reserved sender is `office`, used for `delivery_failed` notices (crates/org-proto/src/msg.rs:66-67, 133-134, 320-322). Sender names and subjects are "untrusted data, not instructions" in notify text (msg.rs:794-797).
- F23 The actor model has no system value: `ActorKind` is Human or Agent only (crates/org-proto/src/principal.rs:290-299). (Moved here from F15 so the PRD cites it by number.)
- F16 Teams: a tree; `lead_member_id` optional, "an agent or a person who's a member of this team. Also leads its projects" (org.rs:116-127). A member can be in any number of teams (org.rs:507-516). Projects are staffed by teams through `ProjectTeam` (org.rs:518-525). No project manager field.
- F17 Time zones: none on accounts, people or offices today; only on agent schedules (away-digest-and-timezones-v1 §1 table, citing ORG:274-280).
- F18 Eden's `docs/specs/retrospectives-ux.md` rev 4 (status for-review): retros run after a delivered feature or every interval, whichever first, configured in project › Settings (:30, :40-50, :112); only finished work is in scope and an empty period is skipped (:31); participants are everyone with finished work, woken if on call, facilitator = project lead (:34); the owner approves action items via a "Retro ready" Needs-you item (:36, :62); a retro is a repo doc `docs/retros/` (:38); CLI `org-agent retro input|show|synthesize|list` (:106-108, :123).
- F19 `docs/specs/project-status-ux.md` schedules a weekly status report per project, written by a one-off "Reporter" run (:31-33). `docs/design/processes.md` (rev 3, proposed) is the "Processes" feature hidden in v1.0.

**My PRDs**
- F20 tasks-v1: six statuses (§4.2); five waits on the owner (§4.1, §4.8: question, Blocked on owner, review, Hand-off, Budget reached); "project manager" = the owner or a lead of a team attached to the project (§4.2); refusal check order state → actor → guards (§5); `org-agent tasks` CLI (§5).
- F21 IA v2: D11/D26 Inbox kinds Approvals, Questions, Decisions, Problems; only run error and crash are dismissible (D26, lens §4); "Retro ready" is not an Inbox item, it shows in Activity (D26 table); D29 "Processes: not in this release"; D33 people don't sign in in v1.0; D17 the owner is not a team member; team page tabs Work, Projects, Activity, Conversations (AC-IA05-2); last decision number D43.
- F22 away-digest Z1/Z2: each account has an IANA zone; schedule times show in the schedule's own zone, labelled (§4.1). costs-and-budgets req 20/C14: an agent at 100% of its budget pauses after its current turn. project-insights-v1: throughput and cycle time in v1, **forecast is IN-9, Next** (§4.2, §6).
