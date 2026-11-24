---
id: decision-4
title: 'ADR-0004: Agent sessions run headless; people interact through chat'
date: '2026-11-21T00:43:00Z'
status: accepted
---

> **Review note (2026-11-21):** Aligned with `doc-1` (Vision: no browser terminal or raw prompt console). Implementation is pending: headless session runner (`TASK-42`, C3), REQ-05 conversation log viewer (`TASK-43`, C4), and REQ-04 structured asks (`TASK-46`, B7). Decision 4 (local CLI takeover) is deferred to v2 (`TASK-75`). Narrow PTY exception for one-time provider OAuth enrollment is governed by ADR-0032 (`decision-32`).

# ADR-0004 · Agent sessions run headless; people interact through chat

Status: **accepted by the owner 2026-11-17**.
This number was reserved for an unmerged draft, "session panel embedded terminal", which
proposed running Claude with `--bg` and attaching to it. That draft was replaced before
it merged; this ADR records the decision that replaced it, which until now lived only in
[agent-sessions.md](../docs/design/doc-29%20-%20Design-Agent-Sessions-and-Runtime-Isolation.md) decision 1. Implementation pending.

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
   *Narrow exception (2026-11-19, [ADR-0032](decision-32%20-%20ADR-0032-Browser-driven-CLI-sign-in-enrollment-credential-distribution-and-permission-presets.md)):*
   during one-time provider connection enrollment, the office worker (`agentic-org-office`)
   may run a bounded, short-lived local PTY helper strictly to drive a vendor's sign-in CLI
   (`claude setup-token`, `claude auth login --claudeai`, or `codex login --device-auth`).
   No terminal frames or arbitrary keystrokes cross the browser boundary—only a structured
   OAuth URL/code and, for Claude's paste-code flow, one validated authorization code
   string—and agent sessions themselves never use a PTY or browser terminal.
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
   [REQ-05](../docs/specs/doc-56%20-%20REQ-05-Virtual-Console-and-Conversation-Log-Viewer.md), a console that
   is decoupled from the agent's process and is built by parsing the agent tool's
   conversation files (Claude and Codex formats), with toggles for tool calls and for
   file-based visualizations; and
   [REQ-04](../docs/specs/doc-55%20-%20REQ-04-Channel-Messaging-and-Question-Components.md), structured
   questions from an agent to a person in the person's channel, rendered as choice,
   yes/no or free-text components. They replace the app-attached terminal section of
   the session design.

## Consequences

- No browser-to-process channel exists, which removes a large attack surface.
- A person cannot steer a running agent from the browser except by message. Delivery
  never interrupts a busy turn ([chat UX](../docs/specs/doc-40%20-%20Spec-Chat-Channels-and-Structured-Asks-UX.md#42-system-notices-and-delivery)).
