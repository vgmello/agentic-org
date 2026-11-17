---
process: decide-and-escalate
name: Decide, record, escalate
kind: process
level: organization
version: 2
owner: owner
status: active
summary: Decide yourself inside the non-negotiables, record the decision where the next person looks, and bring to the owner only what changes the product for people or cannot be undone.
applies_to: every engineer and agent working on this repository
overridable: false
---

# Decide, record, escalate

How engineers and their agents make decisions while building Agentic Org. The rules
are those of [architecture non-negotiables](../architecture-non-negotiables.md),
"Who decides what"; this page restates them as a procedure. If the two ever differ,
the non-negotiables page wins.

## 1. Decide yourself

Decide, without asking, anything the requirement documents do not settle or settle
in two ways: internal structure, algorithms, names, file formats inside the contract's
intent, the order of work, what to delete, how to test.

When two documents disagree, follow the
[documentation authority map](../design/documentation-authority.md). If that does not
settle it, take the reading that fits the non-negotiables and their aims best.

When unsure, you may ask a fresh subagent for an independent view: give it the
question and the facts, not your conclusion. You still own the decision.

## 2. Record it where the next person will look

- Change the document that owns the topic so that it states the decision. Replace the
  old text; do not leave it beside the new.
- For a decision about architecture, add a short decision record in
  [`docs/adr/`](../adr/): the decision, why, and what you rejected.
- Update the [feature checklist](../FEATURES.md) row.

## 3. A non-negotiable does not vary

If a non-negotiable cannot be met, look again for a design that meets it. If there is
truly none, do not stop: choose the best option, build it, and write a decision record
that names the non-negotiable, says why it could not hold, what you chose and what it
costs. Tell the owner in your next report. An exception is recorded, never silent.

## 4. Bring to the owner only what needs the owner

Bring to the owner only what changes the product for the people using it (adding or
removing something a person can do) or a choice that cannot be undone later. Bring it
with a recommendation, in one short message.

## 5. Keep moving

While a question waits for the owner, keep working on everything that does not depend
on the answer. When the answer comes, record it as in step 2.
