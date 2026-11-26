import { describe, expect, it } from 'vitest';
import type { TaskRow } from '$lib/modules/workitems/types';
import {
	aggregateProjectWork,
	buildProjectActivity,
	readProjectConversations
} from './live-hydration';

function task(overrides: Partial<TaskRow>): TaskRow {
	return {
		id: 't-1',
		ref: 'T-A1',
		revision: 1,
		type: 'task',
		title: 'Ship live wiring',
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

describe('projects live-hydration', () => {
	it('aggregates task status counts and open bug severities per project', () => {
		const rows = [
			task({ id: 't-1', status: 'todo' }),
			task({ id: 't-2', status: 'in_progress' }),
			task({ id: 't-3', status: 'done' }),
			task({ id: 'b-1', type: 'bug', status: 'blocked', severity: 'critical' }),
			task({ id: 'b-2', type: 'bug', status: 'done', severity: 'major' }),
			task({ id: 't-4', project: null })
		];
		const totals = aggregateProjectWork(rows).get('proj-1');
		expect(totals).toEqual({
			tasks: { todo: 1, inProgress: 1, blocked: 1, done: 2, cancelled: 0 },
			bugs: { critical: 1, major: 0, minor: 0, trivial: 0, unset: 0 }
		});
	});

	it('derives newest-first project activity entries from tasks', () => {
		const entries = buildProjectActivity(
			[
				task({ id: 't-1', ref: 'T-A1', updatedAt: '2026-11-25T10:00:00Z' }),
				task({ id: 't-2', ref: 'T-A2', status: 'done', updatedAt: '2026-11-25T14:00:00Z' }),
				task({ id: 't-3', project: { id: 'other', name: 'Other', number: 2 } })
			],
			'proj-1'
		);
		expect(entries.map((e) => e.text)).toEqual([
			'T-A2 · Ship live wiring (Done)',
			'T-A1 · Ship live wiring (To do)'
		]);
	});

	it('reads project-linked conversations and latest previews via gateway http', async () => {
		const convs = await readProjectConversations(
			{
				read: async () => ({
					status: 200,
					body: {
						ok: true,
						result: {
							conversations: [
								{
									id: 'c-proj',
									kind: 'channel',
									name: 'agentic-org-v1',
									auto: { type: 'project', project_id: 'proj-1' },
									created_at: '2026-11-25T08:00:00Z'
								},
								{
									id: 'c-general',
									kind: 'channel',
									name: 'general',
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
									body: 'EU worker passed all checks.',
									created_at: '2026-11-25T15:00:00Z'
								}
							]
						}
					}
				})
			},
			'org-1',
			'proj-1',
			'agentic-org-v1'
		);

		expect(convs).toEqual([
			{
				id: 'c-proj',
				title: '#agentic-org-v1',
				kind: 'channel',
				lastAt: '2026-11-25T15:00:00Z',
				preview: 'EU worker passed all checks.'
			}
		]);
	});
});
