# ADR-0015 · Agent isolation is a selectable mode, with a dedicated OS user provisioned at setup

Status: **accepted by the owner 2026-11-17** during the architecture review (conflict
plan entry F-03). Implementation pending; decision 7 names a spike that must pass
before `dedicated-user` becomes the default. Amends the isolation statements in
[standalone-server.md](../design/standalone-server.md) section 3,
[agent-sessions.md](../design/agent-sessions.md) and the driver contract of
[execution](../design/organization-data/11-model-profiles-and-agent-execution.md)
section 8.

## Context

- Agents run today as the same OS user as the server and its data. The standalone
  design says sharing an OS user is not an isolation boundary between instances and
  that stronger isolation needs separate users or containers. The Git design lists
  running agents as a separate macOS user as v2
  ([projects-git-github.md](../design/projects-git-github.md)).
- Protection today is the vendor runtime's own sandbox for the commands it runs, plus
  the office's permission policy and fixed denies. The vendor program itself is not
  confined.
- The launch contract has no isolation setting. `SpawnSpec` in `org-supervise` carries
  a program, arguments, a working directory and an environment, and the office can
  only signal processes of its own user.
- The office's local socket admits only its own OS user
  (`org_socket::peer_allowed` in `apps/agentic-org-office/src/serve.rs`). `agent-os`
  run by another account is refused today.
- Agent workspaces are inside the organization data root, under
  `runtime/<office-id>/teams/…/agents/<employee-id>/`.
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
   - `dedicated-user`: the agent runs as a dedicated OS account, one per instance in
     v1, that owns only agent workspaces and the agents' private homes (decision 10).
     Ordinary file permissions close the rest of `durable/`, the office database and the
     host's files to that account. Its workspaces stay
     where the storage contract puts them, reached through parent directories it may
     traverse and not list. Because all agents share the one account, separation
     between agents remains the vendor sandbox and the office's rules; an account per
     agent is a possible later mode.
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
5. **The local socket admits the agent account.** In `dedicated-user` the office's
   socket also admits the dedicated account, by an explicit allowed-user check and
   matching socket permissions. The session token, binding and ancestry rules apply
   unchanged; admission by user is never authorization by itself.
6. **Who chooses.** The instance has a default mode. The owner may set a different
   mode for one agent, for example `same-user` for an agent that builds iOS features.
   An agent's own execution file cannot weaken its isolation.
7. **Spike before default.** Before `dedicated-user` becomes the default, a spike on
   macOS must show that each supported vendor runtime can sign in and run under a
   second account, and that an Xcode build works there or is confirmed to need
   `same-user`. Until then the default is `same-user`.
8. **No silent downgrade.** An agent configured for `dedicated-user` is not runnable
   when the launcher or account is unavailable. It is never started as `same-user`
   instead.
9. **Workspaces.** In `dedicated-user`, agent workspaces and each agent's private home
   (decision 10) are owned by or shared with the agent account. Where the sign-in lives
   is decision 11.
10. **Every agent has a private home, in both modes (owner decision 2026-11-17,
    conflict plan entry A-08).** The office creates one home directory per agent at
    `durable/local/offices/<office-id>/agents/<employee-id>/home/` and starts the agent with `HOME` and each vendor's configuration
    location pointing into it. The agent sees a vendor folder (a "virtual Claude
    folder") that contains only what the office put there: the office's settings, the
    agent's own transcripts, and one sign-in. The user's own `~/.claude`, Codex home and
    `~/.gemini` are never read or written by an office-run agent. This replaces the
    earlier choice in the session design to reuse the user's real folders.
    The home is durable office-local data, not a cache: it holds the transcripts a
    session is resumed from and that the session view reads. In `dedicated-user` it is
    the one subtree under `durable/` that the agent account owns; its parent
    directories can be traversed and not listed.
11. **The sign-in comes from the subscription pool.** For each launch the pool
    ([document 14](../design/organization-data/14-model-providers-and-connection-pools.md))
    selects a connection. The source of truth for a sign-in stays in the office's
    binding, `auth/<connection-id>/`, which the agent account cannot read. At launch the
    office places a copy of that sign-in in the agent's private home for the run, and
    takes back any refreshed token when the run ends. An agent can therefore read the
    sign-in it is running with, in either mode; that cannot be avoided, because the
    vendor tool runs as the agent and needs it. Keeping an agent away from other
    sign-ins is not something the operating system does in v1: in `same-user` the agent
    runs as the service user, and in `dedicated-user` all agents share one account and
    could read each other's homes. As for everything between agents (decision 2), that
    barrier is the vendor tool's sandbox and the office's rules. The spike of decision 7
    also covers two agents on one connection each refreshing a copied token, and office
    settings kept in a home the agent can write. Switching an agent to another connection is a
    controlled relaunch with the other sign-in. Two agents on one subscription use
    the same connection state through the office, never through each other's homes.
    How each vendor runtime is pointed at a relocated sign-in (an environment variable
    for the configuration folder, and on macOS where the Keychain entry is looked up)
    is the first item of the spike in decision 7.
12. **Changing the filesystem root was evaluated and is the reserved `os-sandbox`
    mode, not v1.** The owner asked whether putting each agent in its own root
    directory (chroot) would be a cheap sandbox.
    | Question | Finding |
    | --- | --- |
    | Does it need privilege? | Yes for `chroot` on both systems. On Linux an unprivileged process can get the same effect with user and mount namespaces. |
    | Does it work on macOS? | Not for this use. Tools, compilers and Xcode need the system volume, the shared library cache and per-user services, which a changed root hides. macOS confines a process with a Seatbelt profile instead. |
    | What does it add over decision 10? | Decision 10 redirects where well-behaved tools look. A changed root, or a Seatbelt profile that denies everything outside the private home and workspace, also stops a tool that ignores `HOME`. |
    | Cost | A per-agent root must contain or bind every tool the agent runs. That is a packaging problem per platform and per toolchain. |
    The decision: v1 ships the private home of decision 10 in both modes, which gives
    the "virtual folder" the owner described without privilege. `os-sandbox` is
    specified next as one mode with two adapters: a mount-namespace root on Linux and
    a Seatbelt deny-by-default profile on macOS. It needs its own spike and is not a
    v1 commitment.

## Acceptance criteria

| ID | Passing evidence |
| --- | --- |
| ISO-1 | In `dedicated-user`, a command run by an agent cannot read or list `durable/` outside the agents' private homes, the office database or the host's files; the attempt fails with a permission error from the operating system. It can read and write its own workspace. |
| ISO-7 | In `dedicated-user`, `agent-os` run by the agent account is admitted by the office socket and still needs its session binding; any other OS user is refused. |
| ISO-2 | Start, stop, cleanup after a crash and orphan recovery all work for an agent owned by the dedicated account. |
| ISO-3 | Setup without administrator rights completes in `same-user` and reports the reduced isolation; nothing is half-provisioned. |
| ISO-4 | An agent set to `dedicated-user` with the launcher removed is reported not runnable and no process starts. |
| ISO-5 | Changing an agent's mode changes its launch digest and causes a controlled relaunch. |
| ISO-6 | The spike of decision 7 is recorded with the vendor runtime versions and the Xcode result. |
| ISO-8 | In both modes, an office-run agent of each supported vendor starts with a private home, signs in with the connection the pool selected, and a file search from its tools finds nothing from the user's own vendor folders. |
| ISO-9 | Changing an agent's pool connection relaunches it with the other sign-in; the previous connection's state is no longer reachable from its home. |

## Consequences

- In `dedicated-user`, an agent cannot read the organization's sources even if a tool
  escapes the vendor sandbox. This strengthens ADR-0016, which does not depend on it.
- Installation gains an optional administrator step and a privileged launcher to
  maintain.
- In `same-user`, the vendor sandbox and the office's rules remain the only barrier
  between an agent and the data. The product must say so where the mode is shown.
