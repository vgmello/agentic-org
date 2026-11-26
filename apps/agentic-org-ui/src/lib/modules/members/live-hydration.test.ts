import { describe, expect, it } from 'vitest';
import type { TaskRow } from '$lib/modules/workitems/types';
import {
	buildAssignableTasks,
	buildMemberActivity,
	buildMemberWorkRows,
	pickCurrentTask,
	readMemberConversations
} from './live-hydration';

function task(overrides: Partial<TaskRow>): TaskRow {
	return {
		id: 't-1',
		ref: 'T-A1',
		revision: 1,
		type: 'task',
		title: 'Audit worker host',
		status: 'todo',
		priority: 'normal',
		severity: null,
		effort: null,
		project: { id: 'proj-1', name: 'Agentic Org v1', number: 1 },
		team: { id: 'team-1', name: 'Agentic Org' },
		assignedTeam: null,
		assignee: { id: 'm-1', name: 'Worker EU', kind: 'agent' },
		reporter: { id: 'm-0', name: 'Owner', kind: 'person' },
		createdAt: '2026-11-25T10:00:00Z',
		updatedAt: '2026-11-25T12:00:00Z',
		statusAt: '2026-11-25T11:00:00Z',
		checklist: { done: 0, total: 0, requiredLeft: 0 },
		blockedReason: null,
		reopenCount: 0,
		needsMe: null,
		budget: null,
		awaitingVerification: false,
		...overrides
	};
}

describe('members live-hydration', () => {
	it('builds member WorkRows, picks top active task, and extracts assignable tasks', () => {
		const rows = [
			task({ id: 't-1', ref: 'T-A1', status: 'todo' }),
			task({ id: 't-2', ref: 'T-A2', status: 'in_progress', title: 'Ship profile rail' }),
			task({ id: 't-3', ref: 'T-A3', status: 'done' }),
			task({ id: 't-4', ref: 'T-A4', status: 'cancelled' }),
			task({ id: 't-5', ref: 'T-A5', assignee: { id: 'm-2', name: 'Other', kind: 'agent' } })
		];

		expect(buildMemberWorkRows(rows, 'm-1').map((r) => [r.ref, r.state])).toEqual([
			['T-A1', 'todo'],
			['T-A2', 'in_progress'],
			['T-A3', 'done']
		]);
		expect(pickCurrentTask(rows, 'm-1')).toEqual({
			ref: 'T-A2',
			title: 'Ship profile rail',
			project: 'Agentic Org v1'
		});
		expect(buildAssignableTasks(rows).map((t) => t.ref)).toEqual(['T-A1', 'T-A2', 'T-A5']);
	});

	it('derives newest-first member activity and reads member conversations', async () => {
		const activity = buildMemberActivity(
			[
				task({ id: 't-1', ref: 'T-A1', updatedAt: '2026-11-25T09:00:00Z' }),
				task({ id: 't-2', ref: 'T-A2', status: 'done', updatedAt: '2026-11-25T13:00:00Z' })
			],
			'm-1'
		);
		expect(activity.map((a) => a.text)).toEqual([
			'T-A2 · Audit worker host (Done)',
			'T-A1 · Audit worker host (To do)'
		]);

		const convs = await readMemberConversations(
			{
				read: async () => ({
					status: 200,
					body: {
						ok: true,
						result: {
							conversations: [
								{
									id: 'c-dm',
									kind: 'dm',
									name: 'Worker EU',
									participant_ids: ['m-0', 'm-1'],
									created_at: '2026-11-25T08:00:00Z'
								},
								{
									id: 'c-other',
									kind: 'channel',
									name: 'other',
									participant_ids: ['m-0'],
									created_at: '2026-11-25T08:00:00Z'
								}
							]
						}
					}
				}),
				submit: async () => ({
					status: 200,
					body: {
						ok: true,
						result: {
							messages: [
								{ id: 'msg-1', body: 'Ready for review.', created_at: '2026-11-25T16:00:00Z' }
							]
						}
					}
				})
			},
			'org-1',
			'm-1'
		);
		expect(convs).toEqual([
			{
				id: 'c-dm',
				title: 'Worker EU',
				kind: 'direct',
				lastAt: '2026-11-25T16:00:00Z',
				preview: 'Ready for review.'
			}
		]);
	});
});
