import { M, OFFICES, officeName } from "../data.js";
import { agentSetupFields } from "./teams.js";
import { searchResults } from "../shell/cmdk.js";
import { ARCHIVE_DAY_OPTIONS, CHAT_SEND_SHORTCUTS, S, THEME_OPTIONS, getOrgName } from "../state.js";
import { SEP, av, avStack, back, empty, esc, hdr, ic, mlink, pill, plural, presPill, seg } from "../ui.js";

const SET = [["general", "General", "cog"], ["offices", "Offices", "office"], ["agents", "Agent setup", "chip"], ["roles", "Roles", "id"], ["processes", "Processes", "refresh"], ["access", "Members and access", "key"], ["updates", "Updates", "download"], ["appearance", "Appearance", "adjustments"]];
const health = (a) => (M[a].offboarded ? "Offboarded" : M[a].setup.needs ? "Needs setup: " + M[a].setup.needs : M[a].paused ? "Paused" : "Ready");
const healthMark = (a) => { const h = health(a); const [icon, tone] = h === "Ready" ? ["check-circle", "green"] : (h === "Paused" || h === "Offboarded") ? ["pause", "gray"] : ["wrench", "amber"]; return pill(h, tone, icon); };
function settingsFrame(active, header, content) {
  const path = (key) => key === "general" ? "#/settings" : "#/settings/" + key;
  return `<div class="settings">
    ${header}
    <nav class="subnav" aria-label="Organization settings">${SET.map(([key, label, icon]) =>
      `<a href="${path(key)}"${active === key ? ' aria-current="page"' : ""}>${ic(icon, "ic20")}${label}</a>`).join("")}</nav>
    <div class="settings-picker">
      <label for="settings-section">Settings section</label>
      <select class="field" id="settings-section">${SET.map(([key, label]) =>
        `<option value="${path(key)}"${active === key ? " selected" : ""}>${label}</option>`).join("")}</select>
    </div>
    <div class="settings-body">${content}</div>
  </div>`;
}
function pageSettings(sec = "general", sub) {
  if (sec === "appearance") return pagePreferences();
  const label = SET.find(([key]) => key === sec)[1];
  let header = hdr(label, "Organization settings"), body;
  const agents = Object.keys(M).filter((k) => M[k].kind === "agent");
  if (sec === "general") body = `<form class="form" data-form="org-settings"><div><label for="o-name">Organization name</label><input class="field" id="o-name" name="name" value="${esc(getOrgName())}" required maxlength="80" aria-describedby="org-name-help"><p class="help" id="org-name-help">Shown in this prototype’s sidebar.</p></div>
    <div><label for="o-archive-days">Auto-archive closed tasks</label><select class="field" id="o-archive-days" name="archiveDays" aria-describedby="org-archive-help">${ARCHIVE_DAY_OPTIONS.map(([days, txt]) => `<option value="${days}"${S.archiveAfterDays === days ? " selected" : ""}>${esc(txt)}</option>`).join("")}</select><p class="help" id="org-archive-help">Closed tasks stay visible for recent context, then archive automatically across Work, Projects, Teams, and Agent Tasks. Toggle “Show archived” in Display to inspect them.</p></div>
    <div><label for="o-owner">Owner</label><input class="field" id="o-owner" value="You" disabled></div><div><label for="o-data">Data location</label><input class="field" id="o-data" value="This Mac · HQ office" disabled><p class="help">Change where things run in Offices.</p></div>
    <div><button class="btn primary" type="submit">Save changes</button></div></form>`;
  else if (sec === "offices") {
    header = hdr("Offices", "2 offices · 1 online");
    body = `<p class="desc">Where your agents run. HQ is this Mac; another office syncs with it.</p><ul class="rows settings-rows" role="list" style="margin-top:8px">${Object.entries(OFFICES).map(([k, o]) => {
    const here = agents.filter((a) => M[a].setup.office === k);
    return `<li>${ic("office", "ic20 chev")}<div class="grow"><p class="rt">${esc(o.name)}<span class="sec">${esc(o.machine)}</span></p><div class="meta"><span>${esc(o.note)}</span>${SEP}<span>${plural(here.length, "agent", "agents")}: ${here.map((a) => mlink(a)).join(", ")}</span></div><div style="margin-top:8px">${avStack(here, 10)}</div></div>${pill(o.status, o.tone, o.icon)}</li>`;
  }).join("")}</ul><div class="section"><button class="btn outline" type="button" data-act="proto" data-v="Add an office">${ic("plus", "ic20")}Add an office</button></div>`;
  } else if (sec === "agents" && M[sub]?.kind === "agent") {
    const agent = M[sub];
    header = hdr(agent.name, esc(agent.role) + " · Agent setup", `<a class="btn outline" href="#/members/${sub}/sessions">${ic("terminal", "ic20")}Sessions</a><a class="btn outline" href="#/members/${sub}">${ic("user", "ic20")}View profile</a>`);
    body = back("#/settings/agents", "Agent setup") + `<div class="section" style="padding-bottom:0">${healthMark(sub)} ${presPill(sub, true)}</div>` + agentSetupFields(sub);
  } else if (sec === "agents") {
    header = hdr("Agent setup", [plural(agents.length, "agent", "agents"), agents.filter((a) => M[a].setup.needs).length + " need setup", agents.filter((a) => M[a].paused && !M[a].setup.needs).length + " paused"].join(" · "), `<button class="btn outline" type="button" data-act="proto" data-v="Hire an agent">${ic("user-plus", "ic20")}Hire an agent</button>`);
    body = `<p class="desc">How each agent runs. To see an agent as a colleague (work, conversations, activity), open its profile.</p>
    <div class="only-wide" style="overflow-x:auto;margin-top:12px"><table class="tbl"><thead><tr><th scope="col">Agent</th><th scope="col">Runtime</th><th scope="col">Model</th><th scope="col">How it runs</th><th scope="col">Office</th><th scope="col">Health</th></tr></thead><tbody>${agents.map((a) => {
      const s = M[a].setup;
      return `<tr><td><a class="nm" href="#/settings/agents/${a}">${av(a, 24)}${esc(M[a].name)}</a><span class="muted" style="display:block;font-size:12px;padding-left:34px">${esc(M[a].role)}</span></td><td>${esc(s.runtime)}</td><td>${esc(s.model)}</td><td>${esc(s.how)}</td><td>${esc(officeName(s.office))}</td><td>${healthMark(a)}</td></tr>`;
    }).join("")}</tbody></table></div>
    <ul class="rows only-sm settings-rows" role="list" style="margin-top:8px">${agents.map((a) => { const s = M[a].setup; return `<li class="linked">${av(a, 32)}<div class="grow"><p class="rt"><a class="stretch" href="#/settings/agents/${a}">${esc(M[a].name)}</a><span class="sec">${esc(M[a].role)}</span></p><div class="meta"><span>${esc(s.runtime)} · ${esc(s.model)}</span>${SEP}<span>${esc(s.how)}</span>${SEP}<span>${esc(officeName(s.office))}</span></div></div>${healthMark(a)}${ic("chev-right", "ic20 chev")}</li>`; }).join("")}</ul>`;
  } else {
    const [icon, description] = {
      roles: ["id", "Roles define what each agent does."],
      processes: ["refresh", "Processes define how work moves through the organization."],
      access: ["key", "Members and access manages who can use this organization."],
      updates: ["download", "Updates installs new versions of Agentic Org."],
    }[sec];
    body = empty(icon, "Not available in this prototype", description);
  }
  return settingsFrame(sec, header, body);
}
function pagePreferences() {
  return settingsFrame("appearance", hdr("Your preferences", ""), `<section class="section" aria-labelledby="h-theme"><div class="sec-h"><h2 id="h-theme">Appearance</h2></div>
    <div class="pref theme-pref">${seg("theme", THEME_OPTIONS, "Theme")}
      ${S.theme === "cozy" ? `<div class="pref theme-contrast">
        <div><p class="lbl" id="cozy-contrast-label">AA Contrast</p><p class="muted help" id="cozy-contrast-help">Makes glass surfaces more opaque and text easier to read.</p></div>
        <button class="switch" type="button" role="switch" data-act="cozy-contrast" aria-checked="${S.cozyContrast}" aria-labelledby="cozy-contrast-label" aria-describedby="cozy-contrast-help"></button>
      </div>` : ""}
      <div class="pref theme-contrast">
        <div><p class="lbl" id="tess-pill-label">Show Tess on screen</p><p class="muted help" id="tess-pill-help">Keep Tess parked at bottom-right. You can always press ⌘/ (or Ctrl /) to open Tess even when hidden.</p></div>
        <button class="switch" type="button" role="switch" data-act="toggle-tess-pill" aria-checked="${S.showTessPill}" aria-labelledby="tess-pill-label" aria-describedby="tess-pill-help"></button>
      </div>
    </div></section>
    <section class="section" aria-labelledby="h-keys"><div class="sec-h"><h2 id="h-keys">Keyboard</h2></div>
    <div class="pref"><div><p class="lbl" id="sk-l">Single-key shortcuts</p><p class="muted" id="sk-d" style="font-size:13px">Outside text fields, N opens New task and / focuses Search. Press S for the sidebar or M for the Inbox menu, then type an item's first letter. Repeat the letter to select the next match.</p></div>
    <button class="switch" type="button" role="switch" aria-checked="${S.singleKey}" aria-labelledby="sk-l" aria-describedby="sk-d" data-act="single-key"></button></div>
    <div class="pref chat-send-pref">
      <div><label class="lbl" for="chat-send-shortcut">Chat send shortcut</label>
        <p class="muted" id="chat-send-shortcut-help">Enter adds a new line.</p></div>
      <select class="field" id="chat-send-shortcut" aria-describedby="chat-send-shortcut-help">
        ${Object.entries(CHAT_SEND_SHORTCUTS).map(([value, { label }]) =>
          `<option value="${value}"${S.chatSend === value ? " selected" : ""}>${label}</option>`).join("")}
      </select>
    </div></section>`);
}
function pageSearch(q) {
  const groups = q.trim() ? searchResults(q.trim(), Infinity) : [];
  const n = groups.reduce((a, g) => a + g[1].length, 0);
  return hdr("Search", q.trim() ? `${plural(n, "result", "results")} for “${esc(q.trim())}”` : "") +
    (n ? groups.map(([g, items]) => `<h2 class="grp-h">${esc(g)} <span class="muted">(${items.length})</span></h2><ul class="rows" role="list">${items.map((it) => `<li class="linked">${it.av ? av(it.av, 32) : ic(it.icon, "ic20 chev")}<div class="grow"><p class="rt"><a class="stretch" href="${esc(it.href)}">${esc(it.label)}</a></p><p class="meta">${esc(it.sub)}</p></div>${ic("chev-right", "ic20 chev")}</li>`).join("")}</ul>`).join("")
      : `<p class="empty-row">${q.trim() ? `Nothing matches “${esc(q.trim())}”.` : "Type in Search to find projects, work and members."}</p>`);
}
function notFound() { return hdr("Not found", "") + empty("question", "This page doesn't exist", "The link may be from an older version of the prototype."); }

export {
  SET,
  healthMark,
  notFound,
  pagePreferences,
  pageSearch,
  pageSettings,
};
