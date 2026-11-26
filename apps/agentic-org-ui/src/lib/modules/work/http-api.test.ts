import { describe, expect, it, vi } from 'vitest';
import { createHttpWorkApi } from './http-api';

describe('createHttpWorkApi', () => {
	it('maps live gateway tasks into WorkItemSummary items and preserves project filtering', async () => {
		const read = vi.fn().mockResolvedValue({
			status: 200,
			body: {
				result: {
					tasks: [
						{
							id: 'task-1',
							key: 'T-A1',
							number: 1,
							type: 'task',
							title: 'Remote EU Health & Release Audit',
							status: 'todo',
							priority: 'normal',
							project_id: 'proj-1',
							assignee_id: 'agent-1',
							team_id: 'team-1',
							created_by_id: 'owner-1',
							created_at: '2026-11-25T14:23:28.967Z',
							updated_at: '2026-11-25T14:23:28.967Z',
							checklist_done: 1,
							checklist_total: 3,
							revision: 2
						},
						{
							id: 'task-2',
							key: 'T-A2',
							number: 2,
							type: 'task',
							title: 'Cancelled cleanup',
							status: 'cancelled',
							created_by_id: 'owner-1',
							created_at: '2026-11-25T14:23:29.378Z',
							updated_at: '2026-11-25T14:23:29.378Z',
							checklist_done: 0,
							checklist_total: 0,
							revision: 4
						}
					],
					total: 2
				}
			}
		});

		const api = createHttpWorkApi({ http: { read } });
		const snap = await api.snapshot('org-1', 'proj-1');

		expect(read).toHaveBeenCalledWith('/api/v1/organizations/org-1/tasks?project_id=proj-1');
		expect(snap.position).toEqual({ incarnation: 1, seq: 4 });
		expect(snap.items).toHaveLength(2);
		expect(snap.items[0]).toMatchObject({
			id: 'task-1',
			short_id: 'T-A1',
			kind: 'task',
			number: 1,
			status: 'todo',
			priority: 'normal',
			project_id: 'proj-1',
			assignee_id: 'agent-1',
			checklist: { total: 3, done: 1, required_left: 0 },
			version: 2
		});
		expect(snap.items[1]).toMatchObject({
			id: 'task-2',
			status: 'closed',
			resolution: 'cancelled'
		});
	});

	it('fails closed on non-200 or malformed responses', async () => {
		const read = vi.fn().mockResolvedValueOnce({
			status: 503,
			body: { error: { code: 'unavailable', message: 'Office offline.' } }
		});
		const api = createHttpWorkApi({ http: { read } });
		await expect(api.snapshot('org-1')).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable',
			message: 'Office offline.'
		});

		read.mockResolvedValueOnce({ status: 200, body: { result: { tasks: 'bad' } } });
		await expect(api.snapshot('org-1')).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable'
		});
	});
});
