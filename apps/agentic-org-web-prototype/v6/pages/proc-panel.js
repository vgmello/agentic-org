/* The right side panel used by processes, development processes and documents: one <dialog> with the Work panel's
   surface and edge shadow. Modal, so Escape and the backdrop close it; focus returns to what opened it. */
import { esc, ic } from "../ui.js";

let panel = null, trigger = null;

function element() {
  if (panel?.isConnected) return panel;
  panel = document.createElement("dialog");
  panel.className = "side-panel proc-panel";
  panel.id = "proc-panel";
  panel.setAttribute("aria-labelledby", "proc-panel-title");
  panel.addEventListener("cancel", (e) => { e.preventDefault(); closeProcPanel(); });
  panel.addEventListener("click", (e) => {
    if (e.target.closest("[data-proc-close]")) { closeProcPanel(); return; }
    if (e.target !== panel) return;
    const r = panel.getBoundingClientRect();
    if (e.clientX < r.left || e.clientX > r.right || e.clientY < r.top || e.clientY > r.bottom) closeProcPanel();
  });
  document.body.append(panel);
  return panel;
}

/* head: { eyebrow, title, badges } ; body: trusted markup built by the caller with esc() */
function panelHtml({ eyebrow, title, badges = "", actions = "" }, body) {
  return `<header class="side-panel-header proc-panel-bar">
      <p class="proc-panel-eyebrow">${eyebrow}</p>
      <button class="btn ghost icon" type="button" data-proc-close aria-label="Close" title="Close">${ic("x", "ic20")}</button>
    </header>
    <div class="proc-panel-head">
      <h2 id="proc-panel-title" tabindex="-1">${esc(title)}</h2>
      ${badges ? `<div class="proc-badges">${badges}</div>` : ""}
      ${actions ? `<div class="proc-panel-actions">${actions}</div>` : ""}
    </div>
    <div class="proc-panel-body">${body}</div>`;
}

function openProcPanel(head, body, from, wide = false) {
  const el = element();
  if (from) trigger = from;
  const scroll = el.open ? el.scrollTop : 0;
  el.toggleAttribute("data-wide", wide);
  delete el.dataset.doc; delete el.dataset.dev;
  el.innerHTML = panelHtml(head, body);
  if (!el.open) { el.classList.add("is-opening"); el.showModal(); el.scrollTop = 0; } else el.scrollTop = scroll;
  el.querySelector("#proc-panel-title")?.focus({ preventScroll: true });
}

function closeProcPanel(restore = true) {
  if (!panel?.open) return;
  panel.close();
  panel.classList.remove("is-opening");
  const back = trigger; trigger = null;
  if (restore && back?.isConnected) back.focus({ preventScroll: true });
}
const procPanelOpen = () => Boolean(panel?.open);
const procPanelData = () => panel?.dataset || {};

const panelSection = (id, title, body, right = "") => `<section class="proc-sec" aria-labelledby="${id}"><div class="proc-sec-h"><h3 id="${id}">${title}</h3>${right}</div>${body}</section>`;
const factList = (rows) => `<dl class="proc-facts">${rows.filter(Boolean).map(([dt, dd]) => `<div><dt>${esc(dt)}</dt><dd>${dd}</dd></div>`).join("")}</dl>`;

addEventListener("hashchange", () => closeProcPanel(false));

export { closeProcPanel, element as procPanelElement, factList, openProcPanel, panelSection, procPanelData, procPanelOpen };
