/* Task context: what a new task carries so its assignee "has access to everything".
   Chat's Turn into task attaches a conversation excerpt by default; any other page is offered, never attached,
   and Add context… searches conversations and work items. The created task stores a snapshot in `context`. */
import { CH, IN, M, P, TEAMS, W } from "../data.js";
import { qs } from "../state.js";
import { esc, ic, plural } from "../ui.js";
import { chatTitle } from "./chat-model.js";

const DEFAULT_EARLIER = 5;
const PICKER_LIMIT = 8;
const KIND_ICON = { conversation: "chat", project: "folder", work: "clipboard", inbox: "inbox", member: "user", team: "user-group", page: "doc-check" };
const TAB_NAMES = { overview: "Overview", work: "Work", calendar: "Calendar", team: "Team", activity: "Activity", insights: "Insights", conversations: "Conversations" };

const clip = (text, n = 180) => {
  const flat = String(text || "").replace(/\s+/g, " ").trim();
  return flat.length > n ? flat.slice(0, n - 1).replace(/\s+\S*$/, "") + "…" : flat;
};
const joinFacts = (facts, summary) => clip([facts.filter(Boolean).join(" · "), summary].filter(Boolean).join(". "));
const memberName = (id) => M[id]?.name || id || "";

/* ---------------------------------------------------------------- conversation excerpts */
function messageIndex(item) {
  return CH[item.ch]?.msgs.findIndex((m) => m.id === item.mid) ?? -1;
}

/* The message, up to `earlier` messages before it, and its thread replies. Never the whole conversation. */
function excerptLines(item) {
  const msgs = CH[item.ch]?.msgs || [], i = messageIndex(item);
  if (i < 0) return [];
  const picked = [...msgs.slice(Math.max(0, i - item.earlier), i + 1), ...(msgs[i].thread || [])];
  return picked.map((m) => ({ who: memberName(m.who), time: m.time, text: m.text }));
}

function chatExcerpt(chId, mid, earlier) {
  const msgs = CH[chId]?.msgs || [];
  const msg = msgs.find((m) => m.id === mid);
  if (!msg) return null;
  /* A thread is its own conversation: it starts with the root and its replies; earlier messages are opt-in. */
  const wanted = earlier ?? (msg.thread?.length ? 0 : DEFAULT_EARLIER);
  const item = { key: `chat:${chId}:${mid}`, kind: "conversation", ch: chId, mid, earlier: 0 };
  item.earlier = Math.min(wanted, messageIndex(item));
  return item;
}

function latestExcerpt(chId) {
  const last = CH[chId]?.msgs.at(-1);
  return last ? chatExcerpt(chId, last.id, DEFAULT_EARLIER) : null;
}

/* The fields every context item shows; an excerpt's title, preview and messages follow its current size. */
function resolveContext(item) {
  if (item.kind !== "conversation") return item;
  const lines = excerptLines(item), last = lines.at(-1);
  return {
    ...item,
    title: `Conversation excerpt · ${chatTitle(CH[item.ch])} · ${plural(lines.length, "message", "messages")}`,
    href: `#/chat/${item.ch}/${item.mid}`,
    preview: last ? clip(`${last.who}: ${last.text}`) : "",
    lines,
  };
}

/* ---------------------------------------------------------------- page and entity context */
function workContext(w) {
  return {
    key: `work:${w.id}`, kind: "work", href: `#/work?item=${encodeURIComponent(w.id)}`,
    title: `${w.id} · ${w.title}`,
    preview: joinFacts([w.status, w.who ? memberName(w.who) : "Unassigned", P[w.proj]?.name], w.description),
  };
}

function projectContext(id, tab = "overview") {
  const pr = P[id];
  if (!pr) return null;
  return {
    key: `project:${id}:${tab}`, kind: "project", href: `#/projects/${id}${tab === "overview" ? "" : "/" + tab}`,
    title: `${pr.name} · ${TAB_NAMES[tab] || tab}`,
    preview: joinFacts([pr.health, pr.due && `Due ${pr.due}`, pr.lead && `Lead ${memberName(pr.lead)}`, pr.blocker], pr.goal),
  };
}

function inboxContext(id) {
  const it = IN.find((x) => x.id === id);
  if (!it) return null;
  return { key: `inbox:${id}`, kind: "inbox", href: `#/inbox/${id}`, title: it.what, preview: joinFacts([it.sub, memberName(it.from)], it.why) };
}

function memberContext(id) {
  const m = M[id];
  if (!m || m.owner) return null;
  return { key: `member:${id}`, kind: "member", href: `#/members/${id}`, title: `${m.name} · ${m.role}`, preview: joinFacts([m.task, P[m.proj]?.name], m.goal) };
}

function teamContext(id) {
  const t = TEAMS[id];
  return t ? { key: `team:${id}`, kind: "team", href: `#/teams/${id}`, title: t.name, preview: joinFacts([`Lead ${memberName(t.lead)}`], t.purpose) } : null;
}

/* Pages without a known entity offer their heading and opening text. */
function domPageContext(page) {
  const main = document.getElementById("main");
  const title = main?.querySelector("h1")?.textContent.trim();
  if (!title) return null;
  const lead = [...main.querySelectorAll("p")].map((p) => p.textContent.trim()).find((t) => t.length > 24) || "";
  return { key: `page:${location.hash}`, kind: "page", href: location.hash || `#/${page}`, title, preview: clip(lead) };
}

const PAGE_CONTEXT = {
  projects: (id, tab) => projectContext(id, tab),
  inbox: (id) => inboxContext(id),
  members: (id) => memberContext(id),
  teams: (id) => teamContext(id),
  chat: (id) => latestExcerpt(id),
};

function pageContextFor([page, id, tab]) {
  const selected = W.find((w) => w.id === qs().get("item"));
  if (selected) return workContext(selected);
  return (id && PAGE_CONTEXT[page]?.(id, tab)) || domPageContext(page);
}

/* ---------------------------------------------------------------- composer state */
function createContextState(excerpt, currentRoute) {
  const attached = excerpt ? chatExcerpt(excerpt.ch, excerpt.mid) : null;
  return {
    items: attached ? [attached] : [],
    /* Only Chat attaches by default; anywhere else the page is offered. */
    offer: attached ? null : pageContextFor(currentRoute),
    picker: false,
    q: "",
    results: [],
    expanded: new Set(),
  };
}

const isAttached = (ctx, key) => ctx.items.some((it) => it.key === key);

function searchContextSources(ctx) {
  const q = ctx.q.trim().toLowerCase();
  const conversations = Object.keys(CH).map(latestExcerpt).filter(Boolean);
  const all = [...conversations, ...W.map(workContext)].filter((it) => !isAttached(ctx, it.key));
  return all.map(resolveContext).filter((it) => !q || `${it.title} ${it.preview}`.toLowerCase().includes(q)).slice(0, PICKER_LIMIT);
}

/* What the task stores: a snapshot, so the assignee reads what the owner saw when the task was made. */
function contextForTask(ctx) {
  return ctx.items.map(resolveContext).map(({ kind, title, href, preview, lines, ch, mid }) => ({
    kind, title, preview,
    href: kind === "conversation" ? `#/chat/${ch}` : href,
    /* A conversation keeps its ids (as Inbox's origin does), not a link written into the prompt. */
    ...(kind === "conversation" ? { source: { ch, msg: mid }, messages: lines } : {}),
  }));
}

/* ---------------------------------------------------------------- composer rendering */
const keyAttr = (key) => `data-key="${esc(key)}"`;
const sel = (act, key) => `[data-stc="${act}"][data-key="${CSS.escape(key)}"]`;

function excerptLinesHtml(lines, id) {
  return `<ol class="stc-ctx-lines" id="${id}" role="list" tabindex="0" aria-label="Messages in the excerpt">${lines.map((l) => `<li><p class="stc-ctx-line-h"><b>${esc(l.who)}</b><time>${esc(l.time)}</time></p><p>${esc(l.text)}</p></li>`).join("")}</ol>`;
}

function excerptControls(ctx, item, index) {
  const max = messageIndex(item), open = ctx.expanded.has(item.key), id = `stc-ctx-lines-${index}`;
  const step = (act, label, glyph, off) => `<button class="stc-ctx-step-btn" type="button" data-stc="${act}" ${keyAttr(item.key)} aria-label="${label}" aria-disabled="${off}">${glyph}</button>`;
  return `<div class="stc-ctx-tools">
      <button class="stc-ctx-toggle" type="button" data-stc="ctx-expand" ${keyAttr(item.key)} aria-expanded="${open}" aria-controls="${id}">${ic(open ? "chev-down" : "chev-right", "ic14")}${open ? "Hide messages" : `Show ${plural(item.lines.length, "message", "messages")}`}</button>
      <span class="stc-ctx-step" role="group" aria-label="Earlier messages in the excerpt">
        ${step("ctx-fewer", "Include one fewer earlier message", "−", item.earlier <= 0)}
        <span class="stc-ctx-step-n">${plural(item.earlier, "earlier message", "earlier messages")}</span>
        ${step("ctx-more", "Include one more earlier message", "+", item.earlier >= max)}
      </span>
    </div>
    ${open ? excerptLinesHtml(item.lines, id) : ""}`;
}

function contextCard(ctx, raw, index) {
  const item = resolveContext(raw);
  return `<li class="stc-ctx-card" ${keyAttr(item.key)}>
    <span class="stc-ctx-ic">${ic(KIND_ICON[item.kind] || "doc-check", "ic16")}</span>
    <div class="stc-ctx-main">
      <a class="stc-ctx-title" href="${esc(item.href)}" target="_blank" rel="noopener">${esc(item.title)}<span class="sr-only"> (opens in a new tab)</span></a>
      ${item.preview ? `<p class="stc-ctx-preview">${esc(item.preview)}</p>` : ""}
      ${item.kind === "conversation" ? excerptControls(ctx, item, index) : ""}
    </div>
    <button class="stc-ctx-rm" type="button" data-stc="ctx-remove" ${keyAttr(item.key)} aria-label="Remove context: ${esc(item.title)}" title="Remove">${ic("x", "ic14")}</button>
  </li>`;
}

function contextResultsHtml(ctx) {
  if (!ctx.results.length) return `<li class="stc-ctx-none">No conversations or work items match.</li>`;
  return ctx.results.map((it) => `<li><button class="stc-ctx-result" type="button" data-stc="ctx-pick" ${keyAttr(it.key)}>
    ${ic(KIND_ICON[it.kind], "ic14")}<span class="stc-ctx-result-t">${esc(it.title)}</span><span class="stc-ctx-result-p">${esc(it.preview)}</span>
  </button></li>`).join("");
}

function contextPicker(ctx) {
  if (!ctx.picker) return "";
  ctx.results = searchContextSources(ctx);
  return `<div class="stc-ctx-picker">
    <label class="sr-only" for="stc-ctx-q">Search conversations and work items</label>
    <input id="stc-ctx-q" type="search" autocomplete="off" placeholder="Search conversations and work items…" value="${esc(ctx.q)}">
    <ul class="stc-ctx-results" id="stc-ctx-results" role="list" aria-label="Context you can add">${contextResultsHtml(ctx)}</ul>
  </div>`;
}

function contextOffer(ctx) {
  if (!ctx.offer || isAttached(ctx, ctx.offer.key)) return "";
  const title = resolveContext(ctx.offer).title;
  return `<button class="stc-ctx-offer" type="button" data-stc="ctx-add-page">${ic("plus", "ic14")}<span>Add this page as context: <b>${esc(title)}</b></span></button>`;
}

function renderContextSection(ctx) {
  const n = ctx.items.length;
  return `<section class="stc-ctx" aria-labelledby="stc-ctx-h">
    <div class="stc-ctx-head">
      <h3 class="stc-section-h" id="stc-ctx-h">Context${n ? ` <span class="stc-ctx-n">${n}</span>` : ""}</h3>
      <button class="stc-head-link" type="button" data-stc="ctx-picker" aria-expanded="${ctx.picker}">${ic("plus", "ic14")}Add context…</button>
    </div>
    ${n ? `<p class="stc-ctx-note">Sent with the task, so the assignee can read it.</p><ul class="stc-ctx-list" role="list">${ctx.items.map((it, i) => contextCard(ctx, it, i)).join("")}</ul>` : ""}
    ${contextOffer(ctx)}
    ${contextPicker(ctx)}
  </section>`;
}

/* Typing in the picker updates only its results, so the search box keeps focus and caret. */
function refreshContextResults(dialog, ctx) {
  const list = dialog?.querySelector("#stc-ctx-results");
  if (!list) return;
  ctx.results = searchContextSources(ctx);
  list.innerHTML = contextResultsHtml(ctx);
}

/* ---------------------------------------------------------------- composer actions: each returns where focus goes */
function stepExcerpt(ctx, key, delta) {
  const item = ctx.items.find((it) => it.key === key);
  if (!item) return null;
  item.earlier = Math.max(0, Math.min(messageIndex(item), item.earlier + delta));
  return sel(delta < 0 ? "ctx-fewer" : "ctx-more", key);
}

function attachContext(ctx, item) {
  if (!item || isAttached(ctx, item.key)) return '[data-stc="ctx-picker"]';
  ctx.items.push({ ...item });
  return `.stc-ctx-card[data-key="${CSS.escape(item.key)}"] .stc-ctx-title`;
}

function removeContext(ctx, key) {
  ctx.items = ctx.items.filter((it) => it.key !== key);
  ctx.expanded.delete(key);
  return '[data-stc="ctx-picker"]';
}

function toggleExpanded(ctx, key) {
  if (ctx.expanded.has(key)) ctx.expanded.delete(key);
  else ctx.expanded.add(key);
  return sel("ctx-expand", key);
}

function togglePicker(ctx) {
  ctx.picker = !ctx.picker;
  ctx.q = "";
  return ctx.picker ? "#stc-ctx-q" : '[data-stc="ctx-picker"]';
}

/* Escape inside the composer closes an open picker first; focus returns to Add context…. */
function closeContextPicker(ctx) {
  ctx.picker = false;
  ctx.q = "";
  return '[data-stc="ctx-picker"]';
}

const CONTEXT_ACTIONS = {
  "ctx-picker": (ctx) => togglePicker(ctx),
  "ctx-add-page": (ctx) => attachContext(ctx, ctx.offer),
  "ctx-pick": (ctx, key) => { const focus = attachContext(ctx, ctx.results.find((it) => it.key === key)); ctx.picker = false; return focus; },
  "ctx-remove": (ctx, key) => removeContext(ctx, key),
  "ctx-expand": (ctx, key) => toggleExpanded(ctx, key),
  "ctx-more": (ctx, key) => stepExcerpt(ctx, key, 1),
  "ctx-fewer": (ctx, key) => stepExcerpt(ctx, key, -1),
};

/* Runs a ctx-* button; returns the focus selector for the repaint, or undefined when it is not a context action. */
function runContextAction(btn, ctx) {
  const fn = CONTEXT_ACTIONS[btn.dataset.stc];
  if (!fn || btn.getAttribute("aria-disabled") === "true") return fn ? sel(btn.dataset.stc, btn.dataset.key) : undefined;
  return fn(ctx, btn.dataset.key) || undefined;
}

/* ---------------------------------------------------------------- task detail */
/* Chat's message deep link, as Inbox renders it. */
function sourceMessageLink(src) {
  if (!src?.ch || !src.msg || !CH[src.ch]) return "";
  return ` · <a class="alink" href="#/chat/${esc(src.ch)}/${esc(src.msg)}">Open source message</a>`;
}

function detailContextItem(c) {
  const msgs = c.messages?.length ? `<details class="work-ctx-msgs"><summary>Show ${plural(c.messages.length, "message", "messages")}</summary>
      <ol role="list">${c.messages.map((l) => `<li><p class="work-ctx-line-h"><b>${esc(l.who)}</b><time>${esc(l.time)}</time></p><p>${esc(l.text)}</p></li>`).join("")}</ol>
    </details>` : "";
  return `<li class="work-ctx-item">
    <p class="work-ctx-links"><a class="alink" href="${esc(c.href)}">${ic(KIND_ICON[c.kind] || "doc-check", "ic16")}<span>${esc(c.title)}</span></a>${sourceMessageLink(c.source)}</p>
    ${c.preview ? `<p class="work-ctx-preview">${esc(c.preview)}</p>` : ""}
    ${msgs}
  </li>`;
}

function workDetailContext(w) {
  if (!w.context?.length) return "";
  return `<section class="work-detail-section work-ctx" aria-labelledby="work-detail-ctx">
    <h3 id="work-detail-ctx">Context <span class="cnt">${w.context.length}</span></h3>
    <p class="work-ctx-note">Attached when the task was created. The assignee reads it with the task.</p>
    <ul class="work-ctx-list" role="list">${w.context.map(detailContextItem).join("")}</ul>
  </section>`;
}

export { closeContextPicker, contextForTask, createContextState, refreshContextResults, renderContextSection, runContextAction, workDetailContext };
