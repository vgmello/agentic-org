import { describe, expect, it, vi } from 'vitest';
import type { OrgSnapshot } from '$lib/proto';
import type { TaskRow } from '$lib/modules/workitems/types';
import { createHttpSearchApi } from './http-api';

const snapshot: OrgSnapshot = {
	position: { epoch: 1, seq: 1 },
	teams: [{ id: 't1', name: 'Core', version: 1 }],
	members: [
		{
			id: 'm1',
			kind: 'agent',
			name: 'Ada Lovelace',
			handle: 'ada',
			version: 1,
			agent: {
				role: 'Staff Engineer',
				title: 'Staff Engineer',
				runtime: 'claude_code',
				messaging_name: 'ada',
				status: 'active'
			}
		}
	],
	projects: [{ id: 'p1', key: 'CORE', name: 'Core Engine', version: 1 }],
	project_teams: []
};

const sampleTask: TaskRow = {
	id: 'task-1',
	ref: 'T-A1',
	type: 'task',
	title: 'Core storage benchmark',
	status: 'in_progress',
	priority: 'high',
	severity: null,
	effort: null,
	team: { id: 't1', name: 'Core' },
	assignedTeam: { id: 't1', name: 'Core' },
	project: { id: 'p1', name: 'Core Engine', number: 1 },
	reporter: { id: 'm1', name: 'Ada Lovelace', kind: 'agent' },
	assignee: { id: 'm1', name: 'Ada Lovelace', kind: 'agent' },
	revision: 1,
	createdAt: '2026-11-25T10:00:00Z',
	updatedAt: '2026-11-26T01:00:00Z',
	statusAt: '2026-11-26T01:00:00Z',
	checklist: { done: 0, total: 0, requiredLeft: 0 },
	blockedReason: null,
	reopenCount: 0,
	needsMe: null,
	budget: null,
	awaitingVerification: false
};

describe('createHttpSearchApi', () => {
	it('returns matching projects, work items, members, and process docs', async () => {
		const read = vi.fn(async () => ({
			status: 200,
			body: {
				result: {
					rows: [
						{
							document: {
								id: 'doc-1',
								key: 'core-release',
								kind: 'process',
								title: 'Core Release Checklist',
								status: 'current',
								scope_name: 'Organization',
								body_markdown: 'Run core benchmarks before release.',
								updated_at: '2026-11-25T10:00:00Z'
							}
						}
					]
				}
			}
		}));

		const api = createHttpSearchApi({
			http: { read, submit: vi.fn() },
			orgApi: { view: async () => ({ snapshot, position: snapshot.position, sample: false }) },
			workItemsApi: {
				queue: async () => ({
					rows: [sampleTask],
					viewer: {
						employee: { id: 'm1', name: 'Ada Lovelace', kind: 'agent' },
						owner: true,
						teamIds: ['t1']
					},
					needsMeSources: []
				})
			},
			now: () => '2026-11-26T04:00:00Z'
		});

		const res = await api.results('org-1', 'core');
		expect(res.projects).toHaveLength(1);
		expect(res.work).toHaveLength(1);
		expect(res.knowledge).toHaveLength(1);
		expect(res.knowledge[0]?.title).toBe('Core Release Checklist');
	});
});
