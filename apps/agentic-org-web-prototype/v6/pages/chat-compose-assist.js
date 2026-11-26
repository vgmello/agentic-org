/* Composer assists (doc-40 §1 "Composer"): `@` mentions with autocomplete in the main and thread composers, and
   `/` commands at the start of the main composer. One listbox under the field's control: the textarea keeps focus and
   points at the highlighted option with aria-activedescendant; ↑/↓ move, Enter chooses, Esc closes. Tab is never
   taken: it closes the list and moves focus on as usual, so only Enter inserts. */
import { CH, M } from "../data.js";
import { av, esc, kindWord, memberIds } from "../ui.js";

const assist = { field: null, kind: "", from: 0, items: [], active: 0, chId: "", allowCommands: false };

const COMMANDS = [
  { name: "task", hint: "Turn this conversation into a task: /task what to do (its latest messages go as context)", insert: "" },
  { name: "ask", hint: "Ask an agent a structured question: /ask @agent your question", insert: "/ask @" },
  { name: "assign", hint: "Assign a work item: /assign @agent T-A…", insert: "/assign @" },
  { name: "files", hint: "Show this conversation's files", insert: "" },
];

/* ---------------------------------------------------------------- who can be mentioned */
function conversationPeople(ch) {
  if (!ch) return [];
  if (ch.kind === "dm") return [ch.with];
  return ch.kind === "a2a" ? ch.pair : ch.members || [];
}
/* This conversation's agents and people first, then everyone else; `@here` for a channel's members. */
function mentionItems(chId, q, agentsOnly) {
  const ch = CH[chId], here = conversationPeople(ch).filter((k) => M[k] && !M[k].owner);
  const others = memberIds().filter((k) => !here.includes(k));
  const matchesQ = (k) => !q || M[k].name.toLowerCase().split(/\s+/).some((w) => w.startsWith(q)) || k.startsWith(q);
  const people = [...here, ...others].filter((k) => matchesQ(k) && (!agentsOnly || M[k].kind === "agent"));
  const items = people.map((k) => ({ value: k, label: M[k].name, hint: `${kindWord(k)}${here.includes(k) ? " · in this conversation" : ""}`, who: k }));
  const offerHere = ch?.kind === "channel" && !agentsOnly && "here".startsWith(q);
  return (offerHere ? [{ value: "here", label: "@here", hint: "Everyone in this channel" }, ...items] : items).slice(0, 8);
}
const commandItems = (q) => COMMANDS.filter((c) => c.name.startsWith(q)).map((c) => ({ value: c.name, label: "/" + c.name, hint: c.hint }));

/* ---------------------------------------------------------------- what the caret is in */
/* `@query` just before the caret, or `/query` as the whole text so far (commands only start a message). */
function triggerAt(field, allowCommands) {
  const before = field.value.slice(0, field.selectionStart);
  const cmd = allowCommands && /^\/(\w*)$/.exec(before);
  if (cmd) return { kind: "command", q: cmd[1].toLowerCase(), from: 0 };
  const at = /(^|\s)@([\w.-]*)$/.exec(before);
  if (!at) return null;
  const agentsOnly = /^\/(ask|assign) @[\w.-]*$/.test(before);
  return { kind: "mention", q: at[2].toLowerCase(), from: before.length - at[2].length - 1, agentsOnly };
}

/* ---------------------------------------------------------------- the listbox */
const listId = (field) => `${field.id}-assist`;
const optionId = (field, i) => `${field.id}-assist-${i}`;
function optionHtml(field, it, i) {
  const lead = it.who ? av(it.who, 20) : `<span class="chat-assist-sym" aria-hidden="true">${it.label[0]}</span>`;
  return `<li role="option" id="${optionId(field, i)}" aria-selected="${i === assist.active}" data-i="${i}">${lead}<span class="chat-assist-label">${esc(it.label)}</span><span class="chat-assist-hint">${esc(it.hint)}</span></li>`;
}
function listboxEl(field) {
  let box = document.getElementById(listId(field));
  if (!box) {
    box = document.createElement("ul");
    Object.assign(box, { id: listId(field), className: "chat-assist" });
    box.setAttribute("role", "listbox");
    box.addEventListener("pointerdown", onOptionPointer);
    field.closest("form").append(box);
  }
  return box;
}
function paint() {
  const { field, items, kind } = assist, box = listboxEl(field);
  box.setAttribute("aria-label", kind === "command" ? "Commands" : "People and agents to mention");
  box.innerHTML = items.map((it, i) => optionHtml(field, it, i)).join("");
  field.setAttribute("aria-expanded", "true");
  field.setAttribute("aria-controls", box.id);
  field.setAttribute("aria-activedescendant", optionId(field, assist.active));
  box.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
}
function closeAssist() {
  const { field } = assist;
  if (!field) return;
  document.getElementById(listId(field))?.remove();
  field.setAttribute("aria-expanded", "false");
  field.removeAttribute("aria-activedescendant");
  Object.assign(assist, { field: null, kind: "", items: [], active: 0 });
}
/* Called on every input in a composer. */
function updateAssist(field, chId, allowCommands) {
  const t = triggerAt(field, allowCommands);
  const items = !t ? [] : t.kind === "command" ? commandItems(t.q) : mentionItems(chId, t.q, t.agentsOnly);
  if (!items.length) { closeAssist(); return; }
  if (assist.field && assist.field !== field) closeAssist();
  Object.assign(assist, { field, kind: t.kind, from: t.from, items, active: Math.min(assist.active, items.length - 1), chId, allowCommands });
  paint();
}

/* ---------------------------------------------------------------- choosing */
/* The caret can leave the token while the list is open (a click, arrow keys, Home). The list then re-reads what the
   caret is in: it closes, or re-anchors on the token the caret is now in. */
const CARET_KEYS = new Set(["ArrowLeft", "ArrowRight", "Home", "End", "PageUp", "PageDown"]);
function recheckAssist() {
  const { field, chId, allowCommands } = assist;
  if (field?.isConnected && document.activeElement === field) updateAssist(field, chId, allowCommands);
  else closeAssist();
}
/* The mention token the list was opened for, if the caret is still in it: from its "@" to the end of its word
   (so completing "@ma|ya" leaves no "ya" behind). Null once the caret has left it. */
function mentionRange(field) {
  const t = triggerAt(field, assist.allowCommands);
  if (!t || t.kind !== "mention" || t.from !== assist.from) return null;
  const caret = field.selectionStart, tail = /^[\w.-]*/.exec(field.value.slice(caret))[0];
  return { from: t.from, to: caret + tail.length };
}
function replaceTrigger(field, text, to = field.selectionStart) {
  field.value = field.value.slice(0, assist.from) + text + field.value.slice(to);
  const caret = assist.from + text.length;
  field.setSelectionRange(caret, caret);
  field.dispatchEvent(new Event("input", { bubbles: true }));
}
/* A mention completes to the member's handle; a command either runs (`onCommand`) or writes its template. */
function choose(i) {
  const { field, items, kind } = assist, it = items[i];
  if (!it) return;
  if (kind === "mention") {
    const range = mentionRange(field);
    closeAssist();
    /* Never write over words the caret has moved to: a stale list only closes. */
    if (range) replaceTrigger(field, `@${it.value}${/^\s/.test(field.value.slice(range.to)) ? "" : " "}`, range.to);
    return;
  }
  const cmd = COMMANDS.find((c) => c.name === it.value);
  closeAssist();
  assist.from = 0;
  /* Words already in the field (after the caret) stay: the command is written in front of them, and runs only
     when the field held nothing else, so choosing a command never discards the owner's text. */
  const after = field.value.slice(field.selectionStart), rest = after.trim();
  if (cmd.insert || rest) { replaceTrigger(field, cmd.insert || `/${cmd.name}${/^\s/.test(after) ? "" : " "}`); return; }
  field.dispatchEvent(new CustomEvent("chat-command", { bubbles: true, detail: { name: cmd.name, text: "" } }));
}
function onOptionPointer(e) {
  const li = e.target.closest('[role="option"]');
  if (!li) return;
  e.preventDefault(); /* the field keeps focus */
  choose(Number(li.dataset.i));
}
const MOVES = { ArrowDown: 1, ArrowUp: -1 };
/* The list's own keys; true when the key was used here. Tab closes the list but is never taken. */
function listKey(e) {
  const n = assist.items.length;
  if (MOVES[e.key]) assist.active = (assist.active + MOVES[e.key] + n) % n;
  else if (e.key === "Enter" && !e.ctrlKey && !e.metaKey) choose(assist.active);
  else if (e.key === "Escape") closeAssist();
  else { if (e.key === "Tab") closeAssist(); return false; }
  return true;
}
/* Returns true when the key belonged to the open list. Caret keys move the caret as usual, then re-read the token. */
function onAssistKeydown(e) {
  if (!assist.field || e.target !== assist.field || e.isComposing) return false;
  if (CARET_KEYS.has(e.key)) { setTimeout(recheckAssist); return false; }
  if (!listKey(e)) return false;
  e.preventDefault();
  e.stopPropagation();
  if (assist.field) paint();
  return true;
}

/* ---------------------------------------------------------------- on send */
/* The explicit mentions in a message: members by handle; `@here` names the channel's members. */
function mentionsIn(text, chId) {
  const keys = new Set();
  for (const [, h] of text.matchAll(/@([A-Za-z][\w.-]*)/g)) {
    const k = h.toLowerCase();
    if (k === "here") conversationPeople(CH[chId]).forEach((x) => keys.add(x));
    else if (M[k]) keys.add(k);
  }
  return [...keys];
}
/* A command is a complete first token: "/" and a word, then a space or the end. "/files.txt" or "/etc/hosts is
   missing" is prose. Returns { name, args, known } or null when the text is not a command. */
/* The token is read from the very first character: " /assign …" (indented) is prose and sends as written. */
function commandToken(text) {
  const t = /^\/([A-Za-z]+)(?:\s+([\s\S]*))?$/.exec(text.trimEnd());
  if (!t) return null;
  const name = t[1].toLowerCase();
  return { name, args: (t[2] || "").trim(), known: COMMANDS.some((c) => c.name === name) };
}
/* `/ask @agent question` and `/assign @agent T-A…`, parsed; `invalid` when a known command is unfinished,
   `unknown` for a slash word that is no command (offered to send as an ordinary message). */
function parseCommand(text) {
  const tok = commandToken(text);
  if (!tok) return null;
  if (!tok.known) return { name: tok.name, unknown: true };
  if (tok.name === "task" || tok.name === "files") return { name: tok.name, text: tok.args };
  const ask = tok.name === "ask" && /^@(\w+)\s+([\s\S]+)$/.exec(tok.args);
  if (ask) return { name: "ask", who: ask[1].toLowerCase(), question: ask[2].trim() };
  const assign = tok.name === "assign" && /^@(\w+)\s+(T-[A-Z]\d+)$/i.exec(tok.args);
  if (assign) return { name: "assign", who: assign[1].toLowerCase(), item: assign[2].toUpperCase() };
  return { name: tok.name, invalid: true };
}

export { closeAssist, mentionsIn, onAssistKeydown, parseCommand, recheckAssist, updateAssist };
