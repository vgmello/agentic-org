import { describe, expect, it } from 'vitest';
import {
	PRIVATE_NOTES_VISIBILITY_NOTICE,
	authorKindWord,
	filterPrivateNoteGroups,
	groupPrivateNotes,
	isNoteEdited,
	privateNoteCountLabel
} from './private-notes';
import type { PrivateNote } from './types';

function makeNote(overrides: Partial<PrivateNote> = {}): PrivateNote {
	return {
		id: 'pn-1',
		author: { id: 'sam', name: 'Sam Okafor', kind: 'agent' },
		body: 'Implementation plan',
		createdAt: '2026-11-20T10:00:00Z',
		updatedAt: '2026-11-20T10:00:00Z',
		revision: 1,
		mine: false,
		...overrides
	};
}

describe('private-notes helpers', () => {
	it('groups notes by author with own group first, then agents by latest activity, then other people', () => {
		const notes: PrivateNote[] = [
			makeNote({
				id: 'pn-person-other',
				author: { id: 'avery', name: 'Avery Stone', kind: 'person' },
				createdAt: '2026-11-20T15:00:00Z',
				updatedAt: '2026-11-20T15:00:00Z',
				mine: false
			}),
			makeNote({
				id: 'pn-noor-1',
				author: { id: 'noor', name: 'Noor Haddad', kind: 'agent' },
				createdAt: '2026-11-20T11:00:00Z',
				updatedAt: '2026-11-20T11:00:00Z',
				mine: false
			}),
			makeNote({
				id: 'pn-sam-2',
				author: { id: 'sam', name: 'Sam Okafor', kind: 'agent' },
				createdAt: '2026-11-20T14:00:00Z',
				updatedAt: '2026-11-20T14:30:00Z',
				revision: 2,
				mine: false
			}),
			makeNote({
				id: 'pn-sam-1',
				author: { id: 'sam', name: 'Sam Okafor', kind: 'agent' },
				createdAt: '2026-11-20T09:00:00Z',
				updatedAt: '2026-11-20T09:00:00Z',
				mine: false
			}),
			makeNote({
				id: 'pn-mine',
				author: { id: 'you', name: 'Alex Kim', kind: 'person' },
				createdAt: '2026-11-20T08:00:00Z',
				updatedAt: '2026-11-20T08:00:00Z',
				mine: true
			})
		];

		const groups = groupPrivateNotes(notes);
		expect(groups.map((g) => g.author.id)).toEqual(['you', 'sam', 'noor', 'avery']);
		expect(groups[0].mine).toBe(true);
		expect(groups[1].mine).toBe(false);
		// Chronological (oldest first) within an author's group.
		expect(groups[1].notes.map((n) => n.id)).toEqual(['pn-sam-1', 'pn-sam-2']);
		expect(groups[1].latestAt).toBe('2026-11-20T14:30:00Z');
	});

	it('filters groups by author and falls back cleanly when selected author has no remaining notes', () => {
		const groups = groupPrivateNotes([
			makeNote({ id: 'pn-1', author: { id: 'sam', name: 'Sam Okafor', kind: 'agent' } }),
			makeNote({ id: 'pn-2', author: { id: 'noor', name: 'Noor Haddad', kind: 'agent' } })
		]);

		expect(filterPrivateNoteGroups(groups, 'all')).toHaveLength(2);
		expect(filterPrivateNoteGroups(groups, 'sam').map((g) => g.author.id)).toEqual(['sam']);
		expect(filterPrivateNoteGroups(groups, 'removed-author')).toHaveLength(2);
	});

	it('formats kind words, counts, edited status and plain-language visibility copy', () => {
		expect(authorKindWord('agent')).toBe('Agent');
		expect(authorKindWord('person')).toBe('Person');
		expect(privateNoteCountLabel(1)).toBe('1 note');
		expect(privateNoteCountLabel(3)).toBe('3 notes');
		expect(isNoteEdited(makeNote())).toBe(false);
		expect(isNoteEdited(makeNote({ revision: 2 }))).toBe(true);
		expect(
			isNoteEdited(
				makeNote({ createdAt: '2026-11-20T10:00:00Z', updatedAt: '2026-11-20T11:00:00Z' })
			)
		).toBe(true);
		expect(PRIVATE_NOTES_VISIBILITY_NOTICE).toBe(
			'Visible to people and the author; hidden from other agents'
		);
	});

	it('sorts RFC 3339 timestamps with differing timezone offsets and fractional seconds chronologically', () => {
		const notes: PrivateNote[] = [
			makeNote({
				id: 'pn-sam-late-z',
				author: { id: 'sam', name: 'Sam Okafor', kind: 'agent' },
				createdAt: '2026-11-20T10:00:00.500Z',
				updatedAt: '2026-11-20T10:00:00.500Z'
			}),
			makeNote({
				id: 'pn-sam-early-offset',
				author: { id: 'sam', name: 'Sam Okafor', kind: 'agent' },
				// 09:30:00Z, even though lexicographically "11:30" > "10:00"
				createdAt: '2026-11-20T11:30:00+02:00',
				updatedAt: '2026-11-20T11:30:00+02:00'
			}),
			makeNote({
				id: 'pn-noor-most-recent',
				author: { id: 'noor', name: 'Noor Haddad', kind: 'agent' },
				// 10:30:00Z, even though lexicographically "05:30" < "10:00"
				createdAt: '2026-11-20T05:30:00-05:00',
				updatedAt: '2026-11-20T05:30:00-05:00'
			})
		];

		const groups = groupPrivateNotes(notes);
		// Noor (10:30Z) is more recent than Sam (10:00:00.500Z).
		expect(groups.map((g) => g.author.id)).toEqual(['noor', 'sam']);
		// Within Sam's group: 09:30Z precedes 10:00:00.500Z.
		expect(groups[1].notes.map((n) => n.id)).toEqual(['pn-sam-early-offset', 'pn-sam-late-z']);
	});
});
