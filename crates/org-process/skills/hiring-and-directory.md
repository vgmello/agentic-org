# Directory and hiring

## Who is who

```bash
agent-os org inspect
agent-os directory list --team <team-id>
agent-os team list
agent-os team inspect --team <team-id>
```

- `agent-os org inspect` shows the organization's teams and members.
- `agent-os directory list` (also `agent-os who`) gives directory cards with each member's presence: `available`, `busy`, `waiting_for_answer`, `asleep`, `offline` and so on. Check presence before you expect a quick answer.
- `agent-os person inspect --member <member>` and `agent-os agent inspect --member <member>` show one person or agent.
- `agent-os role list` and `agent-os role inspect --role <role>` show the role catalogue.

Address people by handle (`@<handle>`) in messages; use member IDs in flags when you have them.

## Who decides

Each team has a lead, and teams sit under parent teams. Your standing instructions name your home team and the leads above you. Take questions about priorities and scope to your team lead first, and go further up only when they cannot decide.

## Hiring

Agents are hired through the organization's built-in HR lead, never by creating processes or accounts yourself.

- Only people and agents with hiring authority may hire, and only into their own part of the organization. The owner grants it; without it a hire is refused with `not_authorized`.
- If you have no hiring authority and need help, ask your team lead with `agent-os msg ask`; do not ask HR directly for a hire on your own.

With hiring authority, the usual flow is a draft that HR prepares and you confirm:

```bash
agent-os hr hire --draft --team <team-id> --name <name> --handle <handle> --title <title> --reason <text>
agent-os hr list --state drafting
agent-os hr inspect --request-id <request-id>
```

Read the draft, then confirm it with the revision you read, or withdraw it:

```bash
agent-os hr hire --id <request-id> --if-match <revision>
agent-os hr hire --id <request-id> --if-match <revision> --withdraw --closed-reason <text>
```

- `--role-id <role-id>` uses a catalogue role; `--role-text <text>` describes a role in your own words.
- A team has a member limit. A refusal that the team is full means the hire needs another team or a decision from its lead.

## After a hire

The new agent appears in `agent-os directory list`. Introduce the work to them with a message or a task assignment, and say who they report to.
