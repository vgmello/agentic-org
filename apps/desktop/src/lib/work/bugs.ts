// What the bug screens say and decide, as pure functions (work-items-ux §4-§7; BUG-*, TRI-*, FIX-*).
import type { Severity, WorkItemSummary } from '$lib/proto';

export const SEVERITIES: { id: Exclude<Severity, 'unknown'>; label: string; help: string }[] = [
	{ id: 'critical', label: 'Critical', help: 'Data loss, a crash, or nobody can work.' },
	{ id: 'major', label: 'Major', help: 'A main function is broken and there is no good workaround.' },
	{ id: 'minor', label: 'Minor', help: 'Something is wrong, but there is a workaround.' },
	{ id: 'trivial', label: 'Trivial', help: 'Cosmetic or very rare.' }
];
const RANK: Record<string, number> = { critical: 0, major: 1, minor: 2, trivial: 3 };

export const severityWord = (s: Severity | undefined): string => SEVERITIES.find((x) => x.id === s)?.label ?? 'Severity not set';

/** "B-7" (or "chk-B-7" where items of several projects are listed), or "Pending · 3k9f2x" while HQ has not numbered it. */
export const bugRef = (b: WorkItemSummary, projectKey?: string): string => (b.number ? `${projectKey ? `${projectKey}-` : ''}B-${b.number}` : `Pending · ${b.short_id}`);

export const BUG_STATUS_LABEL: Record<string, string> = { new: 'New', triaged: 'Triaged', in_progress: 'In progress', fixed: 'Fixed', closed: 'Closed' };

/** Closed is the end; everything else is open. */
export const isOpenBug = (b: WorkItemSummary): boolean => b.status !== 'closed';

/** How a closed bug ended, in words ("Verified", "Won't fix", "Can't reproduce", "Duplicate of B-3"). */
export function closedWord(b: WorkItemSummary, original?: WorkItemSummary): string {
	switch (b.resolution) {
		case 'verified':
			return 'Verified';
		case 'wont_fix':
			return "Won't fix";
		case 'cant_reproduce':
			return "Can't reproduce";
		case 'duplicate':
			return original ? `Duplicate of ${bugRef(original)}` : 'Duplicate';
		default:
			return 'Closed';
	}
}

export type BugFilter = 'open' | 'needs_triage' | 'fixed' | 'closed' | 'all';

export const FILTERS: { id: BugFilter; label: string }[] = [
	{ id: 'open', label: 'Open' },
	{ id: 'needs_triage', label: 'Needs triage' },
	{ id: 'fixed', label: 'Waiting to verify' },
	{ id: 'closed', label: 'Closed' },
	{ id: 'all', label: 'All' }
];

export function filterBugs(bugs: WorkItemSummary[], filter: BugFilter): WorkItemSummary[] {
	const only = bugs.filter((b) => b.kind === 'bug');
	switch (filter) {
		case 'open':
			return only.filter(isOpenBug);
		case 'needs_triage':
			return only.filter((b) => b.status === 'new');
		case 'fixed':
			return only.filter((b) => b.status === 'fixed');
		case 'closed':
			return only.filter((b) => b.status === 'closed');
		default:
			return only;
	}
}

/** Most severe first, then oldest. */
export const bySeverityThenAge = (a: WorkItemSummary, b: WorkItemSummary): number =>
	(RANK[a.severity ?? 'trivial'] ?? 3) - (RANK[b.severity ?? 'trivial'] ?? 3) || a.created_at.localeCompare(b.created_at) || (a.number ?? 0) - (b.number ?? 0);

export interface AreaGroup {
	areaId: string;
	name: string;
	bugs: WorkItemSummary[];
}

/** The bugs grouped by area, areas A–Z with General first; a bug with no known area goes under General. */
export function groupByArea(bugs: WorkItemSummary[], areas: { id: string; name: string; general?: boolean }[]): AreaGroup[] {
	const known = new Set(areas.map((a) => a.id));
	const general = areas.find((a) => a.general);
	const groups = areas
		.slice()
		.sort((a, b) => Number(!!b.general) - Number(!!a.general) || a.name.localeCompare(b.name, undefined, { sensitivity: 'base' }))
		.map((a) => ({ areaId: a.id, name: a.name, bugs: [] as WorkItemSummary[] }));
	for (const b of bugs) {
		const target = groups.find((g) => g.areaId === (b.area_id && known.has(b.area_id) ? b.area_id : general?.id));
		(target ?? groups[0])?.bugs.push(b);
	}
	for (const g of groups) g.bugs.sort(bySeverityThenAge);
	return groups.filter((g) => g.bugs.length > 0);
}

/** Bugs similar to a title being typed: open bugs sharing a word of three letters or more (BUG-4). */
export function similarBugs(bugs: WorkItemSummary[], title: string, max = 3): WorkItemSummary[] {
	const words = new Set(title.toLowerCase().split(/[^\p{L}\p{N}]+/u).filter((w) => w.length >= 3));
	if (words.size === 0) return [];
	return bugs
		.filter((b) => b.kind === 'bug' && isOpenBug(b))
		.map((b) => ({ b, hits: b.title.toLowerCase().split(/[^\p{L}\p{N}]+/u).filter((w) => words.has(w)).length }))
		.filter((x) => x.hits > 0)
		.sort((a, b) => b.hits - a.hits || bySeverityThenAge(a.b, b.b))
		.slice(0, max)
		.map((x) => x.b);
}
