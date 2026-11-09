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
	add(agents, (m) => `${title(m)}`);
	return picks;
}

export interface ActivityLine {
	at: string;
	/** One sentence: "You moved this from ada to Eden". */
	text: string;
	/** What was written, for a note. */
	note?: string;
}

const FIELD_WORD: Record<string, string> = { title: 'title', request_kind: 'kind', project_id: 'project' };

/**
 * A request's conversation and history (§3): who did what, in plain words, oldest first. `me` is the owner, who reads
 * as "You". Events of a kind this build does not know read "changed it".
 */
export function activityLines(events: WorkEvent[], snapshot: OrgSnapshot, me: string): ActivityLine[] {
	const who = (id: string | undefined) => (id === me ? 'You' : id ? nameOf(snapshot, id) : 'The office');
	const whom = (id: string | undefined) => (id === me ? 'you' : nameOf(snapshot, id));
	return events.flatMap((e): ActivityLine[] => {
		const c = e.change;
		const by = who(e.actor_id);
		switch (c.type) {
			case 'created':
				return [{ at: e.at, text: `${by} raised this` }];
			case 'assigned':
				return [{ at: e.at, text: c.from && c.to ? `${by} moved this from ${whom(c.from)} to ${whom(c.to)}` : c.to ? `${by} sent this to ${whom(c.to)}` : `${by} took this back` }];
			case 'comment':
				return [{ at: e.at, text: `${by} wrote`, note: c.body }];
			case 'fields_changed':
				return [{ at: e.at, text: `${by} changed the ${c.fields.map((f) => FIELD_WORD[f] ?? f).join(' and ')}` }];
			case 'status_changed': {
				if (c.to === 'scoping') return c.from === 'scoped' ? [] : [{ at: e.at, text: `${by} accepted it` }];
				if (c.to === 'scoped') return [{ at: e.at, text: `${by} scoped it` }];
				if (c.to === 'approved') return [{ at: e.at, text: `${by} approved it` }];
				if (c.to === 'declined') return [{ at: e.at, text: `${by} declined it`, note: c.reason }];
				if (c.to === 'merged') return [{ at: e.at, text: `${by} merged it`, note: c.reason }];
				// Back to routed or not routed: the line about who it was sent to already says it.
				if (c.to === 'routed' || c.to === 'new') return [];
				return [{ at: e.at, text: `${by} changed it` }];
			}
			default:
				return [{ at: e.at, text: `${by} changed it` }];
		}
	});
}
