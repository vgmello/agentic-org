# Budgets

Your work costs money or tokens. The organization sets budgets for agents and for tasks, and the office enforces them in code.

## Check your budget

```bash
agent-os budget status
agent-os budget status --task <task>
```

The answer has your `daily` and `monthly` budgets and, with `--task`, the task's budget. Each shows its `state` and a readable `state_summary`. When `amounts_visible` is true you also see `limit_amount`, `used_amount` and `remaining_amount` in `unit` (`micro_usd` or `tokens`).

Check before you start large or open-ended work, such as a long test run, a big refactor or many sub-agents.

## Spend well

- Prefer a small verified step over a large speculative one.
- Read with `--fields` and `--format compact` instead of whole records (see `agent-os-basics`).
- Do not poll. End your turn when you are waiting; the office wakes you.
- Stop sub-agents and background commands you no longer need.

## When a budget runs out

The office places a hold when a budget is exhausted:

- Your current session is stopped at a safe point, and new turns do not start. Work addressed to you waits in your queue with the hold reason `budget_hold`; it is not lost.
- Starting a session is refused with a message that begins `BUDGET_HELD:` and names the budget.
- A hold ends when a person raises or resets the budget, or when its period renews.

You cannot lift a hold yourself, and you should not try to work around one, for example by asking another agent to do the same work. If you see the budget getting close, say so early: tell your team lead or the task owner what is left to do and what it is likely to cost.

```bash
agent-os msg send --to @<handle> --body <text>
```

## Task budgets

A task can have its own budget. When it is exhausted, work on that task holds even if your own budget has room. Say so on the task with a comment, so its owner can decide whether to raise it or change the scope.
