---
kind: spec
title: "REQ-05: virtual agent console and conversation log viewer (proof of concept)"
status: owner-requirement
revision: 1
owner: lead
created: "2026-11-17"
updated: "2026-11-17"
requested_by: owner
---

> **Owner requirement.** The text from "REQ-05" down to the line that starts "How this fits" is the owner's document, kept as written. The last section is the lead's alignment with the selected architecture and lists what is settled and what needs the owner's answer.

# REQ-05: Virtual (Decoupled) Agent Console & Conversation Log Viewer (POC)

## Overview

Provide a virtual (decoupled) console window that parses background agent conversation files—supporting both Claude and Codex log formats—and presents an interactive, filterable log view for execution analysis.

## Core Mechanics

- **Decoupled Background Execution:** Agents run behind the scenes in a detached process/window or headless CLI mode (such as `claude -p`) without an interactive terminal visible to the user.  
- **Conversation File Parsing:** Parse execution logs directly from persisted agent conversation files into a dedicated virtual console log window for inspection and analysis.  
- **Multi-Format Log Support (Claude & Codex):** Analyze and support both **Claude** message log formats and **Codex** message log formats so messages, events, and metadata render consistently on screen.  
- **Log Filtering & Visualization Toggles:**  
  - **Tool Call Visibility:** Toggle to hide or show tool call entries within the log stream.  
  - **File-Based Visualizations:** Toggle to hide or show different visualizations driven by the files referenced or modified in the conversation messages.

## Delivery Scope

- **Proof of Concept (POC):** Build an initial POC that parses sample Claude and Codex conversation log files and renders them in the virtual console log window with tool-call and file-visualization toggles.

## How this fits the selected architecture

Written by the lead on 2026-11-17. It adds no requirement; it places the owner's requirement in the existing contracts.

**What it matches already.** The office runs every agent headless and there is no terminal in the browser ([ADR-0004](../adr/0004-headless-agent-sessions.md)). This requirement is the read-only session view that ADR names: a console that is decoupled from the agent's process and built from what the agent's tool wrote to disk.

**Where the conversation files are.** Each agent has a private home that the office controls ([ADR-0015](../adr/0015-agent-isolation-modes.md), decision 10), and the vendor tool writes its conversation files there. So the office always knows where they are and can read them, in both isolation modes. The browser never reads a file: the office parses and serves the entries through the web API, to people authorized to inspect that agent (checklist row FC-150).

**One event model, two parsers.** A Claude parser and a Codex parser each turn their format into the same list of entries: message, tool call, tool result, file reference or change, usage, and notice. The view and its toggles work on that list and do not know the format. The runtime contract already defines a normalized session event for the live stream (`org-runtime`); the parsers produce the same shape, so the view can later show a running session and a finished one the same way.

**Toggles.** Tool calls: show or hide the tool call and result entries. File-based visualizations: show or hide what is derived from files the conversation read or changed, such as the list of touched files and the change to each.

**Proof of concept.** It can be built in the UI application alone, against sample Claude and Codex conversation files kept as test fixtures, with no server work. That proves the parsers and the view before the office serves real sessions.

**Things to settle while building.**

1. **The formats are the vendors' own and can change.** Each parser is tied to the tool versions it was tested with, keeps fixtures for them, and shows an entry it does not recognise as a raw entry instead of failing.
2. **Secrets in transcripts.** Conversation files can contain tokens or file contents. The redaction rule of the [session design](../design/agent-sessions.md) applies before anything is served, and access is limited to authorized people.
3. **A third runtime.** The requirement names Claude and Codex. The session design also lists Antigravity; it would be a third parser later.
4. **Running sessions.** The requirement describes parsing files. Following a file as it grows gives a near-live view without any connection to the agent's process. Whether v1 needs that, or only finished and paused sessions, is the owner's call.
