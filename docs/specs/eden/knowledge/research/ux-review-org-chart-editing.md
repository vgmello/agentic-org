# UX review · Org chart editing (pre-merge)

Reviewer: Eden · 2026-11-08 · Build: branch `app-dev/org-chart-editing` @ `875ac4d` (stacked, not on v1), from app-frontend-dev's Playwright screenshots · Against: `docs/specs/offices-and-teams-ux.md` rev 6, §7 (TEAM, PEOPLE, REPORT, OWNER) · Reviewed: new-team, move-team, add-person, remove-person, change-manager, edit-member-teams, delete-team. Not reviewed: edit-members, add-project.

**Summary.** The editing flows are well done. Every disabled choice says why, consequences are spelled out before anything changes, the handle rule is live, the manager rule follows org.rs, and the copy is plain. Fix one rendering bug and a few clarity issues on the chart itself before merge.

## Outcome (same day)

All findings resolved or accepted on `app-dev/org-chart-editing`: #1 and #7 were captured mid fade-in, and the helper now waits, re-shot and verified. #2 is "Owner of Acme" with the Sample data badge in the bar. #3: the sample owner is Alex Kim. #4: "+N teams" is a text chip. #5: agent tiles show initials plus the robot glyph, labelled "<name>, agent". #6 already defaulted to the owner (the screenshot was taken after the test picked Avery). #8 is accepted for now. #9 is the real "…" character.

## Findings

| # | Severity | Finding | Screen | Fix |
| --- | --- | --- | --- | --- |
| 1 | High | **Delete dialog renders see-through, with the team menu still open on top.** The chart cards and the ⋯ menu (Rename…, Edit members…, Delete…) show through and over the dialog text, and the page isn't dimmed like the other dialogs. It may be a capture mid-transition, but it reads as broken. | delete-team | Close the menu when the dialog opens. Give the dialog an opaque panel and the same backdrop as the others. Re-take the screenshot after the transition. |
| 2 | Medium | **The organization's name disagrees.** The sidebar says "Acme", while the owner bar and notes say "Sample organization". | all | Use one name everywhere (the sample's name, which the Sample data badge already explains). |
| 3 | Medium | **The owner is called "You (you)".** The sample owner's name is "You", so pickers read "You (you)" and the bar "You (you) · Owner…". Spec: "Alex Kim (you)". | change-manager, chart | Give the sample owner a real name. Keep "(you)" as the suffix. |
| 4 | Medium | **"+1 team" is drawn as a circle,** which is the person shape (§7.0: people are circles, agents squares). It reads as a person. | chart | Show it as a small text chip ("+1 team") beside the agent tile, not a round avatar. |
| 5 | Medium | **Agent tiles are identical robot glyphs.** People show initials (HW, LB), but agents can't be told apart without hovering. | chart | Add initials, or the first letters of the name, to the agent square (keeping the square shape and indigo tint), and a tooltip plus accessible name with "<name>, agent". |
| 6 | Medium | **Add person defaults "Reports to" to Avery Stone.** Spec §7.4: it defaults to the owner. | add-person | Default to the owner ("Alex Kim (you)"). |
| 7 | Low | **The primary button looks disabled when it isn't.** "Move Engineering" (with Design chosen) and "Change manager" look muted, unlike "Save" and "Add person". | move-team, change-manager | Same enabled style as other primary buttons. Use the dimmed style only while disabled, and say why. |
| 8 | Low | **Pickers are native selects** for Part of and Reports to, while §2 says a combobox with the team path and secondary text. They work, but long organizations will need search. | new-team, add-person | Use a combobox when the list exceeds about 10. |
| 9 | Low | Typography: "Move Engineering to..." uses three dots. | move-team | Use "…" (one character), as elsewhere. |

## Good, keep it

- Disabled options always say why ("Can't move a team under itself or its own teams", "Inês Duarte reports to Hana Weiss, directly or through others").
- The confirmations list exactly what changes (remove person: teams, lead, who reports where, messages, tasks).
- The handle rule is live with a suggestion. The "Agents address people by name…" hint is clear.
- "Also in" on member options, a title per team in Edit teams, and the lead hint ("the lead also leads the team's projects").
- Undo toasts on Move and Change manager.

## Known not done (per app-frontend-dev, not flagged)

Drag to move, tree keyboard navigation (arrow keys), TEAM-11 search, "Add a new person…" inside member pickers, the "their reports" list in the Reporting-lines quick view. For the goal of managing our own team in the app, I'd prioritize **keyboard navigation** and **search** next, then drag.
