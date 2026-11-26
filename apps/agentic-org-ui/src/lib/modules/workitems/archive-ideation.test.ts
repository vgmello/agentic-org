import { describe, expect, it } from 'vitest';
import { isArchivedTaskRow, isArchivedTimestamp } from './archive';
import {
	cleanCommentDisplayBody,
	formatIdeationCommentBody,
	isDraftIdeationTask,
	splitIdeationComments,
	stripDraftIdeationMarker,
	withDraftIdeationMarker
} from './ideation';
import { assessTessIntakeRouting } from './features/capture/composer-helpers';
import type { Comment } from './types';

const NOW = Date.parse('2026-11-26T12:00:00Z');
const DAY_MS = 86_400_000;

describe('auto-archive helpers (TASK-131)', () => {
	it('archives closed items older than threshold and keeps open or recent items visible', () => {
		const fifteenDaysAgo = new Date(NOW - 15 * DAY_MS).toISOString();
		const twoDaysAgo = new Date(NOW - 2 * DAY_MS).toISOString();

		expect(
			isArchivedTaskRow(
				{ status: 'done', statusAt: fifteenDaysAgo, updatedAt: fifteenDaysAgo },
				10,
				NOW
			)
		).toBe(true);
		expect(
			isArchivedTaskRow({ status: 'done', statusAt: twoDaysAgo, updatedAt: twoDaysAgo }, 10, NOW)
		).toBe(false);
		expect(
			isArchivedTaskRow(
				{ status: 'in_progress', statusAt: fifteenDaysAgo, updatedAt: fifteenDaysAgo },
				10,
				NOW
			)
		).toBe(false);
		expect(isArchivedTimestamp(fifteenDaysAgo, 0, NOW)).toBe(false);
	});
});

describe('draft ideation & clean execution cutoff (TASK-142)', () => {
	it('detects, wraps, and deterministically strips Draft ideation marker', () => {
		const wrapped = withDraftIdeationMarker('Explore two architecture options');
		expect(isDraftIdeationTask({ status: 'todo', description: wrapped })).toBe(true);
		expect(isDraftIdeationTask({ status: 'in_progress', description: wrapped })).toBe(false);
		expect(stripDraftIdeationMarker(wrapped)).toBe('Explore two architecture options');
		expect(
			isDraftIdeationTask({ status: 'todo', description: stripDraftIdeationMarker(wrapped) })
		).toBe(false);
	});

	it('separates ideation comments from execution comments and cleans prefixes', () => {
		const author = { id: 'm-1', name: 'Victor', kind: 'person' as const };
		const comments: Comment[] = [
			{
				id: 'c-1',
				kind: 'comment',
				author,
				at: '2026-11-25T10:00:00Z',
				body: formatIdeationCommentBody('Should we split this into two PRs?')
			},
			{
				id: 'c-2',
				kind: 'comment',
				author,
				at: '2026-11-26T10:00:00Z',
				body: 'Started implementation on branch v1.'
			}
		];
		const split = splitIdeationComments(comments);
		expect(split.ideation.map((c) => c.id)).toEqual(['c-1']);
		expect(split.execution.map((c) => c.id)).toEqual(['c-2']);
		expect(cleanCommentDisplayBody(split.ideation[0].body)).toBe(
			'Should we split this into two PRs?'
		);
	});
});

describe('async Tess intake routing preview (TASK-139)', () => {
	it('routes straightforward tasks directly and exploratory tasks to Draft ideation', () => {
		expect(
			assessTessIntakeRouting('Fix broken footer link', 'Update URL in layout.', [], 'Ava')
		).toEqual({
			mode: 'direct',
			label: 'Tess will refine & assign to Ava'
		});
		expect(
			assessTessIntakeRouting(
				'Explore new notification drawer?',
				'Compare options for unread badges.',
				[]
			)
		).toEqual({
			mode: 'draft',
			label: 'Tess will file as Draft for ideation & scoping'
		});
	});
});
