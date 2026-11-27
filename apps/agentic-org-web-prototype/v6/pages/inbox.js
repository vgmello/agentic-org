import { CH, IN, KIND, M, P, TEAMS, WORK_DISCUSSIONS } from "../data.js";
import { itemActionsV4 } from "./home.js";
import { boardOrigin } from "../shell/board-return.js";
import { chatTitle } from "./chat-model.js";
import { boardItemHref, findWorkItem, fitTextarea, workCommentPrefix } from "./work.js";
import { route } from "../shell/router.js";
import { getNotifications, getRunningJobs, markNotificationRead, unreadNotificationsCount } from "../shell/topbar-utils.js";
import { S } from "../state.js";
import { av, displayCheckbox, esc, ic, mlink, plink, plural, queueHeadline, recommendationCallout, searchRow, searchToggle, showMenu, toast, unhandled } from "../ui.js";

const INBOX_TABS = ["all", "approval", "decision", "notifications"];
const INBOX_MORE = ["question", "problem", "suggestion", "handled"];
const inboxUrgent = (item) => Boolean(item.expires || item.assign);
function matchesInboxQuery(item, query) {
  if (!query) return true;
  const fields = [item.what, item.why, item.rec, item.sub, item.task, M[item.from].name, P[item.proj].name, CH[P[item.proj].channel].name];
  return fields.some((value) => value && value.toLowerCase().includes(query));
}
function matchesInboxKind(item, kind) {
  if (kind === "handled") return Boolean(item.handled);
  return !item.handled && (kind === "all" || item.kind === kind);
}
function inboxItems(kind = S.inboxKind) {
  const query = S.inboxQuery.trim().toLowerCase();
  const priorityFirst = S.inboxOrder === "priority";
  return IN.filter((item) =>
    matchesInboxKind(item, kind)
    && (S.inboxProj === "all" || item.proj === S.inboxProj)
    && (S.inboxTeam === "all" || TEAMS[S.inboxTeam].members.includes(item.from))
    && (!S.inboxUrgent || inboxUrgent(item))
    && matchesInboxQuery(item, query)
  ).sort((a, b) => (priorityFirst ? Number(inboxUrgent(b)) - Number(inboxUrgent(a)) : 0)
    || Number(b.group === "Today") - Number(a.group === "Today"));
}
function filteredNotifications() {
  const q = S.inboxQuery.trim().toLowerCase();
  const list = getNotifications();
  if (!q) return list;
  return list.filter((n) => [n.title, n.summary, n.source].some((v) => v && v.toLowerCase().includes(q)));
}
function inboxTabLabel(key) {
  if (key === "all") return "All";
  if (key === "handled") return "Handled";
  if (key === "notifications") return "Notifications";
  return KIND[key].plural;
}
function inboxTabCount(key) {
  return key === "notifications" ? getRunningJobs().length + filteredNotifications().length : inboxItems(key).length;
}
function inboxViewControls() {
  if (S.inboxKind === "notifications") {
    const unread = unreadNotificationsCount();
    const hasRead = getNotifications().some((n) => n.read);
    const markAll = unread > 0 ? `<button class="btn ghost sm" type="button" data-act="notif-mark-all">Mark all read</button>` : "";
    const clearRead = hasRead ? `<button class="btn ghost sm" type="button" data-act="notif-clear-read">Clear read</button>` : "";
    return `<div class="queue-controls notif-inbox-acts" role="group" aria-label="Notification actions">${inboxSearchToggle()}${markAll}${clearRead}</div>`;
  }
  const inboxFiltered = S.inboxUrgent || S.inboxProj !== "all" || S.inboxTeam !== "all";
  const filter = showMenu("inbox-filter", [
    ["inboxUrgent", "Urgency", [[false, "All items"], [true, "Urgent only"]]],
    ["inboxProj", "Project", [["all", "All projects"], ...Object.keys(P).filter((p) => !P[p].archived).map((p) => [p, P[p].name])]],
    ["inboxTeam", "Team", [["all", "All teams"], ...Object.keys(TEAMS).map((t) => [t, TEAMS[t].name])]],
  ], { icon: "filter", label: "Filter inbox", active: inboxFiltered });
  const display = showMenu("inbox-display", [
    ["inboxOrder", "Grouping", [["priority", "Priority first"], ["date", "By date"]]],
  ], {
    icon: "adjustments", label: "Inbox display", active: S.inboxOrder === "date",
    footer: `<hr>${displayCheckbox("inbox-dividers", "Row dividers", S.inboxDividers)}`,
  });
  return `<div class="queue-controls" role="group" aria-label="Inbox view controls">${inboxSearchToggle()}${filter}${display}</div>`;
}
const inboxSearchToggle = () => searchToggle("triage-search", "Search inbox", S.inboxQuery);
const inboxSearchRow = () => searchRow("triage-search", "Search inbox", S.inboxQuery, "Search inbox… (Esc to close)");
function inboxCategories() {
  const moreActive = INBOX_MORE.includes(S.inboxKind);
  const notifAlert = unreadNotificationsCount() + getRunningJobs().length > 0;
  return `<div class="primary-tab-row" role="group" aria-label="Inbox queue controls"${S.singleKey ? ' aria-keyshortcuts="M"' : ""}>
    <div class="queue-tabs">${INBOX_TABS.map((key) => {
      const dot = key === "notifications" && notifAlert ? '<span class="tab-live-dot" aria-hidden="true"></span>' : "";
      return `
      <button class="queue-tab" type="button" data-act="inbox-category" data-v="${key}" aria-pressed="${S.inboxKind === key}">${inboxTabLabel(key)}<span>${inboxTabCount(key)}</span>${dot}</button>`;
    }).join("")}
    </div>
      <div class="menu-wrap"><button class="queue-tab" type="button" data-act="menu" data-id="inbox-kinds"
        aria-haspopup="menu" aria-expanded="false" aria-controls="menu-inbox-kinds"${moreActive ? ` data-current-kind title="Viewing ${esc(inboxTabLabel(S.inboxKind))}"` : ""}>More${ic("chev-down", "ic14")}</button>
        <div class="menu" id="menu-inbox-kinds" role="menu" aria-label="More inbox categories" hidden>${INBOX_MORE.map((key) => `
          <button type="button" role="menuitemradio" aria-checked="${S.inboxKind === key}" data-act="inbox-category" data-v="${key}">${inboxTabLabel(key)}<span class="triage-menu-count">${inboxItems(key).length}</span></button>`).join("")}</div></div>
  </div>${inboxViewControls()}`;
}
function inboxSelection(items, sel) {
  return IN.find((item) => item.id === sel) || (narrow() ? null : items[0]);
}
function inboxQueueRow(item, selected) {
  const kind = KIND[item.kind];
  const state = item.handled ? "handled" : inboxUrgent(item) ? "urgent" : "pending";
  const stateLabel = { handled: "Handled", urgent: "Urgent", pending: kind.label }[state];
  const stateIcon = { handled: "check-circle", urgent: "bell", pending: item.kind === "approval" ? "inbox" : kind.icon }[state];
  return `<li><a class="triage-item" href="#/inbox/${item.id}" ${selected?.id === item.id ? 'aria-current="true"' : ""}>
    <span class="triage-person">
      ${av(item.from, 24)}
      <span class="triage-sender"><strong>${esc(M[item.from].name)}</strong><span class="triage-kind">${ic(kind.icon, "ic14")}${esc(item.sub)}</span></span>
      <span class="triage-stamp"><span class="triage-state-icon" data-state="${state}">${ic(stateIcon, "ic14")}<span class="sr-only">${esc(stateLabel)}</span></span><time>${esc(item.time)}</time></span>
    </span>
    <span class="triage-title">${esc(item.what)}</span>
    <span class="triage-meta">${esc(P[item.proj].name)}${item.handled ? `<span>${esc(item.outcome)}</span>` : item.expires ? `<span class="triage-expiry">${ic("clock", "ic14")}${esc(item.expires)}</span>` : ""}</span>
  </a></li>`;
}
function inboxQueue(items, selected) {
  if (!items.length) return `<div class="triage-empty">${ic("inbox", "ic20")}<h2>${S.inboxQuery.trim() ? "No matching items" : "Queue cleared for this view"}</h2><p>${S.inboxQuery.trim() ? "Try another search." : "Pick another category to see more items."}</p></div>`;
  const groups = [["Today", (i) => i.group === "Today"], ["Earlier", () => true]];
  if (S.inboxOrder === "priority") groups.unshift(["Urgent", (i) => !i.handled && inboxUrgent(i)]);
  let remaining = items.slice();
  return groups.map(([label, includes]) => {
    const group = remaining.filter(includes);
    remaining = remaining.filter((item) => !includes(item));
    if (!group.length) return "";
    return `<section class="triage-group"><h2>${label}</h2><ul class="triage-list">${group.map((item) => inboxQueueRow(item, selected)).join("")}</ul></section>`;
  }).join("");
}
function notifQueueRow(n, selected) {
  return `<li><a class="triage-item notif-inbox-item${n.read ? "" : " is-unread"}" href="#/inbox/notifications/${esc(n.id)}" ${selected?.id === n.id ? 'aria-current="true"' : ""}>
    <span class="triage-person">
      <span class="notif-row-ic tone-${esc(n.tone || "accent")}" aria-hidden="true">${ic(n.icon || "check-circle", "ic14")}</span>
      <span class="triage-sender"><strong>${esc(n.source || "System")}</strong></span>
      <span class="triage-stamp">${n.read ? "" : '<span class="notif-unread-dot" aria-hidden="true"></span>'}<time>${esc(n.time)}</time></span>
    </span>
    <span class="triage-title">${esc(n.title)}</span>
    <span class="triage-meta">${esc(n.summary)}</span>
  </a></li>`;
}
function notifRunningGroupHtml() {
  const jobs = getRunningJobs();
  if (!jobs.length) return "";
  const rows = jobs.map((job) => `<li class="notif-job-row">
    <span class="notif-job-spin" aria-hidden="true"></span>
    <span class="notif-row-copy">
      <span class="notif-row-title">${esc(job.title)}</span>
      <span class="notif-row-sub">${esc(job.step || "Processing in background…")}</span>
    </span>
    <span class="notif-job-pill">Active</span>
  </li>`).join("");
  return `<section class="triage-group"><h2>In progress</h2><ul class="notif-list" role="list">${rows}</ul></section>`;
}
function inboxNotificationsQueue(list, selected) {
  const runningHtml = notifRunningGroupHtml();
  if (!list.length && !runningHtml) {
    return `<div class="triage-empty">${ic("bell", "ic20")}<h2>${S.inboxQuery.trim() ? "No matching notifications" : "All caught up"}</h2><p>${S.inboxQuery.trim() ? "Try another search." : "Background jobs and completions will appear here."}</p></div>`;
  }
  const unread = list.filter((n) => !n.read);
  const read = list.filter((n) => n.read);
  const unreadHtml = unread.length ? `<section class="triage-group"><h2>Unread</h2><ul class="triage-list">${unread.map((n) => notifQueueRow(n, selected)).join("")}</ul></section>` : "";
  const readHtml = read.length ? `<section class="triage-group"><h2>Earlier</h2><ul class="triage-list">${read.map((n) => notifQueueRow(n, selected)).join("")}</ul></section>` : "";
  return runningHtml + unreadHtml + readHtml;
}
function inboxNotificationDetail(n) {
  if (!n) return `<div class="triage-empty">${ic("bell", "ic20")}<h2>No notification selected</h2><p>Select a notification on the left to inspect or jump to its source.</p></div>`;
  return `<article class="triage-document" aria-labelledby="triage-detail-title">
    <a class="triage-back btn ghost sm" href="#/inbox/notifications">${ic("chev-left", "ic16")}Back to Notifications</a>
    <header class="triage-intro">
      <p class="triage-from">
        <span class="notif-row-ic tone-${esc(n.tone || "accent")}" aria-hidden="true">${ic(n.icon || "check-circle", "ic14")}</span>
        <span>${esc(n.source || "System notification")}</span>
        <span aria-hidden="true">·</span>
        <span class="triage-from-time">${esc(n.time)}</span>
      </p>
      <h2 tabindex="-1" id="triage-detail-title">${esc(n.title)}</h2>
    </header>
    <p class="triage-description">${esc(n.summary)}</p>
    <div class="triage-decision">
      <div class="triage-actions">
        ${n.href ? `<a class="btn primary sm" href="${esc(n.href)}">${ic("open", "ic16")}${esc(n.hrefLabel || "Open item")}</a>` : ""}
        <button class="btn outline sm" type="button" data-act="notif-toggle-read" data-id="${esc(n.id)}">${n.read ? "Mark as unread" : "Mark as read"}</button>
      </div>
    </div>
  </article>`;
}
function pageInboxNotifications(selId) {
  if (selId) markNotificationRead(selId);
  const list = filteredNotifications();
  const selected = list.find((n) => n.id === selId) || (narrow() ? null : list[0] || null);
  const total = getRunningJobs().length + list.length;
  return `<section class="triage-workspace${selId && selected ? " triage-has-detail" : ""}" aria-label="Inbox workspace">
    <header class="triage-header queue-header">
      ${queueHeadline("Inbox", unhandled().length + " waiting on you", true)}
      <div class="triage-toolbar queue-toolbar" id="triage-categories">${inboxCategories()}</div>
    </header>
    <div class="triage-split">
      <section class="triage-queue" aria-label="Notifications queue">${inboxSearchRow()}<div class="triage-queue-summary"><span>Live &amp; recent</span><span id="triage-visible" role="status">${plural(total, "notification", "notifications")}</span></div>
        <div id="triage-list">${inboxNotificationsQueue(list, selected)}</div>
      </section>
      <section class="triage-reading" id="triage-reading" aria-label="Notification details">${inboxNotificationDetail(selected)}</section>
    </div>
  </section>`;
}
function pageInbox(sel, sub) {
  if (sel === "notifications") S.inboxKind = "notifications";
  /* A link to a specific request (e.g. from a Work card) opens it even if Notifications was last open */
  else if (sel && S.inboxKind === "notifications" && IN.some((i) => i.id === sel)) S.inboxKind = "all";
  if (S.inboxKind === "notifications") return pageInboxNotifications(sel === "notifications" ? sub : null);
  const items = inboxItems(), item = inboxSelection(items, sel);
  const queueLabel = INBOX_MORE.includes(S.inboxKind)
    ? (S.inboxKind === "handled" ? "Handled items" : inboxTabLabel(S.inboxKind))
    : (S.inboxOrder === "date" ? "By date" : "Priority first");
  return `<section class="triage-workspace${sel && item ? " triage-has-detail" : ""}" aria-label="Inbox workspace">
    <header class="triage-header queue-header">
      ${queueHeadline("Inbox", unhandled().length + " waiting on you", true)}
      <div class="triage-toolbar queue-toolbar" id="triage-categories">${inboxCategories()}</div>
    </header>
    <div class="triage-split">
      <section class="triage-queue" aria-label="Inbox queue">${inboxSearchRow()}<div class="triage-queue-summary"><span>${queueLabel}</span><span id="triage-visible" role="status">${plural(items.length, "item in view", "items in view")}</span></div>
        <div id="triage-list">${inboxQueue(items, item)}</div>
      </section>
      <section class="triage-reading" id="triage-reading" aria-label="Item details">${item ? inboxDetail(item) : inboxEmptyDetail()}</section>
    </div>
  </section>`;
}
const inboxEmptyDetail = () => `<div class="triage-empty">${ic("inbox", "ic20")}<h2>No item selected</h2><p>Your queue appears on the left.</p></div>`;
const inboxCopy = (label, value, title) => `<button class="triage-copy" type="button" data-act="inbox-copy" data-v="${esc(value)}" aria-label="Copy ${esc(title)}">${ic("copy", "ic14")}<span>${esc(label)}</span></button>`;
const detailCommentDrafts = new Map();
const inboxCommentPrefix = (id) => "agentic-org.v6.inbox-comment." + id + ".";

function inboxActivityTime(at) {
  if (at == null) return "";
  const date = new Date(at);
  return `<time class="triage-event-time" datetime="${date.toISOString()}">${esc(date.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" }))}</time>`;
}

function localDetailRecords(prefix) {
  const records = [];
  try {
    for (const key of Object.keys(localStorage)) {
      if (!key.startsWith(prefix)) continue;
      try {
        const entry = JSON.parse(localStorage.getItem(key));
        if (Number.isFinite(entry?.at) && Number.isFinite(new Date(entry.at).getTime())) records.push(entry);
      } catch (e) {}
    }
  } catch (e) {}
  return records.sort((a, b) => a.at - b.at);
}

function detailCommentsHtml(prefix, heading, seeded = []) {
  const stored = localDetailRecords(prefix)
    .filter(({ text }) => typeof text === "string" && text.trim() && text.length <= 2000)
    .map((entry) => ({ ...entry, who: "you" }));
  const comments = seeded.map(([who, text, time]) => ({ who, text, time })).concat(stored);
  if (!comments.length) return "";
  return `<section class="triage-comments" aria-labelledby="${heading}">
    <h4 id="${heading}">Comments</h4>
    <ol class="triage-comment-list" role="list" aria-labelledby="${heading}">${comments.map(({ who, text, time, at }) => `
      <li>${av(who, 24)}<div><p><strong>${who === "you" ? "You" : mlink(who)}</strong> ${time ? `<span class="triage-event-time">${esc(time)}</span>` : inboxActivityTime(at)}</p>
      <p class="triage-comment-text">${esc(text)}</p></div></li>`).join("")}</ol>
  </section>`;
}

const inboxCommentsHtml = (id) => detailCommentsHtml(inboxCommentPrefix(id), "triage-comments-title");
const workCommentsHtml = (id) => detailCommentsHtml(workCommentPrefix(id), "work-comments-title", WORK_DISCUSSIONS[id]?.comments);

function detailEvent(icon, text, time = "") {
  return `<li><span class="triage-event-marker">${ic(icon, "ic16")}</span><p>${text} ${time}</p></li>`;
}

function inboxActivityRows(it) {
  const outcomes = it.activity || (it.handled ? [{ icon: "check-circle", text: it.outcome }] : []);
  return [
    detailEvent("inbox", `<strong>${esc(M[it.from].name)}</strong> opened this request`, `<span class="triage-event-time">${esc(it.time)}</span>`),
    detailEvent("clock", it.kind === "approval" ? "Waiting for approval" : "Waiting for your input"),
    ...(it.audit || []).map((text) => detailEvent("check-circle", `<span class="sr-only">Audit entry. </span>${esc(text)}`)),
    ...outcomes.map(({ icon, text, at }) => detailEvent(icon, esc(text), inboxActivityTime(at)))
  ].join("");
}

function postDetailComment(form) {
  const field = form.elements.comment;
  const text = field.value.trim();
  if (!text) {
    field.setCustomValidity("Enter a comment.");
    field.reportValidity();
    return;
  }
  const id = form.dataset.id, work = form.dataset.form === "work-comment";
  const prefix = work ? workCommentPrefix(id) : inboxCommentPrefix(id);
  try {
    localStorage.setItem(prefix + crypto.randomUUID(), JSON.stringify({ text, at: Date.now() }));
  } catch (e) {
    toast("Could not save the comment on this device.");
    field.focus();
    return;
  }
  detailCommentDrafts.delete(prefix);
  field.value = "";
  if (work) fitTextarea(field);
  document.getElementById(work ? "work-comments" : "triage-comments").innerHTML = work ? workCommentsHtml(id) : inboxCommentsHtml(id);
  document.getElementById(work ? "work-comment-status" : "triage-comment-status").textContent = "Comment posted.";
  field.focus();
}

function inboxActivity(it) {
  return `<section class="triage-activity" aria-labelledby="triage-activity-title">
    <h3 id="triage-activity-title">Activity</h3>
    <form class="triage-comment-form" data-form="inbox-comment" data-id="${esc(it.id)}">
      <label class="sr-only" for="triage-comment">Comment on this request</label>
      <textarea id="triage-comment" name="comment" rows="2" maxlength="2000" placeholder="Leave a comment…" required>
${esc(detailCommentDrafts.get(inboxCommentPrefix(it.id)) || "")}</textarea>
      <button class="btn outline icon triage-send" type="submit" aria-label="Send comment" title="Send comment">${ic("send", "ic16")}</button>
    </form>
    <p id="triage-comment-status" class="sr-only" role="status"></p>
    <div id="triage-comments">${inboxCommentsHtml(it.id)}</div>
    <div class="triage-history">
      <h4 id="triage-history-title">Request history</h4>
      <ol class="triage-activity-log" role="list" aria-labelledby="triage-history-title">${inboxActivityRows(it)}</ol>
    </div>
  </section>`;
}

function inboxDetail(it) {
  const m = M[it.from], k = KIND[it.kind], project = P[it.proj];
  const channel = CH[project.channel], work = it.task && findWorkItem(it.task).item, origin = boardOrigin();
  return `<article class="triage-document" aria-labelledby="triage-detail-title">
    <a class="triage-back btn ghost sm" href="#/inbox">${ic("chev-left", "ic16")}Back to Inbox</a>
    ${origin ? `<a class="triage-board-return btn outline sm" href="${esc(origin)}">${ic("chev-left", "ic16")}Back to work board</a>` : ""}
    <header class="triage-intro">
      <p class="triage-from">
        <span>From:</span>
        <a class="alink triage-from-name" href="#/members/${esc(it.from)}" data-person="${esc(it.from)}">${esc(m.name)}</a>
        <span aria-hidden="true">·</span><span class="triage-from-time">${esc(it.time)}</span>
      </p>
      <h2 tabindex="-1" id="triage-detail-title">${esc(it.what)}</h2>
    </header>
    ${it.why ? `<p class="triage-description">${esc(it.why)}</p>` : ""}
    <section class="triage-context" aria-labelledby="triage-context-title">
      <h3 id="triage-context-title">Request details</h3>
      <dl class="triage-fields">
        <div><dt>Kind</dt><dd>${esc(k.label)} · ${esc(it.sub)}</dd></div>
        <div><dt>Project</dt><dd>${plink(it.proj)}</dd></div>
        ${it.task ? `<div><dt>Work item</dt><dd><a class="alink" href="${esc(boardItemHref(origin, it.task))}">${esc(it.task)}${work ? " · " + esc(work.title) : ""}</a>${inboxCopy("Copy", it.task, it.task)}</dd></div>` : ""}
        ${it.expires ? `<div><dt>Expires</dt><dd${it.handled ? "" : ' class="triage-expiry"'}>${esc(it.expires)}</dd></div>` : ""}
        <div><dt>Clears</dt><dd>${esc(it.clears || k.clears)}</dd></div>
        ${inboxConversation(it, project, channel)}
        ${it.src ? `<div><dt>Related page</dt><dd><a class="alink" href="${esc(it.src.href)}">${esc(it.src.label)}</a></dd></div>` : ""}
      </dl>
    </section>
    <div class="triage-decision">
      ${it.rec ? recommendationCallout(it, "h3") : ""}
      <div class="triage-actions">${itemActionsV4(it, { hideDetails: true })}${it.handled
        ? `<button class="btn outline sm" type="button" data-act="inbox-reopen" data-id="${it.id}">${ic("refresh", "ic16")}Reopen item</button>` : ""}</div>
    </div>
    ${inboxActivity(it)}
  </article>`;
}
/* Where the ask was raised (it.origin) when known, with a deep link to that message; else the project's channel. */
function inboxConversation(it, project, channel) {
  const src = it.origin && CH[it.origin.ch];
  if (!src) return channel ? `<div><dt>Conversation</dt><dd><a class="alink" href="#/chat/${project.channel}">#${esc(channel.name)}</a>${inboxCopy("Copy", "#" + channel.name, "#" + channel.name)}</dd></div>` : "";
  const title = src.kind === "dm" ? "Direct message with " + chatTitle(src) : chatTitle(src);
  return `<div><dt>Conversation</dt><dd><a class="alink" href="#/chat/${esc(it.origin.ch)}">${esc(title)}</a>
    · <a class="alink" href="#/chat/${esc(it.origin.ch)}/${esc(it.origin.msg)}">Open source message</a></dd></div>${inboxRelated(it)}`;
}
/* A message the ask came out of (e.g. an agent-to-agent exchange the owner can read), kept as context. */
function inboxRelated(it) {
  const ch = it.related && CH[it.related.ch], m = ch && ch.msgs.find((x) => x.id === it.related.msg);
  if (!m) return "";
  return `<div><dt>Related</dt><dd><a class="alink" href="#/chat/${esc(it.related.ch)}/${esc(m.id)}">${esc(M[m.who].name)}'s message in ${esc(chatTitle(ch))}</a></dd></div>`;
}
function refreshInboxSearch() {
  if (S.inboxKind === "notifications") {
    const list = filteredNotifications(), sel = route()[2];
    const selected = list.find((n) => n.id === sel) || (narrow() ? null : list[0] || null);
    const total = getRunningJobs().length + list.length;
    document.getElementById("triage-categories").innerHTML = inboxCategories();
    document.getElementById("triage-visible").textContent = plural(total, "notification", "notifications");
    document.getElementById("triage-list").innerHTML = inboxNotificationsQueue(list, selected);
    document.getElementById("triage-reading").innerHTML = inboxNotificationDetail(selected);
    return;
  }
  const items = inboxItems(), sel = route()[1];
  const keep = items.some((item) => item.id === sel);
  if (sel && !keep) resetInboxSelection();
  const item = inboxSelection(items, keep ? sel : null);
  document.querySelector(".triage-workspace").classList.toggle("triage-has-detail", Boolean(keep && item));
  document.getElementById("triage-categories").innerHTML = inboxCategories();
  document.getElementById("triage-visible").textContent = plural(items.length, "item in view", "items in view");
  document.getElementById("triage-list").innerHTML = inboxQueue(items, item);
  document.getElementById("triage-reading").innerHTML = item ? inboxDetail(item) : inboxEmptyDetail();
}
function resetInboxSelection() {
  history.replaceState(history.state, "", "#/inbox");
  S.lastRoute = location.hash;
  S.confirmDeny = null;
  S.replying = null;
  S.replyMode = null;
}
const narrow = () => !!(window.matchMedia && window.matchMedia("(max-width: 959px)").matches);

export {
  detailCommentDrafts,
  detailEvent,
  inboxActivityTime,
  inboxCommentPrefix,
  inboxItems,
  inboxTabLabel,
  localDetailRecords,
  narrow,
  pageInbox,
  postDetailComment,
  refreshInboxSearch,
  resetInboxSelection,
  workCommentsHtml,
};
