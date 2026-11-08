// The optional team hints file (our-org-in-the-app IMP-10): teams, parents, leads and members the owner wrote down, since
// HR has departments and nothing finer. Pure: it fits the file onto the rows the scan found.

import type { Row } from './rows';
import type { TeamDraft } from './teams';
import type { Hints } from './types';

export interface Fitted {
	rows: Row[];
	/** The teams: the file's, and the department teams it does not mention. */
	drafts: TeamDraft[];
	/** Names the file lists that match no agent, and parents it names that it does not define. Shown, never blocking. */
	notFound: string[];
	/** How many teams came from the file. */
	fromFile: number;
}

const lower = (s: string) => s.trim().toLowerCase();

/**
 * Put the file's teams on the rows. A row listed in several teams goes to the deepest one. A lead who is not among the
 * team's members becomes a member (HQ refuses a lead who is not one). A team of the file with the same name as a
 * department's team fills that team in instead of adding another. The rows are returned as new objects.
 */
export function applyHints(rows: Row[], hints: Hints, departmentTeams: TeamDraft[]): Fitted {
	if (hints.error || hints.teams.length === 0) return { rows, drafts: departmentTeams, notFound: [], fromFile: 0 };
	const byRef = new Map<string, Row>();
	for (const row of rows) {
		byRef.set(lower(row.messagingName), row);
		if (row.hr) byRef.set(lower(row.hr.id), row);
	}
	const keys = new Map(hints.teams.map((t) => [lower(t.key), t]));
	const nameOf = (t: { key: string; name?: string }) => (t.name?.trim() ? t.name.trim() : t.key);
	const notFound: string[] = [];
	const missing = (ref: string) => notFound.includes(ref) || notFound.push(ref);

	const depthOf = (key: string): number => {
		let depth = 0;
		const seen = new Set<string>();
		for (let t = keys.get(lower(key)); t?.parent && !seen.has(lower(t.key)); t = keys.get(lower(t.parent))) {
			seen.add(lower(t.key));
			depth += 1;
		}
		return depth;
	};

	const next = new Map(rows.map((r) => [r.key, { ...r }]));
	const placed = new Map<string, number>(); // row key -> depth of the team it was placed in
	const place = (ref: string, team: (typeof hints.teams)[number]) => {
		const row = byRef.get(lower(ref));
		if (!row) return missing(ref), undefined;
		const depth = depthOf(team.key);
		if ((placed.get(row.key) ?? -1) <= depth) {
			next.get(row.key)!.teamKey = nameOf(team);
			placed.set(row.key, depth);
		}
		return row;
	};
	const drafts: TeamDraft[] = hints.teams.map((t) => {
		const draft: TeamDraft = { key: lower(nameOf(t)), name: nameOf(t) };
		const parent = t.parent ? keys.get(lower(t.parent)) : undefined;
		if (t.parent && !parent) missing(t.parent);
		if (parent) draft.parentKey = lower(nameOf(parent));
		return draft;
	});
	// A team can't be its own owner, however far round: the team that closes a circle loses its parent.
	const ownerOf = new Map(drafts.map((d) => [d.key, d.parentKey]));
	for (const draft of drafts) {
		for (let at = draft.parentKey, hops = 0; at; at = ownerOf.get(at), hops++) {
			if (at === draft.key || hops > drafts.length) {
				missing(`${draft.name} (its owner team is a circle)`);
				delete draft.parentKey;
				ownerOf.set(draft.key, undefined);
				break;
			}
		}
	}
	for (const team of hints.teams) for (const ref of team.members) place(ref, team);
	// A lead leads the team they are placed in. One who is also in a deeper team keeps that membership here as well: the
	// executor adds the membership HQ needs before it names them lead.
	hints.teams.forEach((team, i) => {
		if (!team.lead) return;
		const lead = place(team.lead, team);
		if (lead) drafts[i].leadKey = lead.key;
	});

	const fromFile = new Set(drafts.map((d) => d.key));
	return {
		rows: rows.map((r) => next.get(r.key)!),
		drafts: [...drafts, ...departmentTeams.filter((d) => !fromFile.has(d.key))],
		notFound,
		fromFile: hints.teams.length
	};
}
