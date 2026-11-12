// What the Requests screens say about a request (feature-requests-ux §2, §3, §4): the groups of the queue, the state
// in words, the estimate and age in words, and who the app suggests to scope a request when no copilot is on (RQS-5).
import { isFormer, leadOf, teamsOfProject } from '$lib/org/model';
import type { Member, OrgSnapshot, RequestKind, RequestOutcome, WorkEvent, WorkItemSummary } from '$lib/proto';
import { nameOf } from './people';
import { span } from './time';

export type RequestGroupId = 'needs_you' | 'with_agents' | 'not_routed' | 'approved' | 'closed';

export interface RequestGroup {
	id: RequestGroupId;
	label: string;
	/** Collapsed until opened: what has ended. */
	collapsed: boolean;
}

/** The queue's groups in order (§2). */
export const REQUEST_GROUPS: readonly RequestGroup[] = [
	{ id: 'needs_you', label: 'Needs you', collapsed: false },
	{ id: 'with_agents', label: 'With agents', collapsed: false },
	{ id: 'not_routed', label: 'Not routed', collapsed: false },
	{ id: 'approved', label: 'Approved', collapsed: true },
	{ id: 'closed', label: 'Declined and merged', collapsed: true }
];

/** The statuses of a request that is still going. */
export const OPEN_STATUSES: readonly string[] = ['new', 'routed', 'scoping', 'scoped'];

/** Where a request sits in the queue; nothing for a status a request does not have. */
export function groupOf(item: Pick<WorkItemSummary, 'status'>): RequestGroupId | undefined {
	switch (item.status) {
		case 'scoped':
			return 'needs_you';
		case 'routed':
		case 'scoping':
			return 'with_agents';
		case 'new':
			return 'not_routed';
		case 'approved':
			return 'approved';
		case 'declined':
		case 'merged':
			return 'closed';
		default:
			return undefined;
	}
}

/** The requests of each group, newest number first within it. */
export function groupRequests(items: WorkItemSummary[]): Record<RequestGroupId, WorkItemSummary[]> {
	const groups: Record<RequestGroupId, WorkItemSummary[]> = { needs_you: [], with_agents: [], not_routed: [], approved: [], closed: [] };
	for (const item of items) {
		if (item.kind !== 'request' || item.deleted) continue;
		const id = groupOf(item);
		if (id) groups[id].push(item);
	}
	for (const list of Object.values(groups)) list.sort((a, b) => (b.number ?? Infinity) - (a.number ?? Infinity));
	return groups;
}

/** Requests that wait for the owner: scoped, ready to approve (the nav badge counts these too). */
export const requestsNeedingYou = (items: WorkItemSummary[]): number => items.filter((i) => i.kind === 'request' && !i.deleted && i.status === 'scoped').length;

/** `REQ-12`, or "Pending · short id" until HQ numbers it. */
export const requestRef = (item: Pick<WorkItemSummary, 'number' | 'short_id'>): string => (item.number ? `REQ-${item.number}` : `Pending · ${item.short_id}`);

export const KIND_WORD: Record<Exclude<RequestKind, 'unknown'>, string> = { feature: 'Feature', change: 'Change', technical: 'Technical', question: 'Question' };
export const kindWord = (kind: RequestKind | undefined): string => (kind && kind !== 'unknown' ? KIND_WORD[kind] : 'Not set');

/** The state in words, with who (decision 3, RQS-14): "Sent to Eden · not accepted yet", "Eden is scoping". */
export function stateWords(item: WorkItemSummary, snapshot: OrgSnapshot): string {
	const who = nameOf(snapshot, item.assignee_id);
	switch (item.status) {
		case 'new':
			return 'Not routed';
		case 'routed':
			return `Sent to ${who} · not accepted yet`;
		case 'scoping':
			return `${who} is scoping`;
		case 'scoped':
			return `Scoped by ${who}`;
		case 'approved':
			return 'Approved';
		case 'declined':
			return 'Declined';
		case 'merged':
			return 'Merged';
		default:
			return 'Changed';
	}
}

const dollars = (cents: number): string => {
	const d = cents / 100;
	return Number.isInteger(d) ? `${d}` : d.toFixed(2);
};

/** "≈ $40–70", "≈ $40", "3 days · ≈ $40–70": the estimate an agent gave, if any (decision 7). */
export function estimateWords(outcome: RequestOutcome | undefined): string | undefined {
	const e = outcome?.estimate;
	if (!e) return undefined;
	const { cost_usd_cents_min: lo, cost_usd_cents_max: hi } = e;
	const cost = lo !== undefined && hi !== undefined ? (lo === hi ? `≈ $${dollars(lo)}` : `≈ $${dollars(lo)}–${dollars(hi)}`) : lo !== undefined ? `from $${dollars(lo)}` : hi !== undefined ? `up to $${dollars(hi)}` : undefined;
	return [e.effort, cost].filter(Boolean).join(' · ') || undefined;
}

/** "5 min", "1 h", "2 d": how long ago, in the largest unit only (the queue's age column). */
export function ageWord(at: string, now: string): string {
	const ms = Math.max(0, Date.parse(now) - Date.parse(at));
	return span(ms).split(' ').slice(0, 2).join(' ');
}

/** What the owner can do next in the queue's last column. */
export const nextAction = (item: Pick<WorkItemSummary, 'status'>): string | undefined => (item.status === 'scoped' ? 'Review' : item.status === 'new' ? 'Route' : undefined);

export interface Suggestion {
	member: Member;
	/** One line of why: "product designer: a new user-facing flow". */
	why: string;
}

const matches = (m: Member, pattern: RegExp) => pattern.test(`${m.agent?.role ?? ''} ${m.agent?.title ?? ''}`);

/**
 * Who to suggest without the copilot (RQS-5): Feature or Change go to the product designer, Technical to the staff
 * engineer or the project's lead, Question to the project's lead. Up to three agents that take work, the best first;
 * the owner changes it. Marked "Suggested by rule" by the caller.
 */
export function suggestScopers(snapshot: OrgSnapshot, kind: Exclude<RequestKind, 'unknown'>, projectId: string | undefined): Suggestion[] {
	const takesWork = (m: Member) => m.kind === 'agent' && !isFormer(m) && m.agent?.status !== 'paused';
	const agents = snapshot.members.filter(takesWork);
	const leads: Member[] = projectId
		? teamsOfProject(snapshot, projectId)
				.map((t) => leadOf(snapshot, t))
				.filter((m): m is Member => !!m && takesWork(m))
		: [];
	const designers = agents.filter((m) => matches(m, /design/i));
	const engineers = agents.filter((m) => matches(m, /staff|architect|tech(nical)? lead|engineering manager|sdm/i));
	const picks: Suggestion[] = [];
	const add = (members: Member[], why: (m: Member) => string) => {
		for (const member of members) if (picks.length < 3 && !picks.some((p) => p.member.id === member.id)) picks.push({ member, why: why(member) });
	};
	const title = (m: Member) => (m.agent?.title ?? m.agent?.role ?? 'agent').toLowerCase();
	if (kind === 'feature' || kind === 'change') {
		add(designers, (m) => `${title(m)}: ${kind === 'feature' ? 'a new capability for users' : 'a change to something that exists'}`);
		add(leads, (m) => `${title(m)}: leads the project`);
		add(engineers, (m) => `${title(m)}: knows how it fits together`);
	} else if (kind === 'technical') {
		add(engineers, (m) => `${title(m)}: architecture and technical choices`);
		add(leads, (m) => `${title(m)}: leads the project`);
		add(designers, (m) => `${title(m)}: shapes how it works for users`);
	} else {
		add(leads, (m) => `${title(m)}: leads the project`);
		add(engineers, (m) => `${title(m)}: knows how it fits together`);
		add(designers, (m) => `${title(m)}: shapes how it works for users`);
	}
	add(agents, (m) => `${title(m)}: another agent who can take it`);
	return picks;
}

export interface ActivityLine {
	at: string;
	/** One sentence: "You moved this from ada to Eden". */
	text: string;
	/** What was written, for a note. */
	note?: string;
}

const FIELD_WORD = new Map([
	['title', 'title'],
	['request_kind', 'kind'],
	['project_id', 'project']
]);

const text = (v: unknown): string | undefined => (typeof v === 'string' ? v : undefined);

/** The fields of a `fields_changed` event as objects, whatever an older or newer office wrote: a bare name counts as a field, the rest is dropped. */
function fieldsOf(value: unknown): { field: string; from?: unknown; to?: unknown }[] {
	if (!Array.isArray(value)) return [];
	return value.flatMap((f): { field: string; from?: unknown; to?: unknown }[] => {
		if (typeof f === 'string') return [{ field: f }];
		if (typeof f === 'object' && f !== null && typeof (f as { field?: unknown }).field === 'string') return [f as { field: string; from?: unknown; to?: unknown }];
		return [];
	});
}

/**
 * A request's conversation and history (§3): who did what, in plain words, oldest first. `me` is the owner, who reads
 * as "You". Events of a kind this build does not know read "changed it".
 */
export function activityLines(events: WorkEvent[], snapshot: OrgSnapshot, me: string): ActivityLine[] {
	const who = (id: string | undefined) => (id === me ? 'You' : id ? nameOf(snapshot, id) : 'The office');
	const whom = (id: string | undefined) => (id === me ? 'you' : nameOf(snapshot, id));
	/** "sent this to B", "moved this from A to B", "took this back". */
	const sentence = (by: string, from: string | undefined, to: string | undefined) => (from && to && from !== to ? `${by} moved this from ${whom(from)} to ${whom(to)}` : to ? `${by} sent this to ${whom(to)}` : `${by} took this back`);
	// The note an ask for changes cites was written by its own comment just before; the "asked for changes" line carries it,
	// so that one comment (the nearest before it, by the same member, with the same words) is left out. Others stay.
	const cited = new Set<number>();
	events.forEach((e, i) => {
		const c = e.change;
		if (c.type !== 'status_changed' || c.from !== 'scoped' || c.to !== 'scoping' || !c.reason) return;
		for (let j = i - 1; j >= 0; j--) {
			const earlier = events[j];
			if (!cited.has(j) && earlier.actor_id === e.actor_id && earlier.change.type === 'comment' && earlier.change.body === c.reason) {
				cited.add(j);
				return;
			}
		}
	});
	return events.flatMap((e, i): ActivityLine[] => {
		const c = e.change;
		const by = who(e.actor_id);
		switch (c.type) {
			case 'created':
				return [{ at: e.at, text: `${by} raised this` }];
			case 'routed': {
				const out: ActivityLine[] = [];
				// Someone else's request taken up: both who it was with and that it was accepted.
				if (c.scoper_from && c.scoper_to && c.scoper_from !== c.scoper_to && c.to === 'scoping') out.push({ at: e.at, text: `${by} moved this from ${whom(c.scoper_from)} to ${whom(c.scoper_to)}` });
				out.push({ at: e.at, text: c.to === 'scoping' ? `${by} accepted it` : sentence(by, c.scoper_from, c.scoper_to), note: c.to === 'scoping' ? undefined : c.reason });
				return out;
			}
			case 'comment':
				return cited.has(i) ? [] : [{ at: e.at, text: `${by} wrote`, note: c.body }];
			case 'fields_changed': {
				const names = fieldsOf(c.fields).map((f) => (FIELD_WORD.get(f.field) ?? f.field));
				return [{ at: e.at, text: names.length > 0 ? `${by} changed the ${names.join(' and ')}` : `${by} changed it` }];
			}
			case 'status_changed': {
				if (c.to === 'scoping') return c.from === 'scoped' ? [{ at: e.at, text: `${by} asked for changes`, note: c.reason }] : [{ at: e.at, text: `${by} accepted it` }];
				if (c.to === 'scoped') return [{ at: e.at, text: `${by} scoped it` }];
				if (c.to === 'approved') return [{ at: e.at, text: `${by} approved it` }];
				if (c.to === 'declined') return [{ at: e.at, text: `${by} declined it`, note: c.reason }];
				if (c.to === 'merged') return [{ at: e.at, text: `${by} merged it`, note: c.reason }];
				return [{ at: e.at, text: `${by} changed it` }];
			}
			default:
				return [{ at: e.at, text: `${by} changed it` }];
		}
	});
}

// ---- capture: @name routing and similar requests (§1) ----------------------------------------------------------------

/** Lower case without accents, so `@Éden` finds Eden. */
const plain = (text: string) => text.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase();
const handleOf = (m: Member) => plain(m.agent?.messaging_name ?? '');
/** A display name as a handle: "Juno Park" is `juno-park`. */
const slugOf = (name: string) => plain(name).trim().replace(/\s+/g, '-');
/** `@handle` at the start of the words: letters, numbers, `-` and `_` only, so `@eden:` and `@eden,` still name Eden; `@@x` names nobody. */
const AT = /^(\s*)@(?!@)([\p{L}\p{N}_-]*)/u;

export type AtName = { kind: 'exact'; member: Member; token: string } | { kind: 'partial'; matches: Member[]; token: string } | { kind: 'unknown'; token: string };

/**
 * Starting the words with `@` and a handle sends the request straight to that agent (decision 5): the messaging name
 * (`@ac-product-designer-eden`), the display name as a slug (`@juno-park`) or a first name (`@juno`) when only one agent
 * has it. While the handle is still being typed, the agents it could become are offered, and a handle nobody has is
 * `unknown` (Send is disabled until it is fixed or removed). The words themselves are never changed.
 */
export function atName(text: string, snapshot: OrgSnapshot): AtName | undefined {
	const found = AT.exec(text);
	if (!found) return undefined;
	const token = plain(found[2]);
	const agents = snapshot.members.filter((m) => m.kind === 'agent' && !isFormer(m));
	const keys = (m: Member) => [handleOf(m), slugOf(m.name), slugOf(m.name).split('-')[0]].filter(Boolean);
	if (token === '') return { kind: 'partial', matches: agents.slice(0, 5), token };
	const exact = agents.filter((m) => keys(m).includes(token));
	// A full handle or slug wins; a first name only when nobody else shares it.
	const strong = exact.filter((m) => handleOf(m) === token || slugOf(m.name) === token);
	if (strong.length === 1) return { kind: 'exact', member: strong[0], token };
	if (strong.length === 0 && exact.length === 1) return { kind: 'exact', member: exact[0], token };
	const matches = agents.filter((m) => keys(m).some((k) => k.startsWith(token)) || plain(m.name).split(/\s+/).some((p) => p.startsWith(token)));
	return matches.length > 0 ? { kind: 'partial', matches: matches.slice(0, 5), token } : { kind: 'unknown', token: found[2] };
}

/** How far the `@handle` at the start reaches: the characters from the start of the text to the end of the handle (nothing when there is none). */
export const atSpan = (text: string): number | undefined => AT.exec(text)?.[0].length;

/** What the handle in `@handle` is replaced by when a match is chosen: the messaging name, which is always unique. */
export const handleFor = (m: Member): string => m.agent?.messaging_name ?? slugOf(m.name);

/** The words without the `@handle` that routed them (and the colon or comma after it): "fix the build time", not "@sam: fix the build time". */
export const withoutHandle = (text: string): string => text.replace(/^(\s*)@(?!@)[\p{L}\p{N}_-]*[:,;.!?]?\s*/u, '$1').trimStart();

/** The words with their `@token` at the start replaced by `@handle`. */
export const withHandle = (text: string, m: Member): string => text.replace(AT, (_, space: string) => `${space}@${handleFor(m)}`);

// ---- the queue's filters and search (§2) -------------------------------------------------------------------------------

/** What the State filter can say: Open is the default; the groups of the queue; All. */
export type RequestStateFilter = 'open' | RequestGroupId | 'all';
export const STATE_FILTERS: readonly { id: RequestStateFilter; label: string }[] = [
	{ id: 'open', label: 'Open' },
	{ id: 'needs_you', label: 'Needs you' },
	{ id: 'with_agents', label: 'With agents' },
	{ id: 'not_routed', label: 'Not routed' },
	{ id: 'approved', label: 'Approved' },
	{ id: 'closed', label: 'Declined and merged' },
	{ id: 'all', label: 'All' }
];

export interface RequestFilter {
	/** Open (the default, like a to-do list): New, Routed, being scoped or Scoped. */
	state: RequestStateFilter;
	/** Anyone (empty), `none` (not routed), `scoped:<member id>` or `raised:<member id>`. */
	who: string;
	/** A project id, `none` for no project, or any. */
	project: string;
	query: string;
}

/** The filters as the queue opens: Open, nothing else. */
export const NO_FILTER: RequestFilter = { state: 'open', who: '', project: '', query: '' };

/** Whether the queue is narrowed by who, project or words (not by the state, which only changes what it opens on). */
export const narrowed = (f: RequestFilter): boolean => f.who !== '' || f.project !== '' || f.query.trim() !== '';

/** Whether anything differs from how the queue opens. */
export const filterActive = (f: RequestFilter): boolean => f.state !== NO_FILTER.state || f.who !== '' || f.project !== '' || f.query.trim() !== '';

/** The requests that pass the filters: the words of the search must all be in the title, and a REQ-n is the number itself. */
export function filterRequests(items: WorkItemSummary[], f: RequestFilter): WorkItemSummary[] {
	const terms = f.query.toLowerCase().split(/\s+/).filter(Boolean);
	return items.filter((i) => {
		if (f.state === 'open' ? !OPEN_STATUSES.includes(i.status) : f.state !== 'all' && groupOf(i) !== f.state) return false;
		if (f.who === 'none' ? !!i.assignee_id : f.who.startsWith('scoped:') ? i.assignee_id !== f.who.slice(7) : f.who.startsWith('raised:') ? i.created_by !== f.who.slice(7) : false) return false;
		if (f.project === 'none' ? !!i.project_id : f.project !== '' && i.project_id !== f.project) return false;
		const title = i.title.toLowerCase();
		// A request number is matched on the number itself (REQ-1 is not REQ-12); other words are looked for in the title.
		return terms.every((t) => {
			const number = /^req-(\d+)$/.exec(t);
			return number ? i.number === Number(number[1]) : title.includes(t);
		});
	});
}
