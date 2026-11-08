// The optional team hints file (our-org-in-the-app IMP-10): teams, parents, leads and members the owner wrote down, since
// HR has departments and nothing finer. Pure: it fits the file onto the rows the scan found.
//
// A team is known by its id: the file's table id, or for a department team the department's name in lower case. A table
// whose id is a department team's sets that team's name, lead and members instead of adding a second one, and a `parent`
// names any team in the plan.

import type { Row } from './rows';
import type { TeamDraft } from './teams';
import type { Hints } from './types';

/** What the review shows about the file besides the teams. Each is shown and none blocks the import. */
export interface HintNotes {
	/** "Parent not found: <id>": the team went top-level. */
	parents: string[];
	/** "Lead not found: <name>": the team has no lead. */
	leads: string[];
	/** "Not found in this import: <name>": a member the scan did not find. */
	members: string[];
	/** Owner teams that go round in a circle: the team that closes it goes top-level. */
	circles: string[];
}

export interface Fitted {
	rows: Row[];
	/** The teams: the file's, and the department teams it does not mention. */
	drafts: TeamDraft[];
	notes: HintNotes;
	/** Every team in the plan ("Teams: from <file> (n)"). */
	teamCount: number;
}

const lower = (s: string) => s.trim().toLowerCase();
const noNotes = (): HintNotes => ({ parents: [], leads: [], members: [], circles: [] });

/**
 * Put the file's teams on the rows. A row listed in several teams goes to the deepest one. A lead who is not among a team's
 * members becomes one (HQ refuses a lead who is not a member). The rows are returned as new objects.
 */
export function applyHints(rows: Row[], hints: Hints, departmentTeams: TeamDraft[]): Fitted {
	if (hints.error || hints.teams.length === 0) return { rows, drafts: departmentTeams, notes: noNotes(), teamCount: departmentTeams.length };
	const notes = noNotes();
	const note = (list: string[], text: string) => void (list.includes(text) || list.push(text));

	const byRef = new Map<string, Row>();
	for (const row of rows) {
		byRef.set(lower(row.messagingName), row);
		if (row.hr) byRef.set(lower(row.hr.id), row);
	}

	// The plan's teams by id: the department teams, then the file's tables over them.
	const drafts = new Map<string, TeamDraft>(departmentTeams.map((d) => [d.key, { ...d }]));
	const tables = new Map(hints.teams.map((t) => [lower(t.key), t]));
	for (const table of tables.values()) {
		const key = lower(table.key);
		const name = table.name?.trim() || table.key.trim();
		// A table over a department team keeps nothing of the department's lead: the file says who leads it.
		drafts.set(key, { key, name });
	}
	for (const table of tables.values()) {
		const draft = drafts.get(lower(table.key))!;
		if (!table.parent) continue;
		const parent = lower(table.parent);
		if (drafts.has(parent) && parent !== draft.key) draft.parentKey = parent;
		else note(notes.parents, table.parent);
	}
	breakCircles(drafts, notes);

	const depthOf = (key: string): number => {
		let depth = 0;
		for (let at = drafts.get(key)?.parentKey; at && depth <= drafts.size; at = drafts.get(at)?.parentKey) depth += 1;
		return depth;
	};

	const next = new Map(rows.map((r) => [r.key, { ...r }]));
	const placed = new Map<string, number>(); // row key -> depth of the team it was placed in
	const place = (ref: string, key: string): Row | undefined => {
		const row = byRef.get(lower(ref));
		if (!row) return undefined;
		const depth = depthOf(key);
		if ((placed.get(row.key) ?? -1) <= depth) {
			next.get(row.key)!.teamKey = key;
			placed.set(row.key, depth);
		}
		return row;
	};
	for (const table of tables.values()) for (const ref of table.members) if (!place(ref, lower(table.key))) note(notes.members, ref);
	// A lead leads the team the file says. One who is also in a deeper team keeps that membership, and the executor adds the
	// membership HQ needs here before it names them lead.
	for (const table of tables.values()) {
		if (!table.lead) continue;
		const lead = place(table.lead, lower(table.key));
		if (lead) drafts.get(lower(table.key))!.leadKey = lead.key;
		else note(notes.leads, table.lead);
	}

	const all = [...drafts.values()];
	return { rows: rows.map((r) => next.get(r.key)!), drafts: all, notes, teamCount: all.length };
}

/** A team can't be its own owner, however far round: the team that closes a circle loses its parent. */
function breakCircles(drafts: Map<string, TeamDraft>, notes: HintNotes): void {
	for (const draft of drafts.values()) {
		for (let at = draft.parentKey, hops = 0; at; at = drafts.get(at)?.parentKey, hops++) {
			if (at === draft.key || hops > drafts.size) {
				if (!notes.circles.includes(draft.name)) notes.circles.push(draft.name);
				delete draft.parentKey;
				break;
			}
		}
	}
}
