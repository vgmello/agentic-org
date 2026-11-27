/* An agent's conversations with other agents, read inside the agent's page (#/members/<agent>/conversations[/<id>[/<msg>]]).
   The owner reads them; they cannot reply. A list of the agent's conversations sits beside the open transcript; below
   the pane's narrow width the list comes first and a chosen conversation replaces it, with a Back link. The transcript
   is Chat's own message stream, so messages look and behave as they do in Chat. Old #/chat/a2a-<id> links land here. */
import { CH, M, P } from "../data.js";
import { agentConversations, lastActiveLabel, openDirectMessage } from "./chat-model.js";
import { streamHtml } from "./chat.js";
import { VIEW_CLICKS } from "./chat-attach-view.js";
import { S } from "../state.js";
import { av, esc, ic, plink } from "../ui.js";

const isA2A = (k) => Boolean(k) && Object.hasOwn(CH, k) && CH[k].kind === "a2a";
const base = (aid) => `#/members/${encodeURIComponent(aid)}/conversations`;
const conversationHref = (aid, k, msg) => `${base(aid)}/${encodeURIComponent(k)}${msg ? "/" + encodeURIComponent(msg) : ""}`;
const otherOf = (aid, ch) => ch.pair.find((x) => x !== aid) || aid;

/* An old Chat link to an agents' conversation (#/chat/a2a-…[/<msg>]) opens it in an agent's page: the agent the owner
   came from when that agent is in it (their page or their DM), otherwise the conversation's first agent. A link that
   names an agent who is not in the conversation goes to its first agent too. Returns the new hash, or null. */
function cameFrom(lastRoute) {
  const r = /^#\/?(?:members\/([^/?]+)|chat\/dm-([^/?]+))/.exec(lastRoute || "");
  return r ? r[1] || r[2] : "";
}
function a2aRedirect(r, lastRoute) {
  if (r[0] === "chat" && isA2A(r[1])) {
    const pair = CH[r[1]].pair, from = cameFrom(lastRoute);
    return conversationHref(pair.includes(from) ? from : pair[0], r[1], r[2]);
  }
  if (r[0] === "members" && r[2] === "conversations" && isA2A(r[3]) && !CH[r[3]].pair.includes(r[1])) {
    return conversationHref(CH[r[3]].pair[0], r[3], r[4]);
  }
  return null;
}

/* ---------------------------------------------------------------- list */
function lastLine(ch) {
  const m = ch.msgs[ch.msgs.length - 1];
  return m ? `${M[m.who].name}: ${m.text}` : "No messages yet";
}
function rowHtml(aid, k, open) {
  const ch = CH[k], other = otherOf(aid, ch), when = lastActiveLabel(ch);
  return `<li><a class="ac-row" href="${esc(conversationHref(aid, k))}" data-k="${esc(k)}"${open ? ' aria-current="true"' : ""}>
    ${av(other, 32)}
    <span class="ac-row-main"><span class="ac-row-top"><span class="ac-row-who">With ${esc(M[other].name)}</span>${when ? `<time class="ac-row-time">${esc(when)}</time>` : ""}</span>
    <span class="ac-row-last">${esc(lastLine(ch))}</span></span></a></li>`;
}
function listHtml(aid, keys, open) {
  const body = keys.length
    ? `<ul class="ac-list" role="list">${keys.map((k) => rowHtml(aid, k, k === open)).join("")}</ul>`
    : `<p class="ac-empty">No conversations with other agents yet. When ${esc(M[aid].name)} talks with another agent, you can read it here.</p>`;
  return `<nav class="ac-nav" aria-labelledby="ac-nav-h"><h2 class="ac-nav-h" id="ac-nav-h">With other agents</h2>${body}</nav>`;
}

/* ---------------------------------------------------------------- transcript */
function headHtml(aid, k, ch) {
  const proj = ch.proj && P[ch.proj] ? `<span class="ac-proj">${ic("folder", "ic14")}${plink(ch.proj)}</span>` : "";
  return `<header class="ac-head">
    <a class="ac-back" href="${esc(base(aid))}" data-k="${esc(k)}">${ic("chev-left", "ic16")}All conversations</a>
    <div class="ac-title">
      <span class="chat-pair" aria-hidden="true">${ch.pair.map((x) => av(x, 24)).join("")}</span>
      <h2 id="ac-title" tabindex="-1">${esc(ch.name)}</h2>
    </div>
    <p class="ac-context"><span class="ac-context-in"><span class="chat-readas">${ic("eye", "ic14")}Reading as the owner</span><span class="ac-kind">Between agents</span>${proj}</span></p>
  </header>`;
}
/* The owner steps in through a DM that links back to this conversation. */
function footHtml(k, ch) {
  const names = ch.pair.map((x) => esc(M[x].name)).join(" or ");
  const buttons = ch.pair.map((x) => `<button class="btn outline sm" type="button" data-act="chat-dm" data-id="${esc(x)}" data-v="${esc(k)}">${ic("chat", "ic16")}Message ${esc(M[x].name)}</button>`).join("");
  return `<div class="readfoot ac-foot"><p>${ic("eye", "ic16")}You can't reply here. To weigh in, message ${names}.</p><div class="actions">${buttons}</div></div>`;
}
function transcriptHtml(aid, k) {
  if (!k) return "";
  const ch = CH[k];
  return `<section class="chat ac-transcript" data-ch="${esc(k)}" aria-labelledby="ac-title">
    ${headHtml(aid, k, ch)}
    <div class="chmain"><div class="msgs" aria-label="Messages" data-ch="${esc(k)}">${streamHtml(k)}</div>${footHtml(k, ch)}</div>
  </section>`;
}
function missingHtml(aid) {
  return `<section class="ac-transcript ac-missing" aria-labelledby="ac-title"><h2 id="ac-title" tabindex="-1">This conversation isn't available</h2>
    <p>It may have been removed. <a class="alink" href="${esc(base(aid))}">See ${esc(M[aid].name)}'s conversations</a></p></section>`;
}

/* The tab's body. With no conversation in the address, the newest opens beside the list on wide panes; a narrow pane
   shows only the list until one is chosen. */
function agentConversationsTab(aid, picked) {
  const keys = agentConversations(aid);
  const known = isA2A(picked) && keys.includes(picked);
  const open = known ? picked : keys[0] || "";
  const main = picked && !known ? missingHtml(aid) : transcriptHtml(aid, open);
  return `<div class="ac" data-picked="${Boolean(picked)}"><div class="ac-split">${listHtml(aid, keys, open)}${main}</div></div>`;
}

/* ---------------------------------------------------------------- after render */
/* A linked message (…/conversations/<id>/<msg>) is marked and brought into view, as a Chat source link is. */
function landOnMessage(root, msg) {
  const el = msg && root.querySelector(`.msgs > [data-mid="${CSS.escape(msg)}"]`);
  if (!el) return;
  el.classList.add("is-linked");
  el.tabIndex = -1;
  el.scrollIntoView({ block: "center" });
  el.focus({ preventScroll: true });
}
function onClick(e) {
  const row = e.target.closest(".ac-row");
  if (row) { S.focusAfter = "#ac-title"; return; }
  const back = e.target.closest(".ac-back");
  if (back) { S.focusAfter = `.ac-row[data-k="${CSS.escape(back.dataset.k)}"]`; return; }
  const t = e.target.closest("[data-act]"), chId = e.currentTarget.querySelector(".ac-transcript")?.dataset.ch;
  if (!t || !chId) return;
  if (t.dataset.act === "chat-dm") { S.focusAfter = "#compose"; return; }
  if (t.dataset.act === "chat-dm-msg") {
    const m = CH[chId].msgs.find((x) => x.id === t.dataset.id);
    if (!m) return;
    S.focusAfter = "#compose";
    location.hash = openDirectMessage(m.who, { ch: chId, msg: m.id });
    return;
  }
  VIEW_CLICKS[t.dataset.act]?.(t.dataset.id, t, chId);
}
function mountAgentConversations(main, r) {
  const root = main.querySelector(".ac");
  if (!root) return;
  root.addEventListener("click", onClick);
  if (r[4]) landOnMessage(root, r[4]);
}

export { a2aRedirect, agentConversationsTab, conversationHref, mountAgentConversations };
