import { dayKey, dayLabel } from '$lib/modules/preferences';
import type { ActivityAction, ActivityRow } from './types';

/** Consecutive rows of the same action from the same member collapse once there are this many. */
export const GROUP_MIN = 3;

export type FeedEntry =
	| { type: 'row'; row: ActivityRow }
	| { type: 'group'; id: string; rows: ActivityRow[]; summary: string };

const PLURAL: Record<ActivityAction, (n: number) => string> = {
	message: (n) => `sent ${n} messages`,
	broadcast: (n) => `sent ${n} broadcasts`,
	status: (n) => `changed status ${n} times`,
	task: (n) => `updated ${n} tasks`,
	'task-done': (n) => `finished ${n} tasks`,
	bug: (n) => `reported ${n} bugs`,
	comment: (n) => `commented ${n} times`,
	merge: (n) => `merged ${n} changes`,
	run: (n) => `ran ${n} times`,
	team: (n) => `made ${n} team changes`,
	member: (n) => `made ${n} member changes`,
	decision: (n) => `recorded ${n} decisions`
};

/** One line for a collapsed group: "Tomas changed status 4 times". */
export const groupSummary = (rows: ActivityRow[]): string =>
	`${rows[0].actor.name} ${PLURAL[rows[0].action](rows.length)}`;

/** Collapses runs of at least `GROUP_MIN` consecutive rows with one actor and one action. Order is kept. */
export function groupRows(rows: ActivityRow[]): FeedEntry[] {
	const entries: FeedEntry[] = [];
	let i = 0;
	while (i < rows.length) {
		let j = i + 1;
		while (
			j < rows.length &&
			rows[j].actor.id === rows[i].actor.id &&
			rows[j].action === rows[i].action
		)
			j++;
		const run = rows.slice(i, j);
		if (run.length >= GROUP_MIN)
			entries.push({
				type: 'group',
				id: `group-${run[0].id}`,
				rows: run,
				summary: groupSummary(run)
			});
		else for (const row of run) entries.push({ type: 'row', row });
		i = j;
	}
	return entries;
}

export interface DayBlock {
	key: string;
	label: string;
	entries: FeedEntry[];
}

/** Entries under day headers ("Today", "Yesterday", "Mon 16 Nov") in the viewing zone, newest first. */
export function dayBlocks(rows: ActivityRow[], now: Date, zone: string): DayBlock[] {
	const blocks: DayBlock[] = [];
	const byDay = new Map<string, ActivityRow[]>();
	for (const row of rows) {
		const key = dayKey(new Date(row.at), zone);
		const list = byDay.get(key);
		if (list) list.push(row);
		else {
			byDay.set(key, [row]);
			blocks.push({ key, label: dayLabel(new Date(row.at), now, zone), entries: [] });
		}
	}
	for (const block of blocks) block.entries = groupRows(byDay.get(block.key) ?? []);
	return blocks;
}
