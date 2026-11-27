/* Chat composer attachments: Attach button, picker, drag and drop, pasted images, the tray above the field,
   a client-side upload state and size validation. Pending attachments are kept per composer (like drafts)
   while the page stays loaded, so a re-render never loses them. */
import { LIMIT_TEXT, PICKER_ACCEPT, altOf, fileFacts, fileIcon, isImage, rejectReason } from "./chat-attach-files.js";
import {
  bindRefHighlight, deleteTokenBeforeCaret, insertTokens, labelledIds, markFieldUsed, refLabels, refTag, relabelMap, rewriteTokens,
} from "./chat-attach-refs.js";
import { esc, ic, plural } from "../ui.js";

const pending = new Map(); /* composer key -> { items: [{ id, name, size, type, url, status, progress }], errors: [] } */
let seq = 0;

/* The main composer is keyed by conversation, a thread composer by its root message. */
const composerKey = (form) => (form.dataset.form === "thread-send" ? `t:${form.dataset.ch}:${form.dataset.id}` : `c:${form.dataset.id}`);
const isComposerField = (t) => t?.matches?.("form.chat-compose textarea[name=message]");
const stateOf = (key) => {
  if (!pending.has(key)) pending.set(key, { items: [], errors: [] });
  return pending.get(key);
};

/* ---------------------------------------------------------------- markup */
const attachButton = (field) => `<button class="btn ghost icon chat-attach-btn" type="button" data-act="chat-attach" aria-label="Attach files, ${LIMIT_TEXT}" title="Attach files (${LIMIT_TEXT})" aria-describedby="${field}-help">${ic("paperclip", "ic20")}</button>
      <input class="chat-att-input" type="file" multiple accept="${PICKER_ACCEPT}" tabindex="-1" aria-hidden="true" hidden>`;

function statusText(it) {
  return it.status === "ready" ? "Ready" : `Uploading… ${Math.round(it.progress)}%`;
}
function trayItem(it, key, label) {
  const thumb = isImage(it) ? `<img src="${esc(it.url)}" alt="">` : fileIcon(it);
  const ready = it.status === "ready";
  return `<li class="chat-att-item${ready ? "" : " is-uploading"}" data-att="${esc(it.id)}">
    <span class="chat-att-thumb">${thumb}</span>
    <span class="chat-att-meta"><span class="chat-att-line">${refTag(label)}<span class="chat-att-name" title="${esc(it.name)}">${esc(it.name)}</span></span>
      <span class="chat-att-sub">${esc(fileFacts(it))} · <span data-att-status>${statusText(it)}</span></span>
      <span class="chat-att-bar" role="progressbar" aria-label="Uploading ${esc(it.name)}" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${Math.round(it.progress)}"${ready ? " hidden" : ""}><span style="--p:${it.progress}%"></span></span></span>
    <button class="btn ghost icon chat-att-remove" type="button" data-act="chat-att-remove" data-id="${esc(it.id)}" data-v="${esc(key)}" aria-label="Remove ${esc(it.name)}, ${esc(label)}" title="Remove">${ic("x", "ic16")}</button>
  </li>`;
}
function trayInner(key) {
  const st = stateOf(key);
  const labels = refLabels(st.items);
  const items = st.items.length ? `<ul class="chat-att-tray" aria-label="Attachments to send">${st.items.map((it, i) => trayItem(it, key, labels[i])).join("")}</ul>` : "";
  const errors = st.errors.map((e) => `<p class="chat-att-error">${ic("warn", "ic14")}<span>${esc(e)}</span></p>`).join("");
  return `${items}<div class="chat-att-errors" role="alert">${errors}</div><span class="sr-only" aria-live="polite" data-att-live></span>`;
}
const trayHtml = (key) => `<div class="chat-att-wrap" data-att-key="${esc(key)}">${trayInner(key)}</div>`;

/* ---------------------------------------------------------------- state changes */
const wrapFor = (form) => form.querySelector(".chat-att-wrap");
function redrawTray(form) {
  const wrap = wrapFor(form);
  if (wrap) wrap.innerHTML = trayInner(composerKey(form));
}
function announce(form, text) {
  const live = form.querySelector("[data-att-live]");
  if (live) live.textContent = text;
}
/* Patch one row in place while it uploads; a later render draws it from state. */
function patchItem(key, it) {
  const row = document.querySelector(`.chat-att-wrap[data-att-key="${CSS.escape(key)}"] [data-att="${CSS.escape(it.id)}"]`);
  if (!row) return;
  const ready = it.status === "ready", bar = row.querySelector(".chat-att-bar");
  row.classList.toggle("is-uploading", !ready);
  row.querySelector("[data-att-status]").textContent = statusText(it);
  bar.setAttribute("aria-valuenow", String(Math.round(it.progress)));
  bar.firstElementChild.style.setProperty("--p", `${it.progress}%`);
  bar.hidden = ready;
  if (ready) announce(row.closest("form"), `${it.name} is ready to send.`);
}
/* Prototype upload: progress in steps scaled to the size, then ready. */
function simulateUpload(key, it) {
  const step = Math.max(5, 18 - Math.round(it.size / (1024 * 1024)));
  const timer = setInterval(() => {
    if (!stateOf(key).items.includes(it)) { clearInterval(timer); return; }
    it.progress = Math.min(100, it.progress + step);
    if (it.progress >= 100) { it.status = "ready"; clearInterval(timer); }
    patchItem(key, it);
  }, 120);
}
function addOne(key, file) {
  const st = stateOf(key), why = rejectReason(file, st.items.length);
  if (why) { st.errors.push(why); return null; }
  const it = { id: `att${++seq}`, name: file.name || `pasted-image-${seq}.png`, size: file.size, type: file.type, url: URL.createObjectURL(file), status: "uploading", progress: 0 };
  st.items.push(it);
  return it;
}
function addFiles(form, files) {
  const key = composerKey(form), st = stateOf(key);
  st.errors = [];
  const added = [...files].map((f) => addOne(key, f)).filter(Boolean);
  redrawTray(form);
  added.forEach((it) => simulateUpload(key, it));
  if (!added.length) return;
  /* Each new attachment is referenced where the owner was writing: "[Image 1]" at the caret. */
  const labels = labelledIds(st.items).filter((x) => added.some((it) => it.id === x.id)).map((x) => x.label);
  insertTokens(form.elements.message, labels);
  announce(form, `${plural(added.length, "file", "files")} attached as ${labels.join(", ")}. Uploading…`);
}
function removeAttachment(key, id) {
  const st = stateOf(key), i = st.items.findIndex((it) => it.id === id);
  if (i < 0) return;
  const before = labelledIds(st.items);
  const [it] = st.items.splice(i, 1);
  URL.revokeObjectURL(it.url);
  st.errors = [];
  const form = document.querySelector(`.chat-att-wrap[data-att-key="${CSS.escape(key)}"]`)?.closest("form");
  if (!form) return;
  /* Its token leaves the text, and later ones renumber with the tray (Image 3 becomes Image 2). */
  rewriteTokens(form.elements.message, relabelMap(before, labelledIds(st.items)));
  redrawTray(form);
  announce(form, `Removed ${before[i].label}, ${it.name}.`);
  /* Focus moves to the next attachment's Remove, else the previous one, else the message field. */
  const buttons = form.querySelectorAll(".chat-att-remove");
  (buttons[Math.min(i, buttons.length - 1)] || form.elements.message).focus();
}

/* What a send takes: the ready attachments, or that some are still uploading. Sending clears the tray. */
function attachmentsForSend(form) {
  const st = stateOf(composerKey(form));
  const busy = st.items.filter((it) => it.status !== "ready").length;
  return { busy, files: busy ? [] : st.items.map(({ id, name, size, type, url }) => ({ id, name, size, type, url, alt: altOf({ name }) })) };
}
function clearAttachments(form) {
  pending.set(composerKey(form), { items: [], errors: [] });
  redrawTray(form);
}

/* ---------------------------------------------------------------- binding: picker, paste, drag and drop */
function openPicker(_id, t) {
  t.closest("form")?.querySelector(".chat-att-input")?.click();
}
function onPickerChange(e) {
  const input = e.target;
  if (!input.classList?.contains("chat-att-input") || !input.files?.length) return;
  addFiles(input.form, input.files);
  input.value = "";
}
/* A pasted image attaches; pasted text still pastes. */
function onPaste(e) {
  const form = e.target.closest?.("form.chat-compose");
  const files = [...(e.clipboardData?.files || [])];
  if (!form || !files.length) return;
  if (isComposerField(e.target)) markFieldUsed(e.target); /* a paste lands at the caret */
  if (!e.clipboardData.types.includes("text/plain")) e.preventDefault();
  addFiles(form, files);
}
/* Drop on the open thread attaches to its reply; anywhere else in the conversation, to the main composer. */
function dropTarget(root, target) {
  const zone = target.closest?.("aside.thread:not(.chat-files-panel, .chat-members-panel)") || root.querySelector(".chmain");
  const form = zone?.querySelector("form.chat-compose");
  return form ? { zone, form } : null;
}
const hasFiles = (e) => [...(e.dataTransfer?.types || [])].includes("Files");
function showDrop(root, zone) {
  root.querySelectorAll(".chat-drop").forEach((el) => { if (el.parentElement !== zone) el.remove(); });
  if (!zone || zone.querySelector(":scope > .chat-drop")) return;
  const title = zone.matches("aside.thread") ? "this thread" : document.getElementById("ch-title")?.textContent || "this conversation";
  zone.insertAdjacentHTML("beforeend", `<div class="chat-drop" aria-hidden="true"><p>${ic("paperclip", "ic20")}<b>Drop to attach to ${esc(title)}</b><span>Images and files, ${LIMIT_TEXT}</span></p></div>`);
}
function clearDrop(root, st) {
  st.depth = 0;
  root.querySelectorAll(".chat-drop").forEach((el) => el.remove());
}
function onDragOver(root, e) {
  if (!hasFiles(e)) return;
  e.preventDefault();
  const t = dropTarget(root, e.target);
  e.dataTransfer.dropEffect = t ? "copy" : "none"; /* an agents' conversation has no composer to take files */
  showDrop(root, t?.zone);
}
function onDrop(root, st, e) {
  if (!hasFiles(e)) return;
  e.preventDefault();
  clearDrop(root, st);
  const t = dropTarget(root, e.target);
  if (!t) return;
  addFiles(t.form, e.dataTransfer.files);
  t.form.elements.message.focus();
}
function bindDrop(root) {
  const st = { depth: 0 };
  root.addEventListener("dragenter", (e) => { if (hasFiles(e)) { st.depth += 1; showDrop(root, dropTarget(root, e.target)?.zone); } });
  root.addEventListener("dragleave", () => { st.depth -= 1; if (st.depth <= 0) clearDrop(root, st); });
  root.addEventListener("dragover", (e) => onDragOver(root, e));
  root.addEventListener("drop", (e) => onDrop(root, st, e));
}
/* Deleting a token un-links its attachment, which stays in the tray and still sends; the tray's Remove drops it. */
function onTokenBackspace(e) {
  if (e.key !== "Backspace" || e.isComposing || e.defaultPrevented || !isComposerField(e.target)) return;
  const field = e.target, form = field.form, labels = refLabels(stateOf(composerKey(form)).items);
  const before = field.value.slice(0, field.selectionStart);
  if (!labels.some((l) => before.endsWith(`[${l}]`))) return;
  e.preventDefault();
  const label = deleteTokenBeforeCaret(field);
  if (label) announce(form, `${label} is no longer referenced in your message. It still sends; remove it from the attachments to leave it out.`);
}
function bindAttachments(root) {
  root.addEventListener("change", onPickerChange);
  root.addEventListener("paste", onPaste);
  root.addEventListener("keydown", onTokenBackspace);
  /* The caret counts once the owner has typed or clicked in the field. */
  const used = (e) => { if (isComposerField(e.target)) markFieldUsed(e.target); };
  root.addEventListener("pointerdown", used);
  root.addEventListener("keyup", used);
  bindDrop(root);
  bindRefHighlight(root);
}

const ATTACH_CLICKS = {
  "chat-attach": openPicker,
  "chat-att-remove": (id, t) => removeAttachment(t.dataset.v, id),
};

export { ATTACH_CLICKS, attachButton, attachmentsForSend, bindAttachments, clearAttachments, composerKey, trayHtml };
