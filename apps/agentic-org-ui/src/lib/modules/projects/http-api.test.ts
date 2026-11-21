import { describe, expect, it, vi } from 'vitest';
import type { OrgSnapshot } from '$lib/proto';
import { createHttpProjectsApi } from './http-api';
import { OUTCOME_UNKNOWN } from './outcome';

const INSTANCE = '0193a5c0-0000-7000-8000-000000000001';
const ORG = '0193a5c0-0000-7000-8000-000000000002';
const ROOT_TEAM = '0193a5c0-0000-7000-8000-000000000003';
const CONTRIB_TEAM = '0193a5c0-0000-7000-8000-000000000004';
const PROJ = '0193a5c0-0000-7000-8000-000000000005';
const NEW_TEAM = '0193a5c0-0000-7000-8000-000000000006';
const OP = '01a1c554-5200-7000-8000-000000000007';

const SNAPSHOT: OrgSnapshot = {
	position: { epoch: 1, seq: 1 },
	teams: [
		{ id: ROOT_TEAM, name: 'Acme', version: 1 },
		{ id: CONTRIB_TEAM, name: 'Design', owner_team_id: ROOT_TEAM, version: 1 }
	],
	members: []
};

describe('createHttpProjectsApi', () => {
	it('creates a project with UUIDv7 operation_id (even from ULID draft id) and reads back HQ number', async () => {
		const ids = [PROJ, NEW_TEAM];
		const submit = vi.fn().mockResolvedValue({
			status: 200,
			body: { outcome: 'completed', operation_id: OP, result: {} }
		});
		const read = vi.fn().mockResolvedValue({
			status: 200,
			body: {
				result: {
					id: PROJ,
					number: 1,
					key: 'P-1',
					slug: 'checkout',
					name: 'Checkout',
					owner_team_id: NEW_TEAM,
					contributing_team_ids: [],
					archived: false,
					revision: 1
				}
			}
		});
		const api = createHttpProjectsApi({
			http: { read, submit },
			orgApi: {
				view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }),
				command: vi.fn()
			},
			instanceId: () => INSTANCE,
			newId: () => OP,
			newEntityId: () => ids.shift()!
		});

		const created = await api.create(ORG, {
			operationId: '01ARZ3NDEKTSV4RRFFQ69G5FAV',
			name: 'Checkout',
			slug: 'checkout',
			owner: { kind: 'new', parentTeamId: ROOT_TEAM },
			developmentProcessId: null
		});
		expect(created).toEqual({ projectId: PROJ, number: 1 });
		expect(submit).toHaveBeenCalledWith(`/api/v1/organizations/${ORG}/projects`, {
			operation_id: OP,
			scope: { kind: 'organization', instance_id: INSTANCE, org_id: ORG },
			params: {
				project_id: PROJ,
				name: 'Checkout',
				slug: 'checkout',
				create_team_id: NEW_TEAM,
				parent_team_id: ROOT_TEAM
			}
		});
	});

	it('updates contributing teams via project.add_team and project.remove_team and refuses owner change honestly', async () => {
		let revision = 2;
		const read = vi.fn().mockImplementation(async () => ({
			status: 200,
			body: {
				result: {
					id: PROJ,
					number: 1,
					key: 'P-1',
					slug: 'checkout',
					name: 'Checkout',
					owner_team_id: ROOT_TEAM,
					contributing_team_ids: [CONTRIB_TEAM],
					archived: false,
					revision
				}
			}
		}));
		const command = vi.fn().mockImplementation(async () => {
			revision++;
			return { status: 'committed' };
		});
		const api = createHttpProjectsApi({
			http: { read, submit: vi.fn() },
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }), command },
			instanceId: () => INSTANCE,
			newEntityId: () => NEW_TEAM
		});

		await expect(
			api.setTeams(ORG, PROJ, {
				ownerTeamId: CONTRIB_TEAM,
				contributorTeamIds: [],
				expectedRevision: 2
			})
		).rejects.toMatchObject({ code: 'unsupported' });

		await api.setTeams(ORG, PROJ, {
			ownerTeamId: ROOT_TEAM,
			contributorTeamIds: [],
			expectedRevision: 2
		});
		expect(command).toHaveBeenCalledWith(ORG, {
			method: 'project.remove_team',
			params: { project_team_id: `${PROJ}:${CONTRIB_TEAM}`, expected_version: 2 }
		});
	});

	it('returns OUTCOME_UNKNOWN on 504 create and resolves createOperation via /api/v1/operations/org', async () => {
		const ids = [PROJ];
		const submit = vi.fn().mockResolvedValue({ status: 504, body: { outcome: 'unknown' } });
		const read = vi
			.fn()
			.mockResolvedValueOnce({ status: 200, body: { result: { status: 'completed' } } })
			.mockResolvedValueOnce({
				status: 200,
				body: {
					result: {
						id: PROJ,
						number: 4,
						key: 'P-4',
						slug: 'billing',
						name: 'Billing',
						owner_team_id: ROOT_TEAM,
						contributing_team_ids: [],
						archived: false,
						revision: 1
					}
				}
			});
		const api = createHttpProjectsApi({
			http: { read, submit },
			orgApi: {
				view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }),
				command: vi.fn()
			},
			instanceId: () => INSTANCE,
			newId: () => OP,
			newEntityId: () => ids.shift()!
		});

		await expect(
			api.create(ORG, {
				operationId: 'ulid-op-1',
				name: 'Billing',
				slug: 'billing',
				owner: { kind: 'existing', teamId: ROOT_TEAM },
				developmentProcessId: null
			})
		).rejects.toMatchObject({ code: OUTCOME_UNKNOWN });

		const resolved = await api.createOperation(ORG, 'ulid-op-1');
		expect(resolved).toEqual({ state: 'applied', projectId: PROJ, number: 4 });
	});
});
