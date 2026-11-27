/* Chat model: conversation names, direct-message creation, conversation search and owner asks.
   Shared by the Chat page, its conversation list and other pages' Message entry points. */
import { CH, IN, M, P } from "../data.js";
import { memberIds } from "../ui.js";

const DEFAULT_CHAT = "agency-v1";
const dmSources = new Map(); /* DM id -> { ch, msg? } the owner's first message will link to */

/* "Nov 10, 16:02" / "Yesterday, 09:26" / "09:12" -> [day, time]; a bare time is today. */
const DAY_RE = /^([A-Z][a-z]{2} \d{1,2}|Yesterday),?\s*/;
function splitTime(time) {
  const day = DAY_RE.exec(time || "");
  return day ? [day[1], time.slice(day[0].length)] : ["Today", time || ""];
}
const sentAt = (m) => splitTime(m.time).join(" ");

/* Prototype clock: today is Thu Nov 26 2026 (data.js CAL_TODAY), 10:30, so "the last day" never depends on
   when the prototype is opened. */
const NOW = new Date(2026, 10, 26, 10, 30);
const DAY_MS = 24 * 60 * 60 * 1000;
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
function sentDate(time) {
  const [day, hm] = splitTime(time);
  const [h, min] = (hm || "0:0").split(":").map(Number);
  const d = new Date(NOW);
  d.setHours(h || 0, min || 0, 0, 0);
  if (day === "Yesterday") d.setDate(d.getDate() - 1);
  else if (day !== "Today") { const [mon, date] = day.split(" "); d.setMonth(MONTHS.indexOf(mon), Number(date)); }
  return d.getTime();
}
/* A conversation's last activity: its newest message or thread reply. */
function lastActivity(ch) {
  const times = ch.msgs.flatMap((m) => [m.time, ...(m.thread || []).map((r) => r.time)]);
  return times.length ? Math.max(...times.map(sentDate)) : 0;
}
/* A conversation's last activity in a few characters: "09:33" today, "Yesterday", else "Nov 24". */
function lastActiveLabel(ch) {
  const t = lastActivity(ch);
  if (!t) return "";
  const d = new Date(t), days = Math.round((new Date(NOW).setHours(0, 0, 0, 0) - new Date(t).setHours(0, 0, 0, 0)) / DAY_MS);
  if (days <= 0) return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  return days === 1 ? "Yesterday" : `${MONTHS[d.getMonth()]} ${d.getDate()}`;
}
const activeToday = (ch) => NOW.getTime() - lastActivity(ch) <= DAY_MS;
const newestFirst = (a, b) => lastActivity(CH[b]) - lastActivity(CH[a]);
/* Agents' conversations one agent is in, newest first. */
const agentConversations = (who) => Object.keys(CH).filter((k) => CH[k].kind === "a2a" && CH[k].pair.includes(who)).sort(newestFirst);

const chatTitle = (ch) => (ch.kind === "channel" ? "#" + ch.name : ch.kind === "dm" ? M[ch.with].name : ch.name);
const inboxItem = (id) => IN.find((i) => i.id === id);

/* Asks raised in this conversation that still wait on the owner (items only mentioned here are counted where asked). */
function openAsks(ch) {
  const ids = new Set(ch.msgs.map((m) => m.needs).filter(Boolean));
  return [...ids].filter((id) => { const it = inboxItem(id); return it && !it.handled; });
}

/* Anyone but the owner can be messaged directly. */
const canMessage = (who) => Boolean(M[who]) && !M[who].owner;
const dmId = (who) => "dm-" + who;
function ensureDirectMessage(who) {
  const key = dmId(who);
  CH[key] ||= { name: M[who].name, kind: "dm", with: who, msgs: [] };
  return key;
}
/* Chat ids in links: a known conversation, or dm-<member> for a first-time DM, which is created on open. */
function resolveChatId(id) {
  if (id && CH[id]) return id;
  const who = id && id.startsWith("dm-") ? id.slice(3) : "";
  return canMessage(who) ? ensureDirectMessage(who) : DEFAULT_CHAT;
}
/* Every Message entry point: opens (or creates) the DM; `source` is a conversation id or { ch, msg }. */
function openDirectMessage(who, source) {
  const key = ensureDirectMessage(who);
  const src = typeof source === "string" ? { ch: source } : source;
  if (src && CH[src.ch]) dmSources.set(key, src);
  return "#/chat/" + key;
}
const sourceHref = (src) => `#/chat/${src.ch}${src.msg ? "/" + src.msg : ""}`;
function sourceLabel(src) {
  const ch = CH[src.ch], m = src.msg && ch.msgs.find((x) => x.id === src.msg);
  return m ? `${M[m.who].name}'s message in ${chatTitle(ch)}` : chatTitle(ch);
}

/* "Find a conversation": channels, DMs and agent-to-agent, by name, topic or member. */
function conversationNames(ch) {
  const people = ch.kind === "dm" ? [ch.with] : ch.pair || [];
  return [ch.name, chatTitle(ch), ch.topic || "", ...people.map((k) => M[k].name)].join(" ").toLowerCase();
}
const projectName = (ch) => (ch.proj && P[ch.proj] ? P[ch.proj].name : "");
const allTexts = (ch) => ch.msgs.flatMap((m) => [m.text || "", ...(m.thread || []).map((r) => r.text || "")]);
function snippet(text, q) {
  const i = text.toLowerCase().indexOf(q), from = Math.max(0, i - 16), to = Math.min(text.length, i + q.length + 24);
  return `${from ? "…" : ""}${text.slice(from, to).trim()}${to < text.length ? "…" : ""}`;
}
const TASK_REF = /\bT-A\d+\b/g;
/* A task reference the messages make that the query names: "t-a121" or "T-A12" (a prefix with its digits). */
const looksLikeTask = (q) => /^t-a\d+$/.test(q);
function taskReason(ch, q) {
  if (!looksLikeTask(q)) return null;
  const tasks = allTexts(ch).flatMap((t) => t.match(TASK_REF) || []);
  const task = tasks.find((t) => t.toLowerCase() === q) || tasks.find((t) => t.toLowerCase().startsWith(q));
  return task ? `mentions ${task}` : null;
}
/* What an agents' conversation's messages matched: a task id by name, otherwise the words around the match. */
function textReason(ch, q) {
  const text = allTexts(ch).find((t) => t.toLowerCase().includes(q));
  if (!text) return null;
  const task = (text.match(TASK_REF) || []).find((t) => t.toLowerCase().includes(q));
  return task ? `mentions ${task}` : `“${snippet(text, q)}”`;
}
/* Why a conversation matches the filter: "" when its name, topic or people match (the row shows that), the project's
   name, a task its messages mention (every kind of conversation), or (agents' conversations only) any words its
   messages say; null when it does not match. */
function matchReason(ch, q) {
  if (conversationNames(ch).includes(q)) return "";
  if (projectName(ch).toLowerCase().includes(q)) return `in ${projectName(ch)}`;
  return taskReason(ch, q) ?? (ch.kind === "a2a" ? textReason(ch, q) : null);
}
const findConversations = (q) => Object.keys(CH).filter((k) => matchReason(CH[k], q) !== null);
/* Recipients for a new message, people first, then agents; existing DMs are included and simply reopen. */
function recipients(q) {
  const ids = memberIds().filter(canMessage).filter((k) => !q || `${M[k].name} ${M[k].role}`.toLowerCase().includes(q));
  return [...ids.filter((k) => M[k].kind === "person"), ...ids.filter((k) => M[k].kind !== "person")];
}

export {
  activeToday,
  agentConversations,
  lastActivity,
  lastActiveLabel,
  matchReason,
  newestFirst,
  sentAt,
  splitTime,
  DEFAULT_CHAT,
  chatTitle,
  dmSources,
  findConversations,
  inboxItem,
  openAsks,
  openDirectMessage,
  recipients,
  resolveChatId,
  sourceHref,
  sourceLabel,
};
