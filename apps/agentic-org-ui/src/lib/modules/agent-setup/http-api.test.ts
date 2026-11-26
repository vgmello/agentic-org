import { describe, expect, it, vi } from 'vitest';
import type { OrgSnapshot } from '$lib/proto';
import { createHttpAgentSetupApi } from './http-api';

const snapshot: OrgSnapshot = {
	position: { epoch: 1, seq: 4 },
	teams: [{ id: 't-eng', name: 'Engineering', version: 1 }],
	members: [
		{ id: 'm-owner', kind: 'person', name: 'Vinicius', owner: true, version: 1 },
		{
			id: 'm-ada',
			kind: 'agent',
			name: 'Ada',
			handle: 'ada',
			home_team_id: 't-eng',
			office_id: 'off-hq',
			version: 3,
			agent: {
				role: 'Staff Engineer',
				title: 'Staff Engineer',
				runtime: 'claude_code',
				messaging_name: 'ada',
				status: 'active'
			}
		}
	],
	projects: [],
	project_teams: []
};

describe('createHttpAgentSetupApi', () => {
	function makeHttp(submit = vi.fn(async () => ({ status: 200, body: { result: { ok: true } } }))) {
		const read = vi.fn(async (path: string) => {
			if (path === '/api/v1/organizations') {
				return {
					status: 200,
					body: {
						result: {
							organizations: [
								{
									id: 'org-1',
									slug: 'agentic-org',
									name: 'Agentic Org',
									created_at: '2026-11-25T12:00:00Z',
									employee_id: 'm-owner',
									recovery: 'acknowledged',
									offices: []
								}
							]
						}
					}
				};
			}
			if (path === '/api/v1/organizations/org-1/offices') {
				return {
					status: 200,
					body: {
						result: {
							offices: [
								{
									id: 'off-hq',
									organization_id: 'org-1',
									name: 'HQ',
									role: 'hq',
									host: 'This Mac',
									status: 'active'
								}
							]
						}
					}
				};
			}
			if (path === '/api/v1/organizations/org-1/provider-connections') {
				return {
					status: 200,
					body: {
						result: {
							connections: [
								{
									id: 'conn-1',
									name: 'Claude Code',
									runtime_driver: 'claude',
									enabled: true
								}
							]
						}
					}
				};
			}
			if (path === '/api/v1/organizations/org-1/model-profiles') {
				return {
					status: 200,
					body: {
						result: {
							profiles: [
								{
									id: 'prof-1',
									name: 'Claude Code',
									connection_ids: ['conn-1'],
									archived: false
								}
							]
						}
					}
				};
			}
			if (path === '/api/v1/organizations/org-1/model-profiles/prof-1/models') {
				return {
					status: 200,
					body: {
						result: {
							models: [
								{
									model_id: 'sonnet',
									ready_connections: 1,
									supporting_connections: ['conn-1'],
									stale: false,
									as_of: '2026-11-26T04:00:00Z'
								}
							]
						}
					}
				};
			}
			if (path === '/api/v1/organizations/org-1/roles') {
				return { status: 200, body: { result: { roles: [{ id: 'role-1', name: 'Engineer' }] } } };
			}
			if (path === '/api/v1/organizations/org-1/agents/m-ada/execution') {
				return {
					status: 200,
					body: {
						result: {
							employee_id: 'm-ada',
							enabled: true,
							model_id: 'sonnet',
							model_profile_id: 'prof-1',
							revision: 2,
							tools_config: ['shell'],
							system_prompt_markdown: 'Be helpful'
						}
					}
				};
			}
			if (path === '/api/v1/organizations/org-1/agents/m-ada/readiness') {
				return {
					status: 200,
					body: {
						result: {
							employee_id: 'm-ada',
							ready: true,
							status: 'READY',
							candidates: [{ connection_id: 'conn-1', eligible: true, priority: 1 }]
						}
					}
				};
			}
			return { status: 404, body: { error: { code: 'not_found', message: 'not found' } } };
		});
		return { read, submit };
	}

	it('lists agents with execution, placement, readiness, and catalog', async () => {
		const api = createHttpAgentSetupApi({
			http: makeHttp(),
			orgApi: { view: async () => ({ snapshot, position: snapshot.position, sample: false }) },
			session: () => ({ accountAdmin: true }),
			now: () => '2026-11-26T04:00:00Z'
		});

		const list = await api.list('org-1');
		expect(list.viewer.canChange).toBe(true);
		expect(list.agents).toHaveLength(1);
		expect(list.agents[0]).toMatchObject({
			memberId: 'm-ada',
			name: 'Ada',
			execution: { modelId: 'sonnet', runtimes: ['Claude Code'] },
			placement: { office: { id: 'off-hq', name: 'HQ' } },
			readiness: { known: true, blockers: [] }
		});
	});

	it('loads agent detail and submits execution and role mutations', async () => {
		const http = makeHttp();
		const api = createHttpAgentSetupApi({
			http,
			orgApi: { view: async () => ({ snapshot, position: snapshot.position, sample: false }) },
			session: () => ({ accountAdmin: true }),
			now: () => '2026-11-26T04:00:00Z'
		});

		const detail = await api.agent('org-1', 'm-ada');
		expect(detail.execution?.revision).toBe(2);
		expect(detail.execution?.connections[0]?.state).toBe('ready');
		expect(detail.profiles[0]?.models?.[0]?.id).toBe('sonnet');

		await api.saveExecution('org-1', 'm-ada', {
			profileId: 'prof-1',
			modelId: 'claude-opus-4-6',
			expectedRevision: 2
		});
		expect(http.submit).toHaveBeenCalledWith(
			'/api/v1/organizations/org-1/agents/execution',
			expect.objectContaining({
				employee_id: 'm-ada',
				model_profile_id: 'prof-1',
				model_id: 'claude-opus-4-6',
				expected_version: 2,
				tools_config: ['shell'],
				system_prompt_markdown: 'Be helpful'
			})
		);

		await api.setRole('org-1', 'm-ada', { roleId: 'role-1', expectedRevision: 3 });
		expect(http.submit).toHaveBeenCalledWith(
			'/api/v1/organizations/org-1/agents/update',
			expect.objectContaining({
				member_id: 'm-ada',
				role_id: 'role-1',
				expected_version: 3
			})
		);
	});
});
