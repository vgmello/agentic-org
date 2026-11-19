# ADR-0004 · Agent sessions run headless; people interact through chat

Status: **accepted by the owner 2026-11-17**.
This number was reserved for an unmerged draft, "session panel embedded terminal", which
proposed running Claude with `--bg` and attaching to it. That draft was replaced before
it merged; this ADR records the decision that replaced it, which until now lived only in
[agent-sessions.md](../design/agent-sessions.md) decision 1. Implementation pending.

## Decision

1. **The office drives every agent headless.** It starts each vendor runtime as a child
   process and speaks its structured protocol over standard input and output. The
   background-and-attach approach is not used: it has no programmatic prompt channel,
   no structured events and no usage stream, and the office needs all three.
2. **People interact with agents through chat.** A person talks to an agent by sending
   it messages, and the agent answers the same way.
3. **There is no terminal in the browser.** The browser shows a session as a read-only
   view of what the agent did. It never attaches to an agent's process, never forwards
   keystrokes, and never carries terminal frames.
4. **Taking over an agent is a local command-line operation.** A person at the machine
   runs an `agentic-org` command. The office stops the agent's headless run, keeps the
   agent's lock, and gives the authorized local caller that session's launch
   description. The command then starts the vendor tool's interactive resume itself,
   in the person's own terminal, with the agent's private home and environment; in
   `dedicated-user` mode it goes through the same launcher. When the person exits,
   the command hands the agent back and the office resumes it headless. The office
   owns no terminal and carries no terminal data. This is not offered through the
   browser in v1.
5. **The session view and the question components are specified by two owner
   requirements,** received 2026-11-17:
   [REQ-05](../specs/req-05-virtual-console-conversation-log-viewer.md), a console that
   is decoupled from the agent's process and is built by parsing the agent tool's
   conversation files (Claude and Codex formats), with toggles for tool calls and for
   file-based visualizations; and
   [REQ-04](../specs/req-04-channel-messaging-and-question-components.md), structured
   questions from an agent to a person in the person's channel, rendered as choice,
   yes/no or free-text components. They replace the app-attached terminal section of
   the session design.

## Consequences

- No browser-to-process channel exists, which removes a large attack surface.
- A person cannot steer a running agent from the browser except by message. Delivery
  never interrupts a busy turn ([chat UX](../specs/chat-ux.md#42-system-notices-and-delivery)).
