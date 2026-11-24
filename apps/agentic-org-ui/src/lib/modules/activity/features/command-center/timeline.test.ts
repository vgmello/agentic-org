import { describe, expect, it } from 'vitest';
import { groupStreamEntriesByDay, type StreamEntry } from './assembly-model';

const NOW = Date.parse('2026-11-24T15:30:00Z');

function makeEntry(id: string, at: string): StreamEntry {
	return {
		id,
		kind: 'work',
		kindLabel: 'Work',
		actorName: 'Ada',
		actorInitials: 'AD',
		actorKind: 'agent',
		text: `Event ${id}`,
		projects: [],
		at,
		tone: 'accent',
		href: '/work',
		needsYou: false
	};
}

describe('groupStreamEntriesByDay', () => {
	it('returns empty list for no entries', () => {
		expect(groupStreamEntriesByDay([], NOW, 'UTC')).toEqual([]);
	});

	it('groups entries into Today, Yesterday, and older formatted date headings in newest-first order', () => {
		const groups = groupStreamEntriesByDay(
			[
				makeEntry('old-1', '2026-11-20T09:00:00Z'),
				makeEntry('today-older', '2026-11-24T08:15:00Z'),
				makeEntry('yesterday-2', '2026-11-23T19:45:00Z'),
				makeEntry('today-newest', '2026-11-24T15:00:00Z'),
				makeEntry('yesterday-1', '2026-11-23T11:00:00Z'),
				makeEntry('old-2', '2026-11-20T18:30:00Z')
			],
			NOW,
			'UTC'
		);

		expect(
			groups.map((g) => ({ key: g.key, label: g.label, ids: g.entries.map((e) => e.id) }))
		).toEqual([
			{ key: '2026-11-24', label: 'Today', ids: ['today-newest', 'today-older'] },
			{ key: '2026-11-23', label: 'Yesterday', ids: ['yesterday-2', 'yesterday-1'] },
			{ key: '2026-11-20', label: 'Fri 20 Nov', ids: ['old-2', 'old-1'] }
		]);
	});

	it('respects the viewing time zone when splitting Today and Yesterday', () => {
		// 2026-11-24T01:30:00Z is still 2026-11-23 20:30 in America/New_York (UTC-5).
		const groups = groupStreamEntriesByDay(
			[
				makeEntry('morning-ny', '2026-11-24T14:00:00Z'),
				makeEntry('late-evening-ny', '2026-11-24T01:30:00Z')
			],
			NOW,
			'America/New_York'
		);

		expect(groups.map((g) => [g.label, g.entries.map((e) => e.id)])).toEqual([
			['Today', ['morning-ny']],
			['Yesterday', ['late-evening-ny']]
		]);
	});

	it('preserves input order on identical timestamps and places unparseable timestamps last', () => {
		const groups = groupStreamEntriesByDay(
			[
				makeEntry('bad-date', 'not-a-timestamp'),
				makeEntry('tie-a', '2026-11-24T12:00:00Z'),
				makeEntry('tie-b', '2026-11-24T12:00:00Z')
			],
			NOW,
			'UTC'
		);

		expect(groups.map((g) => [g.label, g.entries.map((e) => e.id)])).toEqual([
			['Today', ['tie-a', 'tie-b']],
			['Earlier', ['bad-date']]
		]);
	});

	it('labels Yesterday accurately across spring-forward DST boundaries', () => {
		// 2026-03-08 is a 23-hour spring-forward day in America/New_York; 24h before 2026-03-09T00:30-04:00 lands on Mar 7.
		const dstNow = Date.parse('2026-03-09T00:30:00-04:00');
		const groups = groupStreamEntriesByDay(
			[
				makeEntry('mar-9', '2026-03-09T00:10:00-04:00'),
				makeEntry('mar-8', '2026-03-08T14:00:00-04:00'),
				makeEntry('mar-7', '2026-03-07T23:45:00-05:00')
			],
			dstNow,
			'America/New_York'
		);

		expect(groups.map((g) => [g.key, g.label, g.entries.map((e) => e.id)])).toEqual([
			['2026-03-09', 'Today', ['mar-9']],
			['2026-03-08', 'Yesterday', ['mar-8']],
			['2026-03-07', 'Sat 7 Mar', ['mar-7']]
		]);
	});
});
