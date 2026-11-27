# Organizations, teams, roles, and hiring

Agentic Org models one organization as a tree of teams, a directory of people and agents, a reusable catalog of roles, and a formal HR hiring flow.

## Organization and teams

- One organization owns a hierarchy of teams rooted at top-level teams. Each team has an optional lead, a member cap (`max_members`, 1 to 100), and zero or more child teams.
- Inspect the whole structure with `agent-os org inspect`, list teams with `agent-os team list`, or inspect one team and its members with `agent-os team inspect --team <team>`.
- Every project has one accountable owning team and zero or more contributing teams. Owning-team hierarchy determines which processes and standards apply; contributing teams collaborate on work without changing the inheritance chain.
- Add or remove contributing teams with `agent-os project add-team --help` and `agent-os project remove-team --help`. Adjust team caps with `agent-os team set-limit --help`.

## People, agents, and roles

- People and agents share one directory. List active members and live presence with `agent-os directory list` (or `agent-os who`), inspect a person with `agent-os person inspect --member <member>`, or inspect an agent with `agent-os agent inspect --member <member>`.
- The organization owner is a protected person who cannot be removed or placed under a manager.
- Reusable role templates live in the role catalog (`agent-os role list`, `agent-os role inspect --role <role>`). Roles describe duties and instructions shared across agents hired into that role.

## Hiring authority and the HR flow

- Hiring requires `may_hire` permission. The owner may hire into any team; a person with `may_hire` may hire into teams they lead and their descendant teams. Grant or revoke hiring power with `agent-os member set-hiring --help`.
- All agent hires go through the HR workflow (`agent-os hr list`, `agent-os hr inspect --request-id <id>`, and `agent-os hr hire --help`, which supports drafting with `--draft`, confirming an existing draft with `--id <request>`, and withdrawing with `--withdraw`).
- Hires respect team `max_members` limits and produce an active agent placed on the target home team and office.
- Agents can be updated, moved between teams, paused, resumed, or permanently offboarded (`agent-os agent update --help`, `agent-os agent move --help`, `agent-os agent pause --help`, `agent-os agent resume --help`, `agent-os agent offboard --help`).
