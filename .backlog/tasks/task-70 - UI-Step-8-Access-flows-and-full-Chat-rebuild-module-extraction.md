---
id: TASK-70
title: 'UI Step 8: Access flows and full Chat rebuild/module extraction'
status: Done
assignee: []
created_date: '2026-11-21T00:11:00Z'
updated_date: '2026-11-21T21:40:00Z'
labels:
  - ui
  - chat
  - auth
  - m1
milestone: m-0
dependencies:
  - TASK-17
  - TASK-65
documentation:
  - docs/plans/v1-ui-screens.md
  - docs/workstreams/ui.md
  - docs/specs/chat-ux.md
  - docs/specs/server-installation-and-access-ux.md
priority: high
type: feature
ordinal: 70000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Implement Step 8 of v1-ui-screens.md: /set-password, /approve (terminal sign-in approval), invited /register, Change password, organization switcher, and finish the Chat rebuild from branch v1-ui-chat-wip (extracting the 1,482-line route into src/lib/modules/chat/, moving sample mocks out of src/, and adding question/approval/hand-off cards, browse channels, pins, mute, system notices, Inbox link, and error/retry states).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Access routes (/set-password, /approve, invited /register, Change password, organization switcher) match server-installation-and-access-ux.
- [x] #2 Chat route is thin, domain code lives in src/lib/modules/chat/, in-src sample mocks are deleted, and all missing Chat features (pins, hand back, withdraw, cards, browse channels, retry) work.
- [x] #3 Type check, unit tests, lint, build, and Playwright specs pass.
<!-- AC:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Delivered UI Step 8 (Access routes /set-password, /approve, invited /register, ChangePasswordDialog, OrganizationSwitcher; full Chat rebuild extracted into src/lib/modules/chat/ with sample mocks removed from src/, question/approval/handoff cards, pins, mute, hand back, withdraw, browse channels, retry). Verified with svelte-autofixer (0), format:check (0), check (0/0), lint --max-warnings=0 (0/0), vitest (2,546/2,546), build, Playwright Chromium (55/55), production-unavailable (7/7), Pass 1 review (1d27302e) + repairs, Pass 2 separate fresh final review VERDICT: APPROVE (7e038d54) on exact commit 5d09a362, merged --no-ff to v1.
<!-- SECTION:FINAL_SUMMARY:END -->
