# ADR-0015 · Agent isolation is a selectable mode, with a dedicated OS user provisioned at setup

Status: **accepted by the owner 2026-11-17** during the architecture review (conflict
plan entry F-03). Implementation pending; decision 6 names a spike that must pass
before `dedicated-user` becomes the default. Amends the isolation statements in
[standalone-server.md](../design/standalone-server.md) section 3,
[agent-sessions.md](../design/agent-sessions.md) and the driver contract of
[execution](../design/organization-data/11-model-profiles-and-agent-execution.md)
section 8.

## Context

- Agents run today as the same OS user as the server and its data. The standalone
  design says this is not an isolation boundary and that stronger isolation needs
  separate users or containers, left for later.
- Protection today is the vendor runtime's own sandbox for the commands it runs, plus
  the office's permission policy and fixed denies. The vendor program itself is not
  confined.
- The launch contract has no isolation setting. `SpawnSpec` in `org-supervise` carries
  a program, arguments, a working directory and an environment, and the office can
  only signal processes of its own user.
- Not every agent can run in a container: some will build iOS features and need the
  host's Xcode, signing identities and simulators.

## Decision

1. **Isolation is a named, replaceable part of the launch contract.** The launch
   description carries an isolation mode from the runtime contract down to the process
   library. The mode is part of the launch digest, so changing it forces a controlled
   relaunch.
2. **Two modes in v1.**
   - `same-user`: today's behaviour. The agent runs as the service's own OS user,
     protected by the vendor tool sandbox and the office's permission policy.
   - `dedicated-user`: the agent runs as a dedicated OS account that owns only agent
     workspaces. The organization's data root is closed to that account by ordinary
     file permissions.
   The names `os-sandbox` and `container` are reserved for later modes. Adding a mode
   must not change the rest of the launch contract.
3. **Setup provisions the dedicated user.** `agentic-org setup` offers the mode. For
   `dedicated-user` it performs the one administrator step: it creates the account,
   installs the launcher of decision 4, and sets ownership and permissions. If
   administrator rights are not available, setup continues in `same-user` and says so.
   The mode can be changed later with a maintenance command.
4. **A small privileged launcher starts and stops agents in `dedicated-user`.** An
   ordinary process cannot start or signal a process of another user. Start, stop,
   cleanup and orphan recovery go through the launcher, with the same ownership mark
   and process-identity evidence as today.
5. **Who chooses.** The instance has a default mode. The owner may set a different
   mode for one agent, for example `same-user` for an agent that builds iOS features.
   An agent's own execution file cannot weaken its isolation.
6. **Spike before default.** Before `dedicated-user` becomes the default, a spike on
   macOS must show that each supported vendor runtime can sign in and run under a
   second account, and that an Xcode build works there or is confirmed to need
   `same-user`. Until then the default is `same-user`.
7. **No silent downgrade.** An agent configured for `dedicated-user` is not runnable
   when the launcher or account is unavailable. It is never started as `same-user`
   instead.
8. **Workspaces and sign-in.** In `dedicated-user`, agent workspaces are owned by or
   shared with the agent account, and each vendor sign-in is stored under it. This
   fits the selected rule that credentials are bound per office.

## Acceptance criteria

| ID | Passing evidence |
| --- | --- |
| ISO-1 | In `dedicated-user`, a command run by an agent cannot read or list the organization data root or the host's files; the attempt fails with a permission error from the operating system. |
| ISO-2 | Start, stop, cleanup after a crash and orphan recovery all work for an agent owned by the dedicated account. |
| ISO-3 | Setup without administrator rights completes in `same-user` and reports the reduced isolation; nothing is half-provisioned. |
| ISO-4 | An agent set to `dedicated-user` with the launcher removed is reported not runnable and no process starts. |
| ISO-5 | Changing an agent's mode changes its launch digest and causes a controlled relaunch. |
| ISO-6 | The spike of decision 6 is recorded with the vendor runtime versions and the Xcode result. |

## Consequences

- In `dedicated-user`, an agent cannot read the organization's data even if a tool
  escapes the vendor sandbox. This is what allows ADR-0016.
- Installation gains an optional administrator step and a privileged launcher to
  maintain.
- In `same-user`, the vendor sandbox and the office's rules remain the only barrier
  between an agent and the data. The product must say so where the mode is shown.
