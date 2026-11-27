/* A small, safe Markdown reader for the mock's documents: headings, paragraphs, lists, code fences, inline code
   and bold. Everything is escaped first; nothing in a document can add markup or run script. */
import { esc } from "../ui.js";

const inline = (text) => esc(text)
  .replace(/`([^`]+)`/g, "<code>$1</code>")
  .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");

function listBlock(lines, ordered) {
  const tag = ordered ? "ol" : "ul", strip = ordered ? /^\d+\.\s+/ : /^[-*]\s+/;
  return `<${tag}>${lines.map((l) => `<li>${inline(l.replace(strip, ""))}</li>`).join("")}</${tag}>`;
}

function block(chunk, level) {
  const lines = chunk.split("\n");
  const first = lines[0];
  const heading = first.match(/^(#{2,4})\s+(.*)$/);
  if (heading) {
    const h = Math.min(6, level + heading[1].length - 2);
    const rest = lines.slice(1).join("\n").trim();
    return `<h${h}>${inline(heading[2])}</h${h}>${rest ? block(rest, level) : ""}`;
  }
  if (lines.every((l) => /^[-*]\s+/.test(l))) return listBlock(lines, false);
  if (lines.every((l) => /^\d+\.\s+/.test(l))) return listBlock(lines, true);
  return `<p>${lines.map(inline).join("<br>")}</p>`;
}

/* level: the heading level a "## " heading becomes inside the host page */
function md(text, level = 3) {
  if (!text) return "";
  const parts = String(text).replace(/\r/g, "").split(/^```.*$/m);
  return parts.map((part, i) => (i % 2
    ? `<pre><code>${esc(part.replace(/^\n|\n$/g, ""))}</code></pre>`
    : part.split(/\n{2,}/).map((c) => c.trim()).filter(Boolean).map((c) => block(c, level)).join(""))).join("");
}

/* The first paragraph under the first heading: the summary an agent sees before it retrieves the full text */
function mdSummary(text, max = 180) {
  const para = String(text || "").split(/\n{2,}/).map((c) => c.trim()).find((c) => c && !c.startsWith("#") && !/^[-*\d]/.test(c))
    || String(text || "").replace(/^#+\s+.*$/gm, "").trim();
  const flat = para.replace(/\s+/g, " ").replace(/`/g, "");
  return flat.length > max ? flat.slice(0, max - 1).replace(/\s+\S*$/, "") + "…" : flat;
}

/* Line comparison for "difference from the parent": unchanged, removed and added lines, in order */
function lineDiff(before, after) {
  const a = String(before || "").split("\n"), b = String(after || "").split("\n");
  const n = a.length, m = b.length, dp = Array.from({ length: n + 1 }, () => new Array(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
  const out = [];
  let i = 0, j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) { out.push([" ", a[i]]); i++; j++; }
    else if (dp[i + 1][j] >= dp[i][j + 1]) out.push(["-", a[i++]]);
    else out.push(["+", b[j++]]);
  }
  while (i < n) out.push(["-", a[i++]]);
  while (j < m) out.push(["+", b[j++]]);
  return out;
}

export { lineDiff, md, mdSummary };
