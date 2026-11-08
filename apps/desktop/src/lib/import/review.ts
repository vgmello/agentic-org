// What the review screen does with the rows and teams, as plain functions (so it is tested without the screen).

import { handleProblem, hasNoMessages, isValidHandle, missingOf, type Row } from './rows';
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

/** Give the row a different messaging name (the one HR or the inbox had was refused). */
export const renameHandle = (row: Row, name: string): Row => ({ ...row, messagingName: name.trim() });
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
