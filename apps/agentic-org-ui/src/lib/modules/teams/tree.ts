// The Teams index as indented rows (TEAM-1): the recursive parent tree flattened in reading order, each row with
// its depth. A parent missing from the list, or a loop, never hides a team: such a team becomes a top-level row.
import type { TeamRow } from './types';

export interface IndentedTeam {
	team: TeamRow;
	depth: number;
	children: number;
}

const byName = (a: TeamRow, b: TeamRow) =>
	a.name.localeCompare(b.name, undefined, { sensitivity: 'base' }) || a.id.localeCompare(b.id);

export function indentTeams(rows: TeamRow[]): IndentedTeam[] {
	const ids = new Set(rows.map((r) => r.id));
	const kids = new Map<string | undefined, TeamRow[]>();
	for (const r of rows) {
		const parent =
			r.parentId && ids.has(r.parentId) && r.parentId !== r.id ? r.parentId : undefined;
		kids.set(parent, [...(kids.get(parent) ?? []), r]);
	}
	const out: IndentedTeam[] = [];
	const seen = new Set<string>();
	const walk = (team: TeamRow, depth: number) => {
		if (seen.has(team.id)) return;
		seen.add(team.id);
		const children = (kids.get(team.id) ?? []).sort(byName);
		out.push({ team, depth, children: children.length });
		for (const c of children) walk(c, depth + 1);
	};
	for (const r of (kids.get(undefined) ?? []).sort(byName)) walk(r, 0);
	for (const r of [...rows].sort(byName)) walk(r, 0);
	return out;
}

/** Where focus goes when a row disappears: the next surviving row, else the previous, else nothing (the heading). */
export function focusAfterRemoval(
	order: string[],
	removed: string,
	survivors: Set<string>
): string | undefined {
	const at = order.indexOf(removed);
	if (at < 0) return undefined;
	return (
		order.slice(at + 1).find((id) => survivors.has(id)) ??
		order
			.slice(0, at)
			.reverse()
			.find((id) => survivors.has(id))
	);
}
