---
kind: spec
title: "REQ-04: agent-to-user channel messaging and structured question components"
status: owner-requirement
revision: 1
owner: lead
created: "2026-11-17"
updated: "2026-11-17"
requested_by: owner
---

> **Owner requirement.** The text from "REQ-04" down to the line that starts "How this fits" is the owner's document, kept as written. The last section is the lead's alignment with the selected architecture and lists what is settled and what needs the owner's answer.

# REQ-04: Agent-to-User Channel Messaging & Structured Question Components

## Overview

Enable agents to communicate with users inside a dedicated user channel using structured messages, automatically rendering interactive UI components matched to the type of question being asked.

## Core Mechanics

- **Channel-Based Communication:** Whenever an agent needs to communicate with or ask something of the user, it sends messages directly to the user channel.  
- **Agent Question-Framing Instructions:** Provide explicit instructions guiding agents to frame questions in a structured format that the channel UI can reliably parse and display.  
- **Dynamic Question Components:** Render tailored UI components in the user channel based on the question type:  
  - **Choice Questions:** Render selectable option components (single-select or multi-select) for predefined choices.  
  - **Yes/No Questions:** Render binary confirmation components (Yes / No controls).  
  - **Free-Text Questions:** Standard text response input when structured options do not apply.

## Standardized Agent Question Contract (ask\_user Schema)

- Instruct agents to emit a unified ask\_user tool payload or structured JSON block rather than unformatted prose whenever user input or confirmation is required.  
- Schema fields:  
  - question\_type: "choice" \| "yes\_no" \| "free\_text"  
  - prompt: The question text presented to the user in the channel.  
  - options: List of selectable items (id, label, optional description) required when question\_type is "choice".  
  - allow\_multiple: Boolean indicating single-select (false) vs. multi-select (true) for "choice" questions.  
  - default\_value: Optional preselected or fallback value.

Example ask\_user payload for a choice question:

```json
{
  "question_type": "choice",
  "prompt": "Which deployment environment would you like to target?",
  "options": [
    { "id": "dev", "label": "Development", "description": "Staging build for internal testing" },
    { "id": "prod", "label": "Production", "description": "Live customer-facing environment" }
  ],
  "allow_multiple": false,
  "default_value": "dev"
}
```

## Adapter-Based Channel Rendering & Library Recommendations

- **External Messaging Channels ([Vercel Chat SDK](https://chat-sdk.dev)):**  
  - Use the open-source TypeScript chat package (vercel/chat) to compile a unified component model into native interactive UI across Slack (Block Kit), Microsoft Teams (Adaptive Cards), Google Chat (Cards), Discord (Components), and WhatsApp (interactive buttons and lists).  
  - Map "choice" questions to Select, RadioSelect, or Actions button groups routed to typed bot.onAction handlers.  
  - Map "yes\_no" questions to Card \+ Actions binary Button pairs, leveraging built-in approval callbacks to pause and resume workflows until the user responds.  
  - Handle free-text replies via thread message listeners (bot.onNewMessage) and structured multi-field inputs via Modal dialogs where supported.  
  - Account for cross-platform parity differences (such as modal support on Slack/Teams and WhatsApp's 3-button native limit before falling back to formatted text).  
- **Custom Web Channel Libraries (Internal Console / Dashboard):**  
  - [**Vercel AI SDK**](https://ai-sdk.dev) **\+ AI Elements:** Define a typed ask\_user tool with Zod schema validation (or stream structured payloads via useObject) to render custom React/Next.js choice and Yes/No components.  
  - [**CopilotKit**](https://www.copilotkit.ai) **\+ AG-UI:** Use useHumanInTheLoop and interactive tool renderers to suspend agent execution, display inline choice or confirmation components, and resume upon user submission.  
  - [**assistant-ui**](https://www.assistant-ui.com)**:** Use headless Thread, Message, and tool-UI primitives bound to AI SDK tool calls for custom React component rendering.  
  - [**Google A2UI**](https://a2ui.sh)**:** Use declarative JSON UI blueprints validated against a whitelisted client component catalog for cross-platform rendering without executable agent code.

## How this fits the selected architecture

Written by the lead on 2026-11-17. It adds no requirement; it places the owner's requirement in the existing contracts.

**What it matches already.** People interact with agents through chat ([ADR-0004](../adr/0004-headless-agent-sessions.md)). A question from an agent is an answerable card ([chat UX](chat-ux.md), decision 6), stored as a versioned `card` object in the message ([messaging contract](../design/organization-data/15-messaging-and-chat.md)).

**Field mapping.** The `ask_user` payload is the agent-facing form of an ask. It maps onto the card like this:

| `ask_user` field | Card field | Change needed |
| --- | --- | --- |
| `question_type` | `question_type`: `choice`, `yes_no` or `free_text` | New field |
| `prompt` | The message body | None |
| `options[].id`, `.label` | `options[{id,label}]` | None |
| `options[].description` | `options[].description` | New optional field |
| `allow_multiple` | `allow_multiple` | New field; default false |
| `default_value` | `default_option`, with `default_at` | None. Preselected in the component; given to the agent with the timeout of a non-required ask (owner point 4) |
| the answer | `answer{by,at,option_id,text}` | Add `option_ids[]` for a multi-select answer |

**Validation.** The office validates the payload before it becomes a message and refuses a malformed one with a reason the agent can act on: `choice` needs at least one option with unique IDs (the existing ask contract allows at most six options; more needs that limit raised); `yes_no` and `free_text` take no options; `default_value` must be an option ID for `choice`, `yes` or `no` for `yes_no`.

**How the agent emits it.** Two routes with one schema: an `ask_user` tool that the office provides to the agent's runtime, so the payload is checked at the tool boundary and the agent cannot fall back to unformatted prose; and `agent-os msg ask` accepting the same JSON for a runtime without tool support. The instruction to use it ships with the product as built-in guidance, not as an organization process ([processes design](../design/processes.md), decision 4).

**Waiting for the answer.** Every ask waits for a bounded time and may be marked required ([ADR-0020](../adr/0020-required-asks-suspend-and-resume.md)). Answering never makes the person wait: the answer is saved in their office and relayed.

**Rendering.** The product's own chat renders each question type with its own component, as Svelte components in `apps/agentic-org-ui`. There is one channel, so no adapter layer is built.

**Points settled and points that need the owner.**

1. **External channels: settled.** Owner, 2026-11-17: there are no external channels; the product has its own Slack-like chat and questions are asked and answered there. The "External Messaging Channels" part of the requirement above, and the Vercel Chat SDK with it, is not built. Nothing in v1 is shaped for a bridge to another chat product.
2. **Web libraries: settled.** Owner, 2026-11-17: the product does not use React; use shadcn-svelte and Bits UI. Both are already in `apps/agentic-org-ui` (shadcn-svelte components under `src/lib/components/ui`, built on Bits UI). The lead's mapping, to be adjusted while building: a radio group for a single choice, checkboxes for a multiple choice, a button pair for yes/no, a text area for free text, and Bits UI used directly where shadcn-svelte has no component. The React libraries named above are not used.
3. **Waiting for an answer: settled.** Owner, 2026-11-17, recorded as [ADR-0020](../adr/0020-required-asks-suspend-and-resume.md). An ask waits for a bounded time (a system setting, 10 minutes by default), the same for a person or an agent. The agent marks the ask `required` when it cannot continue without the answer. If a required answer does not come in time, the system tells the agent to close out its background work, shuts it down, and starts it again in the same session when the answer arrives. If the answer was not required, the ask returns a timeout and the agent decides what to do. `required` is a field of the ask, alongside the schema above.
4. **What `default_value` does: settled.** Owner, 2026-11-17: the default is preselected in the component, and the rest follows the lead's recommendation. For an ask that is not required, when nobody answers by the end of the wait the agent receives the default with the timeout and may continue with it; the card shows that nobody answered and what the agent continued with. A default never stands in for a required answer ([ADR-0020](../adr/0020-required-asks-suspend-and-resume.md) decision 14). Nothing is recorded as the person's answer unless the person gives it.
