import { describe, expect, it, vi } from 'vitest';
import type { OrgSnapshot } from '$lib/proto';
import type { InboxItem } from '$lib/modules/inbox/types';
import type { TaskRow } from '$lib/modules/workitems/types';
import { createHttpActivityApi } from './http-api';

const SNAPSHOT: OrgSnapshot = {
	position: { epoch: 1, seq: 5 },
	teams: [{ id: 'team-1', name: 'Core', version: 1 }],
	members: [
		{ id: 'm-1', name: 'Alex Kim', kind: 'person', owner: true, version: 1 },
		{ id: 'm-2', name: 'Worker EU', kind: 'agent', version: 1 }
	],
	projects: [{ id: 'proj-1', name: 'Agentic Org v1', key: 'P-1', archived: false, version: 1 }],
	areas: []
};

const TASK_ROW: TaskRow = {
	id: 'task-1',
	ref: 'T-A1',
	revision: 2,
	type: 'task',
	title: 'Verify EU worker',
	status: 'in_progress',
	priority: 'high',
	severity: null,
	effort: null,
	project: { id: 'proj-1', name: 'Agentic Org v1', number: 1 },
	team: { id: 'team-1', name: 'Core' },
	assignedTeam: { id: 'team-1', name: 'Core' },
	assignee: { id: 'm-2', name: 'Worker EU', kind: 'agent' },
	reporter: { id: 'm-1', name: 'Alex Kim', kind: 'person' },
	createdAt: '2026-11-25T10:00:00Z',
	updatedAt: '2026-11-25T12:00:00Z',
	statusAt: '2026-11-25T11:00:00Z',
	checklist: { done: 1, total: 2, requiredLeft: 0 },
	blockedReason: null,
	reopenCount: 0,
	needsMe: null,
	budget: null,
	awaitingVerification: false
};

const INBOX_ITEM: InboxItem = {
	id: 'in-1',
	kind: 'decision',
	source: 'Release',
	title: 'Approve v1 release cut',
	who: { id: 'm-2', name: 'm-2', kind: 'agent' },
	why: 'All checks passed',
	project: { id: 'proj-1', name: 'Agentic Org v1' },
	created_at: '2026-11-25T13:00:00Z',
	actions: ['approve', 'deny'],
	state: 'open'
};

describe('createHttpActivityApi', () => {
	it('synthesizes sorted work, chat, and decision rows and hydrates actor names', async () => {
		const api = createHttpActivityApi({
			http: {
				read: async (path) => {
					if (path.endsWith('/chat/conversations')) {
						return {
							status: 200,
							body: {
								ok: true,
								result: {
									conversations: [
										{
											id: 'c-1',
											kind: 'channel',
											name: 'general',
											members: ['m-1', 'm-2'],
											auto: { type: 'project', project_id: 'proj-1' }
										}
									]
								}
							}
						};
					}
					return { status: 404, body: { ok: false } };
				},
				submit: async (path) => {
					if (path.endsWith('/chat/messages/list')) {
						return {
							status: 200,
							body: {
								ok: true,
								result: {
									messages: [
										{
											id: 'msg-1',
											conversation_id: 'c-1',
											from: 'm-2',
											kind: 'announcement',
											body: 'Build passed on Linux x86_64.',
											created_at: '2026-11-25T14:00:00Z'
										}
									]
								}
							}
						};
					}
					return { status: 404, body: { ok: false } };
				}
			},
			orgApi: { view: async () => ({ snapshot: SNAPSHOT }) as never },
			workItemsApi: {
				queue: async () =>
					({
						rows: [TASK_ROW],
						viewer: { employee: null, teamIds: [], owner: true, admin: true },
						needsMeSources: []
					}) as never
			},
			inboxApi: { list: async () => [INBOX_ITEM] },
			viewerId: () => 'm-1'
		});

		const allPage = await api.feed('org-1', { kind: 'all' }, 50);
		expect(allPage.sources.messaging).toBe('connected');
		expect(allPage.rows.map((r) => r.id)).toEqual(['chat:msg-1', 'work:task-1:2']);
		expect(allPage.rows[1].href).toEqual({ path: '/work/T-A1', label: 'T-A1' });

		const decPage = await api.feed('org-1', { kind: 'decision' }, 50);
		expect(decPage.rows.map((r) => r.id)).toEqual(['inbox:in-1']);
		expect(decPage.rows[0].actor.name).toBe('Worker EU');

		const opts = await api.options('org-1');
		expect(opts.projects).toEqual([{ id: 'proj-1', name: 'Agentic Org v1' }]);
		expect(opts.teams).toEqual([{ id: 'team-1', name: 'Core' }]);

		const thread = await api.thread('org-1', 'c-1');
		expect(thread.counterpart.name).toBe('Worker EU');
		expect(thread.ownerMayReply).toBe(true);
		expect(thread.messages).toHaveLength(1);
	});

	it('degrades gracefully when messaging fails and emits new rows on SSE subscribe', async () => {
		let rows: TaskRow[] = [TASK_ROW];
		let triggerSse: (() => void) | undefined;
		const api = createHttpActivityApi({
			http: {
				read: async () => Promise.reject(new Error('offline')),
				submit: async () => Promise.reject(new Error('offline'))
			},
			orgApi: { view: async () => ({ snapshot: SNAPSHOT }) as never },
			workItemsApi: {
				queue: async () =>
					({
						rows,
						viewer: { employee: null, teamIds: [], owner: true, admin: true },
						needsMeSources: []
					}) as never
			},
			inboxApi: { list: async () => [] },
			subscribe: (_orgId, cb) => {
				triggerSse = cb;
				return () => {};
			}
		});

		const page = await api.feed('org-1', { kind: 'all' }, 50);
		expect(page.sources.messaging).toBe('not-connected');
		expect(page.rows).toHaveLength(1);

		const onRow = vi.fn();
		const stop = await api.subscribe('org-1', onRow);
		rows = [TASK_ROW, { ...TASK_ROW, id: 'task-2', ref: 'T-A2', revision: 1 }];
		triggerSse?.();
		await vi.waitFor(() => expect(onRow).toHaveBeenCalledTimes(1));
		expect(onRow.mock.calls[0][0].id).toBe('work:task-2:1');
		stop();
	});
});
