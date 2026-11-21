import { describe, expect, it, vi } from 'vitest';
import { createHttpOrgApi, type OrgHttpTransport } from './http-api';

const INSTANCE = '0193a5c0-0000-7000-8000-000000000001';
const ORG = '0193a5c0-0000-7000-8000-000000000002';
const TEAM = '0193a5c0-0000-7000-8000-000000000003';
const PARENT = '0193a5c0-0000-7000-8000-000000000004';
const OP = '01ARZ3NDEKTSV4RRFFQ69G5FAV';

describe('createHttpOrgApi', () => {
	it('loads a live org snapshot with sample: false and normalizes unknown member kinds', async () => {
		const http: OrgHttpTransport = {
			read: vi.fn().mockResolvedValue({
				status: 200,
				body: {
					result: {
						snapshot: {
							position: { epoch: 1, seq: 2 },
							teams: [{ id: PARENT, name: 'Example', version: 1 }],
							members: [
								{
									id: 'm1',
									kind: 'person',
									name: 'Owner',
									handle: 'owner',
									home_team_id: PARENT,
									owner: true,
									version: 1
								}
							]
						},
						resume_cursor: `org:${ORG}:1:2`
					}
				}
			}),
			submit: vi.fn()
		};
		const api = createHttpOrgApi({ http, instanceId: () => INSTANCE });
		const view = await api.view(ORG);
		expect(http.read).toHaveBeenCalledWith(`/api/v1/organizations/${ORG}/snapshot`);
		expect(view.sample).toBe(false);
		expect(view.snapshot.position).toEqual({ epoch: 1, seq: 2 });
		expect(view.snapshot.teams).toHaveLength(1);
	});

	it('posts team.create with strict organization scope and returns committed on 200', async () => {
		const http: OrgHttpTransport = {
			read: vi.fn(),
			submit: vi.fn().mockResolvedValue({
				status: 200,
				body: { outcome: 'completed', operation_id: OP, result: {} }
			})
		};
		const api = createHttpOrgApi({ http, instanceId: () => INSTANCE });
		const accepted = await api.command(ORG, {
			method: 'team.create',
			params: { command_id: OP, team_id: TEAM, name: 'Platform', owner_team_id: PARENT }
		});
		expect(http.submit).toHaveBeenCalledWith(`/api/v1/organizations/${ORG}/teams`, {
			operation_id: OP,
			scope: { kind: 'organization', instance_id: INSTANCE, org_id: ORG },
			params: { team_id: TEAM, name: 'Platform', owner_team_id: PARENT }
		});
		expect(accepted).toEqual({ status: 'committed' });
	});

	it('surfaces rejected gateway errors and fails closed for unsupported commands', async () => {
		const http: OrgHttpTransport = {
			read: vi.fn().mockResolvedValue({
				status: 404,
				body: { error: { code: 'not_found', message: 'Not found.' } }
			}),
			submit: vi.fn().mockResolvedValue({
				status: 409,
				body: {
					outcome: 'rejected',
					operation_id: OP,
					error: { code: 'already_exists', message: 'Team already exists.' }
				}
			})
		};
		const api = createHttpOrgApi({ http, instanceId: () => INSTANCE });
		await expect(api.view(ORG)).rejects.toEqual({
			source: 'api',
			code: 'not_found',
			message: 'Not found.'
		});
		await expect(
			api.command(ORG, {
				method: 'team.create',
				params: { command_id: OP, team_id: TEAM, name: 'Platform', owner_team_id: PARENT }
			})
		).rejects.toEqual({
			source: 'api',
			code: 'already_exists',
			message: 'Team already exists.'
		});
		await expect(
			api.command(ORG, {
				method: 'team.rename',
				params: { command_id: OP, team_id: TEAM, expected_version: 1, name: 'Renamed' }
			})
		).rejects.toMatchObject({ source: 'api', code: 'unsupported' });
	});
});
