/* Person side panel: clicking any name or avatar that carries data-person="<id>" opens a short summary of that
   member on the right: who they are, what they are doing, their team, Message and View full profile, up to three
   open work items and, for agents, their conversations with other agents. One delegated handler serves every page.
   On wide screens the panel is non-modal so the page stays usable beside it; at 959px and below (and over another
   modal dialog) it is modal with a backdrop, and on phones it fills the screen. */
import { CH, M, OPEN, TEAMS, W } from "../data.js";
import { agentConversations } from "./chat-model.js";
import { av, esc, ic, kindWord, plural, presInline, presence, statusLine } from "../ui.js";

const NARROW = "(max-width: 959px)";
const WORK_LIMIT = 3;
const KIND_ICON = { Agent: "sparkle", Person: "user", Owner: "star" };
const INTERACTIVE = "a, button, label, summary, select, input, textarea, [role='button'], [role='link'], [data-act]";

let panel = null, trigger = null, current = "";

const isMember = (id) => Boolean(id) && Object.hasOwn(M, id);

/* ------------------------------------------------------------------ content */
function kindLine(id) {
  const word = kindWord(id);
  return `<span class="pp-kind" data-kind="${word.toLowerCase()}">${ic(KIND_ICON[word], "ic14")}${word}</span>`;
}

function personStatus(m) {
  if (m.live === "offline") return `<span class="pin c-outline">${ic("offline", "ic14")}Offline</span>${m.last ? `<span>${esc(m.last)}</span>` : ""}`;
  return `<span class="pin c-green">${ic("check-circle", "ic14")}Signed in</span>`;
}

function statusHtml(id) {
  const m = M[id];
  if (m.owner) return "";
  if (m.kind !== "agent") return `<p class="pp-status">${personStatus(m)}</p>`;
  const line = presence(id) === "withyou" && m.task ? `Working on ${esc(m.task)}` : statusLine(id);
  return `<p class="pp-status">${presInline(id)}${line ? `<span>${line}</span>` : ""}</p>`;
}

function teamHtml(m) {
  const teams = m.teams.filter((t) => TEAMS[t]);
  const body = teams.length
    ? teams.map((t) => `<a class="alink" href="#/teams/${esc(t)}">${esc(TEAMS[t].name)}</a>`).join(", ")
    : `<span class="pp-muted">No team</span>`;
  return `<p class="pp-team">${ic("user-group", "ic16")}<span>${body}</span></p>`;
}

function actionsHtml(id) {
  const m = M[id];
  const message = m.owner ? "" : `<button class="btn primary" type="button" data-act="chat-dm" data-id="${esc(id)}">${ic("chat", "ic16")}Message</button>`;
  return `<div class="pp-actions">${message}<a class="btn outline" href="#/members/${esc(id)}">View full profile</a></div>`;
}

const openWork = (id) => W.filter((w) => w.who === id && OPEN(w));

function workRow(w) {
  return `<li><a class="pp-row" href="#/work?item=${encodeURIComponent(w.id)}">
    <span class="pp-row-main">${esc(w.title)}</span>
    <span class="pp-row-meta">${esc(w.id)} · ${esc(w.status)}</span></a></li>`;
}

function workHtml(id) {
  const items = openWork(id);
  const more = items.length > WORK_LIMIT
    ? `<a class="pp-more alink" href="#/members/${esc(id)}/tasks">All ${plural(items.length, "open item", "open items")}</a>` : "";
  const body = items.length
    ? `<ul class="pp-list" role="list">${items.slice(0, WORK_LIMIT).map(workRow).join("")}</ul>${more}`
    : `<p class="pp-empty">No open work assigned.</p>`;
  return section("pp-work-h", "Current work", body);
}

function a2aRow(id, ch, key) {
  const other = ch.pair.find((x) => x !== id) || id, last = ch.msgs[ch.msgs.length - 1];
  return `<li><a class="pp-row pp-a2a" href="#/chat/${esc(key)}">${av(other, 24)}
    <span class="pp-row-main">With ${esc(M[other].name)}</span>
    ${last ? `<time class="pp-row-meta">${esc(last.time)}</time>` : ""}</a></li>`;
}

function a2aHtml(id) {
  const keys = agentConversations(id);
  const body = keys.length
    ? `<ul class="pp-list" role="list">${keys.map((k) => a2aRow(id, CH[k], k)).join("")}</ul>`
    : `<p class="pp-empty">No conversations with other agents yet.</p>`;
  return section("pp-a2a-h", "Conversations with other agents", body);
}

const section = (hid, title, body) => `<section class="pp-section" aria-labelledby="${hid}"><h3 class="pp-h" id="${hid}">${title}</h3>${body}</section>`;

function headerHtml(id) {
  const m = M[id];
  return `<header class="pp-head">
    ${av(id, 40)}
    <div class="pp-id">
      <h2 id="person-panel-title" tabindex="-1">${esc(m.name)}</h2>
      <p class="pp-role">${m.role ? `<span>${esc(m.role)}</span>` : ""}${kindLine(id)}</p>
    </div>
    <button class="btn ghost icon pp-close" type="button" data-person-close aria-label="Close">${ic("x", "ic20")}</button>
  </header>`;
}

function panelHtml(id) {
  const m = M[id];
  return `${headerHtml(id)}
    <div class="pp-body">
      ${statusHtml(id)}
      ${m.owner ? "" : teamHtml(m)}
      ${actionsHtml(id)}
      ${workHtml(id)}
      ${m.kind === "agent" ? a2aHtml(id) : ""}
    </div>`;
}

/* ------------------------------------------------------------------ panel element */
function panelElement() {
  if (panel?.isConnected) return panel;
  panel = document.createElement("dialog");
  panel.className = "side-panel person-panel";
  panel.id = "person-panel";
  panel.setAttribute("aria-labelledby", "person-panel-title");
  panel.addEventListener("keydown", onPanelKeydown);
  panel.addEventListener("cancel", (e) => { e.preventDefault(); closePersonPanel(); });
  panel.addEventListener("click", onPanelClick);
  document.body.append(panel);
  return panel;
}

function onPanelKeydown(e) {
  if (e.key !== "Escape" || e.isComposing) return;
  e.preventDefault();
  e.stopPropagation();
  closePersonPanel();
}

/* A click on the modal backdrop lands on the dialog itself, outside its box */
function onPanelClick(e) {
  if (e.target.closest("[data-person-close]")) { closePersonPanel(); return; }
  if (e.target !== panel || !panel.matches(":modal")) return;
  const r = panel.getBoundingClientRect();
  if (e.clientX < r.left || e.clientX > r.right || e.clientY < r.top || e.clientY > r.bottom) closePersonPanel();
}

const wantsModal = () => matchMedia(NARROW).matches || Boolean(document.querySelector("dialog:modal:not(#person-panel)"));

function showPanel(el) {
  const modal = wantsModal();
  if (el.open && el.matches(":modal") !== modal) el.close();
  if (!el.open) {
    el.classList.add("is-opening");
    if (modal) el.showModal(); else el.show();
  }
  el.querySelector("#person-panel-title")?.focus({ preventScroll: true });
}

function openPersonPanel(id, from) {
  if (!isMember(id)) return;
  const el = panelElement();
  trigger = from || document.activeElement;
  if (current !== id || !el.open) {
    current = id;
    el.dataset.person = id;
    el.innerHTML = panelHtml(id);
    el.scrollTop = 0;
  }
  showPanel(el);
}

function closePersonPanel(restoreFocus = true) {
  if (!panel?.open) return;
  panel.close();
  panel.classList.remove("is-opening");
  current = "";
  const back = trigger;
  trigger = null;
  if (restoreFocus && back?.isConnected) back.focus({ preventScroll: true });
}

/* ------------------------------------------------------------------ delegated triggers */
/* An avatar inside another control (a DM row, a card's link) belongs to that control */
function personTrigger(e) {
  if (e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return null;
  const el = e.target.closest?.("[data-person]");
  if (!el || el === panel || !isMember(el.dataset.person)) return null;
  if (panel?.contains(el) && !el.matches("a")) return null;
  if (!el.matches("a, button") && el.parentElement?.closest(INTERACTIVE)) return null;
  return el;
}

/* Focus returns to the trigger; an avatar is not focusable, so it returns to the name beside it */
function focusTarget(el) {
  if (el.matches("a, button")) return el;
  const near = el.parentElement?.closest("li, tr, article, .msg, p, div");
  return near?.querySelector(`a[data-person="${CSS.escape(el.dataset.person)}"]`) || document.activeElement;
}

function onDocumentClick(e) {
  const el = personTrigger(e);
  if (!el) return;
  e.preventDefault();
  e.stopPropagation();
  openPersonPanel(el.dataset.person, focusTarget(el));
}

document.addEventListener("click", onDocumentClick, true);
addEventListener("hashchange", () => closePersonPanel(false));

export { closePersonPanel, openPersonPanel };
