import { describe, expect, it, vi } from 'vitest';
import type { OrgSnapshot } from '$lib/proto';
import { createHttpMembersApi } from './http-api';

const ORG = '0193a5c0-0000-7000-8000-000000000001';
const TEAM_A = '0193a5c0-0000-7000-8000-000000000002';
const TEAM_B = '0193a5c0-0000-7000-8000-000000000003';
const OWNER = '0193a5c0-0000-7000-8000-000000000004';
const SAM = '0193a5c0-0000-7000-8000-000000000005';
const NEW_MEM = '0193a5c0-0000-7000-8000-000000000006';

const SNAPSHOT: OrgSnapshot = {
	position: { epoch: 1, seq: 2 },
	teams: [
		{ id: TEAM_A, name: 'Engineering', lead_member_id: OWNER, version: 1 },
		{ id: TEAM_B, name: 'Design', owner_team_id: TEAM_A, version: 1 }
	],
	members: [
		{
			id: OWNER,
			kind: 'person',
			name: 'Alex Owner',
			handle: 'alex',
			home_team_id: TEAM_A,
			owner: true,
			version: 1
		},
		{
			id: SAM,
			kind: 'person',
			name: 'Sam Ortiz',
			handle: 'sam',
			home_team_id: TEAM_A,
			reports_to_member_id: OWNER,
			contact: 'sam@acme.test',
			version: 3
		}
	]
};

describe('createHttpMembersApi', () => {
	it('builds directory and profile from live snapshot and access entries', async () => {
		const read = vi.fn(async (path: string) => {
			if (path.endsWith('/access')) {
				return {
					status: 200,
					body: {
						result: {
							entries: [
								{
									member_id: OWNER,
									account_id: 'acc-1',
									login: 'alex',
									admin: true,
									owner: true,
									pending_invite: false,
									pending_password_reset: false
								},
								{
									member_id: SAM,
									account_id: null,
									login: null,
									admin: false,
									owner: false,
									pending_invite: true,
									pending_password_reset: false
								}
							]
						}
					}
				};
			}
			return {
				status: 200,
				body: {
					result: {
						organizations: [
							{
								id: ORG,
								slug: 'acme',
								name: 'Acme',
								created_at: '2026-11-20T00:00:00Z',
								employee_id: OWNER,
								recovery: 'acknowledged',
								offices: []
							}
						],
						creating: []
					}
				}
			};
		});
		const api = createHttpMembersApi({
			http: { read, submit: vi.fn() },
			orgApi: {
				view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }),
				command: vi.fn()
			},
			session: () => ({ accountAdmin: true }),
			newEntityId: () => NEW_MEM
		});

		const rows = await api.directory(ORG);
		expect(rows).toHaveLength(2);
		expect(rows.find((r) => r.id === OWNER)?.canSignIn).toBe(true);
		expect(rows.find((r) => r.id === SAM)?.canSignIn).toBe(false);
		expect(rows.find((r) => r.id === SAM)?.office).toBeUndefined();

		const profile = await api.profile(ORG, SAM);
		expect(profile.reportsTo).toEqual({ id: OWNER, name: 'Alex Owner' });
		expect(profile.viewer.isOwnerOrAdmin).toBe(true);
		expect(profile.invitePending).toBeDefined();
	});

	it('moves home team and wires invite, reset-link, remove-access, and handover routes', async () => {
		const command = vi.fn().mockResolvedValue({ status: 'committed' });
		const submit = vi.fn(async (path: string) => {
			if (path.endsWith('/invite') || path.endsWith('/reset-link')) {
				return {
					status: 200,
					body: {
						result: { url: 'http://127.0.0.1/register#tok', expires_at_ms: 1_800_000_000_000 }
					}
				};
			}
			if (path.endsWith('/owner/handover')) {
				return {
					status: 403,
					body: { error: { code: 'not_authorized', message: 'Wrong password.' } }
				};
			}
			return { status: 200, body: { result: {} } };
		});
		const api = createHttpMembersApi({
			http: { read: vi.fn(), submit },
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }), command },
			session: () => ({ accountAdmin: true }),
			newEntityId: () => NEW_MEM
		});

		await api.moveHomeTeam(ORG, SAM, TEAM_B);
		expect(command).toHaveBeenCalledWith(ORG, {
			method: 'member.move',
			params: {
				membership_id: SAM,
				expected_version: 3,
				new_membership_id: NEW_MEM,
				to_team_id: TEAM_B
			}
		});

		const invite = await api.inviteToSignIn(ORG, SAM);
		expect(invite.url).toBe('http://127.0.0.1/register#tok');
		expect(submit).toHaveBeenCalledWith(`/api/v1/organizations/${ORG}/access/invite`, {
			member_id: SAM
		});

		await api.removeAccess(ORG, SAM);
		expect(submit).toHaveBeenCalledWith(`/api/v1/organizations/${ORG}/access/remove`, {
			member_id: SAM
		});

		await expect(api.handOverOwnership(ORG, SAM, 'wrong')).rejects.toMatchObject({
			code: 'wrong_password'
		});
	});

	it('resolves an agent role_id against /roles/{id}, rejects mismatched or malformed payloads, and handles 404/503', async () => {
		const ROLE_ID = '0193a5c0-0000-7000-8000-000000000040';
		const OTHER_ROLE_ID = '0193a5c0-0000-7000-8000-000000000041';
		const AGENT_ID = '0193a5c0-0000-7000-8000-000000000009';
		const withAgentRole: OrgSnapshot = {
			...SNAPSHOT,
			members: [
				...SNAPSHOT.members,
				{
					id: AGENT_ID,
					kind: 'agent',
					name: 'Linus Agent',
					handle: 'linus',
					home_team_id: TEAM_A,
					role_id: ROLE_ID,
					office_id: '0193a5c0-0000-7000-8000-000000000010',
					agent: {
						role: 'Systems Engineer',
						title: 'Principal Systems Agent',
						runtime: 'claude_code',
						messaging_name: 'linus',
						status: 'active'
					},
					version: 1
				}
			]
		};
		const read = vi
			.fn()
			.mockResolvedValueOnce({
				status: 200,
				body: {
					result: {
						id: ROLE_ID,
						name: 'Software Engineer',
						revision: 2,
						body: '## Mission\nBuild reliably.\n'
					}
				}
			})
			.mockResolvedValueOnce({
				status: 200,
				body: {
					result: {
						id: OTHER_ROLE_ID,
						name: 'Wrong Role',
						revision: 1,
						body: ''
					}
				}
			})
			.mockResolvedValueOnce({
				status: 200,
				body: {
					result: {
						id: ROLE_ID,
						name: 'Unsafe Rev',
						revision: Number.MAX_SAFE_INTEGER + 1,
						body: ''
					}
				}
			})
			.mockResolvedValueOnce({
				status: 404,
				body: { error: { code: 'not_found', message: 'role not found' } }
			})
			.mockResolvedValueOnce({
				status: 503,
				body: { error: { code: 'unavailable', message: 'office unreachable' } }
			});

		const api = createHttpMembersApi({
			http: { read, submit: vi.fn() },
			orgApi: {
				view: vi.fn().mockResolvedValue({ snapshot: withAgentRole, sample: false }),
				command: vi.fn()
			},
			session: () => ({ accountAdmin: true })
		});

		await expect(api.role(ORG, AGENT_ID)).resolves.toEqual({
			role: { name: 'Software Engineer', revision: 2, body: '## Mission\nBuild reliably.\n' },
			roleText: 'Systems Engineer',
			title: 'Principal Systems Agent'
		});
		await expect(api.role(ORG, AGENT_ID)).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable'
		});
		await expect(api.role(ORG, AGENT_ID)).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable'
		});
		await expect(api.role(ORG, AGENT_ID)).resolves.toEqual({
			roleText: 'Systems Engineer',
			title: 'Principal Systems Agent'
		});
		await expect(api.role(ORG, AGENT_ID)).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable'
		});
	});

	it('forwards AbortSignal through profile reads, returns openWork from single queue call, and rethrows on abort', async () => {
		const controller = new AbortController();
		const view = vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false });
		const read = vi.fn(async (_path: string, signal?: AbortSignal) => {
			if (signal?.aborted) throw new DOMException('Aborted', 'AbortError');
			return { status: 200, body: { result: { entries: [], organizations: [], offices: [] } } };
		});
		const queue = vi.fn(async (_orgId: string, _scope: unknown, signal?: AbortSignal) => {
			if (signal?.aborted) throw new DOMException('Aborted', 'AbortError');
			return {
				rows: [
					{
						id: 't1',
						ref: 'ORG-1',
						revision: 1,
						type: 'task' as const,
						title: 'Ship profile abort wiring',
						status: 'in_progress' as const,
						priority: 'normal' as const,
						severity: null,
						effort: null,
						project: null,
						team: { id: TEAM_A, name: 'Engineering' },
						assignedTeam: null,
						assignee: { id: SAM, name: 'Sam Ortiz', kind: 'person' as const },
						reporter: { id: OWNER, name: 'Alex Owner', kind: 'person' as const },
						createdAt: '2026-11-27T09:00:00Z',
						updatedAt: '2026-11-27T10:00:00Z',
						statusAt: '2026-11-27T10:00:00Z',
						checklist: { done: 0, total: 0, requiredLeft: 0 },
						blockedReason: null,
						reopenCount: 0,
						needsMe: null,
						budget: null
					}
				],
				viewer: { employee: null, teamIds: [], owner: true },
				needsMeSources: []
			};
		});

		const api = createHttpMembersApi({
			http: { read, submit: vi.fn() },
			orgApi: { view, command: vi.fn() },
			workItemsApi: { queue },
			session: () => ({ accountAdmin: true })
		});

		const profile = await api.profile(ORG, SAM, controller.signal);
		expect(view).toHaveBeenCalledWith(ORG, controller.signal);
		expect(queue).toHaveBeenCalledTimes(1);
		expect(queue).toHaveBeenCalledWith(ORG, { kind: 'organization' }, controller.signal);
		expect(read).toHaveBeenCalledWith(
			`/api/v1/organizations/${encodeURIComponent(ORG)}/access`,
			controller.signal
		);
		expect(read).toHaveBeenCalledWith('/api/v1/organizations', controller.signal);
		expect(read).toHaveBeenCalledWith(
			`/api/v1/organizations/${encodeURIComponent(ORG)}/offices`,
			controller.signal
		);
		expect(profile.openWork).toEqual({
			total: 1,
			items: [{ id: 't1', ref: 'ORG-1', title: 'Ship profile abort wiring', status: 'in_progress' }]
		});

		const aborted = new AbortController();
		aborted.abort();
		await expect(api.profile(ORG, SAM, aborted.signal)).rejects.toThrow();
	});
});
