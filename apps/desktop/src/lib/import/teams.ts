// The teams and the project the import suggests, from what HR knows (spec our-org-in-the-app §4). HR has departments, not
// teams: a department becomes a top-level team, and the owner shapes the rest (App, Office, ... under Engineering) in the
// editable preview. Pure.

import { breakCycles, isReady, OWNER, takesPart, type Row } from './rows';

/** A team to create. `key` is its name in lower case: the same team is the same team whatever its capitals. */
export interface TeamDraft {
	key: string;
	name: string;
	/** The key of the team that owns it, or none for a top-level team. */
	parentKey?: string;
	/** The row (key) that leads it; it is also a member. */
	leadKey?: string;
}

export interface ProjectDraft {
	key: string;
	name: string;
	/** The teams that share it (team keys). */
	teamKeys: string[];
}

const key = (name: string) => name.trim().toLowerCase();

/**
 * Teams from the rows' departments. A team's lead is the member the others in it report to (the most reports inside the
 * team), or, with one member, that member. Rows with no department are in no team yet.
 */
export function suggestTeams(rows: Row[]): TeamDraft[] {
	const named = new Map<string, string>();
	for (const row of rows) if (row.teamKey?.trim() && !named.has(key(row.teamKey))) named.set(key(row.teamKey), row.teamKey.trim());
	return [...named.entries()]
		.sort(([a], [b]) => a.localeCompare(b))
		.map(([k, name]) => {
			// Only a row that will be brought in can lead.
			const members = rows.filter((r) => takesPart(r) && r.teamKey && key(r.teamKey) === k);
			const reportsIn = (candidate: Row) => members.filter((m) => m.managerKey === candidate.key).length;
			const lead = [...members].sort((a, b) => reportsIn(b) - reportsIn(a) || a.key.localeCompare(b.key))[0];
			return { key: k, name, ...(lead && (reportsIn(lead) > 0 || members.length === 1) ? { leadKey: lead.key } : {}) };
		});
}

/** The rows that go in a team: chosen and complete (one already in is still put in its team if it has none). */
export const membersOf = (rows: Row[], team: TeamDraft): Row[] => rows.filter((r) => takesPart(r) && r.teamKey && key(r.teamKey) === team.key);

/** The teams that will be created: the ones with someone to bring in, and the ones that own them. */
export function teamsToCreate(rows: Row[], drafts: TeamDraft[]): TeamDraft[] {
	const used = new Set<string>();
	for (const draft of drafts) if (membersOf(rows, draft).length > 0) used.add(draft.key);
	let grew = true;
	while (grew) {
		grew = false;
		for (const draft of drafts) if (used.has(draft.key) && draft.parentKey && !used.has(draft.parentKey)) (used.add(draft.parentKey), (grew = true));
	}
	return drafts.filter((d) => used.has(d.key));
}

/**
 * Who a row reports to when HR does not say: the lead of the top-level team it is in, unless that is the row itself; otherwise the
 * owner. A row with a reports_to in HR keeps it, and so does one the owner set by hand. Applied once the teams are known (the hints file and the department teams), so the
 * lines of an organization shaped by its teams need no edits. A circle this could make is cut at the owner.
 */
export function withDefaultManagers(rows: Row[], drafts: TeamDraft[]): Row[] {
	const byKey = new Map(drafts.map((d) => [d.key, d]));
	const topOf = (teamKey: string): TeamDraft | undefined => {
		let at = byKey.get(key(teamKey));
		for (let hops = 0; at?.parentKey && hops <= drafts.length; hops++) at = byKey.get(at.parentKey) ?? at;
		return at;
	};
	const next = rows.map((row) => {
		if (row.hr?.reports_to?.trim() || row.managerSet) return row;
		if (!row.teamKey) return { ...row, managerKey: OWNER };
		const lead = topOf(row.teamKey)?.leadKey;
		return lead && lead !== row.key && rows.some((r) => r.key === lead && r.choice !== 'skip') ? { ...row, managerKey: lead } : { ...row, managerKey: OWNER };
	});
	breakCycles(next);
	return next;
}

/** The project HR knows the agents by (`ac`), shared by the top-level teams that will exist. HR keeps only the code. */
export function suggestProject(rows: Row[], drafts: TeamDraft[]): ProjectDraft | undefined {
	const codes = new Map<string, number>();
	for (const row of rows) if (row.hr?.project) codes.set(row.hr.project, (codes.get(row.hr.project) ?? 0) + 1);
	const code = [...codes.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))[0]?.[0];
	if (!code) return undefined;
	const teamKeys = teamsToCreate(rows, drafts)
		.filter((t) => !t.parentKey)
		.map((t) => t.key);
	return { key: code, name: code, teamKeys };
}

/** "Creates 10 agents, 6 teams and 1 project in <org>. Nothing changes in your HR workspace or in messaging. Agents keep running where they are." */
export function whatHappens(rows: Row[], drafts: TeamDraft[], project: ProjectDraft | undefined, organizationName: string): string {
	const agents = rows.filter(isReady).length;
	const teams = teamsToCreate(rows, drafts).length;
	const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;
	const made = [plural(agents, 'agent'), plural(teams, 'team'), ...(project && agents > 0 ? [plural(1, 'project')] : [])];
	const list = made.length > 1 ? `${made.slice(0, -1).join(', ')} and ${made.at(-1)}` : made[0];
	return `Creates ${list} in ${organizationName}. Nothing changes in your HR workspace or in messaging. Agents keep running where they are.`;
}

