// What the review screen does with the rows and teams, as plain functions (so it is tested without the screen).

import type { Member } from '$lib/proto';
import { handleProblem, hasNoMessages, isValidHandle, missingOf, OWNER, type Row } from './rows';
import type { TeamDraft } from './teams';
import type { Scan } from './types';

export type RowFilter = 'all' | 'needs' | 'ready' | 'skipped';

export const FILTERS: { id: RowFilter; label: string }[] = [
	{ id: 'all', label: 'All' },
	{ id: 'needs', label: 'Needs details' },
	{ id: 'ready', label: 'Ready' },
	{ id: 'skipped', label: 'Skipped' }
];

export function filterRows(rows: Row[], filter: RowFilter): Row[] {
	if (filter === 'all') return rows;
	const settled = (r: Row) => !!r.existingMemberId || r.existingOffboarded;
	if (filter === 'skipped') return rows.filter((r) => r.choice === 'skip');
	if (filter === 'needs') return rows.filter((r) => r.choice !== 'skip' && !settled(r) && missingOf(r).length > 0);
	return rows.filter((r) => r.choice !== 'skip' && !settled(r) && missingOf(r).length === 0);
}

/** Replace one row, by key. */
export const patchRow = (rows: Row[], key: string, change: Partial<Row> | ((row: Row) => Row)): Row[] =>
	rows.map((r) => (r.key === key ? (typeof change === 'function' ? change(r) : { ...r, ...change }) : r));

/** What a row says besides its fields: why it can't be brought in as it is, or what the owner should know. */
export function rowNote(row: Row, scan: Scan, organization: string): { tone: 'info' | 'warn' | 'bad'; text: string } | undefined {
	if (row.existingOffboarded) return { tone: 'warn', text: `Offboarded in ${organization}: its name stays reserved.` };
	if (row.existingKind === 'person') return { tone: 'warn', text: `A person in ${organization} already has this name.` };
	if (row.existingMemberId) return { tone: 'info', text: `Already in ${organization}.` };
	const name = missingOf(row).includes('name') ? handleProblem(row.messagingName) : undefined;
	if (name) return { tone: 'bad', text: `Needs a messaging name. ${name}` };
	if (hasNoMessages(row, scan)) return { tone: 'info', text: `No messages from ${row.name} yet. Active in HR but not on messaging: bring in as Paused if ${row.name} isn't working now.` };
	return undefined;
}

/** The rows `row` may report to: anyone staying but itself and anyone whose own line leads back to it (a circle HQ would refuse). */
export function managerChoices(rows: Row[], row: Row): Row[] {
	const byKey = new Map(rows.map((r) => [r.key, r]));
	const leadsBack = (from: Row): boolean => {
		for (let at: Row | undefined = from, hops = 0; at && hops <= rows.length; at = byKey.get(at.managerKey), hops++) if (at.key === row.key) return true;
		return false;
	};
	return rows.filter((r) => r.key !== row.key && staying(r) && !leadsBack(r));
}

const lowerName = (s: string) => s.trim().toLowerCase();

/**
 * Give the row a different messaging name (the one HR or the inbox had was refused). It must be one HQ accepts, not another row's
 * name; and if the organization already has an agent or a person by it, the row is "Already in" (never made twice).
 */
export function renameHandle(rows: Row[], key: string, raw: string, members: Member[]): { rows: Row[]; error?: string } {
	const name = raw.trim();
	const problem = handleProblem(name);
	if (problem) return { rows, error: problem };
	const other = rows.find((r) => r.key !== key && lowerName(r.messagingName) === lowerName(name));
	if (other) return { rows, error: `${other.name} already uses that name.` };
	const taken = members.find((m) => (m.kind === 'agent' && m.agent && lowerName(m.agent.messaging_name) === lowerName(name)) || (m.kind === 'person' && !m.owner && lowerName(m.name) === lowerName(name)));
	return {
		rows: rows.map((r) => {
			if (r.key !== key) return r;
			const { existingMemberId: _a, existingKind: _b, existingOffboarded: _c, ...rest } = r;
			return { ...rest, messagingName: name, ...(taken ? { existingMemberId: taken.id, existingKind: taken.kind === 'agent' ? ('agent' as const) : ('person' as const), ...(taken.agent?.status === 'offboarded' ? { existingOffboarded: true } : {}) } : {}) };
		})
	};
}
export const handleOk = (name: string): boolean => isValidHandle(name.trim());

// ---- teams

const key = (name: string) => name.trim().toLowerCase();

/** A new team with a name of its own: its key is the name in lower case, made unique among the teams. */
export function addTeam(drafts: TeamDraft[], name: string): TeamDraft[] {
	const clean = name.trim();
	if (!clean) return drafts;
	let id = key(clean);
	for (let n = 2; drafts.some((d) => d.key === id); n++) id = `${key(clean)}-${n}`;
	return [...drafts, { key: id, name: clean }];
}

/** The teams below `teamKey`, however deep. */
export function below(drafts: TeamDraft[], teamKey: string): Set<string> {
	const found = new Set<string>();
	for (let grew = true; grew; ) {
		grew = false;
		for (const d of drafts) if (d.parentKey && (d.parentKey === teamKey || found.has(d.parentKey)) && !found.has(d.key)) (found.add(d.key), (grew = true));
	}
	return found;
}

/** Remove a team: the teams it owned become top-level, and whoever was in it is in no team. */
export function removeTeam(drafts: TeamDraft[], rows: Row[], teamKey: string): { drafts: TeamDraft[]; rows: Row[] } {
	return {
		drafts: drafts.filter((d) => d.key !== teamKey).map((d) => (d.parentKey === teamKey ? { ...d, parentKey: undefined } : d)),
		rows: rows.map((r) => (r.teamKey && key(r.teamKey) === teamKey ? { ...r, teamKey: undefined } : r))
	};
}

/** Move a team under another (or to the top). A team can't go under itself or under what it owns. */
export function setParent(drafts: TeamDraft[], teamKey: string, parentKey: string | undefined): TeamDraft[] {
	if (parentKey && (parentKey === teamKey || below(drafts, teamKey).has(parentKey))) return drafts;
	return drafts.map((d) => (d.key === teamKey ? { ...d, parentKey } : d));
}

/** Whether a row stays in the import (one still to be filled in will be by the time it runs): not skipped, not a person's name, not offboarded. */
export const staying = (r: Row): boolean => r.choice !== 'skip' && r.existingKind !== 'person' && !r.existingOffboarded;

/**
 * After a change, drop what points at nothing: a manager who is no longer part of the import reports to the owner instead, and
 * a team's lead who left it (or the import) leads nothing. A lead may sit in a team below the one they lead (the file puts them
 * there, and the executor adds them to the team they lead).
 */
export function normalize(rows: Row[], drafts: TeamDraft[]): { rows: Row[]; drafts: TeamDraft[] } {
	const parts = new Set(rows.filter(staying).map((r) => r.key));
	const fixed = rows.map((r) => (r.managerKey !== OWNER && !parts.has(r.managerKey) ? { ...r, managerKey: OWNER } : r));
	const byKey = new Map(fixed.map((r) => [r.key, r]));
	return {
		rows: fixed,
		drafts: drafts.map((d) => {
			if (!d.leadKey) return d;
			const lead = byKey.get(d.leadKey);
			const inside = lead?.teamKey && (key(lead.teamKey) === d.key || below(drafts, d.key).has(key(lead.teamKey)));
			return lead && staying(lead) && inside ? d : { ...d, leadKey: undefined };
		})
	};
}
