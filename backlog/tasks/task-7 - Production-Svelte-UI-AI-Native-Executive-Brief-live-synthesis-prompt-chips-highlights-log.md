---
id: TASK-7
title: >-
  Production Svelte UI: Clara (Chief of Staff) Executive Brief + AI suggestions
  in Decision Queue
status: To Do
assignee: []
created_date: '2026-11-22T01:48:00Z'
updated_date: '2026-11-22T02:23:00Z'
labels:
  - ui
  - ai
  - home
  - svelte
dependencies: []
priority: high
type: feature
ordinal: 7000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Port the v5 Clara (built-in AI Chief of Staff) design from poc/v5.html into apps/agentic-org-ui: pure Executive Brief on the right (Today / 7-Day / Board lenses incorporating live status, highlights, and problems, signed by Clara — no Live & Highlights list or action chips on the right), Clara's AI-suggested solutions queued on the left inside Decision & Approval Queue, and a scoped 'Ask about this' trigger on each decision.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Renders Org AI narrative synthesis derived from live workspace KPIs, risks, and pending decisions
- [ ] #2 Quick prompt chips open Org AI in Ask mode and immediately answer the selected question
- [ ] #3 Compact Live & Highlights log starts with live working agents followed by recent key milestones, aligned with the Decision Queue across Ocean, Classic, Report, and Report Night
- [ ] #4 Reference implementation verified in poc/v6.html across Ocean, Classic, Report, and Report Dusk
- [ ] #5 Right column renders pure Executive Brief signed by Clara (Chief of Staff, Built-in AI) with Today / 7-Day / Board lenses and zero action chips
- [ ] #6 Clara's suggested solutions (e.g., rerouting capped-agent tasks) appear in the left-hand Decision & Approval Queue alongside approvals
- [ ] #7 Each active decision card includes a scoped 'Ask about this' button opening Ask Clara
<!-- AC:END -->
