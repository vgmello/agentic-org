/* Inline references to a message's attachments. Adding a file while writing puts a token at the caret, "[Image 1]"
   for images and "[File 1]" for other files, numbered per kind in attachment order. The text keeps the plain token
   (so task context, the Inbox and agents read it as written); a sent message draws each token that matches one of
   its attachments as a chip that points at it. */
import { fileIconName, isImage } from "./chat-attach-files.js";
import { esc, ic } from "../ui.js";

const TOKEN_RE = /\[(Image|File) (\d+)\]/g;
const SPACED_TOKEN_RE = /( ?)\[(Image|File) (\d+)\]( ?)/g;
const TOKEN_BEFORE_CARET = /\[(Image|File) \d+\]$/;
const kindOf = (f) => (isImage(f) ? "Image" : "File");
const tokenOf = (label) => `[${label}]`;

/* Each attachment's label, numbered per kind in order: Image 1, Image 2, File 1. */
function refLabels(files) {
  const n = { Image: 0, File: 0 };
  return files.map((f) => {
    const k = kindOf(f);
    n[k] += 1;
    return `${k} ${n[k]}`;
  });
}
const labelledIds = (items) => refLabels(items).map((label, i) => ({ id: items[i].id, label }));
/* The labels a text refers to. */
const labelsIn = (text) => new Set([...(text || "").matchAll(TOKEN_RE)].map((m) => `${m[1]} ${m[2]}`));

/* ---------------------------------------------------------------- composer text */
/* After attachments change: old label -> new label, or "" for one that was removed. */
function relabelMap(before, after) {
  const now = new Map(after.map((x) => [x.id, x.label]));
  return new Map(before.map((x) => [x.label, now.get(x.id) || ""]));
}
/* Renumbers tokens in one pass (so "Image 2" -> "Image 1" never chains); a removed token takes one space with it.
   Tokens that name no attachment (typed by hand) are left as written. */
function renumberText(text, map) {
  return text.replace(SPACED_TOKEN_RE, (all, lead, k, n, trail) => {
    const to = map.get(`${k} ${n}`);
    if (to === undefined) return all;
    if (!to) return lead && trail ? " " : "";
    return `${lead}${tokenOf(to)}${trail}`;
  });
}
const notify = (field) => field.dispatchEvent(new Event("input", { bubbles: true }));
/* Writes through the editing command where it exists, so Undo takes the insertion back. */
function writeAt(field, start, end, str) {
  field.focus();
  field.setSelectionRange(start, end);
  const done = document.execCommand?.(str ? "insertText" : "delete", false, str);
  if (done) return;
  field.setRangeText(str, start, end, "end");
  notify(field);
}
/* Where a token goes: the caret once the field has been used, else the end of the draft (a drop on the stream). */
const usedFields = new WeakSet();
function insertRange(field) {
  if (usedFields.has(field)) return [field.selectionStart, field.selectionEnd];
  return [field.value.length, field.value.length];
}
/* "Look at this|" + a pasted image -> "Look at this [Image 1] |": spaced from the words around it. */
function insertTokens(field, labels) {
  if (!field || !labels.length) return;
  const [start, end] = insertRange(field), v = field.value;
  const lead = start && !/\s$/.test(v.slice(0, start)) ? " " : "";
  const trail = /^\s/.test(v.slice(end)) ? "" : " ";
  writeAt(field, start, end, lead + labels.map(tokenOf).join(" ") + trail);
}
function rewriteTokens(field, map) {
  if (!field || !map.size) return;
  const next = renumberText(field.value, map);
  if (next === field.value) return;
  field.value = next;
  notify(field);
}
/* Backspace just after a token deletes the whole token, never half of it. Returns the label deleted, or "". */
function deleteTokenBeforeCaret(field) {
  const at = field.selectionStart;
  if (at !== field.selectionEnd) return "";
  const m = TOKEN_BEFORE_CARET.exec(field.value.slice(0, at));
  if (!m) return "";
  writeAt(field, at - m[0].length, at, "");
  return m[0].slice(1, -1);
}
const markFieldUsed = (field) => usedFields.add(field);

/* ---------------------------------------------------------------- in a sent message */
function imageChip(m, files, i, label) {
  const f = files[i], n = files.filter(isImage).indexOf(f);
  return `<button class="chat-ref-chip" type="button" data-act="chat-att-ref" data-id="${esc(m.id)}" data-i="${n}" data-ref="${esc(label)}" aria-label="${esc(`${label}: ${f.alt || f.name}. Open image`)}">${ic("image", "ic14")}<span>${esc(label)}</span></button>`;
}
function fileChip(f, label) {
  return `<a class="chat-ref-chip" href="${esc(f.url)}" target="_blank" rel="noopener" data-ref="${esc(label)}" aria-label="${esc(`${label}: ${f.name}, opens in a new tab`)}">${ic(fileIconName(f), "ic14")}<span>${esc(label)}</span></a>`;
}
/* Message text (already escaped) with each matching token drawn as a chip. Outside a conversation (a shared
   preview) tokens stay plain text. */
function refChipsHtml(html, m, chId) {
  const files = m.files || [];
  if (!files.length || !chId) return html;
  const labels = refLabels(files);
  return html.replace(TOKEN_RE, (all, k, n) => {
    const i = labels.indexOf(`${k} ${n}`);
    if (i < 0) return all;
    return isImage(files[i]) ? imageChip(m, files, i, labels[i]) : fileChip(files[i], labels[i]);
  });
}
/* A sent attachment's label is shown only when the message's text refers to its attachments. */
function sentLabels(m) {
  const files = m.files || [], used = labelsIn(m.text);
  if (!used.size) return null;
  return new Map(refLabels(files).map((label, i) => [files[i], label]));
}
const refTag = (label) => (label ? `<span class="chat-att-tag">${esc(label)}</span>` : "");

/* ---------------------------------------------------------------- pointing: chip <-> attachment */
/* Hovering or focusing a chip marks the attachment it names, and the other way round. */
function setRefHit(el, on) {
  const msg = el?.closest(".msg");
  if (!msg) return;
  const ref = CSS.escape(el.dataset.ref);
  msg.querySelectorAll(`[data-ref="${ref}"]`).forEach((x) => { if (x !== el) x.classList.toggle("is-ref-hit", on); });
}
const refEl = (e) => e.target.closest?.(".msg [data-ref]");
function onRefOver(e) {
  const el = refEl(e);
  if (el && !el.contains(e.relatedTarget)) setRefHit(el, e.type === "pointerover" || e.type === "focusin");
}
function bindRefHighlight(root) {
  ["pointerover", "pointerout", "focusin", "focusout"].forEach((type) => root.addEventListener(type, onRefOver));
}

export {
  bindRefHighlight, deleteTokenBeforeCaret, insertTokens, labelledIds, markFieldUsed, refChipsHtml, refLabels, refTag, relabelMap,
  rewriteTokens, sentLabels,
};
