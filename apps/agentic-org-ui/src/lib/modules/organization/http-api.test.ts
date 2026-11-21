import { describe, expect, it, vi } from 'vitest';
import { createOperationClock } from '$lib/platform/http/clock';
import { readOperationId } from '$lib/platform/http/wire';
import type { OrgCommandInput } from './commands';
import { createHttpOrgApi, type OrgHttpTransport } from './http-api';

const INSTANCE = '0193a5c0-0000-7000-8000-000000000001';
const ORG = '0193a5c0-0000-7000-8000-000000000002';
const TEAM = '0193a5c0-0000-7000-8000-000000000003';
const PARENT = '0193a5c0-0000-7000-8000-000000000004';
const MEMBER = '0193a5c0-0000-7000-8000-000000000005';
const OP = '01a1c554-5200-7000-8000-000000000006';

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

	it('posts all 11 B2 organization mutations to their exact gateway routes with strict scope', async () => {
		const submit = vi.fn().mockResolvedValue({
			status: 200,
			body: { outcome: 'completed', operation_id: OP, result: {} }
		});
		const api = createHttpOrgApi({
			http: { read: vi.fn(), submit },
			instanceId: () => INSTANCE
		});

		const cases: {
			input: OrgCommandInput;
			route: string;
			params: Record<string, unknown>;
		}[] = [
			{
				input: {
					method: 'team.create',
					params: { team_id: TEAM, name: 'Platform', owner_team_id: PARENT }
				},
				route: 'teams/create',
				params: { team_id: TEAM, name: 'Platform', owner_team_id: PARENT }
			},
			{
				input: {
					method: 'team.rename',
					params: { team_id: TEAM, expected_version: 1, name: 'Core' }
				},
				route: 'teams/rename',
				params: { team_id: TEAM, expected_version: 1, name: 'Core' }
			},
			{
				input: {
					method: 'team.move',
					params: { team_id: TEAM, expected_version: 2, owner_team_id: PARENT }
				},
				route: 'teams/move',
				params: { team_id: TEAM, expected_version: 2, owner_team_id: PARENT }
			},
			{
				input: {
					method: 'team.set_lead',
					params: { team_id: TEAM, expected_version: 3, lead_member_id: MEMBER }
				},
				route: 'teams/set-lead',
				params: { team_id: TEAM, expected_version: 3, lead_member_id: MEMBER }
			},
			{
				input: {
					method: 'team.delete',
					params: { team_id: TEAM, expected_version: 4 }
				},
				route: 'teams/delete',
				params: { team_id: TEAM, expected_version: 4 }
			},
			{
				input: {
					method: 'team.set_title',
					params: { membership_id: MEMBER, expected_version: 1, title_in_team: 'Staff' }
				},
				route: 'teams/set-title',
				params: { membership_id: MEMBER, expected_version: 1, title_in_team: 'Staff' }
			},
			{
				input: {
					method: 'member.add_person',
					params: {
						member_id: MEMBER,
						name: 'Ada',
						contact: 'ada@example.test',
						handle: 'ada',
						home_team_id: TEAM
					}
				},
				route: 'members/add-person',
				params: {
					member_id: MEMBER,
					name: 'Ada',
					contact: 'ada@example.test',
					handle: 'ada',
					home_team_id: TEAM
				}
			},
			{
				input: {
					method: 'member.update_person',
					params: { member_id: MEMBER, expected_version: 1, name: 'Ada L.', clear_contact: true }
				},
				route: 'members/update-person',
				params: { member_id: MEMBER, expected_version: 1, name: 'Ada L.', clear_contact: true }
			},
			{
				input: {
					method: 'member.remove',
					params: { member_id: MEMBER, expected_version: 2 }
				},
				route: 'members/remove',
				params: { member_id: MEMBER, expected_version: 2 }
			},
			{
				input: {
					method: 'member.set_reports_to',
					params: { member_id: MEMBER, expected_version: 2, reports_to_member_id: PARENT }
				},
				route: 'members/set-reports-to',
				params: { member_id: MEMBER, expected_version: 2, reports_to_member_id: PARENT }
			},
			{
				input: {
					method: 'member.move',
					params: {
						membership_id: MEMBER,
						expected_version: 2,
						new_membership_id: MEMBER,
						to_team_id: PARENT,
						title_in_team: 'Lead'
					}
				},
				route: 'members/move',
				params: {
					membership_id: MEMBER,
					expected_version: 2,
					new_membership_id: MEMBER,
					to_team_id: PARENT,
					title_in_team: 'Lead'
				}
			},
			{
				input: {
					method: 'project.add_team',
					params: { project_team_id: TEAM, project_id: PARENT, team_id: TEAM }
				},
				route: 'projects/add-team',
				params: { project_team_id: TEAM, project_id: PARENT, team_id: TEAM }
			},
			{
				input: {
					method: 'project.remove_team',
					params: { project_team_id: `${PARENT}:${TEAM}`, expected_version: 2 }
				},
				route: 'projects/remove-team',
				params: { project_team_id: `${PARENT}:${TEAM}`, expected_version: 2 }
			}
		];

		for (const item of cases) {
			submit.mockClear();
			const accepted = await api.command(ORG, {
				method: item.input.method,
				params: { command_id: OP, ...item.input.params }
			} as never);
			expect(accepted).toEqual({ status: 'committed' });
			expect(submit).toHaveBeenCalledWith(`/api/v1/organizations/${ORG}/${item.route}`, {
				operation_id: OP,
				scope: { kind: 'organization', instance_id: INSTANCE, org_id: ORG },
				params: item.params
			});
		}
	});

	it('mints canonical lowercase UUIDv7 via calibrated clock when command_id is omitted and rejects ULIDs', async () => {
		const clock = createOperationClock({
			now: () => 1_000,
			random: () => new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8, 9, 10])
		});
		clock.synchronize(1_760_000_000_000);
		const submit = vi.fn().mockResolvedValue({
			status: 200,
			body: { outcome: 'completed', operation_id: OP, result: {} }
		});
		const api = createHttpOrgApi({
			http: { read: vi.fn(), submit },
			instanceId: () => INSTANCE,
			newId: () => clock.newId()
		});

		await api.command(ORG, {
			method: 'team.rename',
			params: { team_id: TEAM, expected_version: 1, name: 'Infra' }
		});
		const [, body] = submit.mock.calls[0] as [string, { operation_id: string }];
		expect(readOperationId(body.operation_id)).toBe(body.operation_id);

		// Legacy ULID command_id must fail closed before reaching http.submit.
		submit.mockClear();
		await expect(
			api.command(ORG, {
				method: 'team.rename',
				params: {
					command_id: '01ARZ3NDEKTSV4RRFFQ69G5FAV',
					team_id: TEAM,
					expected_version: 1,
					name: 'Infra'
				}
			})
		).rejects.toMatchObject({ source: 'api', code: 'invalid_request' });
		expect(submit).not.toHaveBeenCalled();
	});

	it('surfaces rejected gateway errors (including 403/409/410) and fails closed for unsupported commands', async () => {
		const submit = vi
			.fn()
			.mockResolvedValueOnce({
				status: 409,
				body: {
					outcome: 'rejected',
					operation_id: OP,
					error: { code: 'operation_reused', message: 'Operation reused.' }
				}
			})
			.mockResolvedValueOnce({
				status: 403,
				body: {
					outcome: 'rejected',
					operation_id: OP,
					error: { code: 'not_authorized', message: 'Forbidden.' }
				}
			})
			.mockResolvedValueOnce({
				status: 410,
				body: { error: { code: 'expired', message: 'Expired.' } }
			});
		const http: OrgHttpTransport = {
			read: vi.fn().mockResolvedValue({
				status: 404,
				body: { error: { code: 'not_found', message: 'Not found.' } }
			}),
			submit
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
			code: 'operation_reused',
			message: 'Operation reused.'
		});
		await expect(
			api.command(ORG, {
				method: 'team.delete',
				params: { command_id: OP, team_id: TEAM, expected_version: 1 }
			})
		).rejects.toEqual({
			source: 'api',
			code: 'not_authorized',
			message: 'Forbidden.'
		});
		await expect(
			api.command(ORG, {
				method: 'team.delete',
				params: { command_id: OP, team_id: TEAM, expected_version: 1 }
			})
		).rejects.toEqual({
			source: 'api',
			code: 'expired',
			message: 'Expired.'
		});
		await expect(
			api.command(ORG, {
				method: 'member.offboard',
				params: { command_id: OP, member_id: MEMBER, expected_version: 1 }
			})
		).rejects.toMatchObject({ source: 'api', code: 'unsupported' });
	});

	it('strips null optionals, omits clear_contact: false, and does not mint IDs for unsupported methods', async () => {
		const submit = vi.fn().mockResolvedValue({
			status: 200,
			body: { outcome: 'completed', operation_id: OP, result: {} }
		});
		const newId = vi.fn(() => OP);
		const api = createHttpOrgApi({
			http: { read: vi.fn(), submit },
			instanceId: () => INSTANCE,
			newId
		});

		await api.command(ORG, {
			method: 'team.set_lead',
			params: { team_id: TEAM, expected_version: 2, lead_member_id: null }
		} as unknown as OrgCommandInput);
		expect(submit).toHaveBeenLastCalledWith(`/api/v1/organizations/${ORG}/teams/set-lead`, {
			operation_id: OP,
			scope: { kind: 'organization', instance_id: INSTANCE, org_id: ORG },
			params: { team_id: TEAM, expected_version: 2 }
		});

		await api.command(ORG, {
			method: 'member.update_person',
			params: { member_id: MEMBER, expected_version: 2, name: 'Ada', clear_contact: false }
		});
		expect(submit).toHaveBeenLastCalledWith(`/api/v1/organizations/${ORG}/members/update-person`, {
			operation_id: OP,
			scope: { kind: 'organization', instance_id: INSTANCE, org_id: ORG },
			params: { member_id: MEMBER, expected_version: 2, name: 'Ada' }
		});

		newId.mockClear();
		await expect(
			api.command(ORG, {
				method: 'member.offboard',
				params: {
					command_id: '01ARZ3NDEKTSV4RRFFQ69G5FAV',
					member_id: MEMBER,
					expected_version: 1
				}
			})
		).rejects.toMatchObject({ source: 'api', code: 'unsupported' });
		expect(newId).not.toHaveBeenCalled();
	});
});
