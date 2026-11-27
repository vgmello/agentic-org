/* New channel: the Channels heading's "+" and New message's "Create a channel" open a modal dialog (the same native
   top-layer dialog as Work's New task). Name (formatted as you type to lowercase-hyphen, shown after "#", required
   and unique), optional description (the header's "#name | description"), project and members (the owner is always
   in), and a Private switch. A new channel opens on its intro ("This is the start of #name" and Add members).
   Mock: created channels are kept for this browser session. */
import { CH, M, P } from "../data.js";
import { render } from "../shell/router.js";
import { av, esc, ic, kindWord, memberIds, plural, toast } from "../ui.js";

const STORE = "v6-chat-channels";
const OWNER = Object.keys(M).find((k) => M[k].owner);
const MAX_NAME = 80;

/* ---------------------------------------------------------------- names */
/* "Design Review!" -> "design-review"; a trailing hyphen stays while typing, so "design-" can become "design-review". */
const formatName = (raw) => raw.toLowerCase().replace(/[\s_.]+/g, "-").replace(/[^a-z0-9-]/g, "").replace(/-{2,}/g, "-").replace(/^-+/, "").slice(0, MAX_NAME);
const finalName = (raw) => formatName(raw).replace(/-+$/, "");
const channelNamed = (name) => Object.keys(CH).find((k) => CH[k].kind === "channel" && CH[k].name === name);
function nameError(name, submitting) {
  if (!name) return submitting ? "Enter a name for the channel." : "";
  return channelNamed(name) ? `#${name} already exists. Choose another name.` : "";
}

/* ---------------------------------------------------------------- session store */
const validStored = (c) => c && typeof c.id === "string" && typeof c.name === "string" && finalName(c.name) === c.name && c.name && !CH[c.id] && !channelNamed(c.name);
function toChannel(c) {
  const members = [OWNER, ...(Array.isArray(c.members) ? c.members : []).filter((k) => M[k] && k !== OWNER)];
  const proj = typeof c.proj === "string" && P[c.proj] ? c.proj : undefined;
  return { name: c.name, kind: "channel", topic: typeof c.topic === "string" ? c.topic.slice(0, 250) : "", proj, members, private: c.private === true, created: true, msgs: [] };
}
function loadChannels() {
  let saved = [];
  try { saved = JSON.parse(sessionStorage.getItem(STORE) || "[]"); } catch (e) { saved = []; }
  if (!Array.isArray(saved)) return;
  for (const c of saved) if (validStored(c)) CH[c.id] = toChannel(c);
}
function saveChannels() {
  const rows = Object.keys(CH).filter((k) => CH[k].created).map((id) => {
    const { name, topic, proj, members, private: priv } = CH[id];
    return { id, name, topic, proj, members, private: priv };
  });
  try { sessionStorage.setItem(STORE, JSON.stringify(rows)); } catch (e) { /* kept for this page view only */ }
}
loadChannels();

/* ---------------------------------------------------------------- the form */
const optional = '<span class="chat-channel-opt">(optional)</span>';
function memberOption(k, checked = false) {
  const owner = k === OWNER;
  return `<li><label class="chat-channel-member">
      <input type="checkbox" name="members" value="${esc(k)}"${owner || checked ? " checked" : ""}${owner ? " disabled" : ""}>
      ${av(k, 24)}<span class="chat-row-name">${esc(owner ? "You" : M[k].name)}</span><span class="chat-row-word">${esc(kindWord(k))}</span></label></li>`;
}
const peopleFirst = (ids) => [...ids.filter((k) => M[k].kind === "person"), ...ids.filter((k) => M[k].kind !== "person")];
function membersField(ids, legend, hint) {
  return `<fieldset class="chat-channel-members"><legend>${legend}</legend>${hint ? `<p class="chat-channel-hint">${hint}</p>` : ""}
      <ul class="chat-channel-who">${ids.map((k) => memberOption(k)).join("")}</ul></fieldset>`;
}
function nameField() {
  return `<div class="chat-channel-field"><label for="chat-channel-name">Name</label>
      <div class="chat-channel-name"><span class="chat-channel-hash" aria-hidden="true">#</span>
        <input class="field" id="chat-channel-name" name="name" maxlength="${MAX_NAME}" autocomplete="off" spellcheck="false" placeholder="e.g. release-notes" aria-describedby="chat-channel-name-error chat-channel-name-hint" aria-invalid="false"></div>
      <p class="chat-channel-error" id="chat-channel-name-error" aria-live="polite"></p>
      <p class="chat-channel-hint" id="chat-channel-name-hint">Lowercase letters, numbers and hyphens.</p></div>`;
}
function projectField() {
  const projects = Object.entries(P).filter(([, p]) => !p.archived);
  return `<label><span>Project ${optional}</span><select class="field" name="proj"><option value="">No project</option>${projects.map(([id, p]) => `<option value="${esc(id)}">${esc(p.name)}</option>`).join("")}</select></label>`;
}
function privateField() {
  return `<div class="chat-channel-private">
      <span class="chat-channel-private-text"><span id="chat-channel-private-label">Private channel</span>
        <span class="chat-channel-hint" id="chat-channel-private-desc">Only members can find it and read it.</span></span>
      <button class="switch" type="button" role="switch" id="chat-channel-private" aria-checked="false" aria-labelledby="chat-channel-private-label" aria-describedby="chat-channel-private-desc" data-act="chat-channel-private"></button></div>`;
}
function dialogShell(id, title, body, submit) {
  return `<dialog class="work-task-dialog chat-channel-dialog" id="${id}" aria-labelledby="${id}-title">
    <h2 id="${id}-title">${esc(title)}</h2>
    <form class="work-task-form chat-channel-form" novalidate>${body}
      <div class="actions"><button class="btn outline" type="button" data-act="chat-channel-cancel">Cancel</button><button class="btn primary" type="submit">${esc(submit)}</button></div>
    </form></dialog>`;
}
function createDialogHtml() {
  const body = nameField()
    + `<label><span>Description ${optional}</span><input class="field" name="topic" maxlength="250" autocomplete="off" placeholder="What is this channel about?"></label>`
    + projectField()
    + membersField([OWNER, ...peopleFirst(memberIds())], "Members", "You are always a member. Add people and agents now or later.")
    + privateField();
  return dialogShell("chat-channel-dialog", "Create a channel", body, "Create channel");
}

/* ---------------------------------------------------------------- dialog lifecycle */
/* The dialog sits in #overlay-root, which the narrow conversation drawer makes inert; a modal dialog above the drawer
   must not be. Focus returns to the trigger unless the dialog finished by navigating (`done`). */
function showDialog(html, trigger, handlers) {
  const root = document.getElementById("overlay-root");
  root.inert = false;
  root.insertAdjacentHTML("beforeend", html);
  const dialog = root.lastElementChild, state = { done: false };
  dialog.addEventListener("close", () => { dialog.remove(); if (!state.done && trigger.isConnected) trigger.focus(); }, { once: true });
  dialog.addEventListener("click", (e) => onDialogClick(e, dialog));
  dialog.addEventListener("input", (e) => handlers.input?.(e, dialog));
  dialog.addEventListener("submit", (e) => { e.preventDefault(); handlers.submit(dialog, state); });
  dialog.showModal();
  dialog.querySelector("input:not([disabled]), button")?.focus();
  return dialog;
}
function onDialogClick(e, dialog) {
  const t = e.target.closest("[data-act]");
  if (t?.dataset.act === "chat-channel-cancel") dialog.close();
  if (t?.dataset.act === "chat-channel-private") t.setAttribute("aria-checked", String(t.getAttribute("aria-checked") !== "true"));
}
function showNameError(dialog, message) {
  const input = dialog.querySelector("#chat-channel-name");
  dialog.querySelector("#chat-channel-name-error").textContent = message;
  input.setAttribute("aria-invalid", String(Boolean(message)));
  return input;
}
/* Typing formats the name in place (keeping the caret at the same spot in the formatted text) and checks it is free. */
function onNameInput(e, dialog) {
  const input = e.target;
  if (input.id !== "chat-channel-name") return;
  const before = formatName(input.value.slice(0, input.selectionStart)).length, formatted = formatName(input.value);
  if (formatted !== input.value) { input.value = formatted; input.setSelectionRange(before, before); }
  showNameError(dialog, nameError(finalName(formatted), false));
}
const chosenMembers = (form) => [...form.querySelectorAll('input[name="members"]:checked:not([disabled])')].map((i) => i.value);
function createChannel(form, name) {
  const id = CH["c-" + name] ? `c-${name}-${Date.now()}` : "c-" + name;
  CH[id] = toChannel({
    name, topic: form.elements.topic.value.trim(), proj: form.elements.proj.value, members: chosenMembers(form),
    private: form.querySelector("#chat-channel-private").getAttribute("aria-checked") === "true",
  });
  saveChannels();
  return id;
}
function submitCreate(dialog, state, onCreated) {
  const form = dialog.querySelector("form"), input = form.elements.name, name = finalName(input.value);
  input.value = name;
  const error = nameError(name, true);
  if (error) { showNameError(dialog, error).focus(); return; }
  const id = createChannel(form, name);
  state.done = true;
  dialog.close();
  onCreated(id);
}
/* `onCreated(id)`: the caller opens the new channel and places focus (the composer). */
function openNewChannel(trigger, onCreated) {
  showDialog(createDialogHtml(), trigger, { input: onNameInput, submit: (dialog, state) => submitCreate(dialog, state, onCreated) });
}

/* ---------------------------------------------------------------- the new channel's intro and Add members */
function channelIntroHtml(ch) {
  const made = ch.created ? `<p>You created this channel today.${ch.private ? " It is private: only its members can find it and read it." : ""}</p>` : "";
  return `<div class="chat-empty chat-channel-intro"><span class="chat-channel-intro-ic">${ic(ch.private ? "lock" : "hash", "ic20")}</span>
    <h2>This is the start of #${esc(ch.name)}</h2>${ch.topic ? `<p class="chat-channel-intro-desc">${esc(ch.topic)}</p>` : ""}${made}
    <div class="actions"><button class="btn outline sm" type="button" data-act="chat-add-members">${ic("user-plus", "ic16")}Add members</button></div></div>`;
}
function addMembersHtml(ch) {
  const ids = peopleFirst(memberIds().filter((k) => !(ch.members || []).includes(k)));
  const body = ids.length ? membersField(ids, "People and agents", "")
    : `<p class="chat-channel-hint">Everyone in the organization is already in #${esc(ch.name)}.</p>`;
  return dialogShell("chat-members-dialog", `Add members to #${ch.name}`, body + '<p class="chat-channel-error" id="chat-members-error" aria-live="polite"></p>', "Add");
}
function submitMembers(dialog, state, chId) {
  const ch = CH[chId], added = chosenMembers(dialog.querySelector("form"));
  if (!dialog.querySelector('input[name="members"]')) { dialog.close(); return; }
  if (!added.length) { dialog.querySelector("#chat-members-error").textContent = "Choose at least one person or agent."; return; }
  ch.members = [...(ch.members || []), ...added];
  if (ch.created) saveChannels();
  state.done = true;
  dialog.close();
  render(false);
  document.querySelector('.chat [data-act="chat-add-members"]')?.focus();
  toast(`Added ${plural(added.length, "member", "members")} to #${ch.name}`, "check-circle");
}
function openAddMembers(trigger, chId) {
  if (CH[chId]?.kind !== "channel") return;
  showDialog(addMembersHtml(CH[chId]), trigger, { submit: (dialog, state) => submitMembers(dialog, state, chId) });
}

export { channelIntroHtml, formatName, openAddMembers, openNewChannel };
