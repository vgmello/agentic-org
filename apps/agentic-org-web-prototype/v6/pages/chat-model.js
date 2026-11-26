/* Chat model: conversation names, direct-message creation, conversation search and owner asks.
   Shared by the Chat page, its conversation list and other pages' Message entry points. */
import { CH, IN, M } from "../data.js";
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
const findConversations = (q) => Object.keys(CH).filter((k) => conversationNames(CH[k]).includes(q));
/* Recipients for a new message, people first, then agents; existing DMs are included and simply reopen. */
function recipients(q) {
  const ids = memberIds().filter(canMessage).filter((k) => !q || `${M[k].name} ${M[k].role}`.toLowerCase().includes(q));
  return [...ids.filter((k) => M[k].kind === "person"), ...ids.filter((k) => M[k].kind !== "person")];
}

export {
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
