import { describe, expect, it } from 'vitest';
import { SAMPLE_ORG, flatten, initialsOf, parentIds, reportCount } from './data';

describe('org chart data', () => {
	it('flattens the whole tree, parents first, with unique ids', () => {
		const nodes = flatten(SAMPLE_ORG);
		expect(nodes[0].id).toBe('owner');
		expect(new Set(nodes.map((n) => n.id)).size).toBe(nodes.length);
		expect(nodes.length).toBe(11);
	});

	it('counts direct and indirect reports', () => {
		expect(reportCount(SAMPLE_ORG)).toBe(10);
		expect(reportCount(SAMPLE_ORG.children![0])).toBe(9);
		expect(reportCount({ id: 'x', name: 'X', role: 'r' })).toBe(0);
	});

	it('lists only the nodes that have reports', () => {
		expect(parentIds(SAMPLE_ORG).sort()).toEqual(['avery', 'hana', 'juno', 'owner', 'sam']);
	});

	it('makes initials from the first and last word', () => {
		expect(initialsOf('Avery Stone')).toBe('AS');
		expect(initialsOf('You')).toBe('Y');
		expect(initialsOf('  Inês  Duarte  ')).toBe('ID');
		expect(initialsOf('')).toBe('?');
	});
});
