// The hire flow's rules in words (our-org-in-the-app §7, §7.1): what a request's state reads as, the provisioning
// steps in words, what a proposal still needs, and the refusals the spec words exactly.
import type { tone } from '$lib/components/pills';
import type {
	HireProposal,
	HireRequest,
	HireStep,
	HireTeam,
	HrDrafting,
	HrLeadStatus,
	RuntimeCheck
} from './types';

/** The refusal for someone who may hire nowhere, or not into the team named (§7.1, APPR-2). */
export const REFUSED = "You don't have access to hire. Ask your team lead.";

/** After this long without an answer the HR lead "is still starting" (§7 Suggestion). */
export const STILL_STARTING_MS = 60_000;

export const HIRE_STEPS: readonly HireStep[] = ['record', 'home', 'messaging'];

/** One provisioning step in words, while it runs, once done, and when it failed. */
export function stepWords(
	step: HireStep,
	name: string
): { doing: string; done: string; failed: string } {
	switch (step) {
		case 'record':
			return {
				doing: `Creating ${name}'s record…`,
				done: `Created ${name}'s record`,
				failed: `couldn't create ${name}'s record`
			};
		case 'home':
			return {
				doing: 'Setting up the home folder…',
				done: 'Set up the home folder',
				failed: "couldn't set up the home folder"
			};
		case 'messaging':
			return {
				doing: 'Registering with messaging…',
				done: 'Registered with messaging',
				failed: "couldn't register with messaging"
			};
	}
}

export type StepState = 'done' | 'doing' | 'waiting' | 'failed';

/** Each provisioning step and where it stands, in order. */
export function stepStates(r: Pick<HireRequest, 'state' | 'doneSteps' | 'failedStep'>): {
	step: HireStep;
	state: StepState;
}[] {
	let current = true;
	return HIRE_STEPS.map((step) => {
		if (r.state === 'hired' || r.doneSteps.includes(step)) return { step, state: 'done' };
		if (r.failedStep === step) {
			current = false;
			return { step, state: 'failed' };
		}
		if (current && r.state === 'hiring' && !r.failedStep) {
			current = false;
			return { step, state: 'doing' };
		}
		return { step, state: 'waiting' };
	});
}

/** "created Marlowe's record, set up the home folder": read after "Already done:". */
export function doneText(r: Pick<HireRequest, 'doneSteps'>, name: string): string {
	return HIRE_STEPS.filter((s) => r.doneSteps.includes(s))
		.map((s) => {
			const w = stepWords(s, name).done;
			return w.charAt(0).toLowerCase() + w.slice(1);
		})
		.join(', ');
}

/** A request's name in rows: the proposed name, else that HR is still drafting one. */
export const requestName = (r: Pick<HireRequest, 'name'>): string => r.name || 'Unnamed proposal';

export type RequestStatus = {
	/** The pill: short words that never truncate. */
	label: string;
	tone: keyof typeof tone | 'working';
	/** The whole state in words for the row's meta line, when the pill alone doesn't say it. */
	detail?: string;
	/** What the hirer (or the owner) can do with it from the list. */
	action?: 'open' | 'try_again';
};

/** A request's state as the Hire requests list shows it (§7.1, APPR-6). */
export function requestStatus(
	r: Pick<HireRequest, 'state' | 'failedStep' | 'closedReason'>,
	name = ''
): RequestStatus {
	const failed = r.failedStep
		? `Not finished: ${stepWords(r.failedStep, name || 'the agent').failed}`
		: undefined;
	switch (r.state) {
		case 'drafting':
			return { label: 'Drafting', tone: 'neutral', action: 'open' };
		case 'hiring':
			return r.failedStep
				? { label: 'Not finished', tone: 'bad', detail: failed, action: 'try_again' }
				: { label: 'Hiring…', tone: 'working' };
		case 'hired':
			return { label: 'Hired', tone: 'ok' };
		case 'withdrawn':
			return { label: 'Withdrawn', tone: 'neutral' };
		case 'failed':
			// Given up: the row keeps the step that failed, with no Try again (§7.1).
			return {
				label: 'Not finished',
				tone: 'bad',
				detail: [failed, `Given up: ${givenUpText(r)}`].filter(Boolean).join('. ')
			};
	}
}

/** Why a hire was given up, in words; the recorded reason when there is one. */
export const givenUpText = (r: Pick<HireRequest, 'closedReason'>): string =>
	r.closedReason || 'no reason was recorded';

/** Teams in the Teams index's order: the tree, depth first, each level by name; with each team's depth. */
export function teamsInTreeOrder<T extends { id: string; name: string; parentId?: string }>(
	teams: readonly T[]
): { team: T; depth: number }[] {
	const ids = new Set(teams.map((t) => t.id));
	const byName = (a: T, b: T) =>
		a.name.localeCompare(b.name, undefined, { sensitivity: 'base' }) || a.id.localeCompare(b.id);
	const kids = new Map<string | undefined, T[]>();
	for (const t of teams) {
		const parent =
			t.parentId && ids.has(t.parentId) && t.parentId !== t.id ? t.parentId : undefined;
		kids.set(parent, [...(kids.get(parent) ?? []), t]);
	}
	const out: { team: T; depth: number }[] = [];
	const seen = new Set<string>();
	const walk = (t: T, depth: number) => {
		if (seen.has(t.id)) return;
		seen.add(t.id);
		out.push({ team: t, depth });
		for (const c of (kids.get(t.id) ?? []).sort(byName)) walk(c, depth + 1);
	};
	for (const t of (kids.get(undefined) ?? []).sort(byName)) walk(t, 0);
	// A loop never hides a team.
	for (const t of [...teams].sort(byName)) walk(t, 0);
	return out;
}

/** Waits longer after each failed read, up to 30 s: 1 s, 2 s, 4 s … for a 1 s base. */
export const backoff = (baseMs: number, failures: number): number =>
	Math.min(baseMs * 2 ** Math.min(failures, 5), 30_000);

/** Whether the HR lead is reading, still starting after 60 s, or stopped. */
export function draftingPhase(
	d: HrDrafting,
	now = Date.now()
): 'reading' | 'still_starting' | 'stopped' | 'ready' {
	if (d.state !== 'reading') return d.state;
	return now - Date.parse(d.startedAt) >= STILL_STARTING_MS ? 'still_starting' : 'reading';
}

/** Why Describe can't ask the HR lead, word for word (§7 When the HR lead can't run). */
export function hrUnavailableText(hr: HrLeadStatus): string | undefined {
	if (hr.state === 'not_running') return "The HR lead can't run on this server yet.";
	if (hr.state === 'cant_start') return `The HR lead can't start: ${hr.reason}`;
	return undefined;
}

/** The team member limit refusal, with the limit named (§7.1). */
export const teamFullText = (t: Pick<HireTeam, 'name' | 'maxMembers'>): string =>
	`${t.name} already has ${t.maxMembers} members, its limit. Raise the limit on the team's page first.`;

export const isFull = (t: Pick<HireTeam, 'members' | 'maxMembers'>): boolean =>
	t.members >= t.maxMembers;

const HANDLE = /^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/;
const RESERVED = ['hr', 'owner'];

/** What is wrong with a handle, or nothing. Reserved and taken handles are refused here as HQ would. */
export function handleProblem(handle: string, taken: readonly string[]): string | undefined {
	if (handle === '') return undefined;
	if (!HANDLE.test(handle))
		return 'Use 1 to 64 letters, digits, - _ or ., starting with a letter or digit.';
	const lower = handle.toLowerCase();
	if (RESERVED.includes(lower)) return `"${lower}" is reserved. Choose another.`;
	if (taken.some((t) => t.toLowerCase() === lower)) return `${handle} is taken. Choose another.`;
	return undefined;
}

/** The fields Next needs, in the order the form shows them. */
const REQUIRED: { key: keyof HireProposal; words: string }[] = [
	{ key: 'name', words: 'name' },
	{ key: 'handle', words: 'handle' },
	{ key: 'homeTeamId', words: 'home team' },
	{ key: 'executionProfileId', words: 'execution profile' },
	{ key: 'model', words: 'model' },
	{ key: 'officeId', words: 'office' }
];

/** The required fields still empty, in form order. */
export function missingFields(p: Partial<HireProposal>): string[] {
	return REQUIRED.filter(({ key }) => !String(p[key] ?? '').trim()).map(({ words }) => words);
}

const article = (w: string) => (/^[aeiou]/.test(w) ? `an ${w}` : `a ${w}`);

/** "Choose a home team and handle to continue" (§7): one article, the rest listed plainly. */
export function missingReason(missing: readonly string[]): string | undefined {
	if (missing.length === 0) return undefined;
	const [first, ...rest] = missing;
	const list =
		rest.length === 0
			? article(first)
			: `${[article(first), ...rest.slice(0, -1)].join(', ')} and ${rest[rest.length - 1]}`;
	return `Choose ${list} to continue`;
}

/** The proposal as it will be sent: trimmed, with unset optional fields left out. */
export function cleanProposal(p: Partial<HireProposal>): HireProposal {
	const opt = (v: string | undefined) => (v && v.trim() ? v.trim() : undefined);
	return {
		name: (p.name ?? '').trim(),
		handle: (p.handle ?? '').trim(),
		homeTeamId: p.homeTeamId ?? '',
		executionProfileId: p.executionProfileId ?? '',
		model: p.model ?? '',
		officeId: p.officeId ?? '',
		...(opt(p.roleId) ? { roleId: opt(p.roleId) } : {}),
		...(opt(p.roleText) ? { roleText: opt(p.roleText) } : {}),
		...(opt(p.title) ? { title: opt(p.title) } : {}),
		...(opt(p.focus) ? { focus: opt(p.focus) } : {}),
		...(opt(p.reportsToId) ? { reportsToId: opt(p.reportsToId) } : {})
	};
}

/** Where the flow opens: the launching team's Hire, or a drafting request picked up again. */
export function hireUrl(options: { teamId?: string; requestId?: string } = {}): string {
	const q = new URLSearchParams();
	if (options.teamId) q.set('team', options.teamId);
	if (options.requestId) q.set('request', options.requestId);
	const s = q.toString();
	return s ? `/teams/hire?${s}` : '/teams/hire';
}

/** What the office observed about the runtime on an instance, for Suggestion: "Codex isn't signed in on build-01". */
export function runtimeText(driver: string, check: RuntimeCheck, instance: string): string {
	switch (check) {
		case 'ready':
			return `${driver} is installed and signed in on ${instance}`;
		case 'signed_out':
			return `${driver} isn't signed in on ${instance}`;
		case 'not_installed':
			return `${driver} isn't installed on ${instance}`;
		case 'unknown':
			return `Can't check ${driver} on ${instance}`;
	}
}

/** Review's warning for a runtime that isn't ready, word for word (§7, STAFF-3, STAFF-6); none when it is. */
export function runtimeWarning(
	driver: string,
	check: RuntimeCheck,
	instance: string,
	name: string
): string | undefined {
	const setup = `${name} is hired, but can't start until its administrator completes runtime setup there.`;
	if (check === 'signed_out') return `${driver} isn't signed in on ${instance}. ${setup}`;
	if (check === 'not_installed') return `${driver} isn't installed on ${instance}. ${setup}`;
	if (check === 'unknown')
		return `Can't check whether ${driver} is installed and signed in on ${instance}. ${name} will be hired; check the runtime setup there before ${name} starts.`;
	return undefined;
}

/** The flow's address with the request recorded (or cleared), keeping the rest of the query as it was. */
export function hireUrlWith(search: string, requestId: string | undefined): string {
	const q = new URLSearchParams(search);
	if (requestId) q.set('request', requestId);
	else q.delete('request');
	const s = q.toString();
	return s ? `/teams/hire?${s}` : '/teams/hire';
}

/** "15 min ago", "1 h 30 min ago", or the date once a day has passed: how old a request is. */
export function ageWords(iso: string, now = Date.now()): string {
	const ms = now - Date.parse(iso);
	if (!Number.isFinite(ms)) return 'unknown';
	if (ms >= 24 * 3600_000)
		return new Date(iso).toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
	const min = Math.max(0, Math.floor(ms / 60_000));
	if (min < 1) return 'just now';
	if (min < 60) return `${min} min ago`;
	const h = Math.floor(min / 60);
	return min % 60 ? `${h} h ${min % 60} min ago` : `${h} h ago`;
}

/** 24-hour time of day, e.g. "09:12". */
export const clockWords = (iso: string): string =>
	new Date(iso).toLocaleTimeString(undefined, {
		hour: '2-digit',
		minute: '2-digit',
		hourCycle: 'h23'
	});

export const HIRE_REQUESTS_URL = '/teams/hires';
