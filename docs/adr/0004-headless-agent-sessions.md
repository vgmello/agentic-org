# ADR-0004 · Agent sessions run headless; people interact through chat

Status: **accepted by the owner 2026-11-17** (conflict plan entries D-04 and A-07).
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
   stops the agent's run and resumes its session in their own terminal. This is not
   offered through the browser in v1.
5. **The owner will supply two documents** describing the intended session view and
   interaction. Until they arrive, the read-only view is specified only to the extent
   above, and the app-attach section of the session design is not v1.

## Consequences

- No browser-to-process channel exists, which removes a large attack surface.
- A person cannot steer a running agent from the browser except by message. Delivery
  never interrupts a busy turn ([chat UX](../specs/chat-ux.md#42-system-notices-and-delivery)).
