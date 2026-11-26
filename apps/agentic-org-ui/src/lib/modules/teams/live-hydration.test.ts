import { describe, expect, it } from 'vitest';
import type { TaskRow } from '$lib/modules/workitems/types';
import { buildTeamActivity, buildTeamWorkRows, readTeamConversations } from './live-hydration';

function task(overrides: Partial<TaskRow>): TaskRow {
	return {
		id: 't-1',
		ref: 'T-A1',
		revision: 1,
		type: 'task',
		title: 'Ship team live wiring',
		status: 'todo',
		priority: 'normal',
		severity: null,
		effort: null,
		project: { id: 'proj-1', name: 'Agentic Org v1', number: 1 },
		team: { id: 'team-1', name: 'Core' },
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

describe('teams live-hydration', () => {
	it('builds team-scoped WorkRows excluding cancelled tasks and mapping awaitingVerification to in_review', () => {
		const memberIds = new Set(['m-1', 'm-2']);
		const rows = buildTeamWorkRows(
			[
				task({ id: 't-1', ref: 'T-A1', status: 'todo', priority: 'urgent' }),
				task({
					id: 't-2',
					ref: 'T-A2',
					status: 'in_progress',
					awaitingVerification: true,
					team: { id: 'other', name: 'Other' },
					assignee: { id: 'm-2', name: 'Main EU', kind: 'agent' }
				}),
				task({ id: 't-3', ref: 'T-A3', status: 'cancelled' }),
				task({
					id: 't-4',
					ref: 'T-A4',
					team: { id: 'other', name: 'Other' },
					assignee: { id: 'm-9', name: 'Outside', kind: 'agent' }
				})
			],
			'team-1',
			memberIds
		);
		expect(rows).toEqual([
			{
				ref: 'T-A1',
				kind: 'task',
				title: 'Ship team live wiring',
				project: 'Agentic Org v1',
				assigneeId: 'm-1',
				assignee: 'Worker EU',
				priority: 'urgent',
				state: 'todo'
			},
			{
				ref: 'T-A2',
				kind: 'task',
				title: 'Ship team live wiring',
				project: 'Agentic Org v1',
				assigneeId: 'm-2',
				assignee: 'Main EU',
				priority: 'medium',
				state: 'in_review'
			}
		]);
	});

	it('derives newest-first team activity entries', () => {
		const entries = buildTeamActivity(
			[
				task({ id: 't-1', ref: 'T-A1', updatedAt: '2026-11-25T10:00:00Z' }),
				task({ id: 't-2', ref: 'T-A2', status: 'done', updatedAt: '2026-11-25T15:00:00Z' })
			],
			'team-1',
			new Set(['m-1'])
		);
		expect(entries.map((e) => e.text)).toEqual([
			'T-A2 · Ship team live wiring (Done)',
			'T-A1 · Ship team live wiring (To do)'
		]);
	});

	it('reads team-linked and member-participating conversations via gateway http', async () => {
		const convs = await readTeamConversations(
			{
				read: async () => ({
					status: 200,
					body: {
						ok: true,
						result: {
							conversations: [
								{
									id: 'c-team',
									kind: 'channel',
									name: 'core',
									auto: { type: 'team', team_id: 'team-1' },
									created_at: '2026-11-25T08:00:00Z'
								},
								{
									id: 'c-other',
									kind: 'channel',
									name: 'random',
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
								{
									id: 'm-1',
									body: 'Team standup complete.',
									created_at: '2026-11-25T16:00:00Z'
								}
							]
						}
					}
				})
			},
			'org-1',
			'team-1',
			'Core',
			new Set(['m-1'])
		);
		expect(convs).toEqual([
			{
				id: 'c-team',
				title: '#core',
				kind: 'channel',
				lastAt: '2026-11-25T16:00:00Z',
				preview: 'Team standup complete.'
			}
		]);
	});
});
