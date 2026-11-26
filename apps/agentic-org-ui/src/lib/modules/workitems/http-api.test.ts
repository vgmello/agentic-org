import { describe, expect, it, vi } from 'vitest';
import type { OrgSnapshot } from '$lib/proto';
import type { GatewayHttpTransport, GatewayReply } from '$lib/platform/http/gateway-errors';
import { createHttpWorkItemsApi } from './http-api';

const UUID_V7_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

const SAMPLE_SNAPSHOT: OrgSnapshot = {
	position: { epoch: 1, seq: 1 },
	teams: [
		{ id: 'team-1', name: 'Platform', version: 1 },
		{ id: 'team-2', name: 'Design', owner_team_id: 'team-1', version: 1 }
	],
	members: [
		{
			id: 'emp-1',
			kind: 'person',
			name: 'Alex Kim',
			home_team_id: 'team-1',
			owner: true,
			version: 1
		},
		{
			id: 'emp-2',
			kind: 'agent',
			name: 'Kai',
			home_team_id: 'team-1',
			agent: {
				role: 'Backend Engineer',
				title: 'Staff Engineer',
				runtime: 'codex',
				messaging_name: 'kai',
				status: 'active'
			},
			version: 1
		},
		{
			id: 'emp-3',
			kind: 'agent',
			name: 'Former Bot',
			home_team_id: 'team-1',
			agent: {
				role: 'Archived',
				title: 'Old',
				runtime: 'codex',
				messaging_name: 'old',
				status: 'offboarded'
			},
			version: 1
		}
	],
	project_teams: []
};

const SAMPLE_PROJECTS = [
	{ id: 'proj-1', number: 1, name: 'Core Platform', owner_team_id: 'team-1', archived: false }
];

const SAMPLE_TASK_WIRE = {
	id: '0193a5c0-0000-7000-8000-000000000099',
	key: 'T-A1',
	revision: 2,
	team_id: 'team-1',
	assigned_team_id: 'team-1',
	project_id: 'proj-1',
	type: 'task',
	title: 'Wire live task views',
	status: 'in_progress',
	assignee_id: 'emp-2',
	body_markdown: 'Connect Work UI to live gateway routes.',
	priority: 'high',
	effort: 3,
	severity: null,
	resolution: null,
	blocked_reason: null,
	created_at: '2026-11-25T10:00:00.000Z',
	created_by_kind: 'person',
	created_by_id: 'emp-1',
	updated_at: '2026-11-25T11:00:00.000Z',
	updated_by_kind: 'person',
	updated_by_id: 'emp-1',
	checklist_total: 2,
	checklist_done: 1
};

function ok(result: Record<string, unknown>): GatewayReply {
	return { status: 200, body: { api_version: 1, result } };
}

function makeApi(
	readImpl: (path: string) => Promise<GatewayReply>,
	submitImpl: (path: string, body: unknown) => Promise<GatewayReply>
) {
	const read = vi.fn(readImpl);
	const submit = vi.fn(submitImpl);
	const http: GatewayHttpTransport = { read, submit };
	let seq = 1;
	const newId = () => `0193a5c0-0000-7000-8000-${String(seq++).padStart(12, '0')}`;
	const api = createHttpWorkItemsApi({
		http,
		orgApi: {
			view: vi.fn(async () => ({
				position: { epoch: 1, seq: 1 },
				snapshot: SAMPLE_SNAPSHOT,
				sample: false
			}))
		},
		viewerContext: () => ({ employeeId: 'emp-1', isAdmin: true }),
		newId
	});
	return { api, read, submit };
}

describe('createHttpWorkItemsApi', () => {
	it('loads queue rows, project numbers, teams, and viewer context', async () => {
		const { api, read } = makeApi(
			async (path) => {
				if (path.includes('/projects')) return ok({ projects: SAMPLE_PROJECTS });
				if (path.includes('/tasks')) return ok({ tasks: [SAMPLE_TASK_WIRE], total: 1 });
				return { status: 404, body: {} };
			},
			async () => ok({})
		);

		const queue = await api.queue('org-1', { kind: 'project', projectId: 'proj-1' });
		expect(read).toHaveBeenCalledWith('/api/v1/organizations/org-1/tasks?project_id=proj-1');
		expect(queue.viewer).toEqual({
			employee: { id: 'emp-1', name: 'Alex Kim', kind: 'person' },
			teamIds: ['team-1'],
			owner: true
		});
		expect(queue.needsMeSources).toEqual([]);
		expect(queue.rows).toHaveLength(1);
		expect(queue.rows[0]).toMatchObject({
			id: SAMPLE_TASK_WIRE.id,
			ref: 'T-A1',
			revision: 2,
			type: 'task',
			title: 'Wire live task views',
			status: 'in_progress',
			priority: 'high',
			project: { id: 'proj-1', name: 'Core Platform', number: 1 },
			team: { id: 'team-1', name: 'Platform' },
			assignee: { id: 'emp-2', name: 'Kai', kind: 'agent' },
			reporter: { id: 'emp-1', name: 'Alex Kim', kind: 'person' },
			checklist: { done: 1, total: 2, requiredLeft: 0 }
		});
	});

	it('builds choices excluding offboarded members with hierarchical team paths', async () => {
		const { api } = makeApi(
			async (path) => (path.includes('/projects') ? ok({ projects: SAMPLE_PROJECTS }) : ok({})),
			async () => ok({})
		);

		const choices = await api.choices('org-1');
		expect(choices.organizationName).toBe('Platform');
		expect(choices.teams).toEqual([
			{ id: 'team-1', name: 'Platform', path: 'Platform' },
			{ id: 'team-2', name: 'Design', path: 'Platform › Design' }
		]);
		expect(choices.employees.map((e) => e.id)).toEqual(['emp-1', 'emp-2']);
		expect(choices.projects).toEqual([
			{
				id: 'proj-1',
				name: 'Core Platform',
				number: 1,
				ownerTeam: { id: 'team-1', name: 'Platform' },
				archived: false
			}
		]);
	});

	it('creates a task with canonical lowercase UUIDv7 IDs even when caller passes a ULID', async () => {
		const { api, submit } = makeApi(
			async () => ok({}),
			async (_path, body) => {
				const b = body as Record<string, unknown>;
				return ok({
					task: { ...SAMPLE_TASK_WIRE, id: b.task_id, key: 'T-A2', title: b.title }
				});
			}
		);

		const created = await api.create('org-1', {
			operationId: '01JNOTAUUIDV7ULIDVALUE1234',
			type: 'task',
			title: 'Ship work composer',
			description: 'Smart composer details',
			projectId: 'proj-1',
			teamId: 'team-1',
			assignedTeamId: 'team-1',
			assigneeId: 'emp-2',
			priority: 'urgent',
			severity: null
		});

		expect(created.ref).toBe('T-A2');
		const [path, payload] = submit.mock.calls[0] as [string, Record<string, unknown>];
		expect(path).toBe('/api/v1/organizations/org-1/tasks');
		expect(payload.command_id).toMatch(UUID_V7_RE);
		expect(payload.task_id).toMatch(UUID_V7_RE);
		expect(payload.project_id).toBe('proj-1');
		expect(payload.priority).toBe('urgent');
	});

	it('inspects a task with checklist, comments, and history', async () => {
		const { api } = makeApi(
			async (path) => {
				if (path.includes('/projects')) return ok({ projects: SAMPLE_PROJECTS });
				return ok({
					task: SAMPLE_TASK_WIRE,
					checklist: [
						{
							id: 'step-1',
							position: 0,
							text: 'Run tests',
							required: true,
							done: false,
							revision: 1
						}
					],
					comments: [
						{
							id: 'c-1',
							position: 0,
							author_id: 'emp-2',
							created_at: '2026-11-25T10:30:00.000Z',
							body_markdown: 'Working on it.'
						}
					],
					history: [
						{
							task_id: SAMPLE_TASK_WIRE.id,
							revision: 2,
							occurred_at: '2026-11-25T11:00:00.000Z',
							actor_kind: 'person',
							actor_id: 'emp-1',
							change: { kind: 'transitioned', to: 'in_progress' }
						}
					]
				});
			},
			async () => ok({})
		);

		const detail = await api.task('org-1', 'T-A1');
		expect(detail.steps).toHaveLength(1);
		expect(detail.checklist).toEqual({ done: 0, total: 1, requiredLeft: 1 });
		expect(detail.comments[0]).toMatchObject({
			author: { id: 'emp-2', name: 'Kai', kind: 'agent' },
			body: 'Working on it.'
		});
		expect(detail.activity[0]).toMatchObject({
			kind: 'status',
			text: 'moved status to in progress'
		});
	});

	it('supports mutations (status, assign, checklist, comments, private notes) and fails closed on unserved endpoints', async () => {
		const noteWire = {
			id: '0193a5c0-0000-7000-8000-000000000050',
			task_id: SAMPLE_TASK_WIRE.id,
			author_id: 'emp-1',
			revision: 1,
			body_markdown: 'Private scratchpad',
			created_at: '2026-11-25T12:00:00.000Z',
			updated_at: '2026-11-25T12:00:00.000Z',
			editable: true
		};
		const { api, submit } = makeApi(
			async (path) => {
				if (path.includes('/projects')) return ok({ projects: SAMPLE_PROJECTS });
				if (path.endsWith('/notes')) return ok({ task_id: SAMPLE_TASK_WIRE.id, notes: [noteWire] });
				return ok({});
			},
			async (path) => {
				if (path.endsWith('/notes/add')) return ok({ note: noteWire });
				return ok({ task: SAMPLE_TASK_WIRE });
			}
		);

		await api.setStatus('org-1', 'T-A1', {
			status: 'blocked',
			blockedReason: 'Waiting on review',
			expectedRevision: 2
		});
		await api.assign('org-1', 'T-A1', {
			assignedTeamId: null,
			assigneeId: 'emp-2',
			expectedRevision: 3
		});
		await api.addSteps('org-1', 'T-A1', ['Check mobile']);
		await api.comment('org-1', 'T-A1', 'Ready to test');
		const feed = await api.notes('org-1', 'T-A1');
		expect(feed.canWrite).toBe(true);
		expect(feed.notes[0]).toMatchObject({ body: 'Private scratchpad', mine: true });
		const added = await api.addNote('org-1', 'T-A1', {
			body: 'Private scratchpad',
			operationId: 'op-1'
		});
		expect(added.id).toBe(noteWire.id);

		expect(submit.mock.calls.map((c) => c[0])).toEqual([
			'/api/v1/organizations/org-1/tasks/transition',
			'/api/v1/organizations/org-1/tasks/assign',
			'/api/v1/organizations/org-1/tasks/checklist/add',
			'/api/v1/organizations/org-1/tasks/comment',
			'/api/v1/organizations/org-1/tasks/notes/add'
		]);

		await expect(api.cost('org-1', 'T-A1')).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable'
		});
		await expect(api.whyChoices('org-1', 'T-A1')).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable'
		});
	});
});
