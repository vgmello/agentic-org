/* Chat attachments as sent: an image grid and file cards in a message, the image viewer, and the
   conversation's Files panel, where each file links to its message. */
import { CH, M } from "../data.js";
import { altOf, fileFacts, fileIcon, isImage } from "./chat-attach-files.js";
import { sentAt } from "./chat-model.js";
import { syncOverlayHistory } from "./chat-overlays.js";
import { esc, ic, plural } from "../ui.js";

const GRID_MAX = 4;

/* ---------------------------------------------------------------- in a message */
function imageTile(m, img, i, all) {
  const extra = i === GRID_MAX - 1 && all.length > GRID_MAX ? all.length - GRID_MAX : 0;
  const pos = all.length > 1 ? `, ${i + 1} of ${all.length}` : "";
  const label = `Open image ${altOf(img)}${pos}${extra ? `, and ${extra} more` : ""}`;
  return `<button class="chat-att-img" type="button" data-act="chat-att-view" data-id="${esc(m.id)}" data-i="${i}" aria-label="${esc(label)}">
    <span class="chat-att-frame"><img src="${esc(img.url)}" alt="${esc(altOf(img))}" loading="lazy" decoding="async"><span class="chat-att-zoom" aria-hidden="true">${ic("maximize", "ic14")}</span>${extra ? `<span class="chat-att-more" aria-hidden="true">+${extra}</span>` : ""}</span>
    <span class="chat-att-cap" aria-hidden="true">${ic("image", "ic14")}<span class="chat-att-cap-name">${esc(img.name)}</span></span></button>`;
}
function imageGrid(m, images) {
  if (!images.length) return "";
  const shown = images.slice(0, GRID_MAX);
  return `<div class="chat-att-grid" data-n="${shown.length}">${shown.map((img, i) => imageTile(m, img, i, images)).join("")}</div>`;
}
function fileCard(f) {
  return `<div class="chat-att-file">${fileIcon(f)}
    <span class="chat-att-meta"><span class="chat-att-name" title="${esc(f.name)}">${esc(f.name)}</span><span class="chat-att-sub">${esc(fileFacts(f))}</span></span>
    <span class="chat-att-actions">
      <a class="btn ghost sm" href="${esc(f.url)}" download="${esc(f.name)}" aria-label="Download ${esc(f.name)}">${ic("download", "ic16")}<span class="chat-att-word">Download</span></a>
      <a class="btn ghost sm" href="${esc(f.url)}" target="_blank" rel="noopener" aria-label="Open ${esc(f.name)} in a new tab">${ic("open", "ic16")}<span class="chat-att-word">Open</span></a>
    </span></div>`;
}
/* A message's attachments. Outside a conversation (a shared preview) they are summarised, not interactive. */
function messageAttachmentsHtml(m, chId) {
  const files = m.files || [];
  if (!files.length) return "";
  if (!chId) return `<p class="msg-src">${ic("paperclip", "ic14")}<span>${plural(files.length, "attachment", "attachments")}</span></p>`;
  const images = files.filter(isImage), others = files.filter((f) => !isImage(f));
  const cards = others.length ? `<div class="chat-att-files">${others.map(fileCard).join("")}</div>` : "";
  return `<div class="chat-atts">${imageGrid(m, images)}${cards}</div>`;
}
const attachmentNameHit = (m, q) => (m.files || []).some((f) => f.name.toLowerCase().includes(q) || altOf(f).toLowerCase().includes(q));

/* ---------------------------------------------------------------- finding a message's files */
/* Thread replies render with ids "<root>-r<n>". */
function findMessage(chId, mid) {
  const msgs = CH[chId]?.msgs || [];
  const direct = msgs.find((m) => m.id === mid);
  if (direct) return direct;
  const r = /^(.*)-r(\d+)$/.exec(mid || "");
  return r ? msgs.find((m) => m.id === r[1])?.thread?.[Number(r[2])] : null;
}

/* ---------------------------------------------------------------- image viewer
   The viewer is Chat's top overlay: opening it adds to Chat's overlay history, so browser Back closes it first and
   returns focus to its thumbnail (in the stream or the open thread), leaving the conversation as it was. */
const viewer = { dialog: null, images: [], i: 0, from: "", meta: "" };
function viewerInner() {
  const img = viewer.images[viewer.i], n = viewer.images.length;
  const nav = n > 1 ? `<button class="btn ghost icon chat-lb-prev" type="button" data-lb="prev" aria-label="Previous image">${ic("chev-left", "ic20")}</button>
    <button class="btn ghost icon chat-lb-next" type="button" data-lb="next" aria-label="Next image">${ic("chev-right", "ic20")}</button>` : "";
  return `<div class="chat-lb-bar">
      <div class="chat-lb-title"><h2 id="chat-lb-title">${esc(img.name)}</h2><span>${esc(viewer.meta)} · ${esc(fileFacts(img))}${n > 1 ? ` · ${viewer.i + 1} of ${n}` : ""}</span></div>
      <a class="btn ghost sm" href="${esc(img.url)}" download="${esc(img.name)}" aria-label="Download ${esc(img.name)}" title="Download">${ic("download", "ic16")}<span class="chat-att-word">Download</span></a>
      <button class="btn ghost icon" type="button" data-lb="close" aria-label="Close image viewer">${ic("x", "ic20")}</button>
    </div>
    <figure class="chat-lb-stage"><img src="${esc(img.url)}" alt="${esc(altOf(img))}">${nav}</figure>
    <p class="sr-only" aria-live="polite">${esc(altOf(img))}${n > 1 ? `, image ${viewer.i + 1} of ${n}` : ""}</p>`;
}
function showImage(i) {
  const n = viewer.images.length;
  viewer.i = (i + n) % n;
  const had = document.activeElement?.dataset?.lb;
  viewer.dialog.innerHTML = viewerInner();
  viewer.dialog.querySelector(`[data-lb="${had || "close"}"]`)?.focus();
}
function closeViewer() {
  const d = viewer.dialog;
  if (!d) return;
  viewer.dialog = null;
  d.close();
  d.remove();
  document.querySelector(viewer.from)?.focus();
  syncOverlayHistory();
}
const viewerOpen = () => Boolean(viewer.dialog);
const LB_KEYS = { ArrowLeft: () => showImage(viewer.i - 1), ArrowRight: () => showImage(viewer.i + 1), Escape: closeViewer };
/* Tab cycles inside the viewer; a native modal dialog would otherwise let focus leave to the browser. */
function trapTab(e) {
  const items = [...viewer.dialog.querySelectorAll("a[href], button:not([disabled])")];
  const i = items.indexOf(document.activeElement), last = items.length - 1;
  const next = e.shiftKey ? (i <= 0 ? last : i - 1) : (i === last || i < 0 ? 0 : i + 1);
  e.preventDefault();
  items[next]?.focus();
}
function onViewerKeydown(e) {
  e.stopPropagation(); /* app shortcuts never act behind the viewer */
  if (e.key === "Tab") { trapTab(e); return; }
  const fn = LB_KEYS[e.key];
  if (!fn || (e.key !== "Escape" && viewer.images.length < 2)) return;
  e.preventDefault();
  fn();
}
function onViewerClick(e) {
  const act = e.target.closest("[data-lb]")?.dataset.lb;
  if (act === "close" || e.target === viewer.dialog) closeViewer();
  else if (act === "prev") showImage(viewer.i - 1);
  else if (act === "next") showImage(viewer.i + 1);
}
function openViewer(mid, t, chId) {
  const m = findMessage(chId, mid), images = (m?.files || []).filter(isImage);
  if (!images.length) return;
  const d = document.createElement("dialog");
  d.className = "chat-lightbox";
  d.setAttribute("aria-labelledby", "chat-lb-title");
  Object.assign(viewer, { dialog: d, images, from: `${t.closest("aside.thread") ? "aside.thread" : ".chmain"} [data-act="chat-att-view"][data-id="${CSS.escape(mid)}"][data-i="${t.dataset.i}"]`, meta: `${M[m.who].name} · ${sentAt(m)}` });
  d.addEventListener("keydown", onViewerKeydown);
  d.addEventListener("click", onViewerClick);
  d.addEventListener("cancel", (e) => { e.preventDefault(); closeViewer(); });
  t.closest(".chat").append(d);
  d.showModal();
  showImage(Number(t.dataset.i) || 0);
  syncOverlayHistory();
}

/* ---------------------------------------------------------------- Files panel */
/* Every attachment in the conversation, newest first, each with who sent it, when, and a link to its message. */
function conversationFiles(ch) {
  const out = [];
  for (const m of ch.msgs) {
    (m.files || []).forEach((f) => out.push({ f, m, link: m.id }));
    /* A reply's file links to that exact reply (the thread opens on it), not to the thread's first message. */
    (m.thread || []).forEach((r, i) => (r.files || []).forEach((f) => out.push({ f, m: r, link: `${m.id}-r${i}`, inThread: true })));
  }
  return out.reverse();
}
/* Preview opens an image in the viewer (or another file in a new tab); Download saves it; Open source message goes to
   the message (or exact reply) that carries it. */
function previewAction(f, m, link) {
  const label = `aria-label="Preview ${esc(f.name)}"`;
  if (!isImage(f)) return `<a class="alink" href="${esc(f.url)}" target="_blank" rel="noopener" ${label}>Preview</a>`;
  const i = (m.files || []).filter(isImage).indexOf(f);
  return `<button class="alink" type="button" data-act="chat-att-view" data-id="${esc(link)}" data-i="${i}" ${label}>Preview</button>`;
}
function fileRow({ f, m, link, inThread }, chId) {
  const thumb = isImage(f) ? `<span class="chat-att-thumb"><img src="${esc(f.url)}" alt="" loading="lazy"></span>` : `<span class="chat-att-thumb">${fileIcon(f)}</span>`;
  const where = inThread ? " · in a thread" : "";
  return `<li class="chat-file-row">${thumb}
    <span class="chat-att-meta"><span class="chat-att-name">${esc(f.name)}</span>
      <span class="chat-att-sub">${esc(M[m.who].name)} · ${esc(sentAt(m))}${where} · ${esc(fileFacts(f))}</span>
      <span class="chat-file-acts"><a class="alink" href="#/chat/${esc(chId)}/${esc(link)}" data-act="chat-file-open" aria-label="Open source message for ${esc(f.name)}">Open source message</a>
        ${previewAction(f, m, link)}<a class="alink" href="${esc(f.url)}" download="${esc(f.name)}" aria-label="Download ${esc(f.name)}">Download</a></span></span></li>`;
}
function filesPanelHtml(ch, chId, title, o) {
  if (!o.open) return "";
  const rows = conversationFiles(ch);
  const empty = `<p class="chat-files-empty">No files here yet.${ch.kind === "a2a" ? "" : " Attach images and files from the composer."}</p>`;
  const list = rows.length ? `<ul class="chat-file-list">${rows.map((r) => fileRow(r, chId)).join("")}</ul>` : empty;
  return `<div class="thread-scrim" data-act="chat-files" data-scrim aria-hidden="true"></div>
    <aside class="thread chat-files-panel" id="chat-files"${o.modal} aria-labelledby="chat-files-title"><header class="side-panel-header"><div class="thread-title"><h2 id="chat-files-title" tabindex="-1">Files</h2><span class="muted">${esc(title)} · ${rows.length}</span></div><button class="btn ghost icon" type="button" data-act="chat-files" aria-label="Close files">${ic("x", "ic20")}</button></header>
      ${list}</aside>`;
}
const filesButton = (open, title) => `<button class="btn ghost icon chat-files-btn" type="button" data-act="chat-files" aria-expanded="${open}" aria-controls="chat-files" aria-label="Files in ${esc(title)}" title="Files in ${esc(title)}">${ic("files", "ic20")}</button>`;

const VIEW_CLICKS = { "chat-att-view": openViewer };

export { VIEW_CLICKS, attachmentNameHit, closeViewer, filesButton, filesPanelHtml, messageAttachmentsHtml, viewerOpen };
