// What stops a team from being archived or deleted (offices-and-teams-ux §7.10, TEAM-9), worked out before either is
// offered: the teams under it, the employees whose home team it is, the active projects it is the accountable owner
// of and its open tasks. HQ still decides and refuses on its own; this spares the person a confirmation that would be
// refused, and names each blocker so it can be resolved. Archive ignores teams under it that are archived already;
// Delete counts them, since they still point at it.
import { toAppError } from '$lib/proto';
import { UNAVAILABLE_MESSAGE } from '$lib/platform/unavailable-api';
import type { MemberRow, TeamRef, WorkRow } from '$lib/modules/members';
import type { TeamProjectRow, TeamRow } from './types';

export type TeamRemoval = 'archive' | 'delete';

export interface TeamBlockers {
	teams: TeamRef[];
	employees: { id: string; name: string; kind: 'agent' | 'person' }[];
	/** Active projects it owns (it may contribute to others: that link goes with it). */
	projects: { id: string; name: string }[];
	/** Tasks it is accountable for that are not done. */
	tasks: { ref: string; title: string }[];
}

/** What the check reads, all for the one team. */
export interface TeamFacts {
	teams: TeamRow[];
	members: MemberRow[];
	projects: TeamProjectRow[];
	work: WorkRow[];
}

const byName = (a: { name: string }, b: { name: string }) => a.name.localeCompare(b.name);

export function teamBlockers(teamId: string, facts: TeamFacts, removal: TeamRemoval): TeamBlockers {
	return {
		teams: facts.teams
			.filter((t) => t.parentId === teamId && (removal === 'delete' || t.status !== 'archived'))
			.map((t) => ({ id: t.id, name: t.name, ref: t.ref }))
			.sort(byName),
		employees: facts.members
			.filter((m) => m.homeTeam?.id === teamId && m.status !== 'inactive')
			.map((m) => ({ id: m.id, name: m.name, kind: m.kind }))
			.sort(byName),
		projects: facts.projects
			.filter((p) => p.role === 'owner')
			.map((p) => ({ id: p.id, name: p.name }))
			.sort(byName),
		tasks: facts.work
			.filter((w) => w.state !== 'done')
			.map((w) => ({ ref: w.ref, title: w.title }))
			.sort((a, b) => a.ref.localeCompare(b.ref, undefined, { numeric: true }))
	};
}

export const isBlocked = (b: TeamBlockers) =>
	b.teams.length + b.employees.length + b.projects.length + b.tasks.length > 0;

const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
const list = (parts: string[]) =>
	parts.length <= 1 ? (parts[0] ?? '') : `${parts.slice(0, -1).join(', ')} and ${parts.at(-1)}`;

/** "Checkout still has 1 child team and 2 home employees. Resolve them first." (§7.10) */
export function blockedSentence(name: string, b: TeamBlockers): string {
	const parts = [
		b.teams.length > 0 && count(b.teams.length, 'child team', 'child teams'),
		b.employees.length > 0 && count(b.employees.length, 'home employee', 'home employees'),
		b.projects.length > 0 && count(b.projects.length, 'project it owns', 'projects it owns'),
		b.tasks.length > 0 && count(b.tasks.length, 'open task', 'open tasks')
	].filter((p): p is string => !!p);
	return `${name} still has ${list(parts)}. Resolve them first.`;
}

/**
 * Distinguishes an explicit unserved/unsupported work endpoint stub from a transient read failure (e.g. gateway 503).
 * Never treats a transient 503 as a clean empty work list.
 */
export function classifyTeamWorkCheckError(
	error: unknown
): { kind: 'unserved' } | { kind: 'failed'; message: string } {
	const failure = toAppError(error);
	const isUnservedStub =
		failure.source === 'api' &&
		((failure.code === 'unavailable' && failure.message === UNAVAILABLE_MESSAGE) ||
			failure.code === 'unsupported');
	if (isUnservedStub) return { kind: 'unserved' };
	return { kind: 'failed', message: failure.message };
}

/** Where the check stands while the Archive or Delete dialog is open. */
export type BlockerCheck =
	| { state: 'checking' }
	| { state: 'failed'; message: string }
	| { state: 'done'; blockers: TeamBlockers; workCheckError?: string };
