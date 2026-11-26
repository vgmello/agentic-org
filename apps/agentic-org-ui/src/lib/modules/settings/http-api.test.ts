import { describe, expect, it, vi } from 'vitest';
import type { OrgSnapshot } from '$lib/proto';
import { createHttpSettingsApi } from './http-api';

const snapshot: OrgSnapshot = {
	position: { epoch: 1, seq: 3 },
	teams: [{ id: 't1', name: 'Core', version: 1 }],
	members: [
		{ id: 'm-owner', kind: 'person', name: 'Vinicius', owner: true, version: 1 },
		{ id: 'm-bob', kind: 'person', name: 'Bob', version: 1 },
		{
			id: 'm-agent',
			kind: 'agent',
			name: 'Ada',
			office_id: 'off-1',
			version: 1,
			agent: {
				role: 'Engineer',
				title: 'Engineer',
				runtime: 'claude_code',
				messaging_name: 'ada',
				status: 'active'
			}
		}
	],
	projects: [],
	project_teams: []
};

describe('createHttpSettingsApi', () => {
	it('hydrates general settings from live instance, org, offices, access, and provider connections', async () => {
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
			if (path === '/api/v1/instance') {
				return {
					status: 200,
					body: {
						result: {
							instance_id: 'inst-1',
							name: 'hq-mac',
							server_version: '0.1.0',
							web_status: 'ok',
							listen_scheme: 'http_loopback',
							server_time_ms: 1000
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
									id: 'off-1',
									organization_id: 'org-1',
									name: 'HQ Mac',
									role: 'hq',
									host: 'macbook.local',
									status: 'active'
								}
							]
						}
					}
				};
			}
			if (path === '/api/v1/organizations/org-1/access') {
				return {
					status: 200,
					body: {
						result: {
							entries: [
								{ member_id: 'm-owner', account_id: 'acc-1', login: 'vini', admin: true },
								{ member_id: 'm-bob', account_id: 'acc-2', login: 'bob', admin: false }
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
							connections: [{ provider_type: 'openai', status: 'active', label: 'OpenAI Pro' }]
						}
					}
				};
			}
			return { status: 404, body: { error: { code: 'not_found', message: 'not found' } } };
		});

		const api = createHttpSettingsApi({
			http: { read, submit: vi.fn() },
			orgApi: { view: async () => ({ snapshot, position: snapshot.position, sample: false }) },
			session: () => ({ accountAdmin: true, login: 'vini' })
		});

		const general = await api.general('org-1');
		expect(general.organization).toEqual({ id: 'org-1', name: 'Agentic Org' });
		expect(general.hqMachine).toBe('macbook.local');
		expect(general.instance).toBe('hq-mac');
		expect(general.viewer).toEqual({ isOwner: true, isAdministrator: true });
		expect(general.administrator).toEqual({ username: 'vini', person: 'Vinicius', isYou: true });
		expect(general.handOverCandidates).toEqual([
			{ memberId: 'm-bob', name: 'Bob', username: 'bob' }
		]);
		expect(general.runtimes.find((r) => r.id === 'claude_code')?.availability).toBe('ready');
		expect(general.runtimes.find((r) => r.id === 'codex')?.availability).toBe('ready');
	});

	it('submits administrator handover with resolved account_id', async () => {
		const submit = vi.fn(async () => ({ status: 200, body: { result: { handed_over: true } } }));
		const read = vi.fn(async (path: string) => {
			if (path === '/api/v1/organizations/org-1/access') {
				return {
					status: 200,
					body: {
						result: {
							entries: [
								{ member_id: 'm-owner', account_id: 'acc-1', login: 'vini', admin: false },
								{ member_id: 'm-bob', account_id: 'acc-2', login: 'bob', admin: true }
							]
						}
					}
				};
			}
			return { status: 404, body: { error: { code: 'not_found', message: 'not found' } } };
		});

		const api = createHttpSettingsApi({
			http: { read, submit },
			orgApi: { view: async () => ({ snapshot, position: snapshot.position, sample: false }) },
			session: () => ({ accountAdmin: false, login: 'vini' })
		});

		await api.handOverAdministrator('org-1', 'm-bob', 'secret-password-123');
		expect(submit).toHaveBeenCalledWith('/api/v1/instance/admin/handover', {
			to_account_id: 'acc-2',
			password: 'secret-password-123'
		});
	});
});
