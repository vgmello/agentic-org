import { describe, expect, it } from 'vitest';
import {
	neighbour,
	openCount,
	openItems,
	scopeOptions,
	tabCounts,
	tabItems,
	whoLine
} from './items';
import type { InboxItem } from './types';

const item = (id: string, extra: Partial<InboxItem> = {}): InboxItem => ({
	id,
	kind: 'approval',
	source: 'Run approval',
	title: id,
	who: { id: 'kai', name: 'Kai Lindqvist', kind: 'agent' },
	why: 'because',
	created_at: '2026-11-18T10:00:00Z',
	actions: [],
	state: 'open',
	...extra
});

const all = [
	item('a1', {
		expires_at: '2026-11-18T14:00:00Z',
		team: { id: 'platform', name: 'Platform' },
		project: { id: 'atlas', name: 'Atlas' }
	}),
	item('a2', { created_at: '2026-11-18T12:00:00Z' }),
	item('q1', { kind: 'question', team: { id: 'web', name: 'Web' } }),
	item('d1', { kind: 'decision', expires_at: '2026-11-18T13:00:00Z' }),
	item('p1', {
		kind: 'problem',
		state: 'handled',
		handled: { outcome: 'Dismissed', at: '2026-11-18T11:00:00Z' }
	}),
	item('p2', {
		kind: 'problem',
		state: 'handled',
		handled: { outcome: 'Resolved', at: '2026-11-18T11:30:00Z' }
	})
];

describe('inbox lists', () => {
	it('lists open items soonest-expiry first, then newest', () => {
		expect(openItems(all).map((i) => i.id)).toEqual(['d1', 'a1', 'a2', 'q1']);
	});
	it('counts open items for the badge and Home, handled apart', () => {
		expect(openCount(all)).toBe(4);
		expect(tabCounts(all)).toEqual({
			all: 4,
			approvals: 2,
			questions: 1,
			decisions: 1,
			problems: 0,
			snoozed: 0,
			handled: 2
		});
	});
	it('puts resolved items in Handled, newest first', () => {
		expect(tabItems(all, 'handled').map((i) => i.id)).toEqual(['p2', 'p1']);
	});
	it('applies team and project scopes to every tab', () => {
		expect(tabItems(all, 'all', { team: 'platform' }).map((i) => i.id)).toEqual(['a1']);
		expect(tabItems(all, 'all', { project: 'atlas' }).map((i) => i.id)).toEqual(['a1']);
		expect(tabCounts(all, { team: 'web' }).all).toBe(1);
		expect(scopeOptions(all)).toEqual({
			teams: [
				{ id: 'platform', name: 'Platform' },
				{ id: 'web', name: 'Web' }
			],
			projects: [{ id: 'atlas', name: 'Atlas' }]
		});
	});
	it('selects the next item after one leaves, else the previous', () => {
		const list = openItems(all);
		expect(neighbour(list, 'd1')?.id).toBe('a1');
		expect(neighbour(list, 'q1')?.id).toBe('a2');
		expect(neighbour([list[0]], 'd1')).toBeUndefined();
	});
	it('names who, team and project on one line', () => {
		expect(whoLine(all[0])).toBe('Kai Lindqvist · Platform · Atlas');
		expect(whoLine(all[1])).toBe('Kai Lindqvist');
	});
});
