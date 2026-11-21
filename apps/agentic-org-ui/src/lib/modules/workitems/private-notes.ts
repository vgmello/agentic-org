// Pure presentation and per-author grouping helpers for Private task notes (work-items-ux §7.1, ADR-0031).
import type { EmployeeRef, PrivateNote } from './types';

/** Plain-language visibility notice shown in the Private notes pane. */
export const PRIVATE_NOTES_VISIBILITY_NOTICE =
	'Visible to people and the author; hidden from other agents';

export const PRIVATE_NOTES_VISIBILITY_DETAIL =
	'Working plans, reasoning, and test notes stay separate from public comments and task history. Only the author can edit or delete their notes.';

export interface PrivateNoteAuthorGroup {
	author: EmployeeRef;
	/** True when this group belongs to the signed-in viewer. */
	mine: boolean;
	/** Notes by this author, ordered chronologically (oldest first) to read plans and reasoning in order. */
	notes: PrivateNote[];
	/** Latest activity timestamp across this author's notes (RFC 3339). */
	latestAt: string;
}

export function authorKindWord(kind: EmployeeRef['kind']): string {
	return kind === 'agent' ? 'Agent' : 'Person';
}

export function privateNoteCountLabel(count: number): string {
	return `${count} ${count === 1 ? 'note' : 'notes'}`;
}

export function isNoteEdited(
	note: Pick<PrivateNote, 'createdAt' | 'updatedAt' | 'revision'>
): boolean {
	return note.revision > 1 || note.updatedAt !== note.createdAt;
}

/**
 * Groups private notes by author.
 * Order: signed-in viewer's own group first (when present), then agents (most recently active first),
 * then other people (most recently active first). Within each group, notes stay chronological (oldest first).
 */
export function groupPrivateNotes(notes: PrivateNote[]): PrivateNoteAuthorGroup[] {
	const map = new Map<string, PrivateNoteAuthorGroup>();
	for (const note of notes) {
		const activityAt = note.updatedAt || note.createdAt;
		const existing = map.get(note.author.id);
		if (existing) {
			existing.notes.push(note);
			existing.mine = existing.mine || note.mine;
			if (activityAt.localeCompare(existing.latestAt) > 0) {
				existing.latestAt = activityAt;
			}
		} else {
			map.set(note.author.id, {
				author: note.author,
				mine: note.mine,
				notes: [note],
				latestAt: activityAt
			});
		}
	}

	const groups = [...map.values()].map((group) => ({
		...group,
		notes: [...group.notes].sort(
			(a, b) => a.createdAt.localeCompare(b.createdAt) || a.id.localeCompare(b.id)
		)
	}));

	const tier = (g: PrivateNoteAuthorGroup) => (g.mine ? 0 : g.author.kind === 'agent' ? 1 : 2);

	return groups.sort(
		(a, b) =>
			tier(a) - tier(b) ||
			b.latestAt.localeCompare(a.latestAt) ||
			a.author.name.localeCompare(b.author.name) ||
			a.author.id.localeCompare(b.author.id)
	);
}

/** Filters grouped private notes to a single author, falling back to all groups if the author no longer has notes. */
export function filterPrivateNoteGroups(
	groups: PrivateNoteAuthorGroup[],
	selectedAuthorId: string | 'all'
): PrivateNoteAuthorGroup[] {
	if (selectedAuthorId === 'all') return groups;
	const matched = groups.filter((g) => g.author.id === selectedAuthorId);
	return matched.length > 0 ? matched : groups;
}
