import { CH, IN, M, OFFICES, W } from "../data.js";
import { fitTextarea } from "./work.js";
import { openSmartTaskComposer } from "./task-composer.js";
import {
  applyPos, followThreadEnd, jumpToAsk, jumpToLatest, placeStream, placeThread, replaceStream, savedPos, syncLatestJump, trackPos, watchAsks,
} from "./chat-scroll.js";
import { ASK_CLICKS, answerableHere, messageNeeds, onAskKeydown, resetAsks, saveAskDraft, workRef } from "./chat-asks.js";
import { CHAT_VIEWS, chatViewHtml, isChatView } from "./chat-views.js";
import { bindOverlays, syncOverlayHistory } from "./chat-overlays.js";
import {
  PRESENCE_WORD, chatListHtml, needsCount, onListClick, onListInput, onListKeydown, prepareList, takeListFocus, unreadDot,
} from "./chat-list.js";
import { endVisit, markAllRead, newFromId, startVisit, watchUnread } from "./chat-read.js";
import { listIsOverlay, watchListMode } from "./chat-layout.js";
import { bindPanelResize, panelWidthStyle } from "./chat-panel-width.js";
import { closeAssist, mentionsIn, onAssistKeydown, parseCommand, recheckAssist, updateAssist } from "./chat-compose-assist.js";
import {
  DEFAULT_CHAT, chatTitle, dmSources, inboxItem, openAsks, openDirectMessage, resolveChatId, sentAt, sourceHref, sourceLabel, splitTime,
} from "./chat-model.js";
import "./chat-attach-seed.js";
import { THREAD_SUM_CLICKS, markRoot, rootOpen, setRootOpen, threadSummaryHtml } from "./chat-thread-sum.js";
import { ATTACH_CLICKS, attachButton, attachmentsForSend, bindAttachments, clearAttachments, trayHtml } from "./chat-attachments.js";
import { VIEW_CLICKS, attachmentNameHit, closeViewer, filesButton, filesPanelHtml, messageAttachmentsHtml, viewerOpen } from "./chat-attach-view.js";
import { render, route } from "../shell/router.js";
import { CHAT_SEND_SHORTCUTS, S } from "../state.js";
import {
  av, esc, ic, joinSep, kindWord, mlink, moreMenu, plink, plural, presence, statusLine, toast,
} from "../ui.js";

const chatDrafts = new Map();      /* conversation id -> unsent message */
const threadDrafts = new Map();    /* thread root message id -> { text, also }: unsent reply and its "Also send to" choice */
const local = {
  search: null, linked: null, pendingLink: null, details: false, members: false, files: false, threadFrom: "", openThread: null, threadLand: null,
  focusCompose: false,
};
const OVERLAY_THREAD = "(max-width: 1279px)";
const matchesMedia = (q) => Boolean(window.matchMedia?.(q).matches);

/* ---------------------------------------------------------------- names and time */
function chatDocTitle(id) {
  if (isChatView(id)) return `${CHAT_VIEWS[id].title} · Chat`;
  return `${chatTitle(CH[id && Object.hasOwn(CH, id) ? id : DEFAULT_CHAT])} · Chat`;
}
const clock = () => new Date().toTimeString().slice(0, 5);
const authorWord = (who) => (M[who].owner ? "" : kindWord(who));

/* ---------------------------------------------------------------- message text: T- refs and @mentions become links */
function mentionRef(match, name) {
  const lower = name.toLowerCase();
  if (lower === "here") return `<span class="chat-here">@here</span>`;
  const key = Object.keys(M).find((k) => k === lower || M[k].name.split(" ")[0].toLowerCase() === lower);
  return key && !M[key].owner ? `<a class="alink chat-ref" href="#/members/${key}">@${esc(M[key].name)}</a>` : match;
}
const richText = (text) => esc(text).replace(/\bT-A\d+\b/g, workRef).replace(/@([A-Za-z]+)/g, mentionRef);

/* ---------------------------------------------------------------- one message */
/* Hover tools on wide pointers; one touch-sized menu (with the message's time) on narrow or touch screens. */
function toolButton(act, id, icon, label) {
  return `<button class="btn ghost icon msg-tool" type="button" data-act="${act}" data-id="${id}" aria-label="${esc(label)}" title="${esc(label)}">${ic(icon, "ic16")}</button>`;
}
/* In an agents' conversation the one action is named on wide pointers too, not left to a tooltip. */
function textToolButton(act, id, icon, label) {
  return `<button class="btn ghost sm msg-tool msg-tool-text" type="button" data-act="${act}" data-id="${id}">${ic(icon, "ic16")}${esc(label)}</button>`;
}
const menuEntry = (act, id, icon, label) => `<button role="menuitem" type="button" data-act="${act}" data-id="${id}">${ic(icon, "ic20")}${esc(label)}</button>`;
function messageTools(m, readOnly) {
  const id = esc(m.id), name = M[m.who].name;
  const tools = readOnly
    ? (M[m.who].owner ? [] : [["chat-dm-msg", "chat", `Message ${name} about this`]])
    : [["thread", "thread", "Reply in thread"], ["chat-task", "clipboard", "Turn into task"]];
  if (!tools.length) return "";
  const time = `<p class="msg-menu-time" role="none">${ic("clock", "ic14")}Sent ${esc(sentAt(m))}</p>`;
  const menu = time + tools.map(([act, icon, label]) => menuEntry(act, id, icon, label)).join("");
  return `<span class="msg-tools" role="group" aria-label="Message actions">
    ${tools.map(([act, icon, label]) => (readOnly ? textToolButton : toolButton)(act, id, icon, label)).join("")}
    <span class="msg-menu">${moreMenu("msg-" + m.id, `Actions for message from ${name}, ${sentAt(m)}`, menu, "ghost")}</span>
  </span>`;
}
const sendingStatus = () => `<span class="msg-status" role="status">Sending…</span>`;
function threadButton(m) {
  if (!m.thread || !m.thread.length) return "";
  return `<button class="thr" type="button" data-act="thread" data-id="${esc(m.id)}" aria-expanded="${S.thread === m.id}">${ic("thread", "ic16")}${plural(m.thread.length, "reply", "replies")}</button>`;
}
/* A DM's first message can link to the conversation or the exact message it is about. */
function sourceLine(m) {
  const src = typeof m.src === "string" ? { ch: m.src } : m.src;
  if (!src || !CH[src.ch]) return "";
  return `<p class="msg-src">${ic("eye", "ic14")}<span>About <a class="alink chat-ref" href="${esc(sourceHref(src))}">${esc(sourceLabel(src))}</a></span></p>`;
}
/* While searching, a message whose thread holds a match quotes the first matching reply and opens it in the thread. */
function threadHitLine(m, q) {
  const hits = q ? threadHits(m, q) : [];
  if (!hits.length) return "";
  const r = m.thread[hits[0]], more = hits.length > 1 ? ` (+${hits.length - 1} more)` : "";
  return `<p class="msg-src chat-thread-hit">${ic("thread", "ic14")}<span><b>${esc(M[r.who].name)}</b> in the thread: <q class="chat-hit-quote">${esc(r.text)}</q>${more}
    <button class="alink chat-ref" type="button" data-act="thread" data-id="${esc(m.id)}" data-hit="${hits[0]}">Open in thread</button></span></p>`;
}
/* A structured question the owner asked an agent with /ask. */
function askLine(m) {
  if (!m.ask || !M[m.ask.to]) return "";
  return `<p class="msg-src chat-ask-line">${ic("question", "ic14")}<span>Question for ${mlink(m.ask.to)} · waiting for an answer</span></p>`;
}
/* A thread reply also sent to the channel links back to its thread. */
function threadCopyLine(m) {
  if (!m.inThread) return "";
  const { id, who } = m.inThread;
  return `<p class="msg-src">${ic("thread", "ic14")}<span>Sent from <button class="alink chat-ref" type="button" data-act="thread" data-id="${esc(id)}">the thread on ${esc(M[who].name)}'s message</button></span></p>`;
}
const continues = (m, prev) => prev && prev.who === m.who && splitTime(prev.time)[0] === splitTime(m.time)[0] && !prev.needs;
function messageHead(m, time) {
  const word = authorWord(m.who);
  const status = m.pending ? sendingStatus() : time ? `<time class="msg-time">${esc(time)}</time>` : "";
  return `<p class="hd"><b>${mlink(m.who)}</b>${word ? `<span class="msg-kind">${word}</span>` : ""}${status}</p>`;
}
function messageHtml(m, prev, opts = {}) {
  const time = splitTime(m.time)[1], cont = continues(m, prev);
  const head = cont ? `<p class="sr-only">${esc(M[m.who].name)}</p>` : messageHead(m, time);
  const gutter = cont ? `<span class="msg-gutter">${m.pending ? "" : `<time class="msg-time">${esc(time)}</time>`}</span>` : av(m.who, 32);
  /* A grouped message has no header, so its send status sits after the text, never in the narrow gutter. */
  const status = cont && m.pending ? sendingStatus() : "";
  const tools = opts.tools ? messageTools(m, opts.readOnly) : "";
  const needs = messageNeeds(m, opts.tools && S.thread === m.id ? "in-thread" : opts.cards, opts.ch, opts.threadRoot);
  const text = m.text || status ? `<p class="tx">${richText(m.text || "")}${status}</p>` : "";
  return `<div class="msg${cont ? " cont" : ""}${m.pending ? " is-pending" : ""}" data-mid="${esc(m.id)}">${gutter}<div class="body">${head}
    ${text}${messageAttachmentsHtml(m, opts.ch)}${tools}
    ${askLine(m)}${sourceLine(m)}${threadCopyLine(m)}${threadHitLine(m, opts.q)}${needs}${threadButton(m)}</div></div>`;
}
/* Shared preview (project Overview) renders chips, not cards; it is called as Array.map(msgHtml). */
const msgHtml = (m, i, list) => messageHtml(m, list && i ? list[i - 1] : null);

function emptyConversation(ch) {
  const dm = ch.kind === "dm", name = dm ? M[ch.with].name : chatTitle(ch);
  const text = dm ? `Messages here go only to ${name}.` : ch.topic || "";
  return `<div class="chat-empty">${dm ? av(ch.with, 32) : ic("hash", "ic20")}<h2>This is the start of ${dm ? "your conversation with " : ""}${esc(name)}</h2>${text ? `<p>${esc(text)}</p>` : ""}</div>`;
}
const searchQuery = (id) => (local.search && local.search.id === id ? local.search.q.trim().toLowerCase() : "");
const hit = (m, q) => m.text.toLowerCase().includes(q) || M[m.who].name.toLowerCase().includes(q) || attachmentNameHit(m, q);
/* Indexes of a message's thread replies that match; a match there lists the message with a way into its thread. */
const threadHits = (m, q) => (m.thread || []).flatMap((r, i) => (hit(r, q) ? [i] : []));
const matches = (m, q) => hit(m, q) || threadHits(m, q).length > 0;
/* Posts and thread replies are counted apart, so a post and a reply in its thread never read as "1 message". */
function searchStatus(id, q) {
  if (!q) return "";
  const msgs = CH[id].msgs, posts = msgs.filter((m) => hit(m, q)).length;
  const replies = msgs.reduce((n, m) => n + threadHits(m, q).length, 0);
  if (!posts && !replies) return "No matches";
  return [posts ? plural(posts, "post", "posts") : "", replies ? plural(replies, "thread reply", "thread replies") : ""].filter(Boolean).join(", ");
}
function noMatches(ch, q) {
  return `<div class="chat-empty"><h2>No messages match “${esc(q)}”</h2><p>Search looks at the messages, thread replies and file names in ${esc(chatTitle(ch))}.</p></div>`;
}
function streamHtml(id) {
  const ch = CH[id], q = searchQuery(id);
  if (!ch.msgs.length) return emptyConversation(ch);
  const list = q ? ch.msgs.filter((m) => matches(m, q)) : ch.msgs;
  if (!list.length) return noMatches(ch, q);
  const readOnly = ch.kind === "a2a";
  const opts = { cards: readOnly ? "chip" : "card", tools: true, readOnly, ch: id, q };
  let lastDay = "";
  const newFrom = q ? null : newFromId(id);
  return list.map((m, i) => {
    const [day] = splitTime(m.time);
    /* A static rule marks where the day starts; its label sticks to the stream's top edge until the next day's
       label replaces it, so the first visible message always has a date. */
    const divider = day !== lastDay ? `<div class="chat-day-rule" aria-hidden="true"></div><h2 class="chat-day"><span>${esc(day)}</span></h2>` : "";
    lastDay = day;
    /* Where this visit's unread messages begin. */
    const fresh = m.id === newFrom ? `<p class="chat-new-rule"><span>New messages</span></p>` : "";
    return divider + fresh + messageHtml(m, i && !fresh ? list[i - 1] : null, opts);
  }).join("");
}

/* ---------------------------------------------------------------- composers */
function shortcutHint() {
  const keys = { either: "Ctrl/⌘", control: "Ctrl", command: "⌘" }[S.chatSend] || "Ctrl/⌘";
  return `<span class="sr-only">Enter adds a new line. ${CHAT_SEND_SHORTCUTS[S.chatSend].label} sends.</span><span aria-hidden="true"><kbd>${keys}</kbd> <kbd>Enter</kbd> to send · <kbd>Enter</kbd> new line</span>`;
}
/* While the main composer holds a / command, its submit button names what the command does next. */
/* Phones keep the verb ("Review"); the button's name is always the whole label. */
const COMMAND_SUBMIT = { assign: ["Review", " assignment"], task: ["Create", " task"], ask: ["Send", " question"], files: ["Show", " files"] };
function submitLabel(text, base) {
  const cmd = base === "Send" && parseCommand(text || "");
  return (cmd && !cmd.unknown && COMMAND_SUBMIT[cmd.name]) || [base, ""];
}
const submitInner = ([verb, rest]) => (rest ? `${verb}<span class="chat-long">${rest}</span>` : verb);
function syncSubmitLabel(field) {
  const label = field.form?.querySelector(".chat-submit-label");
  if (!label) return;
  const parts = submitLabel(field.value, "Send");
  label.innerHTML = submitInner(parts);
  if (parts[1]) label.parentElement.setAttribute("aria-label", parts.join(""));
  else label.parentElement.removeAttribute("aria-label");
}
function submitButton(o, shortcut) {
  const parts = submitLabel(o.draft, o.submit), name = parts[1] ? ` aria-label="${esc(parts.join(""))}"` : "";
  return `<button class="btn ${o.button}" type="submit" title="${o.submit} (${shortcut.label})" aria-keyshortcuts="${shortcut.keys}"${name}>${ic("send", "ic20")}<span class="chat-submit-label">${submitInner(parts)}</span></button>`;
}
/* Main and thread composers share markup, the send hint and empty-send validation. */
function composerHtml(o) {
  const shortcut = CHAT_SEND_SHORTCUTS[S.chatSend];
  return `<form class="compose chat-compose${o.cls || ""}" data-form="${o.form}" data-id="${esc(o.id)}"${o.ch ? ` data-ch="${esc(o.ch)}"` : ""}>
    ${o.before || ""}${o.note ? `<p class="chat-compose-note" id="${o.field}-note">${o.note}</p>` : ""}
    ${trayHtml(o.attach)}
    <div class="chat-compose-row">
      ${attachButton(o.field)}
      <label class="sr-only" for="${o.field}">${esc(o.label)}</label>
      <textarea class="field" id="${o.field}" name="message" rows="1" placeholder="${esc(o.label)}" data-placeholder="${esc(o.label)}" aria-describedby="${o.note ? o.field + "-note " : ""}${o.field}-help" autocomplete="off" enterkeyhint="enter" aria-autocomplete="list" aria-haspopup="listbox" aria-expanded="false">
${esc(o.draft || "")}</textarea>
      ${submitButton(o, shortcut)}
    </div>
    <p class="chat-compose-hint" id="${o.field}-help" aria-live="polite">${shortcutHint()}</p>
    ${o.after || ""}
  </form>`;
}
function composeSource(id) {
  const src = dmSources.get(id);
  if (!src || !CH[src.ch]) return "";
  const label = esc(sourceLabel(src));
  return `<p class="chat-compose-src">${ic("eye", "ic14")}<span>Your message will link to <a class="alink chat-ref" href="${esc(sourceHref(src))}">${label}</a></span>
    <button class="btn ghost sm" type="button" data-act="chat-src-remove" data-id="${esc(id)}" aria-label="Don't link ${label}">${ic("x", "ic16")}Remove</button></p>`;
}
/* Why a DM's agent can't run right now (doc-40 §0 item 8): the composer says so beside the field. */
function cannotRun(who) {
  const m = M[who], setup = m.setup, office = setup?.office && OFFICES[setup.office];
  if (m.kind !== "agent" || !setup) return "";
  if (!setup.office) return [`${m.name} can't run right now: ${setup.needs || "not placed in an office"}. Your message waits until ${m.name.split(" ")[0]} is placed.`, `#/settings/agents/${who}`];
  if (m.paused) return [`${m.name} is paused, so it won't answer until it's resumed.`];
  if (office && office.status === "Offline") return [`${m.name} can't run right now: its office, ${office.name}, is offline. Your message waits until it reconnects.`];
  return "";
}
function availabilityNote(ch) {
  const why = ch.kind === "dm" && cannotRun(ch.with);
  if (!why) return "";
  const [text, href] = why;
  return `${ic("info", "ic14")}<span>${esc(text)}${href ? ` <a class="alink chat-ref" href="${esc(href)}">Open Agent setup</a>` : ""}</span>`;
}
const chatComposer = (id, title) => composerHtml({
  form: "send", id, field: "compose", label: "Message " + title, draft: chatDrafts.get(id), button: "primary", submit: "Send", attach: `c:${id}`,
  before: composeSource(id), note: availabilityNote(CH[id]),
});
const threadDraft = (rootId) => threadDrafts.get(rootId) || { text: "", also: false };
const saveThreadDraft = (rootId, patch) => threadDrafts.set(rootId, { ...threadDraft(rootId), ...patch });
/* Cross-posting exists only where a thread hangs off a channel; in a DM it would duplicate the same reader's stream.
   The choice is kept with the draft, so a re-render never changes where the words will go. */
function alsoSendBox(id, rootId) {
  if (CH[id].kind !== "channel") return "";
  return `<label class="chat-also"><input type="checkbox" name="also"${threadDraft(rootId).also ? " checked" : ""}> Also send to ${esc(chatTitle(CH[id]))}</label>`;
}
const threadComposer = (id, root) => composerHtml({
  form: "thread-send", id: root.id, ch: id, cls: " thread-compose", field: "thread-compose", label: "Reply in thread",
  draft: threadDraft(root.id).text, button: "outline", submit: "Reply", attach: `t:${id}:${root.id}`, after: alsoSendBox(id, root.id),
});
function resetHint(form) {
  form.classList.remove("is-confirming");
  const hint = form.querySelector(".chat-compose-hint");
  hint.classList.remove("is-error", "is-confirm");
  hint.innerHTML = shortcutHint();
}
function showEmptySendHint(form, field, text = "Write a message or attach a file first.") {
  const hint = form.querySelector(".chat-compose-hint");
  hint.classList.add("is-error");
  hint.innerHTML = `${ic("info", "ic14")}<span>${esc(text)}</span>`;
  field.focus();
  field.addEventListener("input", () => resetHint(form), { once: true });
}
const streamFor = (id) => document.querySelector(`.chat .chmain .msgs[data-ch="${CSS.escape(id)}"]`);
function confirmSent(id, message) {
  message.pending = false;
  const s = streamFor(id);
  if (s) replaceStream(s, streamHtml(id));
}
/* What a composer would send: its text and ready attachments; null (with the reason shown) when it can't send yet. */
function sendable(form, field) {
  const att = attachmentsForSend(form), text = field.value.trim() ? field.value : "";
  if (att.busy) { showEmptySendHint(form, field, `Wait for ${att.busy === 1 ? "1 file" : `${att.busy} files`} to finish uploading.`); return null; }
  if (!text && !att.files.length) { showEmptySendHint(form, field); return null; }
  clearAttachments(form);
  return att.files.length ? { text, files: att.files } : { text };
}
/* ---------------------------------------------------------------- / commands */
/* The composer's words leave it only when their command succeeded; until then they stay, as the draft. */
function clearComposer(id, field) {
  chatDrafts.delete(id);
  field.value = "";
  fitTextarea(field);
  syncSubmitLabel(field);
}
/* /task what to do: a task whose prompt is those words, with this conversation's latest messages as context (the
   task-context excerpt). The composer keeps "/task …" until the task is created, so cancelling loses nothing. */
function taskFromConversation(id, field, text = "") {
  const ch = CH[id], last = [...ch.msgs].reverse().find((m) => !m.pending);
  const opts = { prompt: text, proj: ch.proj, onCreated: () => clearComposer(id, field) };
  if (last) opts.excerpt = { ch: id, mid: last.id };
  openSmartTaskComposer(field, opts);
}
/* /files opens the Files panel; any words after it stay in the composer. */
function filesCommand(id, field, text = "") {
  if (text) { field.value = text; chatDrafts.set(id, text); fitTextarea(field); syncSubmitLabel(field); } else clearComposer(id, field);
  toggleFiles();
}
function runCommand(name, id, field, text = "") {
  if (name === "task") taskFromConversation(id, field, text);
  else if (name === "files") filesCommand(id, field, text);
}
/* /ask @agent question: posted as the owner's structured question to that agent. */
function postQuestion(form, field, cmd) {
  const id = form.dataset.id;
  if (M[cmd.who]?.kind !== "agent") { showEmptySendHint(form, field, "Use /ask @agent and your question, naming an agent."); return; }
  CH[id].msgs.push({ id: "u" + Date.now(), who: "you", time: clock(), text: cmd.question, ask: { to: cmd.who }, mentions: [cmd.who] });
  markAllRead(id);
  clearComposer(id, field);
  replaceStream(streamFor(id), streamHtml(id), true);
  field.focus();
}
/* /assign @agent T-A…: nothing changes on send. The hint names the change (item, from whom, to whom) and waits for
   an explicit Assign; editing the command or Cancel withdraws it. Assign re-reads the composer first. */
const pendingAssign = { text: "" };
function assignTarget(cmd) {
  const w = W.find((x) => x.id === cmd.item);
  return w && M[cmd.who]?.kind === "agent" ? w : null;
}
function assignItem(form, field, cmd) {
  const w = assignTarget(cmd);
  if (!w) { showEmptySendHint(form, field, "Use /assign @agent T-A…, naming an agent and an existing work item."); return; }
  if (w.who === cmd.who) { showEmptySendHint(form, field, `${w.id} is already assigned to ${M[cmd.who].name}.`); return; }
  pendingAssign.text = field.value;
  const from = w.who && M[w.who] ? ` from ${esc(M[w.who].name)}` : "";
  const hint = form.querySelector(".chat-compose-hint");
  hint.classList.remove("is-error");
  hint.classList.add("is-confirm");
  /* Assign is the one primary action while the confirmation shows; the submit button steps back. */
  form.classList.add("is-confirming");
  hint.innerHTML = `<span class="chat-assign-ask" role="group" aria-label="Confirm assignment">${ic("user-plus", "ic14")}<span>Assign <b>${esc(w.id)}</b> ${esc(w.title)}${from} to <b>${esc(M[cmd.who].name)}</b>?</span>
    <button class="btn primary sm" type="button" data-act="chat-assign-confirm">Assign</button><button class="btn ghost sm" type="button" data-act="chat-assign-cancel">Cancel</button></span>`;
  hint.querySelector('[data-act="chat-assign-confirm"]').focus();
  field.addEventListener("input", () => { pendingAssign.text = ""; resetHint(form); }, { once: true });
}
function cancelAssign(form) {
  pendingAssign.text = "";
  resetHint(form);
  form.elements.message.focus();
}
function confirmAssign(form) {
  const field = form.elements.message, cmd = parseCommand(field.value), w = cmd?.name === "assign" && assignTarget(cmd);
  if (!w || field.value !== pendingAssign.text) { cancelAssign(form); return; }
  pendingAssign.text = "";
  w.who = cmd.who;
  clearComposer(form.dataset.id, field);
  resetHint(form);
  toast(`Assigned ${w.id} to ${M[cmd.who].name}.`, "check-circle");
  field.focus();
}
/* A slash word that is no command: say so, and offer to send the text as it is. */
function showUnknownCommand(form, field, name) {
  showEmptySendHint(form, field, `/${name} isn't a command. Type / to see the commands.`);
  const hint = form.querySelector(".chat-compose-hint");
  hint.insertAdjacentHTML("beforeend", ` <button class="alink chat-ref chat-send-plain" type="button" data-act="chat-send-plain">Send as message</button>`);
}
/* A message starting with a complete "/command" token; true when it was handled (run, or explained). */
function sendCommand(form, field) {
  const cmd = parseCommand(field.value);
  if (!cmd) return false;
  if (cmd.unknown) showUnknownCommand(form, field, cmd.name);
  else if (cmd.invalid) showEmptySendHint(form, field, `Finish the command: /${cmd.name} @agent …`);
  else if (cmd.name === "ask") postQuestion(form, field, cmd);
  else if (cmd.name === "assign") assignItem(form, field, cmd);
  else runCommand(cmd.name, form.dataset.id, field, cmd.text);
  return true;
}
/* Posts the composer's text and ready attachments; `plain` sends slash-led text as an ordinary message. */
function sendChatMessage(form, plain = false) {
  const field = form.elements.message;
  closeAssist();
  if (!plain && sendCommand(form, field)) return;
  const body = sendable(form, field);
  if (!body) return;
  postMessage(form, field, body);
}
function postMessage(form, field, body) {
  const id = form.dataset.id, ch = CH[id];
  const mentions = mentionsIn(body.text || "", id);
  if (mentions.length) body.mentions = mentions;
  /* Prototype: the office confirms after a short delay; until then the message says "Sending…". */
  const message = { id: "u" + Date.now(), who: "you", time: clock(), ...body, pending: true };
  if (dmSources.has(id)) { message.src = dmSources.get(id); dmSources.delete(id); form.querySelector(".chat-compose-src")?.remove(); }
  ch.msgs.push(message);
  markAllRead(id);
  local.search = null;
  document.querySelector(".chat-search-row")?.remove();
  clearComposer(id, field);
  replaceStream(streamFor(id), streamHtml(id), true);
  field.focus();
  setTimeout(() => confirmSent(id, message), 700);
}
function sendThreadReply(form) {
  const field = form.elements.message, body = sendable(form, field);
  if (!body) return false;
  const ch = CH[form.dataset.ch], root = ch.msgs.find((m) => m.id === form.dataset.id);
  closeAssist();
  const mentions = mentionsIn(body.text || "", form.dataset.ch);
  if (mentions.length) body.mentions = mentions;
  /* Like a main send, the reply (and its copy in the channel) says "Sending…" until the office confirms. */
  const reply = { who: "you", time: clock(), ...body, text: body.text.trim(), pending: true }, also = threadDraft(root.id).also;
  (root.thread ||= []).push(reply);
  threadDrafts.delete(root.id);
  followThreadEnd();
  const copy = also && ch.kind === "channel" ? { id: "u" + Date.now(), ...reply, inThread: { id: root.id, who: root.who } } : null;
  if (copy) ch.msgs.push(copy);
  setTimeout(() => confirmThreadSent(form.dataset.ch, root, reply, copy), 700);
  return true;
}
/* Swaps the confirmed reply in place, so the thread composer keeps focus and its next draft. */
function confirmThreadSent(id, root, reply, copy) {
  reply.pending = false;
  const i = root.thread.indexOf(reply), el = document.querySelector(`aside.thread .msgs > [data-mid="${CSS.escape(root.id + "-r" + i)}"]`);
  if (el) el.outerHTML = threadReplyHtml(id, root, i);
  if (copy) confirmSent(id, copy);
}

/* ---------------------------------------------------------------- header */
/* The channel's own admitted members (doc-40 §0 item 4), not its project's or team's roster. */
const channelMembers = (ch) => (ch.members || []).filter((k) => M[k]);
function membersLink(ch) {
  const n = channelMembers(ch).length;
  return n ? `<button class="alink chat-members-btn" type="button" data-act="chat-members" aria-expanded="${local.members}" aria-controls="chat-members">${plural(n, "member", "members")}</button>` : "";
}
/* A compact list of the agent's other requests, each going to its Inbox record. The list is a top-layer popover,
   so the one-line context it sits in never clips it. `slot` keeps ids unique per place. */
function requestsMenu(items, who, slot, label) {
  const id = `chat-more-${slot}`;
  const rows = items.map((it) => `<a role="menuitem" tabindex="-1" href="#/inbox/${esc(it.id)}"><span class="chat-pop-sub">${esc(it.sub)}</span><span class="chat-pop-what">${esc(it.what)}</span></a>`).join("");
  return `<span class="menu-wrap chat-more-wrap"><button class="alink chat-more-btn" type="button" data-act="menu" data-id="${id}" aria-haspopup="menu" aria-expanded="false" aria-controls="menu-${id}">${label}</button><span class="menu chat-pop-menu" id="menu-${id}" role="menu" aria-label="${esc(`Other requests from ${M[who].name}`)}" popover="manual" hidden>${rows}</span></span>`;
}
/* An agent's open requests, split by where they are answered: here (cards in this DM) and elsewhere (Inbox). */
function dmRequests(who, chId) {
  const here = openAsks(CH[chId]);
  return { here, elsewhere: IN.filter((i) => !i.handled && i.from === who && !here.includes(i.id)) };
}
/* A DM whose agent waits on you leads with what can be answered here, linked to its card, then "N in Inbox" opening
   the list of the agent's other requests. The header summary and Details say exactly the same, in the same order. */
function dmWaiting(who, chId, slot, short = false) {
  const { here, elsewhere } = dmRequests(who, chId), parts = [];
  const card = here.length && CH[chId].msgs.find((x) => x.needs === here[0]);
  const hereWord = short ? `${here.length} here` : `${plural(here.length, "request", "requests")} here`;
  if (card) parts.push(`<a class="alink chat-here-link" href="#/chat/${esc(chId)}/${esc(card.id)}"${short ? ` aria-label="${esc(plural(here.length, "request", "requests"))} here"` : ""}>${hereWord}</a>`);
  if (elsewhere.length) parts.push(requestsMenu(elsewhere, who, slot, `${elsewhere.length} in Inbox`));
  return parts.join(" · ");
}
function dmStatus(who, chId, slot) {
  const m = M[who];
  if (m.kind === "person") return esc(m.last || "");
  return presence(who) === "needs" ? dmWaiting(who, chId, slot) : statusLine(who);
}
/* The parts of a conversation's context: a line under the title when wide, the Details panel when narrow. */
function contextParts(ch, chId, slot = "ctx") {
  if (ch.kind === "dm") {
    const status = dmStatus(ch.with, chId, slot);
    return [["Role", esc(M[ch.with].role)], ["Kind", kindWord(ch.with)], ["Status", status ? `<span class="chat-status">${status}</span>` : ""]];
  }
  /* The owner's role leads the context; the footer offers the ways to weigh in. */
  if (ch.kind === "a2a") return [["Access", `<span class="chat-readas">${ic("eye", "ic14")}Reading as the owner</span>`], ["Kind", "Between agents"], ["Agents", ch.pair.map((k) => mlink(k)).join(" and ")]];
  return [
    ["Topic", ch.topic ? `<span class="chat-topic">${esc(ch.topic)}</span>` : ""],
    ["Project", ch.proj ? `<span class="chat-proj">${ic("folder", "ic14")}${plink(ch.proj)}</span>` : ""],
    ["Members", membersLink(ch)],
  ];
}
const headerContext = (ch, chId) => joinSep(contextParts(ch, chId).map(([, v]) => (v ? `<span>${v}</span>` : "")));
/* Plain words for when the full context line would be cut off: no links, so nothing clipped is focusable;
   Details holds the full line with its links. */
/* An agent's summary leads with its status, so the actionable part survives the narrowest header: "Waiting on you · 2"
   (its open requests) needs nothing more; otherwise the status, then the role. Details keeps role and links. */
function dmSummary(who) {
  const m = M[who];
  if (m.kind === "person") return [m.role, m.last];
  const word = PRESENCE_WORD[presence(who)];
  if (presence(who) !== "needs") return [word, m.role];
  const { here, elsewhere } = dmRequests(who, `dm-${who}`);
  if (!here.length && !elsewhere.length) return [word, m.role];
  return [here.length ? plural(here.length, "request", "requests") + " here" : "", elsewhere.length ? `${elsewhere.length} in Inbox` : ""];
}
function summaryParts(ch) {
  if (ch.kind === "dm") return dmSummary(ch.with);
  if (ch.kind === "a2a") return ["Reading as the owner", "Between agents"];
  const n = channelMembers(ch).length;
  return [ch.topic, n ? plural(n, "member", "members") : ""];
}
const headerSummary = (ch) => esc(summaryParts(ch).filter(Boolean).join(" · "));
function detailsPanel(ch, chId) {
  if (!local.details) return "";
  const rows = contextParts(ch, chId, "details").filter(([, v]) => v).map(([k, v]) => `<div><dt>${k}</dt><dd>${v}</dd></div>`).join("");
  return `<dl class="chat-details" id="chat-details" aria-label="Conversation details">${rows}</dl>`;
}
function headerLead(ch) {
  if (ch.kind === "dm") return av(ch.with, 32);
  return ch.kind === "a2a" ? `<span class="chat-pair" aria-hidden="true">${ch.pair.map((k) => av(k, 24)).join("")}</span>` : "";
}
function headerActions(id, title) {
  const open = local.search?.id === id;
  return `<div class="ph-actions chat-actions">
      ${filesButton(local.files, title)}
      <button class="btn ghost sm chat-details-btn" type="button" data-act="chat-details" aria-expanded="${local.details}" aria-controls="chat-details" title="Details">${ic("info", "ic16")}<span class="chat-details-word">Details</span></button>
      <button class="btn ghost icon chat-search" type="button" data-act="chat-search" aria-expanded="${open}" aria-controls="chat-search-row" aria-label="Search in ${esc(title)}" title="Search in ${esc(title)}">${ic("search", "ic20")}</button>
    </div>`;
}
/* Asks waiting in other conversations, each counted once; unread elsewhere shows as a dot. */
function listButton(id) {
  const here = new Set(CH[id] ? openAsks(CH[id]) : []);
  const elsewhere = new Set(Object.keys(CH).filter((k) => k !== id).flatMap((k) => openAsks(CH[k])).filter((x) => !here.has(x))).size;
  const unread = Object.keys(CH).some((k) => k !== id && CH[k].unread);
  /* The name always begins with the visible word, whatever the width hides; the badge and dot are described in it. */
  const name = ["Conversations", elsewhere ? plural(elsewhere, "request", "requests") + " waiting on you in other conversations" : "", unread ? "unread messages" : ""].filter(Boolean).join(", ");
  const badge = elsewhere ? `<span aria-hidden="true">${needsCount(elsewhere)}</span>` : "";
  return `<button class="btn ghost chat-list-btn" type="button" data-act="chat-list" data-id="${id}" aria-expanded="${S.chatList === id}" aria-controls="chat-list" aria-label="${esc(name)}" title="Conversations">${ic("chat", "ic20")}<span class="chat-list-label">Conversations</span>${badge}${unread ? unreadDot : ""}</button>`;
}
/* Below 640 px the context line moves into Details; an agents' conversation keeps its access label by the title. */
const readAsTag = (ch) => (ch.kind === "a2a" ? `<span class="chat-readas chat-readas-tag">${ic("eye", "ic14")}Reading as the owner</span>` : "");
/* Below 640 px a DM whose agent waits on you keeps a compact request summary by the title ("1 here · 2 in Inbox"),
   with the same links as the context line; role and project stay in Details. */
function requestsTag(ch, chId) {
  if (ch.kind !== "dm" || M[ch.with].kind !== "agent" || presence(ch.with) !== "needs") return "";
  const parts = dmWaiting(ch.with, chId, "tag", true);
  return parts ? `<span class="chat-req-tag">${ic("bell", "ic14")}<span>${parts}</span></span>` : "";
}
function chatHeaderHtml(id, ch, title) {
  return `<header class="ph chhead">
    ${listButton(id)}
    <div class="ph-t chat-title">
      ${headerLead(ch)}
      <div class="chat-heading"><h1 id="ch-title" tabindex="-1">${esc(title)}</h1>
        <p class="chat-context"><span class="chat-ctx-full">${headerContext(ch, id)}</span><span class="chat-ctx-summary">${headerSummary(ch)}</span></p>
      </div>
      ${readAsTag(ch)}${requestsTag(ch, id)}
    </div>
    ${headerActions(id, title)}
    ${detailsPanel(ch, id)}
  </header>`;
}
function searchRowHtml(id, title) {
  if (local.search?.id !== id) return "";
  const status = searchStatus(id, local.search.q.trim().toLowerCase());
  return `<div class="chat-search-row" id="chat-search-row" role="search">
    ${ic("search", "ic16")}<label class="sr-only" for="chat-search-input">Search messages in ${esc(title)}</label>
    <input class="field" id="chat-search-input" type="search" placeholder="Search messages in ${esc(title)}" value="${esc(local.search.q)}" autocomplete="off" aria-describedby="chat-search-status">
    <span class="chat-search-status" id="chat-search-status" role="status">${status}</span>
    <button class="btn ghost icon" type="button" data-act="chat-search" aria-label="Close search">${ic("x", "ic20")}</button>
  </div>`;
}

/* ---------------------------------------------------------------- side panels (thread, members) and read-only footer */
/* Where a side panel covers the conversation it is a modal dialog with a scrim (made inert-outside on mount). */
const panelModal = () => (matchesMedia(OVERLAY_THREAD) ? ' role="dialog" aria-modal="true"' : "");
/* A thread reply; a chip for the ask on the thread's own root points up to its card. */
function threadReplyHtml(id, root, i) {
  const replies = root.thread, threadRoot = CH[id].kind === "a2a" ? null : root;
  return messageHtml(Object.assign({ id: root.id + "-r" + i }, replies[i]), i ? replies[i - 1] : null, { ch: id, threadRoot });
}
function chatThreadHtml(id, root, title) {
  if (!root) return "";
  const replies = root.thread || [], readOnly = CH[id].kind === "a2a";
  const count = replies.length ? plural(replies.length, "reply", "replies") : "No replies yet";
  const list = replies.map((_r, i) => threadReplyHtml(id, root, i)).join("");
  /* A writable thread carries the ask's own card, answerable here; the stream points to it while the thread is open. */
  const head = markRoot(messageHtml(Object.assign({}, root, { thread: null }), null, { cards: readOnly ? "chip" : "thread-card", ch: id }));
  return `<div class="thread-scrim" data-act="thread-close" data-scrim aria-hidden="true"></div>
    <aside class="thread${rootOpen(root.id) ? " root-open" : ""}"${panelModal()} aria-labelledby="chat-thread-title"><header class="side-panel-header"><div class="thread-title"><h2 id="chat-thread-title" tabindex="-1">Thread</h2><span class="muted">${esc(title)}</span></div><button class="btn ghost icon" type="button" data-act="thread-close" aria-label="Close thread">${ic("x", "ic20")}</button></header>
      <div class="msgs">${threadSummaryHtml(root)}${head}<p class="chat-thread-count">${count}</p>${list}</div>
      ${readOnly ? "" : threadComposer(id, root)}</aside>`;
}
function memberRow(k) {
  const m = M[k], name = m.owner ? "You" : mlink(k);
  const dm = m.owner ? "" : `<a class="btn ghost sm" href="#/chat/dm-${esc(k)}" aria-label="Message ${esc(m.name)}">${ic("chat", "ic16")}Message</a>`;
  return `<li class="chat-member">${av(k, 32)}<span class="chat-member-who"><b>${name}</b><span>${m.owner ? "Owner" : `${esc(m.role)} · ${kindWord(k)}`}</span></span>${dm}</li>`;
}
function membersPanelHtml(ch, title) {
  if (!local.members || ch.kind !== "channel") return "";
  const ids = channelMembers(ch);
  return `<div class="thread-scrim" data-act="chat-members" data-scrim aria-hidden="true"></div>
    <aside class="thread chat-members-panel" id="chat-members"${panelModal()} aria-labelledby="chat-members-title"><header class="side-panel-header"><div class="thread-title"><h2 id="chat-members-title" tabindex="-1">Members</h2><span class="muted">${esc(title)} · ${ids.length}</span></div><button class="btn ghost icon" type="button" data-act="chat-members" aria-label="Close members">${ic("x", "ic20")}</button></header>
      <ul class="chat-member-list">${ids.map(memberRow).join("")}</ul></aside>`;
}
function readOnlyFoot(id, ch) {
  const buttons = ch.pair.map((k) => `<button class="btn outline sm" type="button" data-act="chat-dm" data-id="${k}" data-v="${esc(id)}">${ic("chat", "ic16")}DM ${esc(M[k].name)}</button>`).join("");
  return `<div class="readfoot"><p>${ic("eye", "ic16")}You can't reply here. To weigh in, message ${ch.pair.map((k) => esc(M[k].name)).join(" or ")}.</p><div class="actions">${buttons}</div></div>`;
}
const askJump = () => `<div class="chat-asks-jump"><button class="btn sm" type="button" data-act="chat-asks-jump" hidden>${ic("bell", "ic16")}<span></span>${ic("chev-down", "ic16 chat-up")}</button></div>`;
const latestJump = () => `<div class="chat-latest-jump"><button class="btn sm" type="button" data-act="chat-latest-jump" aria-label="Jump to latest" hidden>${ic("chev-down", "ic16")}<span><span class="chat-jump-long">Jump to latest</span><span class="chat-jump-short">Latest</span></span></button></div>`;

/* ---------------------------------------------------------------- page */
/* A source link (#/chat/<ch>/<msg>) opens the unfiltered stream on that exact message, once per navigation: every
   hash change, and a click on a link to the URL already open, take it again; in-place re-renders do not. */
/* A thread reply's link (#/chat/<ch>/<root>-r<n>) lands as search's "Open in thread" does: the stream on the root,
   the thread open on that exact reply, marked and focused. */
function replyTarget(id, target) {
  const r = /^(.*)-r(\d+)$/.exec(target), root = r && CH[id].msgs.find((m) => m.id === r[1]);
  return root?.thread?.[Number(r[2])] ? { root: root.id, reply: r[2] } : null;
}
function takeLink(id) {
  const target = route()[2];
  if (!target || local.linked === location.hash) return;
  const reply = !CH[id].msgs.some((m) => m.id === target) && replyTarget(id, target);
  if (reply) local.openThread = { ch: id, id: reply.root, reply: reply.reply };
  else if (!CH[id].msgs.some((m) => m.id === target)) return;
  Object.assign(local, { linked: location.hash, pendingLink: reply ? reply.root : target, search: null });
}
/* "Open thread" from the Threads view opens the conversation with that thread. */
function takeThread(id) {
  if (local.openThread?.ch !== id) return;
  S.thread = local.openThread.id;
  local.threadFrom = "";
  const reply = local.openThread.reply;
  if (reply !== undefined) local.threadLand = { reply: Number(reply) };
}
function chatFrame(id, cls, header, main, side = "") {
  const overlay = listIsOverlay(), listOpen = overlay && S.chatList === id;
  return `<div class="${["chat", ...cls, overlay ? "list-overlay" : "", listOpen ? "list-open" : ""].filter(Boolean).join(" ")}" data-ch="${esc(id)}"${panelWidthStyle()}>
    ${header}
    ${chatListHtml(id, listOpen)}
    <div class="chat-scrim" data-act="chat-list" data-id="${id}" data-scrim aria-hidden="true"></div>
    <section class="chmain" aria-labelledby="ch-title">${main}</section>
    ${side}
  </div>`;
}
function pageChatView(kind) {
  if (S.chatList && S.chatList !== kind) S.chatList = null;
  const view = CHAT_VIEWS[kind];
  const header = `<header class="ph chhead">${listButton(kind)}
    <div class="ph-t chat-title"><div class="chat-heading"><h1 id="ch-title" tabindex="-1">${esc(view.title)}</h1><p class="chat-context">${esc(view.about)}</p></div></div></header>`;
  return chatFrame(kind, [], header, chatViewHtml(kind));
}
function pageChat(id) {
  if (isChatView(id)) return pageChatView(id);
  id = resolveChatId(id);
  const ch = CH[id];
  startVisit(id);
  if (S.chatList && S.chatList !== id) S.chatList = null;
  takeLink(id);
  takeThread(id);
  if (S.thread) Object.assign(local, { members: false, files: false });
  const root = S.thread && ch.msgs.find((m) => m.id === S.thread);
  const title = chatTitle(ch), members = local.members && ch.kind === "channel";
  const cls = [root || members || local.files ? "withthread" : "", local.details ? "details-open" : ""];
  const main = `${searchRowHtml(id, title)}${askJump()}
      <div class="msgs" aria-label="Messages" data-ch="${esc(id)}">${streamHtml(id)}</div>${latestJump()}
      ${ch.kind === "a2a" ? readOnlyFoot(id, ch) : chatComposer(id, title)}`;
  return chatFrame(id, cls, chatHeaderHtml(id, ch, title), main, chatThreadHtml(id, root, title) + membersPanelHtml(ch, title)
    + filesPanelHtml(ch, id, title, { open: local.files, modal: panelModal() }));
}

/* ---------------------------------------------------------------- after render: position, overlays, local controls */
let inerted = [];
function releaseInert() {
  inerted.forEach((el) => { el.inert = false; });
  inerted = [];
}
/* A modal overlay (narrow conversation list, overlay side panel): everything outside it but scrims is inert. */
function inertOutside(el) {
  for (let n = el; n.parentElement && n !== document.body; n = n.parentElement) {
    for (const sib of n.parentElement.children) {
      if (sib === n || sib.inert || sib.hasAttribute("data-scrim") || sib.tagName === "SCRIPT") continue;
      sib.inert = true;
      inerted.push(sib);
    }
  }
}
/* With no saved position the stream opens at its end, newest message fully visible (the sticky day label dates
   the top); phones open on a message boundary so the first line is never the tail of a sentence. */
const NARROW_STREAM = "(max-width: 639px)";
const initialPos = (id) => (local.pendingLink ? { target: local.pendingLink } : savedPos(id) || (matchesMedia(NARROW_STREAM) ? { boundary: true } : { end: true }));
function markLinked(s) {
  const target = route()[2], el = target && s.querySelector(`:scope > [data-mid="${CSS.escape(target)}"]`);
  el?.classList.add("is-linked");
}
/* The linked message takes focus (after the router focused the heading), so keyboard and screen-reader users land on it.
   A message whose ask is answered on its card here focuses the card's question instead (its Inbox link stays on the card). */
function focusLinked(s) {
  const el = local.pendingLink && s.querySelector(`:scope > [data-mid="${CSS.escape(local.pendingLink)}"]`);
  const m = el && CH[s.dataset.ch].msgs.find((x) => x.id === local.pendingLink);
  local.pendingLink = null;
  if (!el) return;
  const card = answerableHere(m) && el.querySelector(".chat-needs-q");
  if (card) { card.focus({ preventScroll: true }); return; }
  el.tabIndex = -1;
  el.setAttribute("aria-label", `Linked message from ${el.querySelector(".hd b")?.textContent || "this conversation"}`);
  el.focus({ preventScroll: true });
}
function highlightMatches(s, q) {
  if (!q) return;
  const walker = document.createTreeWalker(s, NodeFilter.SHOW_TEXT, { acceptNode: (n) => (n.parentElement.closest(".tx, .hd b, .chat-hit-quote, .chat-att-name") ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT) });
  const nodes = [];
  while (walker.nextNode()) nodes.push(walker.currentNode);
  for (const node of nodes) {
    const i = node.data.toLowerCase().indexOf(q);
    if (i < 0) continue;
    const hit = node.splitText(i);
    hit.splitText(q.length);
    const mark = document.createElement("mark");
    hit.replaceWith(mark);
    mark.append(hit);
  }
}
function refreshSearch(s) {
  const id = s.dataset.ch, q = searchQuery(id);
  s.innerHTML = streamHtml(id);
  highlightMatches(s, q);
  applyPos(s, q ? { top: true } : { end: true });
  const status = document.getElementById("chat-search-status");
  if (status) status.textContent = searchStatus(id, q);
  watchUnread(s);
}
function toggleSearch(id) {
  local.search = local.search?.id === id ? null : { id, q: "" };
  render(false);
  (local.search ? document.getElementById("chat-search-input") : document.querySelector(".chat-search"))?.focus();
}
function turnIntoTask(t, id, mid) {
  const ch = CH[id], m = ch.msgs.find((x) => x.id === mid);
  if (!m) return;
  const prompt = `${m.text}\n\nFrom ${M[m.who].name} in ${chatTitle(ch)} (${m.time})`;
  /* The task starts in the channel's project (e.g. #platform -> Office federation). The message and the ones
     before it go along as context, with their ids; the prompt carries no raw link. */
  openSmartTaskComposer(t, { prompt, proj: ch.proj, excerpt: { ch: id, mid } });
}

/* ---------------------------------------------------------------- overlays: open, close, focus return, Back */
const visible = (el) => el && el.getClientRects().length > 0;
const focusFirst = (selectors) => selectors.filter(Boolean).map((sel) => document.querySelector(sel)).find(visible)?.focus();
/* Threads: focus returns to the control that opened it, or the visible one for that message. */
function threadOpener(t, id) {
  const menu = t.closest(".menu"), e = CSS.escape(id);
  if (menu) return `[aria-controls="${menu.id}"]`;
  if (t.dataset.hit !== undefined) return `.chmain .chat-thread-hit [data-id="${e}"]`;
  return t.classList.contains("thr") ? `.chmain .thr[data-id="${e}"]` : `.chmain .msg-tool[data-act="thread"][data-id="${e}"]`;
}
/* Where an opening thread lands: a search match's reply, the replies (from the reply count), or where it was left. */
function threadLanding(t) {
  if (t?.dataset.hit !== undefined) return { reply: Number(t.dataset.hit) };
  return t?.classList.contains("thr") ? { replies: true } : null;
}
/* Focus follows the landing: the matching reply, otherwise the thread's title. */
function focusThreadLanding() {
  const reply = document.querySelector(".thread .msgs > .msg.is-linked");
  if (!reply) { document.getElementById("chat-thread-title")?.focus(); return; }
  reply.tabIndex = -1;
  reply.setAttribute("aria-label", `${local.search ? "Matching" : "Linked"} reply from ${reply.querySelector(".hd b")?.textContent || reply.querySelector(".sr-only")?.textContent || "the thread"}`);
  reply.focus({ preventScroll: true });
}
function focusThreadOpener(id) {
  const e = CSS.escape(id);
  focusFirst([local.threadFrom, `.chmain .thr[data-id="${e}"]`, `[aria-controls="menu-msg-${e}"]`, `.chmain .msg-tool[data-act="thread"][data-id="${e}"]`, `.chmain [data-mid="${e}"] .chat-needs-chip button`]);
}
function toggleThread(id, t) {
  /* A search match always opens (or re-places) its thread; the other controls toggle it. */
  const closing = S.thread === id && t?.dataset.hit === undefined;
  if (!closing) Object.assign(local, { threadFrom: threadOpener(t, id), threadLand: threadLanding(t) });
  S.thread = closing ? null : id;
  render(false);
  if (closing) focusThreadOpener(id);
  else focusThreadLanding();
  syncOverlayHistory();
}
function closeThread() {
  const id = S.thread;
  if (!id) return;
  S.thread = null;
  render(false);
  focusThreadOpener(id);
  syncOverlayHistory();
}
function toggleMembers() {
  local.members = !local.members;
  if (local.members) { S.thread = null; local.files = false; }
  render(false);
  (local.members ? document.getElementById("chat-members-title") : null)?.focus();
  if (!local.members) focusFirst([".chat-context .chat-members-btn", ".chat-details .chat-members-btn", ".chat-details-btn"]);
  syncOverlayHistory();
}
/* Files: every attachment in the conversation, as a side panel like Members. */
function toggleFiles() {
  local.files = !local.files;
  if (local.files) { S.thread = null; local.members = false; }
  render(false);
  document.querySelector(local.files ? "#chat-files-title" : ".chat-files-btn")?.focus();
  syncOverlayHistory();
}
function toggleDetails() {
  local.details = !local.details;
  render(false);
  document.querySelector(".chat-details-btn")?.focus();
  syncOverlayHistory();
}
/* The selected row of the inline list, or of the open drawer. */
const focusSelectedRow = () => document.querySelector('.chlist [aria-current="page"]')?.focus();
/* Conversation list below 768 px of content: an overlay opened from the header (also the "chat-list" app action).
   Where the list is inline, the action moves focus to its selected row. */
function toggleChatList(id) {
  if (!listIsOverlay()) { S.chatList = null; focusSelectedRow(); return; }
  const open = S.chatList !== id;
  S.chatList = open ? id : null;
  render(false);
  if (open) focusSelectedRow(); else document.querySelector(".chat-list-btn")?.focus();
  syncOverlayHistory();
}
/* Crossing the inline-list width. Widening with the drawer open closes it cleanly: no modal state, no inert
   conversation, no history entry left behind, and focus on the selected inline row. Narrowing moves focus that
   was in the (now hidden) list to the Conversations button. */
function closeDrawerInline() {
  S.chatList = null;
  focusSelectedRow();
  syncOverlayHistory();
}
function onListModeCross(root, overlay) {
  if (!overlay && S.chatList) {
    S.chatList = null;
    render(false);
    closeDrawerInline();
    return;
  }
  const inList = root.querySelector(".chlist")?.contains(document.activeElement);
  root.classList.toggle("list-overlay", overlay);
  if (overlay && inList) root.querySelector(".chat-list-btn")?.focus();
}
const anyOverlay = () => route()[0] === "chat" && Boolean(viewerOpen() || S.thread || local.members || local.files || S.chatList || local.details);
/* Back closes the topmost overlay first: the image viewer, a side panel, then the drawer, then Details. */
function closeTopOverlay() {
  if (viewerOpen()) closeViewer();
  else if (S.thread) closeThread();
  else if (local.members) toggleMembers();
  else if (local.files) toggleFiles();
  else if (S.chatList) toggleChatList(S.chatList);
  else if (local.details) toggleDetails();
}
bindOverlays(anyOverlay, closeTopOverlay);
/* The stream's "answer in the thread" chip moves focus to the card in the open thread. */
function focusThreadCard() {
  const aside = document.querySelector("aside.thread .chat-thread-root")?.closest("aside");
  if (aside && !aside.classList.contains("root-open")) setRootOpen(S.thread, true);
  document.querySelector(".thread .chat-needs-q, .thread .chat-needs-done")?.focus();
}
function openViewThread(t) {
  local.openThread = { ch: t.dataset.ch, id: t.dataset.id, reply: t.dataset.hit };
  location.hash = `#/chat/${t.dataset.ch}`;
}

const CLICKS = {
  ...ASK_CLICKS,
  ...ATTACH_CLICKS,
  ...VIEW_CLICKS,
  ...THREAD_SUM_CLICKS,
  "chat-files": () => toggleFiles(),
  "chat-file-open": () => { local.files = false; },
  thread: (id, t) => toggleThread(id, t),
  "thread-close": () => closeThread(),
  "chat-members": () => toggleMembers(),
  "chat-thread-card": () => focusThreadCard(),
  "chat-open-thread": (_id, t) => openViewThread(t),
  "chat-search": (_id, _t, chId) => toggleSearch(chId),
  "chat-details": () => toggleDetails(),
  "chat-task": (id, t, chId) => turnIntoTask(t, chId, id),
  "chat-asks-jump": () => jumpToAsk(),
  "chat-send-plain": (_id, t) => sendChatMessage(t.closest("form"), true),
  "chat-assign-confirm": (_id, t) => confirmAssign(t.closest("form")),
  "chat-assign-cancel": (_id, t) => cancelAssign(t.closest("form")),
  "chat-latest-jump": () => jumpToLatest(),
  "chat-dm-msg": (id, _t, chId) => {
    const m = CH[chId].msgs.find((x) => x.id === id);
    if (!m) return;
    local.focusCompose = true;
    location.hash = openDirectMessage(m.who, { ch: chId, msg: id });
  },
  /* The footer's DM buttons navigate in the shell's handler; the DM opens with its composer focused. */
  "chat-dm": () => { local.focusCompose = true; },
  "chat-src-remove": (id) => {
    dmSources.delete(id);
    render(false);
    document.getElementById("compose")?.focus();
  },
};
/* The conversation already open was chosen (from the list, or New message to the open DM): no navigation
   happens, so close the picker and drawer here. The picker's composer takes focus on mount; a list row's
   choice focuses the conversation heading. */
function stayInConversation(fromRow) {
  S.chatList = null;
  render(false);
  if (fromRow) document.getElementById("ch-title")?.focus();
  syncOverlayHistory();
}
function onChatClick(e) {
  if (e.target.id === "compose" || e.target.id === "thread-compose") recheckAssist();
  const listed = onListClick(e, e.currentTarget);
  if (listed === "handled") return;
  if (listed === "same-conversation") { stayInConversation(Boolean(e.target.closest("#chat-nav a"))); return; }
  const t = e.target.closest("[data-act]"), fn = t && CLICKS[t.dataset.act];
  if (fn) fn(t.dataset.id, t, e.currentTarget.dataset.ch);
}
function onChatInput(e) {
  const t = e.target;
  if (t.id === "compose" || t.id === "thread-compose") updateAssist(t, t.form.dataset.ch || t.form.dataset.id, t.id === "compose");
  if (t.id === "compose") syncSubmitLabel(t);
  if (t.id === "thread-compose") { saveThreadDraft(t.form.dataset.id, { text: t.value }); fitTextarea(t); }
  else if (t.name === "also" && t.form?.dataset.form === "thread-send") saveThreadDraft(t.form.dataset.id, { also: t.checked });
  else if (t.id.startsWith("ar-")) saveAskDraft(t);
  else if (t.id === "chat-search-input" && local.search) { local.search.q = t.value; refreshSearch(document.querySelector(".chat .chmain .msgs[data-ch]")); }
  else onListInput(e, e.currentTarget);
}
function onChatKeydown(e) {
  const t = e.target;
  if (onAssistKeydown(e)) return;
  if (t.id === "chat-search-input" && e.key === "Escape") { e.preventDefault(); e.stopPropagation(); toggleSearch(local.search.id); return; }
  if (e.key === "Escape" && t.closest(".chat-assign-ask")) { e.preventDefault(); e.stopPropagation(); cancelAssign(t.closest("form")); return; }
  if (e.key === "Escape" && t.closest(".chat-members-panel")) { e.preventDefault(); e.stopPropagation(); toggleMembers(); return; }
  if (e.key === "Escape" && t.closest(".chat-files-panel")) { e.preventDefault(); e.stopPropagation(); toggleFiles(); return; }
  onListKeydown(e, e.currentTarget);
  onAskKeydown(e);
}
function mountOverlays(root) {
  if (root.classList.contains("list-open")) inertOutside(root.querySelector(".chlist"));
  const panel = root.querySelector('.thread[aria-modal="true"]');
  if (panel) inertOutside(panel);
}
/* When the full context line under the title would be cut off, the header shows a plain summary instead and offers
   Details with the full line (always below 640 px, by CSS). The full line stays laid out, hidden, for measuring.
   Swapping the lines resizes the observed element, so the swap waits for the next frame: changing it inside the
   observer callback raised "ResizeObserver loop completed with undelivered notifications" while the app nav
   animated (the Bun dev overlay showed it at 1024 px). */
function watchContext(root) {
  const ctx = root.querySelector(".chhead .chat-context"), full = ctx?.querySelector(".chat-ctx-full");
  if (!full || !root.querySelector(".chat-details-btn")) return;
  const measure = () => root.classList.toggle("ctx-overflow", full.scrollWidth > full.clientWidth + 1);
  let frame = 0;
  const ro = new ResizeObserver(() => {
    if (!ctx.isConnected) { ro.disconnect(); return; }
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(measure);
  });
  ro.observe(ctx);
  measure();
}
/* The open thread takes its landing once; while searching, matches in its replies are marked as in the stream. */
function mountThread(root) {
  const panel = S.thread && root.querySelector("aside.thread");
  if (!panel) return;
  const q = searchQuery(root.dataset.ch), land = local.threadLand;
  local.threadLand = null;
  if (q) highlightMatches(panel.querySelector(":scope > .msgs"), q);
  placeThread(panel, S.thread, land);
}
function mountStream(root) {
  const s = root.querySelector(".chmain .msgs[data-ch]");
  if (!s) return;
  const id = s.dataset.ch, q = searchQuery(id);
  highlightMatches(s, q);
  markLinked(s);
  placeStream(s, q ? { top: true } : initialPos(id));
  trackPos(s);
  watchAsks(s);
  watchUnread(s);
  focusLinked(s);
}
/* After a navigation started here (Mentions' "Open in thread", "Message … about this", a footer DM button). */
function takeArrivalFocus() {
  if (local.openThread) {
    local.openThread = null;
    focusThreadLanding();
    syncOverlayHistory();
  }
  if (local.focusCompose) {
    local.focusCompose = false;
    document.getElementById("compose")?.focus();
  }
}
/* Phones keep the composer to one line: a long placeholder ("Message #agentic-org-v1") shortens to "Message"; the
   field's label keeps the full name. */
const SHORT_PLACEHOLDER = 18;
function syncPlaceholders() {
  const narrow = matchesMedia(NARROW_STREAM);
  for (const f of document.querySelectorAll("#main .chat textarea[data-placeholder]")) {
    const full = f.dataset.placeholder;
    f.placeholder = narrow && full.length > SHORT_PLACEHOLDER ? "Message" : full;
  }
}
window.matchMedia?.(NARROW_STREAM).addEventListener?.("change", syncPlaceholders);
function mountChat() {
  releaseInert();
  const root = document.querySelector("#main .chat");
  if (!root) return;
  root.addEventListener("click", onChatClick);
  root.addEventListener("input", onChatInput);
  root.addEventListener("keydown", onChatKeydown);
  root.addEventListener("focusout", (e) => { if (e.target.id === "compose" || e.target.id === "thread-compose") closeAssist(); });
  root.addEventListener("chat-command", (e) => runCommand(e.detail.name, root.dataset.ch, e.target, e.detail.text));
  bindAttachments(root);
  syncPlaceholders();
  mountStream(root);
  mountThread(root);
  fitTextarea(document.getElementById("thread-compose"));
  mountOverlays(root);
  bindPanelResize(root);
  watchContext(root);
  watchListMode(root, onListModeCross);
  /* A render at inline width (the shell re-renders at its own breakpoints) closes a drawer left open. */
  if (S.chatList && !root.classList.contains("list-overlay")) closeDrawerInline();
  takeListFocus();
  takeArrivalFocus();
}

/* "#" and the New message command open the list (as the drawer below 960 px) on its search or recipient picker. */
function openChatList(mode) {
  const here = route()[0] === "chat", r1 = route()[1], id = here ? (isChatView(r1) ? r1 : resolveChatId(r1)) : DEFAULT_CHAT;
  prepareList(mode);
  if (listIsOverlay()) S.chatList = id;
  if (here) { render(false); syncOverlayHistory(); }
  else location.hash = "#/chat/" + id;
}
const findConversation = () => openChatList("find");
const startNewMessage = () => openChatList("new");

/* Leaving Chat (or the conversation) always releases modal overlays' inert background. */
window.addEventListener("hashchange", () => {
  releaseInert();
  resetAsks();
  if (route()[0] !== "chat") endVisit();
  Object.assign(local, { details: false, members: false, files: false });
  /* A link is taken once per arrival at its URL; the router may already have taken this one (it can render before
     this listener runs), and a later in-place re-render must not take it again (a closed thread would reopen). */
  if (local.linked !== location.hash) local.linked = null;
});
/* A link to the URL already open causes no hash change (the shell re-renders in place): capture phase, before it. */
document.addEventListener("click", (e) => {
  const a = e.target.closest?.('a[href^="#/chat/"]');
  if (a && a.getAttribute("href") === location.hash) local.linked = null;
}, true);
/* Crossing 1280 px with a side panel open: the panel's mode follows at once, in place (no re-render, so drafts,
   scroll and focus stay). Where it covers the conversation it is a modal dialog with an inert background, no resize
   edge, and focus inside it; inline it is a plain region with its resize edge. */
function setPanelModal(panel, modal) {
  if (modal) { panel.setAttribute("role", "dialog"); panel.setAttribute("aria-modal", "true"); }
  else { panel.removeAttribute("role"); panel.removeAttribute("aria-modal"); }
}
function syncPanelMode() {
  const root = route()[0] === "chat" && document.querySelector("#main .chat"), panel = root && root.querySelector(":scope > aside.thread");
  if (!panel) return;
  const modal = matchesMedia(OVERLAY_THREAD);
  setPanelModal(panel, modal);
  releaseInert();
  mountOverlays(root);
  panel.querySelector(":scope > .chat-panel-resize")?.remove();
  bindPanelResize(root);
  if (modal && !panel.contains(document.activeElement)) panel.querySelector("h2[tabindex]")?.focus({ preventScroll: true });
}
window.matchMedia?.(OVERLAY_THREAD).addEventListener?.("change", syncPanelMode);

export {
  chatDocTitle,
  chatDrafts,
  findConversation,
  msgHtml,
  openDirectMessage,
  mountChat,
  pageChat,
  sendChatMessage,
  sendThreadReply,
  startNewMessage,
  toggleChatList,
};
