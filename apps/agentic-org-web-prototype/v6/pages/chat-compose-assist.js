/* Composer assists (doc-40 §1 "Composer"): `@` mentions with autocomplete in the main and thread composers, and
   `/` commands at the start of the main composer. One listbox under the field's control: the textarea keeps focus and
   points at the highlighted option with aria-activedescendant; ↑/↓ move, Enter or Tab choose, Esc closes. */
import { CH, M } from "../data.js";
import { av, esc, kindWord, memberIds } from "../ui.js";

const assist = { field: null, kind: "", from: 0, items: [], active: 0 };

const COMMANDS = [
  { name: "task", hint: "Turn this conversation into a task, with its latest messages as context", insert: "" },
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
  Object.assign(assist, { field, kind: t.kind, from: t.from, items, active: Math.min(assist.active, items.length - 1) });
  paint();
}

/* ---------------------------------------------------------------- choosing */
function replaceTrigger(field, text) {
  const to = field.selectionStart;
  field.value = field.value.slice(0, assist.from) + text + field.value.slice(to);
  const caret = assist.from + text.length;
  field.setSelectionRange(caret, caret);
  field.dispatchEvent(new Event("input", { bubbles: true }));
}
/* A mention completes to the member's handle; a command either runs (`onCommand`) or writes its template. */
function choose(i) {
  const { field, items, kind } = assist, it = items[i];
  if (!it) return;
  if (kind === "mention") { closeAssist(); replaceTrigger(field, `@${it.value} `); return; }
  const cmd = COMMANDS.find((c) => c.name === it.value);
  closeAssist();
  if (cmd.insert) { assist.from = 0; replaceTrigger(field, cmd.insert); return; }
  field.value = "";
  field.dispatchEvent(new Event("input", { bubbles: true }));
  field.dispatchEvent(new CustomEvent("chat-command", { bubbles: true, detail: { name: cmd.name } }));
}
function onOptionPointer(e) {
  const li = e.target.closest('[role="option"]');
  if (!li) return;
  e.preventDefault(); /* the field keeps focus */
  choose(Number(li.dataset.i));
}
const MOVES = { ArrowDown: 1, ArrowUp: -1 };
/* Returns true when the key belonged to the open list. */
function onAssistKeydown(e) {
  if (!assist.field || e.target !== assist.field || e.isComposing) return false;
  const n = assist.items.length;
  if (MOVES[e.key]) assist.active = (assist.active + MOVES[e.key] + n) % n;
  else if ((e.key === "Enter" && !e.ctrlKey && !e.metaKey) || e.key === "Tab") choose(assist.active);
  else if (e.key === "Escape") closeAssist();
  else return false;
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
/* `/ask @agent question` and `/assign @agent T-A…`, parsed; null when the text is not a command. */
function parseCommand(text) {
  const ask = /^\/ask\s+@(\w+)\s+([\s\S]+)$/.exec(text.trim());
  if (ask) return { name: "ask", who: ask[1].toLowerCase(), question: ask[2].trim() };
  const assign = /^\/assign\s+@(\w+)\s+(T-[A-Z]\d+)\s*$/i.exec(text.trim());
  if (assign) return { name: "assign", who: assign[1].toLowerCase(), item: assign[2].toUpperCase() };
  const other = /^\/(\w+)/.exec(text.trim());
  return other ? { name: other[1].toLowerCase(), invalid: true } : null;
}

export { closeAssist, mentionsIn, onAssistKeydown, parseCommand, updateAssist };
