import { describe, expect, it } from 'vitest';
import { dayBlocks, groupRows } from './group';
import type { ActivityAction, ActivityRow } from './types';

let n = 0;
const row = (actor: string, action: ActivityAction, at = '2026-11-18T10:00:00Z'): ActivityRow => ({
	id: `r${++n}`,
	at,
	actor: { id: actor, name: actor, kind: 'agent' },
	kind: 'work',
	action,
	text: 'x',
	teamIds: [],
	projectIds: [],
	memberIds: []
});

describe('groupRows', () => {
	it('collapses three or more consecutive rows of one actor and action', () => {
		const entries = groupRows([
			row('tomas', 'status'),
			row('tomas', 'status'),
			row('tomas', 'status'),
			row('tomas', 'status'),
			row('mira', 'task')
		]);
		expect(entries).toHaveLength(2);
		expect(entries[0]).toMatchObject({ type: 'group', summary: 'tomas changed status 4 times' });
		expect(entries[1]).toMatchObject({ type: 'row' });
	});
	it('leaves pairs and interleaved rows alone', () => {
		expect(groupRows([row('a', 'status'), row('a', 'status')]).every((e) => e.type === 'row')).toBe(
			true
		);
		expect(
			groupRows([row('a', 'status'), row('a', 'task'), row('a', 'status')]).every(
				(e) => e.type === 'row'
			)
		).toBe(true);
	});
});

describe('dayBlocks', () => {
	it('puts rows under Today and Yesterday in the viewing zone', () => {
		const now = new Date('2026-11-18T12:00:00Z');
		const blocks = dayBlocks(
			[
				row('a', 'task', '2026-11-18T09:00:00Z'),
				row('b', 'task', '2026-11-17T09:00:00Z'),
				row('c', 'task', '2026-11-17T08:00:00Z')
			],
			now,
			'UTC'
		);
		expect(blocks.map((b) => [b.label, b.entries.length])).toEqual([
			['Today', 1],
			['Yesterday', 2]
		]);
	});
});
