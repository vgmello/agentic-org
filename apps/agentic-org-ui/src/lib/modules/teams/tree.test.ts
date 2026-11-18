import { describe, expect, it } from 'vitest';
import { focusAfterRemoval, indentTeams } from './tree';
import type { TeamRow } from './types';

const t = (id: string, name: string, parentId?: string): TeamRow => ({
	id,
	name,
	ref: '',
	parentId,
	people: 0,
	agents: 0,
	projects: [],
	maxMembers: 6,
	status: 'active'
});

describe('indentTeams', () => {
	it('flattens the tree in reading order with depths', () => {
		const rows = indentTeams([
			t('w', 'Web', 'e'),
			t('e', 'Engineering'),
			t('p', 'Platform', 'e'),
			t('d', 'Design')
		]);
		expect(rows.map((r) => `${r.depth}:${r.team.name}`)).toEqual([
			'0:Design',
			'0:Engineering',
			'1:Platform',
			'1:Web'
		]);
		expect(rows.find((r) => r.team.id === 'e')?.children).toBe(2);
	});
	it('never hides a team in a loop', () => {
		const rows = indentTeams([t('a', 'A', 'b'), t('b', 'B', 'a')]);
		expect(rows.map((r) => r.team.id).sort()).toEqual(['a', 'b']);
	});
});

describe('focusAfterRemoval', () => {
	it('prefers the next row, then the previous', () => {
		expect(focusAfterRemoval(['a', 'b', 'c'], 'b', new Set(['a', 'c']))).toBe('c');
		expect(focusAfterRemoval(['a', 'b', 'c'], 'c', new Set(['a', 'b']))).toBe('b');
		expect(focusAfterRemoval(['a'], 'a', new Set())).toBeUndefined();
	});
});
