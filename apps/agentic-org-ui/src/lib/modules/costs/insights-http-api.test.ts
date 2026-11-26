import { describe, expect, it, vi } from 'vitest';
import type { TaskRow } from '$lib/modules/workitems/types';
import { createHttpInsightsReadApi } from './insights-http-api';

function task(overrides: Partial<TaskRow>): TaskRow {
	return {
		id: 't-1',
		ref: 'T-A1',
		revision: 1,
		type: 'task',
		title: 'Ship insights',
		status: 'todo',
		priority: 'normal',
		severity: null,
		effort: null,
		project: { id: 'proj-1', name: 'Agentic Org v1', number: 1 },
		team: { id: 'team-1', name: 'Agentic Org' },
		assignedTeam: null,
		assignee: { id: 'm-1', name: 'Worker EU', kind: 'agent' },
		reporter: { id: 'm-0', name: 'Owner', kind: 'person' },
		createdAt: '2026-11-10T10:00:00Z',
		updatedAt: '2026-11-20T12:00:00Z',
		statusAt: '2026-11-20T12:00:00Z',
		checklist: { done: 0, total: 0, requiredLeft: 0 },
		blockedReason: null,
		reopenCount: 0,
		needsMe: null,
		budget: null,
		awaitingVerification: false,
		...overrides
	};
}

describe('createHttpInsightsReadApi', () => {
	it('combines live organizationCost totals with live projects and work completion counts', async () => {
		const api = createHttpInsightsReadApi({
			http: {
				read: vi.fn().mockResolvedValue({
					status: 200,
					body: {
						result: {
							projects: [
								{
									id: 'proj-1',
									number: 1,
									slug: 'agentic-org-v1',
									name: 'Agentic Org v1',
									owner_team_id: 'team-1',
									archived: false
								}
							]
						}
					}
				}),
				submit: vi.fn()
			},
			orgApi: {
				view: vi.fn().mockResolvedValue({
					snapshot: {
						position: { epoch: 1, seq: 1 },
						teams: [{ id: 'team-1', name: 'Agentic Org', version: 1 }],
						members: []
					},
					sample: false
				})
			},
			costsReadApi: {
				organizationCost: vi.fn().mockResolvedValue({
					period: {
						kind: 'this_month',
						startsAt: '2026-11-01T00:00:00Z',
						endsAt: '2026-11-25T23:59:59Z',
						timeZone: 'UTC',
						toNow: true
					},
					totals: {
						records: 4,
						knownCostMicroUsd: 250_000,
						knownTokens: 120_000,
						cacheReadTokens: 80_000,
						tokenIncompleteRecords: 0,
						costIncompleteRecords: 0,
						unpricedRecords: 0,
						unknownCostRecords: 0,
						includesApiEquivalent: false
					},
					byProject: [],
					byConsumer: [],
					byWorkType: [],
					coverage: { checkpointAt: '2026-11-25T20:00:00Z', gaps: [] }
				})
			},
			workItemsApi: {
				queue: vi.fn().mockResolvedValue({
					rows: [
						task({ id: 't-1', ref: 'T-A1', status: 'done' }),
						task({ id: 't-2', ref: 'T-A2', status: 'blocked' }),
						task({ id: 'b-1', ref: 'T-A3', type: 'bug', status: 'done' })
					]
				})
			},
			now: () => '2026-11-25T23:00:00Z'
		});

		const orgInsights = await api.organizationInsights('org-1', {
			kind: 'this_month',
			timeZone: 'UTC'
		});
		expect(orgInsights.totals.knownCostMicroUsd).toBe(250_000);
		expect(orgInsights.work.tasksDone).toBe(1);
		expect(orgInsights.work.bugsDone).toBe(1);
		expect(orgInsights.projects).toHaveLength(1);
		expect(orgInsights.projects[0].itemsDone).toBe(2);
		expect(orgInsights.insights[0].id).toBe('blocked-work');

		const projInsights = await api.projectInsights('org-1', 'proj-1', {
			kind: 'this_month',
			timeZone: 'UTC'
		});
		expect(projInsights.project.slug).toBe('agentic-org-v1');
		expect(projInsights.work.tasksDone).toBe(1);
		expect(projInsights.work.bugsDone).toBe(1);
		expect(projInsights.breakdowns.byTeam[0].label).toBe('Agentic Org');
		expect(projInsights.breakdowns.byConsumer[0].label).toBe('Worker EU');
	});
});
