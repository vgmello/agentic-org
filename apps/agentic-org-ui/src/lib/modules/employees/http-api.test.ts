import { describe, expect, it, vi } from 'vitest';
import type { OrgSnapshot } from '$lib/proto';
import { createHttpEmployeesApi } from './http-api';

const snapshot: OrgSnapshot = {
	position: { epoch: 1, seq: 3 },
	teams: [{ id: 't-eng', name: 'Engineering', version: 1 }],
	members: [
		{ id: 'm-owner', kind: 'person', name: 'Vinicius', owner: true, version: 1 },
		{
			id: 'm-hr',
			kind: 'agent',
			name: 'HR',
			handle: 'hr',
			version: 1,
			agent: {
				role: 'HR',
				title: 'HR',
				runtime: 'claude_code',
				messaging_name: 'hr',
				status: 'active'
			}
		}
	],
	projects: [],
	project_teams: []
};

describe('createHttpEmployeesApi', () => {
	it('loads hire access, choices, requests, and submits direct confirmation', async () => {
		const read = vi.fn(async (path: string) => {
			if (path === '/api/v1/organizations') {
				return {
					status: 200,
					body: {
						result: {
							organizations: [
								{
									id: 'org-1',
									slug: 'org',
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
			if (path.endsWith('/offices')) {
				return {
					status: 200,
					body: {
						result: {
							offices: [
								{
									id: 'off-1',
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
			if (path.endsWith('/agents/m-hr/readiness')) {
				return { status: 200, body: { result: { ready: true, status: 'READY', candidates: [] } } };
			}
			if (path.endsWith('/provider-connections')) {
				return {
					status: 200,
					body: {
						result: {
							connections: [{ id: 'c1', name: 'Claude', runtime_driver: 'claude', enabled: true }]
						}
					}
				};
			}
			if (path.endsWith('/model-profiles')) {
				return {
					status: 200,
					body: {
						result: {
							profiles: [{ id: 'p1', name: 'Claude Code', connection_ids: ['c1'], archived: false }]
						}
					}
				};
			}
			if (path.endsWith('/model-profiles/p1/models')) {
				return { status: 200, body: { result: { models: [{ model_id: 'sonnet' }] } } };
			}
			if (path.endsWith('/roles')) {
				return { status: 200, body: { result: { roles: [{ id: 'r1', name: 'Engineer' }] } } };
			}
			if (path.endsWith('/hires')) {
				return {
					status: 200,
					body: {
						result: {
							hires: [
								{
									id: 'h-1',
									revision: 1,
									state: 'hired',
									requested_by_employee_id: 'm-owner',
									home_team_id: 't-eng',
									proposal: { name: 'PM Lead', handle: 'pm', office_id: 'off-1' },
									reason: 'Coordinate PM work',
									created_at: '2026-11-25T13:00:00Z',
									updated_at: '2026-11-25T13:01:00Z',
									employee_id: 'emp-1'
								}
							]
						}
					}
				};
			}
			if (path.includes('/hires/')) {
				return {
					status: 200,
					body: {
						result: {
							id: 'h-new',
							revision: 1,
							state: 'hired',
							requested_by_employee_id: 'm-owner',
							home_team_id: 't-eng',
							proposal: { name: 'QA Bot', handle: 'qa', office_id: 'off-1' },
							reason: 'Automate QA',
							created_at: '2026-11-26T04:00:00Z',
							updated_at: '2026-11-26T04:00:00Z'
						}
					}
				};
			}
			return { status: 404, body: {} };
		});
		const submit = vi.fn(async () => ({ status: 200, body: { result: { ok: true } } }));

		const api = createHttpEmployeesApi({
			http: { read, submit },
			orgApi: { view: async () => ({ snapshot, position: snapshot.position, sample: false }) },
			session: () => ({ accountAdmin: true })
		});

		const access = await api.hireAccess('org-1');
		expect(access.seesAll).toBe(true);
		expect(access.hr.state).toBe('can_run');

		const choices = await api.hireChoices('org-1');
		expect(choices.profiles[0]?.models).toEqual(['sonnet']);
		expect(choices.offices[0]?.name).toBe('HQ');

		const hires = await api.hireRequests('org-1');
		expect(hires).toHaveLength(1);
		expect(hires[0]?.name).toBe('PM Lead');

		await api.confirm('org-1', {
			reason: 'Automate QA',
			proposal: {
				name: 'QA Bot',
				handle: 'qa',
				homeTeamId: 't-eng',
				executionProfileId: 'p1',
				model: 'sonnet',
				officeId: 'off-1'
			}
		});
		expect(submit).toHaveBeenCalledWith(
			'/api/v1/organizations/org-1/hires/direct',
			expect.objectContaining({
				home_team_id: 't-eng',
				reason: 'Automate QA'
			})
		);
	});
});
