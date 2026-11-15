Emery, design system v2.1 is in, matching your rev 4 (D20–D35) and the amended ACs. It's committed locally as 7de9183 (knowledge/design-system.md). The independent review is READY.

**Draft 3:** I checked it at 390 and 1280 px, and it fixes all eight draft-2 gaps:
- 0 "Status unknown" pills in lists;
- no sideways scroll at 390;
- the sidebar fits at 800;
- one Home error callout;
- Kai shown once;
- List/Board at 44 px;
- Show ▾ for Members;
- Maya without presence.
It also has 0 page errors and 0 emoji, `/members/you` redirects to Home, the account menu has your four items, and the team button says "Hire or add".

**One new design rule from draft 3:** at 1280×600 the sidebar hid Chat, Activity and Teams below a full Projects sub-list. The DS now collapses sub-lists first (Teams, then Projects) so top-level items never scroll out of sight. Please apply it in the prototype.

**What's in v2.1:**
- Agent setup Health (Ready, Needs setup: {reason}, Paused).
- The account menu, with no owner profile.
- "Lead {name}" on project cards and the project header.
- A gray Scoped pill.
- The Org changes filter.
- Assign task for people.
- The D26 kinds, clearing and dismiss rules.
- Org-only settings.
- Add a person / Hire or add.
- Sub-teams as indented rows, plus New team, Move team and Change manager.
- Projects A–Z until last-activity exists.
- IA-17 project page, IA-18 Work views, IA-19 New menu.
The old divergences and Open items are gone.

**Refinements, each labelled "for Emery" in the DS** (amend the ACs or tell me to drop them):
1. ⌘N works in the desktop app only, because browsers reserve ⌘N/Ctrl+N (AC-IA19-2).
2. Work's default view is Needs me when you have open items, otherwise Mine (AC-IA18-1 doesn't say the fallback).
3. An agent that's both paused and needs setup shows Needs setup (D20).
4. The account menu leaves out items whose flow doesn't exist yet, as in AC-IA19-1, which could drop Sign out in v1.0 (D21).
5. AC-IA05-2 still says "Hire or invite ▾"; D33 makes it "Hire or add ▾".

**One question:** AC-IA18-1's Needs me includes work "waiting for the owner's review", and the DS shows an amber Needs you on such a work row. D26 has no Inbox kind for a plain review wait (only Fix to verify and scoped requests). Should a review wait become an Inbox Approval, so the badge, the member's Needs you and the row agree? Or should the row stay gray until it is one? My default is to add it as an Approvals source ("Review requested").
