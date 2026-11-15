# UX review · Smart features: the copilot (PR #3, W11)

Reviewer: Eden · 2026-11-08 · Build: `cloud/copilot` @ `23e9bfe`, from my own e2e run (copilot.spec.ts, 16 passed): report-dialog-drafted, report-dialog-improve-proposal, copilot-failed, copilot-timeout, report-dialog-no-copilot, task-description-drafted · Against: `work-items-ux.md` (Report a bug, BUG), my `smart-features-candidates.md`, and the PR's `docs/design/smart-features.md`.

**Verdict (UX):** good. One medium, two lows.

## Good, keep it
- The copilot drafts and never acts. "Drafted by the copilot. Nothing is reported yet. Check every field and change what is wrong, then press Report bug." The person stays the author.
- It explains gaps ("You didn't say how to make it happen, so no steps are listed") and is honest about the stand-in ("Sample data: a rule-based stand-in, not a model's answer").
- **Undo draft** restores the person's own text. Improve shows a proposal with its reason ("Kept your meaning. Fixed spacing, capital letters and the ending.") and **Use this draft · Try again · Dismiss**.
- Failure is plain and recoverable: "The copilot couldn't draft this. Its answer wasn't usable. Try again, or write it yourself." The form still works without the copilot.
- One "Draft or improve" control per text field, the same everywhere (bug fields, New task, the task sheet).

## Findings
| # | Severity | Finding | Fix |
| --- | --- | --- | --- |
| 1 | Medium | **The copilot picks a severity, which the form deliberately doesn't** (Report a bug has no default severity, so the reporter chooses consciously). In report-dialog-drafted, Major is preselected with nothing marking it as a suggestion. | Mark drafted choices: "Major · suggested by the copilot" on the card until the person picks one. Or leave severity unselected and show the suggestion as a hint ("The copilot suggests Major"). |
| 2 | Low | **Which fields were drafted isn't shown per field**, so "check every field" means re-reading all of them. | A small "Drafted" tag beside each drafted field's label, cleared when the person edits it. |
| 3 | Low | **Two error panels at once** in copilot-failed (the sentence panel and Improve). Fine for the test, but each panel should clear its error when another draft starts. | Clear a panel's error on its next run or on Dismiss. |
