/* A feature's documentation on its page (features UX doc-43 §1, §2, FTR-11): the definition by section, with its
   canonical path and revision, edited in place; the designs, decisions and reports beside it; and its history. */
import { featureDocs, featureMeta, featurePath } from "../docs-data.js";
import { S } from "../state.js";
import { esc, ic, sectionHead } from "../ui.js";
import { docHref, historyHtml } from "./docs.js";
import { md } from "./md.js";
import { KIND_ICON, kindTitle } from "../docs-data.js";

Object.assign(S, { featureEditing: null });

const SECTIONS = ["Purpose", "How it works", "Rules", "Known limitations", "Non-goals", "Acceptance criteria"];
export const FEATURE_TEMPLATE = SECTIONS.map((s) => `## ${s}\n`).join("\n");

function splitSections(text) {
  const out = [], lead = [];
  let cur = null;
  for (const line of String(text || "").split("\n")) {
    const m = line.match(/^##\s+(.*)$/);
    if (m) { cur = { title: m[1].trim(), lines: [] }; out.push(cur); } else (cur ? cur.lines : lead).push(line);
  }
  return { lead: lead.join("\n").trim(), sections: out.map((s) => ({ title: s.title, text: s.lines.join("\n").trim() })) };
}

function definitionBody(f) {
  const { lead, sections } = splitSections(f.body_markdown);
  if (!lead && !sections.length) return `<p class="plan-body is-empty">No definition written yet. Edit it to describe what the feature does, its rules and its limits.</p>`;
  const blocks = sections.map((s) => `<div class="feat-def-sec"><h3>${esc(s.title)}</h3>${s.text ? `<div class="proc-md">${md(s.text, 4)}</div>` : `<p class="proc-muted">Not written yet.</p>`}</div>`).join("");
  const missing = SECTIONS.filter((t) => !sections.some((s) => s.title === t));
  return `${lead ? `<div class="proc-md">${md(lead, 4)}</div>` : ""}<div class="feat-def">${blocks}</div>${missing.length && sections.length ? `<p class="proc-muted feat-def-missing">No section yet for ${esc(missing.join(", "))}.</p>` : ""}`;
}

function editorHtml(f) {
  const meta = featureMeta(f.id);
  const text = f.body_markdown && /^##\s/m.test(f.body_markdown) ? f.body_markdown : `## Purpose\n${f.body_markdown || ""}\n\n` + FEATURE_TEMPLATE.split("\n").slice(2).join("\n");
  return `<form class="work-task-form plan-form doc-editor" id="feature-def-form" data-doc-form="feature" data-id="${esc(f.id)}" novalidate aria-label="Edit the definition">
    <label>Text<textarea class="field proc-editor" name="body" rows="18">${esc(text)}</textarea><span class="plan-hint">Markdown. Keep the headings: Purpose, How it works, Rules, Known limitations, Non-goals, Acceptance criteria.</span></label>
    <label>What changed<input class="field" name="note" autocomplete="off" placeholder="One line for the history"></label>
    <div class="doc-editor-actions"><button class="btn outline" type="button" data-doc-act="feature-cancel" data-id="${esc(f.id)}">Cancel</button><button class="btn primary" type="submit">Save rev ${meta.revision + 1}</button></div>
  </form>`;
}

function featureDefinitionHtml(f) {
  const meta = featureMeta(f.id), editing = S.featureEditing === f.id;
  const right = editing ? "" : `<button class="btn outline plan-btn" type="button" data-doc-act="feature-edit" data-id="${esc(f.id)}">${ic("pencil", "ic16")}Edit definition</button>`;
  return `<section class="plan-sec" aria-labelledby="plan-def-h">${sectionHead({ id: "plan-def-h", title: "Definition", count: `rev ${meta.revision}`, right })}
    <p class="feat-path">${ic("file-text", "ic14")}<code>${esc(featurePath(f))}</code><span>Updated ${esc(meta.history[0]?.at || "")}</span></p>
    ${editing ? editorHtml(f) : definitionBody(f)}</section>`;
}

function supportRow(e) {
  return `<li class="linked plan-row proc-row doc-row"><span class="proc-row-ic" aria-hidden="true">${ic(KIND_ICON[e.kind] || "file-text", "ic20")}</span>
    <div class="grow"><p class="rt"><a class="stretch" href="${docHref(e)}">${esc(e.title)}</a></p>${e.summary ? `<p class="dev-summary">${esc(e.summary)}</p>` : ""}
    <div class="meta"><span>${esc(kindTitle(e.project_id, e.kind))}</span><span>rev ${e.revision}</span><span>Updated ${esc(e.updated)}</span></div></div>${ic("chev-right", "ic20 chev")}</li>`;
}
function featureSupportHtml(f) {
  const list = featureDocs(f.id);
  const right = `<button class="btn outline plan-btn" type="button" data-doc-act="new" data-id="${esc(f.project_id)}" data-feature="${esc(f.id)}">${ic("plus", "ic16")}New document</button>`;
  return `<section class="plan-sec" aria-labelledby="plan-sup-h">${sectionHead({ id: "plan-sup-h", title: "Designs, decisions and reports", count: list.length ? String(list.length) : "", right })}
    ${list.length ? `<ul class="rows plan-rows feat-support" role="list">${list.map(supportRow).join("")}</ul>`
      : `<p class="plan-empty">Nothing beside this feature yet. A design explains how it is built, a decision records a choice, a report keeps findings.</p>`}</section>`;
}
function featureHistoryHtml(f) {
  const meta = featureMeta(f.id);
  return `<section class="plan-sec" aria-labelledby="plan-hist-h">${sectionHead({ id: "plan-hist-h", title: "History", count: `${meta.history.length} revisions` })}${historyHtml(meta.history)}</section>`;
}

export { featureDefinitionHtml, featureHistoryHtml, featureSupportHtml };
