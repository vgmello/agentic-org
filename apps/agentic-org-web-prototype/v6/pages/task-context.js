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

/* A line's attachments: each file's name and the link to the exact message (or thread reply) that carries it. */
const lineFiles = (m, href) => (m.files || []).map((f) => ({ name: f.name, href }));
function excerptLine(m, href) {
  const files = lineFiles(m, href);
  return { who: memberName(m.who), time: m.time, text: m.text || "", href, ...(files.length ? { files } : {}) };
}
/* The message, up to `earlier` messages before it, and its thread replies. Never the whole conversation. */
function excerptLines(item) {
  const msgs = CH[item.ch]?.msgs || [], i = messageIndex(item);
  if (i < 0) return [];
  const base = `#/chat/${item.ch}/`;
  const posts = msgs.slice(Math.max(0, i - item.earlier), i + 1).map((m) => excerptLine(m, base + m.id));
  const replies = (msgs[i].thread || []).map((r, n) => excerptLine(r, `${base}${msgs[i].id}-r${n}`));
  return [...posts, ...replies];
}
/* What a line says in one row of text: its words, then its files by name (an attachment-only line is never empty). */
function lineSummary(l) {
  const names = (l.files || []).map((f) => f.name).join(", ");
  return [l.text, names && `Attached: ${names}`].filter(Boolean).join(" · ");
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
    preview: last ? clip(`${last.who}: ${lineSummary(last)}`) : "",
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

/* A line's files by name, each with "Open source message" to the message in Chat that carries it (a new tab from the
   composer, so the draft stays). */
function lineFilesHtml(l, cls, newTab = false) {
  if (!l.files?.length) return "";
  const target = newTab ? ' target="_blank" rel="noopener"' : "";
  const one = (f) => `<span class="ctx-file"><span class="ctx-file-name">${esc(f.name)}</span> · <a class="alink" href="${esc(f.href)}"${target} aria-label="Open source message for ${esc(f.name)}">Open source message</a></span>`;
  return `<p class="${cls}">${ic("paperclip", "ic14")}<span><span class="sr-only">Attached: </span>${l.files.map(one).join("")}</span></p>`;
}
/* "Show 6 messages · 1 file": a collapsed excerpt says what it carries. */
function excerptCount(lines) {
  const files = lines.reduce((n, l) => n + (l.files?.length || 0), 0);
  return plural(lines.length, "message", "messages") + (files ? ` · ${plural(files, "file", "files")}` : "");
}
const lineText = (l) => (l.text ? `<p>${esc(l.text)}</p>` : "");
function excerptLinesHtml(lines, id) {
  return `<ol class="stc-ctx-lines" id="${id}" role="list" tabindex="0" aria-label="Messages in the excerpt">${lines.map((l) => `<li><p class="stc-ctx-line-h"><b>${esc(l.who)}</b><time>${esc(l.time)}</time></p>${lineText(l)}${lineFilesHtml(l, "stc-ctx-files", true)}</li>`).join("")}</ol>`;
}

function excerptControls(ctx, item, index) {
  const max = messageIndex(item), open = ctx.expanded.has(item.key), id = `stc-ctx-lines-${index}`;
  const step = (act, label, glyph, off) => `<button class="stc-ctx-step-btn" type="button" data-stc="${act}" ${keyAttr(item.key)} aria-label="${label}" aria-disabled="${off}">${glyph}</button>`;
  return `<div class="stc-ctx-tools">
      <button class="stc-ctx-toggle" type="button" data-stc="ctx-expand" ${keyAttr(item.key)} aria-expanded="${open}" aria-controls="${id}">${ic(open ? "chev-down" : "chev-right", "ic14")}${open ? "Hide messages" : `Show ${excerptCount(item.lines)}`}</button>
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

/* ---------------------------------------------------------------- saved snapshots (Work reload) */
/* A task's `context` comes back from storage as untrusted data: each entry is checked field by field, links must be
   in-app hashes, and message and file references are kept exactly as saved (e.g. "#/chat/agency-v1/m2-r1"). An entry
   that fails is dropped; the rest load. */
const MAX_CONTEXT = 20, MAX_LINES = 60, MAX_FILES = 20;
const ID_RE = /^[\w-]{1,80}$/;
const str = (v, n) => (typeof v === "string" ? v.slice(0, n) : "");
const appHref = (v) => (typeof v === "string" && /^#\/[\w\-/?=&%.]*$/.test(v) && v.length <= 300 ? v : "");
function savedFiles(files) {
  if (!Array.isArray(files)) return [];
  return files.slice(0, MAX_FILES).map((f) => ({ name: str(f?.name, 200), href: appHref(f?.href) })).filter((f) => f.name && f.href);
}
function savedLine(l) {
  if (!l || typeof l !== "object") return null;
  const files = savedFiles(l.files), href = appHref(l.href);
  const line = { who: str(l.who, 80), time: str(l.time, 40), text: str(l.text, 4000), ...(href ? { href } : {}), ...(files.length ? { files } : {}) };
  return line.who && (line.text || files.length) ? line : null;
}
function savedSource(src) {
  if (!src || !ID_RE.test(src.ch) || (src.msg != null && !ID_RE.test(src.msg))) return null;
  return src.msg ? { ch: src.ch, msg: src.msg } : { ch: src.ch };
}
function savedContextItem(c) {
  if (!c || typeof c !== "object" || !Object.hasOwn(KIND_ICON, c.kind)) return null;
  const title = str(c.title, 300).trim(), href = appHref(c.href);
  if (!title || !href) return null;
  const item = { kind: c.kind, title, preview: str(c.preview, 400), href };
  if (c.kind !== "conversation") return item;
  const source = savedSource(c.source);
  const messages = (Array.isArray(c.messages) ? c.messages.slice(0, MAX_LINES) : []).map(savedLine).filter(Boolean);
  return source ? { ...item, source, messages } : null;
}
function savedTaskContext(raw) {
  if (!Array.isArray(raw)) return [];
  return raw.slice(0, MAX_CONTEXT).map(savedContextItem).filter(Boolean);
}

/* ---------------------------------------------------------------- task detail */
/* Whether an in-app link still opens what it names. Chat links name a conversation and, optionally, a message or a
   thread reply ("<root>-r<n>"); a conversation or message that is gone (e.g. one sent in an earlier session of the
   prototype) cannot open. Other pages are checked by their id where one is named. */
function chatRefOpens(ch, mid) {
  const msgs = CH[ch]?.msgs;
  if (!msgs) return false;
  if (!mid || msgs.some((m) => m.id === mid)) return true;
  const r = /^(.*)-r(\d+)$/.exec(mid);
  return Boolean(r && msgs.find((m) => m.id === r[1])?.thread?.[Number(r[2])]);
}
const PAGE_EXISTS = { projects: (id) => Boolean(P[id]), inbox: (id) => IN.some((i) => i.id === id), members: (id) => Boolean(M[id]), teams: (id) => Boolean(TEAMS[id]) };
function linkOpens(href) {
  const [page, a, b] = href.replace(/^#\//, "").split("?")[0].split("/");
  if (page === "chat") return chatRefOpens(a, b);
  if (page === "work") { const item = new URLSearchParams(href.split("?")[1] || "").get("item"); return !item || W.some((w) => w.id === item); }
  return !a || !PAGE_EXISTS[page] || PAGE_EXISTS[page](a);
}
const unavailable = (what) => `<span class="work-ctx-gone">${ic("info", "ic14")}<span>${what} unavailable</span></span>`;
/* Chat's message deep link, as Inbox renders it; an explicit "Source unavailable" when that message cannot open. */
function sourceMessageLink(src) {
  if (!src?.ch || !src.msg) return "";
  if (!chatRefOpens(src.ch, src.msg)) return ` · ${unavailable("Source")}`;
  return ` · <a class="alink" href="#/chat/${esc(src.ch)}/${esc(src.msg)}">Open source message</a>`;
}
/* A saved line's files: each still links its exact message, or names the file with "Message unavailable". */
function detailFilesHtml(l) {
  if (!l.files?.length) return "";
  const one = (f) => `<span class="ctx-file"><span class="ctx-file-name">${esc(f.name)}</span> ${linkOpens(f.href) ? `· <a class="alink" href="${esc(f.href)}" aria-label="Open source message for ${esc(f.name)}">Open source message</a>` : unavailable("Message")}</span>`;
  return `<p class="work-ctx-files">${ic("paperclip", "ic14")}<span><span class="sr-only">Attached: </span>${l.files.map(one).join("")}</span></p>`;
}
function detailContextItem(c) {
  const msgs = c.messages?.length ? `<details class="work-ctx-msgs"><summary>Show ${excerptCount(c.messages)}</summary>
      <ol role="list">${c.messages.map((l) => `<li><p class="work-ctx-line-h"><b>${esc(l.who)}</b><time>${esc(l.time)}</time></p>${lineText(l)}${detailFilesHtml(l)}</li>`).join("")}</ol>
    </details>` : "";
  const label = `${ic(KIND_ICON[c.kind] || "doc-check", "ic16")}<span>${esc(c.title)}</span>`;
  const title = linkOpens(c.href) ? `<a class="alink" href="${esc(c.href)}">${label}</a>` : `<span class="work-ctx-title">${label}</span> ${unavailable("Source")}`;
  return `<li class="work-ctx-item">
    <p class="work-ctx-links">${title}${linkOpens(c.href) ? sourceMessageLink(c.source) : ""}</p>
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

export { closeContextPicker, contextForTask, createContextState, refreshContextResults, renderContextSection, runContextAction, savedTaskContext, workDetailContext };
