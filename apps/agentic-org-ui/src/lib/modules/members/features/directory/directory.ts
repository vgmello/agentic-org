// The Members directory's rules as pure functions (offices-and-teams-ux §7.0, §7.2, PEOPLE-1, PEOPLE-6, TEAM-4):
// the All · People · Agents filter, the order rows are listed in, current versus former members, and the words a
// row shows beside the name.
import { ageText } from '../../present';
import type { MemberRow } from '../../types';

export const DIRECTORY_FILTERS = [
	{ id: 'all', label: 'All' },
	{ id: 'people', label: 'People' },
	{ id: 'agents', label: 'Agents' }
] as const;
export type DirectoryFilter = (typeof DIRECTORY_FILTERS)[number]['id'];

export function matchesFilter(row: Pick<MemberRow, 'kind'>, filter: DirectoryFilter): boolean {
	if (filter === 'people') return row.kind === 'person';
	if (filter === 'agents') return row.kind === 'agent';
	return true;
}

/** By name, then handle, so equal display names (allowed, PEOPLE-4) keep a stable order. */
export function byName(
	a: Pick<MemberRow, 'name' | 'handle'>,
	b: Pick<MemberRow, 'name' | 'handle'>
) {
	return (
		a.name.localeCompare(b.name, undefined, { sensitivity: 'base' }) ||
		a.handle.localeCompare(b.handle)
	);
}

export interface DirectoryView {
	/** Current members (active or paused) that match the filter, by name. */
	current: MemberRow[];
	/** Former members (inactive) that match the filter, by name. Their records stay; they are listed apart. */
	former: MemberRow[];
	/** Current members per filter, for the filter's counts. */
	counts: Record<DirectoryFilter, number>;
}

export function directoryView(rows: MemberRow[], filter: DirectoryFilter): DirectoryView {
	const now = rows.filter((r) => r.status !== 'inactive');
	return {
		current: now.filter((r) => matchesFilter(r, filter)).sort(byName),
		former: rows.filter((r) => r.status === 'inactive' && matchesFilter(r, filter)).sort(byName),
		counts: {
			all: now.length,
			people: now.filter((r) => r.kind === 'person').length,
			agents: now.filter((r) => r.kind === 'agent').length
		}
	};
}

/**
 * The row's second fact after its home team (§7.0): an agent's executing office and how fresh that report is, or
 * Unplaced; a person's contact, if any. A person never shows an office (PEOPLE-6).
 */
export function placementText(
	row: Pick<MemberRow, 'kind' | 'office' | 'contact'>,
	now = Date.now()
): string | undefined {
	if (row.kind === 'person') return row.contact || undefined;
	if (row.office === null) return 'Unplaced';
	if (!row.office) return undefined;
	return `Runs at ${row.office.name} · ${row.office.observedAt ? `reported ${ageText(row.office.observedAt, now)}` : 'never reported'}`;
}

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** What the polite status line announces after the filter changes: "3 agents shown". */
export function shownText(count: number, filter: DirectoryFilter): string {
	const noun =
		filter === 'people'
			? plural(count, 'person', 'people')
			: filter === 'agents'
				? plural(count, 'agent', 'agents')
				: plural(count, 'member', 'members');
	return `${noun} shown`;
}

/** "Left 3 Nov", for a former member whose end date HQ recorded. */
export function formerText(row: Pick<MemberRow, 'inactiveSince'>): string {
	if (!row.inactiveSince) return 'Former member';
	const at = new Date(row.inactiveSince);
	if (Number.isNaN(at.getTime())) return 'Former member';
	return `Left ${at.toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' })}`;
}

/** The name a sentence uses after the full name: "Sam" for "Sam Ortiz" (§7.4's toast). */
export function firstName(name: string): string {
	return name.trim().split(/\s+/)[0] || name;
}
