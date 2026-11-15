# Merged feature sets on v1: what a first product batch can review

Emery (research via subagent) · 2026-11-13 · read-only scan of `origin/v1` at `f40634dd` (2026-11-13), ada's STATUS.md and BUGS.md, my PRDs and Eden's specs on v1.

## The headline: nothing runs on real server data yet

- **The UI has no data path to the server.** In production and plain `bun run dev`, every UI API is `unavailableCall`: `lib/office/api.ts`, `lib/org/api.ts` and `platform/shell/capabilities.ts` (`areaCapability` returns unavailable for every path). The org-ui README says "No real HTTP routes ... are claimed yet".
- **Sample data only in a test build.** The screens show sample data only under `vite dev --mode e2e`. That build swaps in `e2e/fakes` for office, org and work (`e2e/vite-composition.ts`). Chat, Processes and Copilot still build their in-memory mocks at import time (`messaging/legacy-api.ts`, `processes/sample.ts`, `copilot/mock.ts`).
- **The gateway has no API routes.** `apps/org-gateway` serves the static UI and only classifies `/api` paths (`path.rs`). The B1-02 codecs are merged (`e35db5e3`, `6802d350`), but no route is wired. The F1-02 per-feature cutovers are still open (STATUS F1-02, D-F13/D-F14).
- **The server side is real but reachable only on the office socket** (`apps/org-server/src/serve/host/operations.json`):
  - Served: `org.snapshot`, `who`, all `team.*`, the `member.*` changes (`set_run_mode` refused), `project.add_team` and `remove_team`, `task.*`, `bug.*`, `checklist.*`, the `work.*` writes (comment, link, unlink, watch, undo, delete), `msg.send`, `msg.inbox`, `msg.read` and `inbox.list`.
  - Unserved: `work.list`, `work.get`, `work.snapshot`, `work.activity`, `work.times`, `work.set_current`, every `chat.*`, `dm.open`, `msg.list`, `msg.threads`, `msg.reply`, `msg.ask`, `project.create`, `project.update`, `project.archive` and `area.*`.
- **By the brief's definition (end-to-end usable on real data), no feature set is READY.** The sets below are PARTIAL: a product review of the merged UI on sample data against the story ACs, and separately of the served backend operations against their rules. Each verdict marks that split.
- **The status column in BUGS.md lags merges.** For example, BUG-81/82/91/92 are fixed by `64a68813`, BUG-94..99 by `053702ef` and BUG-100 by `cc83d8be`. The open-bug lists below leave those out.

## Per feature set

### 1. Offices (Eden offices-and-teams-ux §3, §6.1: OFC-1, OFC-11, OFC-12)
- **(a) Merged:**
  - Status, reasons and restart: `ff8301ed`, `64cad970`, `5afb7640` (11-08).
  - Refresh order and failures: `705158f3`, `5068a6ae`, `2c2d540f` (11-10).
  - Pills: `423c3f04` (11-11).
  - A-OF1 rows: `64a68813` (11-12), then BUG-126..128 in `29530a93` and BUG-150 in `34ed1b29` (11-12).
  - The host (org-office-host) is real, with a lot of X-02 hardening on 11-11..13.
- **(b) Data:** sample data (`?mock=` scenarios). `office.list` and `office.restart` are host calls with no gateway route.
- **(c) IDs:**
  - Merged: OFC-1 (HQ list, rows), OFC-11 (reasons and remedy, restart) and OFC-12 (live status and Unknown, on the mock feed).
  - Not merged: OFC-2..10 (Add office, invitation, join approval, remove, Unplaced) and JOIN-*.
  - From my stories: AC-IA03-2 (`/settings/offices`) is not merged; `/offices` is still the old route.
- **(d) Open bugs:** BUG-26 (S4 countdown), BUG-64, BUG-166 (WebKit Retry focus), BUG-61 and BUG-178 (host label "This Mac"), BUG-89.
- **(e) Verdict: PARTIAL.** Review the Offices screen for OFC-1, OFC-11 and OFC-12 on sample data. It is the most polished screen.

### 2. Chat and messaging (Eden chat-ux: CHAT-1..35; A-1 puts CHAT-3..7, 12, 14, 15 and 18 and the DM halves of CHAT-1 and 8 in M3)
- **(a) Merged:**
  - Chat on a mock: `3cd08bfa` (11-08).
  - Server: message store `266105e3`, routing `4b3ff8dc`, events `1d4e7707` (11-09).
  - Messaging module: `4216b73d` (11-10).
  - Fixes: `be62fa23` (11-10); `1c05b7eb`, `39709507`, `ef0ce00b` (no emoji) (11-11); `053702ef` (BUG-94..99) and `6072bf08` (BUG-134..136) (11-12).
- **(b) Data:** UI on the in-memory sample (`messaging/sample.ts`). The server serves only `msg.send`, `msg.inbox` and `msg.read`, for agents; all `chat.*`, `dm.open` and thread calls are unserved.
- **(c) IDs:**
  - On the mock: most of the CHAT-1..35 screen behaviour, plus E-01 (Chat emoji removal).
  - Real: nothing; there is no owner chat path.
  - Mine: AC-IA08-3 (one presence component in Chat) and AC-IA16-1 (render-time emoji strip) are not merged.
  - MA-1..9 (messaging-asks) and CP-1 are not merged. Memory work (MEM-1/2) is still open.
- **(d) Open bugs:** BUG-62, 63, 65, 66, 67, 68, 71, 84, 85, 86, 119/162, 158, 160, 161, BUG-190 (S? anchor lost).
- **(e) Verdict: PARTIAL.** A UX and flow review on sample data is possible. Product sign-off waits for the real chat ops.

### 3. Work: tasks, bugs, checklists and Requests (Eden work-items-ux TSK/WORK/BUG/TRI/FIX/CHK; feature-requests-ux RQS-1..18, REQ-9..17)
- **(a) Merged:**
  - UI: work items `ee65b1b0`, bugs `9f5d41db`, copilot `b6ca4a49` (11-08); requests-1 `73e31783` and requests-2 `ec63d087` (11-09).
  - Server B1-11: kernel `2c32233f`, call `f84fb483`, assembly `b7ca88fc`, OrgView `e0e9c054` and C1 write path `d8e579da` (11-11); C2a.1a current task `2abab31c` (11-12).
  - Not merged: requests-3 (green, merge queued first per ada at 10:35).
- **(b) Data:** UI on sample (`e2e/fakes/work-api`). The server serves the writes but no reads (`work.list`, `work.get`, `work.snapshot` unserved).
- **(c) IDs:**
  - Merged in the UI: the TSK, BUG, TRI, FIX and CHK screens, and RQS/REQ through requests-2.
  - Not merged: TK-1..10 (tasks-v1), PL-1..10 (projects-planning), and my stories IA-18 (Work views) and IA-19 (New ▾).
- **(d) Open bugs:** BUG-6, 7, 8, 9, 11, 12 (Report a bug and copilot), BUG-54.
- **(e) Verdict: PARTIAL.** Review the Tasks board, Bugs and Requests on sample data against work-items-ux and feature-requests-ux. Requests are best reviewed after requests-3 lands. TK and PL: NOT YET.

### 4. Inbox (IA-09, IA-10, IA-11; MA-1)
- **(a) Merged, backend only:** IA-10a1a read types `fb4044a9`, IA-10a1b `inbox.list` with HQ serving and owner authorization `4acb522a` (11-12), and IA-10a2 hint publisher `e73de445` (11-13).
- **(b) Data:** real, but only on the office socket. There is no `/inbox` route, no gateway route and no UI.
- **(c) IDs:** part of AC-IA10-1 (read API; sources limited to what a1/a2 collect) and the groundwork for AC-IA10-2 (live hint). IA-09 (page and decision card), IA-11 and MA-1 are not merged.
- **(d) Open bugs:** BUG-184 (hint cursor, LOW), BUG-55 (no Needs you count source).
- **(e) Verdict: NOT YET** as a product feature. A backend contract check of AC-IA10-1 against `inbox.list` is possible.

### 5. Teams, Members, Org chart and agents roster (Eden TEAM-1..15, PEOPLE-1..8, REPORT-1..5, OWNER-1..3; EMP-*, RUN-*)
- **(a) Merged:**
  - Org: live org `01f39139` (11-08) and server agent rules `1020e14b` (11-08).
  - Employees module `ad305cd0` (11-10); S2 Agents tabs `673e1cb7` and Agents URL intents `aaa82b8f` (11-11).
  - Org chart: A-OC1 toolbar `92ca950e`, chart chips and focus `2b86877b`, BUG-123 `d49446c0`, BUG-129 `5ba5d947` (11-12).
- **(b) Data:** UI on sample. The server serves `org.snapshot` and the team and member writes, with no gateway route.
- **(c) IDs:**
  - Merged on sample: TEAM-1..15, PEOPLE-1..8, REPORT-1..5 and OWNER-1..3 (Teams, People and Reporting views), Add agent, Pause, Offboard and How it runs (EMP/RUN).
  - Not merged: IA-02 (D36 replaces these views with a vertical tree plus Members), IA-05 (Teams index and team page), IA-06 (Member profile) and IA-07 (Agent setup).
- **(d) Open bugs:** BUG-28..37 (agent dialogs), BUG-90, 103..109, 111, 114, 115, 117, 156, 157, 159, 183, BUG-22 (team version).
- **(e) Verdict: PARTIAL, low value.** The Org chart views are slated for replacement by IA-02. A batch should cover only the agent actions (hire, pause, offboard, How it runs) on sample data.

### 6. Projects (project page: PROJ/PD; my IA-17)
- **(a) Merged:** projects and areas `351f088c` (11-08), project detail aligned with the prototype `ea5ca768` (11-11), chips `2b86877b`.
- **(b) Data:** sample. The server serves only `project.add_team` and `remove_team`; `project.create` and `project.update` are unserved.
- **(c) IDs:** Brief · Team · Tasks (PROJ-2) is merged on sample. IA-17 tabs and PL-* are not merged.
- **(d) Open bugs:** BUG-112, 113, 116, 118, 168.
- **(e) Verdict: NOT YET.** IA-17 replaces the page.

### 7. Processes and docs (processes-ux PROC-1..34)
- **(a) Merged:**
  - UI: Processes screens `33f263e4` (11-09) and module `06efbae3` (11-10).
  - Backend: org-docs front matter `7d868a55` (B1-12 P1, 11-13).
  - Not merged: Rory's org-process P2 (at Tomas).
- **(b) Data:** UI on `processes/sample.ts`, and gated "not available" in production.
- **(e) Verdict: NOT YET.** No reader is wired to the UI. Bugs: BUG-60.

### 8. NOT YET (nothing reviewable merged)
- **Nav shell IA-01:** `nav.ts` is still the old list (Chat, Agents, Org chart ...); there is no `/home`, `/inbox`, `/teams` or `/settings` subnav. Merged shell pieces (topbar `536f36be`, responsive `2b1f39b2`, Tauri removal `07b9b9d3`) predate rev 5. Bugs: BUG-52, 59, 69, 78..80, 88, 89, 119, BUG-70 (S2 look drift).
- **Home (IA-04), Organization settings (IA-03) and Agent setup (IA-07):** no routes; `/settings` is a placeholder.
- **Activity (IA-12):** backend ports only, org activity `1636a188` and work activity adapter `d2fcb90e` (11-13). `work.activity` is unserved and `/activity` is a placeholder.
- **Presence (IA-08, IA-15):** no presence component and no runtime presence reporting. The server has `member.presence_changed` events (`1d4e7707`), but `status.set` is `invalid_request` on the HQ socket.
- **Search (IA-13), New ▾ (IA-19) and Notes and memory (IA-14):** not started.
- **Onboarding (welcome, create organization):** routes exist, but `createOrganization` is unavailable in production.

## What a first batch could be
- **Batch 1, UI on sample data (`vite dev --mode e2e`):** Offices (OFC-1, 11, 12); Work › Tasks, Bugs and Requests (TSK, BUG, TRI, FIX, CHK, RQS; after requests-3); Chat (CHAT M3 subset).
- **Batch 1b, backend contract checks:** `inbox.list` against AC-IA10-1, and the task, bug and checklist writes against work-items-ux rules.
- **Real-data product review** starts when the first gateway route and F1-02 cutover land (org snapshot first).
