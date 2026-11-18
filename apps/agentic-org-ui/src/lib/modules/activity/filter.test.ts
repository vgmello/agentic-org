import { describe, expect, it } from 'vitest';
import { matchesQuery, queryFromParams, queryToParams } from './filter';
import type { ActivityRow } from './types';

const row = (extra: Partial<ActivityRow> = {}): ActivityRow => ({
	id: 'r1',
	at: '2026-11-18T10:00:00Z',
	actor: { id: 'noor', name: 'Noor Haddad', kind: 'agent' },
	kind: 'work',
	action: 'task',
	text: 'opened T-14 in Beacon',
	teamIds: ['web'],
	projectIds: ['beacon'],
	memberIds: ['mira'],
	...extra
});

describe('matchesQuery', () => {
	it('hides decisions and status rows by default', () => {
		expect(matchesQuery(row({ kind: 'decision' }), { kind: 'all' })).toBe(false);
		expect(matchesQuery(row({ kind: 'decision' }), { kind: 'decision' })).toBe(true);
		expect(matchesQuery(row({ status: true }), { kind: 'all' })).toBe(false);
		expect(matchesQuery(row({ status: true }), { kind: 'all', includeStatus: true })).toBe(true);
	});
	it('filters by kind, team, project and member (actor or concerned)', () => {
		expect(matchesQuery(row(), { kind: 'run' })).toBe(false);
		expect(matchesQuery(row(), { kind: 'work', team: 'web' })).toBe(true);
		expect(matchesQuery(row(), { kind: 'all', team: 'design' })).toBe(false);
		expect(matchesQuery(row(), { kind: 'all', project: 'atlas' })).toBe(false);
		expect(matchesQuery(row(), { kind: 'all', member: 'noor' })).toBe(true);
		expect(matchesQuery(row(), { kind: 'all', member: 'mira' })).toBe(true);
		expect(matchesQuery(row(), { kind: 'all', member: 'kai' })).toBe(false);
	});
	it('searches actor, text and preview, every word', () => {
		expect(matchesQuery(row(), { kind: 'all', q: 'noor t-14' })).toBe(true);
		expect(matchesQuery(row({ preview: 'the basket' }), { kind: 'all', q: 'basket' })).toBe(true);
		expect(matchesQuery(row(), { kind: 'all', q: 'basket' })).toBe(false);
	});
});

describe('query in the URL', () => {
	it('round-trips and ignores unknown kinds', () => {
		const query = { kind: 'run' as const, team: 'web', q: 'noor', includeStatus: true };
		const params = queryToParams(query);
		expect(params.toString()).toBe('team=web&kind=run&q=noor&status=1');
		expect(queryFromParams(params)).toMatchObject(query);
		expect(queryFromParams(new URLSearchParams('kind=bogus')).kind).toBe('all');
		expect(queryToParams({ kind: 'all' }).toString()).toBe('');
	});
});
