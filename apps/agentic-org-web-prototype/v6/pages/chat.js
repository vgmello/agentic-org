import { CH, M } from "../data.js";
import { fitTextarea } from "./work.js";
import { CHAT_SEND_SHORTCUTS, S } from "../state.js";
import { av, esc, ic, kindWord, mlink, pill, plink, presInline } from "../ui.js";

function msgHtml(m) {
  const ans = m.ask && S.answered[m.ask.id];
  return `<div class="msg">${av(m.who, 32, "tip")}<div class="body"><p class="hd"><b>${mlink(m.who)}</b><span>${M[m.who].kind === "agent" ? "Agent · " : ""}${esc(m.time)}</span></p><p class="tx">${esc(m.text)}</p>
    ${m.ask ? `<div class="ask" role="group" aria-label="Ask for you from ${esc(M[m.who].name)}"><p>${ans ? pill("Answered", "gray", "check-circle") : pill("Needs you", "amber", "bell")}</p><p class="q">${esc(m.ask.q)}</p>
      ${ans ? `<p class="muted">You answered: ${esc(ans)}</p>` : `<div class="actions">${m.ask.opts.map((o, i) => `<button class="btn ${i ? "outline" : "primary"} sm" type="button" data-act="answer" data-id="${m.ask.id}" data-inbox="${m.ask.inbox}" data-v="${esc(o)}">${esc(o)}</button>`).join("")}</div>`}</div>` : ""}
    ${m.thread ? `<button class="thr" type="button" data-act="thread" data-id="${m.id}" aria-expanded="${S.thread === m.id}">${ic("thread", "ic16")}${m.thread.length} replies</button>` : ""}</div></div>`;
}
const chatDrafts = new Map();
function chatComposer(id, title) {
  const shortcut = CHAT_SEND_SHORTCUTS[S.chatSend];
  return `<form class="compose" data-form="send" data-id="${id}">
    <label class="sr-only" for="compose">Message ${esc(title)}</label>
    <p class="sr-only" id="chat-compose-help">Enter adds a new line. ${shortcut.label} sends.</p>
    <textarea class="field" id="compose" name="message" rows="1" placeholder="Message ${esc(title)}" aria-describedby="chat-compose-help" autocomplete="off" enterkeyhint="enter">
${esc(chatDrafts.get(id) || "")}</textarea>
    <button class="btn primary" type="submit" title="Send (${shortcut.label})" aria-keyshortcuts="${shortcut.keys}">${ic("send", "ic20")}Send</button>
  </form>`;
}
function sendChatMessage(form) {
  const field = form.elements.message;
  const text = field.value;
  if (!text.trim()) return;
  const id = form.dataset.id;
  const message = { id: "u" + Date.now(), who: "you", time: "now", text };
  CH[id].msgs.push(message);
  chatDrafts.delete(id);
  field.value = "";
  fitTextarea(field);
  const messages = form.previousElementSibling;
  messages.insertAdjacentHTML("beforeend", msgHtml(message));
  messages.scrollTop = messages.scrollHeight;
  field.focus();
}
function chatLinkHtml(k, activeId) {
  const c = CH[k];
  const label = c.kind === "channel" ? `${ic("hash", "ic16")}${esc(c.name)}` : c.kind === "dm" ? `${av(c.with, 24, "tip")}${esc(c.name)}` : `${ic("eye", "ic16")}${esc(c.name)}`;
  return `<li><a href="#/chat/${k}" ${k === activeId ? 'aria-current="page"' : ""}>${label}</a></li>`;
}
function chatHeaderHtml(ch, title) {
  const sub = ch.kind === "dm" ? [esc(M[ch.with].role), kindWord(ch.with), presInline(ch.with)].filter(Boolean).join(" · ") : esc(ch.topic || (ch.kind === "a2a" ? "Between agents" : ""));
  const leading = ch.kind === "dm" ? av(ch.with, 32) : ch.kind === "a2a" ? ic("eye", "ic20") : "";
  return `<header class="chhead"><div class="ph-t">
      ${leading}
      <div class="chat-heading"><h1 id="ch-title" tabindex="-1">${esc(title)}</h1>
        <p class="chat-context"><span>${sub}</span>${ch.proj ? plink(ch.proj) : ""}</p>
      </div>
    </div></header>`;
}
function chatThreadHtml(threadMsg, title) {
  if (!threadMsg) return "";
  return `<aside class="thread" aria-labelledby="chat-thread-title"><header class="side-panel-header"><div class="thread-title"><h2 id="chat-thread-title" tabindex="-1">Thread</h2><span class="muted">${esc(title)}</span></div><button class="btn ghost icon" type="button" data-act="thread-close" aria-label="Close thread">${ic("x", "ic20")}</button></header>
      <div class="msgs">${msgHtml(Object.assign({}, threadMsg, { thread: null }))}<hr style="border:0;border-top:1px solid var(--line2);width:100%">${threadMsg.thread.map((r) => msgHtml(Object.assign({ id: "r" }, r))).join("")}</div></aside>`;
}
function pageChat(id) {
  id = id && CH[id] ? id : "agency-v1";
  const ch = CH[id], keys = Object.keys(CH);
  const threadMsg = S.thread && ch.msgs.find((m) => m.id === S.thread);
  const title = ch.kind === "channel" ? "#" + ch.name : ch.name;
  const wide = matchMedia("(min-width: 960px)").matches;
  const jumpShortcut = S.singleKey ? ' aria-keyshortcuts="#"' : "";
  const jumpKbd = S.singleKey ? '<kbd aria-hidden="true">#</kbd>' : "";
  const isA2a = ch.kind === "a2a";
  return `<div class="chat${threadMsg ? " withthread" : ""}">
    ${chatHeaderHtml(ch, title)}
    <div class="chlist">
      <button class="btn ghost sm chat-jump" id="chat-jump" type="button" data-act="chat-jump"
        aria-haspopup="dialog" aria-controls="cmdk-dialog"${jumpShortcut}>${ic("search", "ic16")}Jump to channel${jumpKbd}</button>
      <details class="chat-directory"${wide ? " open" : ""}>
        <summary>Browse conversations</summary>
        <nav aria-label="Conversations">
          ${[["channel", "Channels"], ["dm", "Direct messages"], ["a2a", "Between agents"]].map(([kind, label]) => `<h2>${label}</h2><ul>${keys.filter((k) => CH[k].kind === kind).map((k) => chatLinkHtml(k, id)).join("")}</ul>`).join("")}
        </nav>
      </details>
    </div>
    <section class="chmain" aria-labelledby="ch-title">
      ${isA2a ? `<p class="readnote">${ic("eye", "ic16")}<span>Reading as the owner. To weigh in, message ${mlink(ch.pair[0])}.</span></p>` : ""}
      <div class="msgs" aria-label="Messages">${ch.msgs.map(msgHtml).join("")}</div>
      ${isA2a ? `<p class="readfoot">${ic("eye", "ic16")}You can read this conversation but not reply in it.</p>` : chatComposer(id, title)}</section>
    ${chatThreadHtml(threadMsg, title)}
  </div>`;
}

export {
  chatDrafts,
  msgHtml,
  pageChat,
  sendChatMessage,
};
