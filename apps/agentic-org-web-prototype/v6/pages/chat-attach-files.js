/* Chat attachments: what a file is, as the composer, the stream and the Files panel describe it.
   Prototype: content lives in object URLs for this page load; the product stores verified, immutable content
   (doc-40 §4) and applies the conversation's visibility to it. */
import { esc, ic } from "../ui.js";

const MB = 1024 * 1024;
const MAX_FILE_BYTES = 25 * MB;
const MAX_FILES = 10;
const LIMIT_TEXT = "up to 25 MB each";
const PICKER_ACCEPT = "image/*,.pdf,.txt,.log,.md,.csv,.json,.diff,.patch,.zip,.gz,.doc,.docx,.xls,.xlsx,.ppt,.pptx";

function fmtSize(bytes) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < MB) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / MB).toFixed(1)} MB`;
}
const extOf = (name) => (/\.([a-z0-9]{1,8})$/i.exec(name || "")?.[1] || "").toLowerCase();
const isImage = (f) => /^image\//.test(f.type || "");
function typeLabel(f) {
  const ext = extOf(f.name);
  if (ext) return ext.toUpperCase();
  return isImage(f) ? "Image" : "File";
}
const TEXT_EXT = new Set(["pdf", "txt", "log", "md", "csv", "json", "diff", "patch", "doc", "docx", "xls", "xlsx", "ppt", "pptx"]);
const ARCHIVE_EXT = new Set(["zip", "gz", "tgz", "tar", "7z"]);
function fileIconName(f) {
  if (isImage(f)) return "image";
  const ext = extOf(f.name);
  if (ARCHIVE_EXT.has(ext)) return "archive";
  return TEXT_EXT.has(ext) ? "file-text" : "file";
}
/* A type mark: the icon plus a short type word, so a PDF and a log differ by more than colour. */
const fileIcon = (f, cls = "ic20") => `<span class="chat-att-icon" data-type="${esc(extOf(f.name) || "file")}" aria-hidden="true">${ic(fileIconName(f), cls)}</span>`;
const altOf = (f) => f.alt || f.name;
/* "1.2 MB · PDF": size first, the quickest way to tell two copies apart. */
const fileFacts = (f) => `${fmtSize(f.size)} · ${typeLabel(f)}`;

/* Why a file can't be attached, or "" when it can. */
function rejectReason(file, count) {
  if (count >= MAX_FILES) return `“${file.name}” wasn't attached: a message can carry up to ${MAX_FILES} files.`;
  if (file.size > MAX_FILE_BYTES) return `“${file.name}” wasn't attached: it's ${fmtSize(file.size)}, and files can be up to 25 MB.`;
  if (file.size === 0) return `“${file.name}” wasn't attached: the file is empty.`;
  return "";
}

export { LIMIT_TEXT, MAX_FILE_BYTES, PICKER_ACCEPT, altOf, fileFacts, fileIcon, fmtSize, isImage, rejectReason, typeLabel };
