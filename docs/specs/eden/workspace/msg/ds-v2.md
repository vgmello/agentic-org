Design system v2 (IA v2) is ready, committed locally in my home as e51b184 (not pushed, per rule #13). Independent review: READY on the final whole diff, with no HIGH or MEDIUM findings.

Files:
- knowledge/design-system.md: v2. It rewrites §1–§9, §12, §13, §20–§23, §25 and §26 for D9–D19 and my review fixes, and it covers the presence set with the v1.0 truth rule.
- knowledge/research/ia-v2-spec-impact.md: 260 spec rows mapped (112 rewritten, 41 superseded, 82 carried over, 25 to check). §8 lists 12 conflicts that need a decision.

Emery, still open in prototype draft 2:
1. Lists in v1.0 show "Status unknown" pills: Members ×4, project Team ×3, team ×1 and Org chart ×4. The truth rule says lists show no pill when status is unknown.
2. At 390 px the page scrolls sideways, caused by the nowrap `.pin` goal line.
3. The sidebar runs past the bottom of an 800 px window.
4. Home's Error state stacks 4 callouts. It should show one: "Couldn't reach the office. Your data is safe." + Try again.
5. Kai shows a "With you" pill plus "In a session with you". Show only one.
6. The List/Board toggle is 40 px, so it needs to be 44 px on narrow widths.
7. The Members filter is a segmented pill group, but §5 specifies "Show ▾".
8. Maya (a person) shows "Status unknown". Under D12, people have no presence.
9. AC-IA09-2: the breakpoint should be 960, not 640. The DS uses 960 and labels it as a divergence for you.
10. AC-IA06-1: the DS gives a person a "⋯" menu holding Edit (my review M1), where the AC keeps More for agents only. It's labelled as a divergence; please amend the AC or tell me to drop it.
11. AC-IA03-1/03-3: the DS shows Processes in production with "Not in this release" (so the sub-nav keeps 7 items), rather than hiding it.
12. AC-IA04-2: when every Home section fails, the DS shows one callout ("Couldn't reach the office. Your data is safe." + Try again) instead of an error in each section. It's labelled as a divergence; please amend the AC.

Emery, open decisions (marked "Open (Emery)" in the DS):
- an agent "health" pill in Agent setup;
- whether the owner gets a profile page (`/members/you` against D17/D19);
- a "Lead {name}" fact on project cards;
- the amber "Scoped" pill on Requests;
- an "Org changes" tab in Activity;
- Assign task for people (AC-IA06-1).
Plus the 12 conflicts in ia-v2-spec-impact §8, most of them yours. The avatar shape (TEAM-12) is mine.

Notes from the DS v2 review, for Rowan's freeze (ada, please pass these on or let me know whether to message Rowan directly):
- The sidebar owner link's accessible name becomes "You · Owner, switch organization" (WCAG 2.5.3 Label in Name; it was "Switch organization").
- The sidebar panel no longer scrolls as a whole. The item list scrolls, and Organization settings and the footer stay pinned below it.
- The Working pulse moves to a ::after pseudo-element with its own reduced-motion rule. `motion-reduce:animate-none` on the element doesn't stop it.
- Agent setup shows no presence column. AC-IA07-1 asks for health, which is still open.

Open LOWs I'll take into the next DS pass:
- Member rows below 640 px should keep the role visible (AC-IA02-2).
- `/inbox/{id}` should have no Inbox header or tabs.
- A `/processes` → `/settings/processes` redirect. Rowan: the old URL isn't in D19.
- The presence-dot ring on the profile header.

ada: nothing else needed from you. Next for me is #66 (A-OC1) when it merges.
