import { describe, expect, it, vi } from 'vitest';
import type { OrgSnapshot } from '$lib/proto';
import { createHttpRolesApi } from './http-api';
import { HR_CANT_RUN, conflictLatest, refusedField } from './present';

const ORG = '0193a5c0-0000-7000-8000-000000000001';
const TEAM_A = '0193a5c0-0000-7000-8000-000000000002';
const OWNER = '0193a5c0-0000-7000-8000-000000000004';
const MEMBER_B = '0193a5c0-0000-7000-8000-000000000005';
const AGENT_ACTIVE = '0193a5c0-0000-7000-8000-000000000006';
const AGENT_OFFBOARDED = '0193a5c0-0000-7000-8000-000000000007';
const ROLE_SWE = '0193a5c0-0000-7000-8000-000000000040';
const ROLE_QA = '0193a5c0-0000-7000-8000-000000000041';
const NEW_ROLE = '0193a5c0-0000-7000-8000-000000000042';
const CMD_ID = '0193a5c0-0000-7000-8000-000000000099';

const SNAPSHOT: OrgSnapshot = {
	position: { epoch: 1, seq: 5 },
	teams: [{ id: TEAM_A, name: 'Engineering', lead_member_id: OWNER, version: 1 }],
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
			id: MEMBER_B,
			kind: 'person',
			name: 'Sam Ortiz',
			handle: 'sam',
			home_team_id: TEAM_A,
			role_id: ROLE_SWE,
			title_in_team: 'Tech Lead',
			version: 1
		},
		{
			id: AGENT_ACTIVE,
			kind: 'agent',
			name: 'Ada Agent',
			handle: 'ada',
			home_team_id: TEAM_A,
			role_id: ROLE_SWE,
			office_id: '0193a5c0-0000-7000-8000-000000000010',
			agent: {
				role: 'Software Engineer',
				title: 'Backend Agent',
				runtime: 'claude_code',
				messaging_name: 'ada',
				status: 'active'
			},
			version: 1
		},
		{
			id: AGENT_OFFBOARDED,
			kind: 'agent',
			name: 'Retired Bot',
			handle: 'retired',
			home_team_id: TEAM_A,
			role_id: ROLE_SWE,
			office_id: '0193a5c0-0000-7000-8000-000000000010',
			agent: {
				role: 'Software Engineer',
				title: 'Former Agent',
				runtime: 'claude_code',
				messaging_name: 'retired',
				status: 'offboarded',
				offboarded_at: '2026-11-20T00:00:00Z'
			},
			version: 2
		}
	]
};

function orgListReply(employeeId: string) {
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
						employee_id: employeeId,
						recovery: 'acknowledged',
						offices: []
					}
				],
				creating: []
			}
		}
	};
}

describe('createHttpRolesApi', () => {
	it('lists roles with active employee counts, owner edit rights, and honest HR unavailable state', async () => {
		const read = vi.fn(async (path: string) => {
			if (path === `/api/v1/organizations/${ORG}/roles`) {
				return {
					status: 200,
					body: {
						result: {
							roles: [
								{
									id: ROLE_SWE,
									key: 'software-engineer',
									name: 'Software Engineer',
									body: '## Mission\nBuild reliable software.\n',
									revision: 2,
									created_at: '2026-11-20T10:00:00Z',
									updated_at: '2026-11-21T10:00:00Z'
								},
								{
									id: ROLE_QA,
									name: 'QA Engineer',
									body: '## Mission\nVerify releases.\n',
									revision: 1
								}
							]
						}
					}
				};
			}
			return orgListReply(OWNER);
		});

		const api = createHttpRolesApi({
			http: { read, submit: vi.fn() },
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) }
		});

		const overview = await api.list(ORG);
		expect(overview.viewer).toEqual({ canEdit: true });
		expect(overview.hr).toEqual({ canRun: false });
		expect(overview.roles).toEqual([
			{
				id: ROLE_SWE,
				key: 'software-engineer',
				name: 'Software Engineer',
				revision: 2,
				employeeCount: 2
			},
			{
				id: ROLE_QA,
				name: 'QA Engineer',
				revision: 1,
				employeeCount: 0
			}
		]);
	});

	it('marks viewer.canEdit false for non-owner members and fails closed on askHrToDraft', async () => {
		const read = vi.fn(async (path: string) => {
			if (path.endsWith('/roles')) return { status: 200, body: { result: { roles: [] } } };
			return orgListReply(MEMBER_B);
		});

		const api = createHttpRolesApi({
			http: { read, submit: vi.fn() },
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) }
		});

		const overview = await api.list(ORG);
		expect(overview.viewer.canEdit).toBe(false);
		await expect(api.askHrToDraft(ORG, 'Release manager')).rejects.toMatchObject({
			source: 'api',
			code: 'not_ready',
			message: HR_CANT_RUN
		});
	});

	it('reads a role page with resolved attribution, sorted active employees, and surfaces 404 not_found', async () => {
		const read = vi.fn(async (path: string) => {
			if (path === `/api/v1/organizations/${ORG}/roles/${ROLE_SWE}`) {
				return {
					status: 200,
					body: {
						result: {
							id: ROLE_SWE,
							key: 'software-engineer',
							name: 'Software Engineer',
							body: '## Mission\nShip clean code.\n',
							revision: 3,
							created_at: '2026-11-20T08:00:00Z',
							created_by_kind: 'system',
							updated_at: '2026-11-22T09:30:00Z',
							updated_by_kind: 'employee',
							updated_by_id: OWNER
						}
					}
				};
			}
			if (path.endsWith('/roles/missing')) {
				return {
					status: 404,
					body: { error: { code: 'not_found', message: 'role not found' } }
				};
			}
			return orgListReply(OWNER);
		});

		const api = createHttpRolesApi({
			http: { read, submit: vi.fn() },
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) }
		});

		const detail = await api.role(ORG, ROLE_SWE);
		expect(detail.role.created).toEqual({ at: '2026-11-20T08:00:00Z', byKind: 'system' });
		expect(detail.role.updated).toEqual({
			at: '2026-11-22T09:30:00Z',
			byKind: 'employee',
			byId: OWNER,
			byName: 'Alex Owner'
		});
		expect(detail.employees).toEqual([
			{ id: AGENT_ACTIVE, name: 'Ada Agent', kind: 'agent', title: 'Backend Agent' },
			{ id: MEMBER_B, name: 'Sam Ortiz', kind: 'person', title: 'Tech Lead' }
		]);
		expect(detail.history).toHaveLength(1);
		expect(detail.history[0]?.revision).toBe(3);

		await expect(api.role(ORG, 'missing')).rejects.toMatchObject({
			source: 'api',
			code: 'not_found'
		});
	});

	it('creates a role, attaches field on duplicate/invalid key refusals, and falls back if post-write inspect fails', async () => {
		const submit = vi
			.fn()
			.mockResolvedValueOnce({
				status: 409,
				body: {
					error: { code: 'already_exists', message: 'a role with that key already exists' }
				}
			})
			.mockResolvedValueOnce({
				status: 400,
				body: {
					error: { code: 'invalid_name', message: 'the role name is one line of text' }
				}
			})
			.mockResolvedValueOnce({
				status: 200,
				body: { result: { id: NEW_ROLE, revision: 1, generation: 7 } }
			});
		const read = vi.fn().mockRejectedValue(new Error('transient'));
		const api = createHttpRolesApi({
			http: { read, submit },
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) },
			newEntityId: () => NEW_ROLE,
			newCommandId: () => CMD_ID
		});

		const dupErr = await api
			.create(ORG, { name: 'Release', key: 'software-engineer', bodyMarkdown: '## Mission\n' })
			.catch((e) => e);
		expect(refusedField(dupErr)).toBe('key');

		const nameErr = await api
			.create(ORG, { name: 'Bad\nName', bodyMarkdown: '## Mission\n' })
			.catch((e) => e);
		expect(refusedField(nameErr)).toBe('name');

		const created = await api.create(ORG, {
			name: 'Release Manager',
			key: 'release-manager',
			bodyMarkdown: '## Mission\nShip.\n'
		});
		expect(submit).toHaveBeenLastCalledWith(`/api/v1/organizations/${ORG}/roles`, {
			command_id: CMD_ID,
			role_id: NEW_ROLE,
			name: 'Release Manager',
			body: '## Mission\nShip.\n',
			key: 'release-manager'
		});
		expect(created).toMatchObject({
			id: NEW_ROLE,
			key: 'release-manager',
			name: 'Release Manager',
			revision: 1
		});
	});

	it('updates roles with explicit clear_key and fetches latest role on 409 conflict', async () => {
		const submit = vi
			.fn()
			.mockResolvedValueOnce({
				status: 409,
				body: {
					error: {
						code: 'conflict',
						message: 'it changed since you last looked; refresh and try again'
					}
				}
			})
			.mockResolvedValueOnce({
				status: 200,
				body: { result: { id: ROLE_SWE, revision: 4, generation: 9 } }
			});
		const read = vi
			.fn()
			.mockResolvedValueOnce({
				status: 200,
				body: {
					result: {
						id: ROLE_SWE,
						key: 'software-engineer',
						name: 'Software Engineer',
						body: '## Mission\nUpdated elsewhere.\n',
						revision: 3,
						updated_at: '2026-11-23T00:00:00Z',
						updated_by_kind: 'employee',
						updated_by_id: OWNER
					}
				}
			})
			.mockResolvedValueOnce({
				status: 200,
				body: {
					result: {
						id: ROLE_SWE,
						name: 'Principal Engineer',
						body: '## Mission\nLead architecture.\n',
						revision: 4,
						updated_at: '2026-11-23T01:00:00Z',
						updated_by_kind: 'employee',
						updated_by_id: OWNER
					}
				}
			});

		const api = createHttpRolesApi({
			http: { read, submit },
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) },
			newCommandId: () => CMD_ID
		});

		const conflictErr = await api
			.update(ORG, ROLE_SWE, {
				name: 'Principal Engineer',
				bodyMarkdown: '## Mission\nLead architecture.\n',
				expectedRevision: 2
			})
			.catch((e) => e);
		expect(conflictLatest(conflictErr)).toMatchObject({
			id: ROLE_SWE,
			revision: 3,
			bodyMarkdown: '## Mission\nUpdated elsewhere.\n'
		});

		const saved = await api.update(ORG, ROLE_SWE, {
			name: 'Principal Engineer',
			bodyMarkdown: '## Mission\nLead architecture.\n',
			expectedRevision: 3
		});
		expect(submit).toHaveBeenLastCalledWith(`/api/v1/organizations/${ORG}/roles/update`, {
			command_id: CMD_ID,
			role_id: ROLE_SWE,
			expected_version: 3,
			name: 'Principal Engineer',
			body: '## Mission\nLead architecture.\n',
			clear_key: true
		});
		expect(saved.revision).toBe(4);
		expect(saved.key).toBeUndefined();
	});

	it('rejects malformed gateway payloads with unavailable', async () => {
		const api = createHttpRolesApi({
			http: {
				read: vi
					.fn()
					.mockResolvedValue({ status: 200, body: { result: { roles: [{ id: 123 }] } } }),
				submit: vi.fn()
			},
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) }
		});
		await expect(api.list(ORG)).rejects.toMatchObject({ source: 'api', code: 'unavailable' });
	});

	it('requires person kind with strict owner === true for viewer.canEdit', async () => {
		const corruptedSnapshot: OrgSnapshot = {
			...SNAPSHOT,
			members: [
				{
					id: OWNER,
					kind: 'person',
					name: 'Alex',
					owner: 'false' as unknown as boolean,
					version: 1
				},
				{
					id: AGENT_ACTIVE,
					kind: 'agent',
					name: 'Agent Owner',
					owner: true,
					version: 1
				}
			]
		};
		for (const employeeId of [OWNER, AGENT_ACTIVE]) {
			const api = createHttpRolesApi({
				http: {
					read: vi.fn(async (path: string) =>
						path.endsWith('/roles')
							? { status: 200, body: { result: { roles: [] } } }
							: orgListReply(employeeId)
					),
					submit: vi.fn()
				},
				orgApi: { view: vi.fn().mockResolvedValue({ snapshot: corruptedSnapshot, sample: false }) }
			});
			const overview = await api.list(ORG);
			expect(overview.viewer.canEdit).toBe(false);
		}
	});

	it('rejects ID mismatches, non-UUIDv7 IDs, invalid/impossible calendar timestamps, unsafe revisions, and empty mutation acks', async () => {
		const badReads = [
			{ id: ROLE_QA, name: 'Wrong ID', body: '', revision: 1 },
			{ id: 'update', name: 'Reserved Path ID', body: '', revision: 1 },
			{ id: ROLE_SWE, name: 'Bad Time', body: '', revision: 1, updated_at: 'not-a-timestamp' },
			{
				id: ROLE_SWE,
				name: 'Impossible Feb 30',
				body: '',
				revision: 1,
				updated_at: '2026-02-30T09:30:00Z'
			},
			{
				id: ROLE_SWE,
				name: 'Non-leap Feb 29',
				body: '',
				revision: 1,
				created_at: '2027-02-29T00:00:00Z'
			},
			{
				id: ROLE_SWE,
				name: 'Century non-leap Feb 29',
				body: '',
				revision: 1,
				created_at: '2100-02-29T00:00:00Z'
			},
			{ id: ROLE_SWE, name: 'Unsafe Rev', body: '', revision: Number.MAX_SAFE_INTEGER + 1 }
		];
		for (const bad of badReads) {
			const api = createHttpRolesApi({
				http: {
					read: vi.fn(async (path: string) =>
						path.includes('/roles/') ? { status: 200, body: { result: bad } } : orgListReply(OWNER)
					),
					submit: vi.fn()
				},
				orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) }
			});
			await expect(api.role(ORG, ROLE_SWE)).rejects.toMatchObject({
				source: 'api',
				code: 'unavailable'
			});
		}

		// Catalogue list also rejects non-UUIDv7 IDs like "update" before generating links.
		const apiBadListId = createHttpRolesApi({
			http: {
				read: vi.fn(async (path: string) =>
					path.endsWith('/roles')
						? {
								status: 200,
								body: { result: { roles: [{ id: 'update', name: 'Bad', body: '', revision: 1 }] } }
							}
						: orgListReply(OWNER)
				),
				submit: vi.fn()
			},
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) }
		});
		await expect(apiBadListId.list(ORG)).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable'
		});

		// Valid RFC 3339 lowercase t/z, offsets, leap day, leap second, and 9999 offset boundary normalize cleanly.
		const apiValidTimes = createHttpRolesApi({
			http: {
				read: vi.fn(async (path: string) =>
					path.includes('/roles/')
						? {
								status: 200,
								body: {
									result: {
										id: ROLE_SWE,
										name: 'Leap Role',
										body: '## Mission\nParity.\n',
										revision: 1,
										created_at: '2028-02-29t08:00:00z',
										updated_at: '2026-12-31T23:59:60Z'
									}
								}
							}
						: orgListReply(OWNER)
				),
				submit: vi.fn()
			},
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) }
		});
		const leapDetail = await apiValidTimes.role(ORG, ROLE_SWE);
		expect(leapDetail.role.created.at).toBe('2028-02-29T08:00:00.000Z');
		expect(leapDetail.role.updated.at).toBe('2027-01-01T00:00:00.000Z');

		const apiYearBoundary = createHttpRolesApi({
			http: {
				read: vi.fn(async (path: string) =>
					path.includes('/roles/')
						? {
								status: 200,
								body: {
									result: {
										id: ROLE_SWE,
										name: 'Boundary Role',
										body: '## Mission\nBoundary.\n',
										revision: 1,
										created_at: '9999-12-31t23:59:59-00:01',
										updated_at: '9999-12-31T23:59:59.999Z'
									}
								}
							}
						: orgListReply(OWNER)
				),
				submit: vi.fn()
			},
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) }
		});
		const boundaryDetail = await apiYearBoundary.role(ORG, ROLE_SWE);
		expect(boundaryDetail.role.created.at).toBe('9999-12-31T23:59:59-00:01');
		expect(boundaryDetail.role.updated.at).toBe('9999-12-31T23:59:59.999Z');

		// Error-only 200 envelopes must map to `unavailable`, never `not_found`.
		const apiErrorOn200 = createHttpRolesApi({
			http: {
				read: vi.fn(async (path: string) =>
					path.includes('/roles/')
						? { status: 200, body: { error: { code: 'not_found', message: 'missing' } } }
						: orgListReply(OWNER)
				),
				submit: vi.fn()
			},
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) }
		});
		await expect(apiErrorOn200.role(ORG, ROLE_SWE)).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable'
		});

		const apiEmptyAck = createHttpRolesApi({
			http: {
				read: vi.fn(),
				submit: vi.fn().mockResolvedValue({ status: 200, body: { result: {} } })
			},
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) },
			newEntityId: () => NEW_ROLE,
			newCommandId: () => CMD_ID
		});
		await expect(
			apiEmptyAck.create(ORG, { name: 'Empty Ack', bodyMarkdown: '## Mission\n' })
		).rejects.toMatchObject({ source: 'api', code: 'unavailable' });
		await expect(
			apiEmptyAck.update(ORG, ROLE_SWE, {
				name: 'Empty Ack',
				bodyMarkdown: '## Mission\n',
				expectedRevision: 1
			})
		).rejects.toMatchObject({ source: 'api', code: 'unavailable' });
	});

	it('preserves retry IDs across 504 -> 401 reauth -> contradictory ack -> replay, isolates callers and independent saves, and never reports another editor revision', async () => {
		let nextSeq = 1;
		const nextId = () => `0193a5c0-0000-7000-8000-0000000002${String(nextSeq++).padStart(2, '0')}`;
		let currentCaller = 'inst-1:acct-a';
		const submit = vi
			.fn()
			// Create attempt A: 504 -> 401 (expired session) -> contradictory 200 -> 200 replay (rev 1)
			.mockResolvedValueOnce({ status: 504, body: { outcome: 'unknown' } })
			.mockResolvedValueOnce({
				status: 401,
				body: { error: { code: 'not_authorized', message: 'Sign in again.' } }
			})
			.mockResolvedValueOnce({
				status: 200,
				body: {
					result: { id: '0193a5c0-0000-7000-8000-000000000202', revision: 1, generation: 5 },
					error: { code: 'unavailable', message: 'contradictory envelope' }
				}
			})
			.mockResolvedValueOnce({
				status: 200,
				body: {
					result: { id: '0193a5c0-0000-7000-8000-000000000202', revision: 1, generation: 5 }
				}
			})
			// Independent second save with identical text (different attemptId) gets fresh IDs
			.mockResolvedValueOnce({
				status: 200,
				body: {
					result: { id: '0193a5c0-0000-7000-8000-000000000204', revision: 1, generation: 6 }
				}
			})
			// Update attempt: 504 -> 401 -> 200 replay (ack rev 2 while readback is rev 3 by another editor)
			.mockResolvedValueOnce({ status: 504, body: { outcome: 'unknown' } })
			.mockResolvedValueOnce({
				status: 401,
				body: { error: { code: 'not_authorized', message: 'Sign in again.' } }
			})
			.mockResolvedValueOnce({
				status: 200,
				body: {
					result: { id: '0193a5c0-0000-7000-8000-000000000202', revision: 2, generation: 7 }
				}
			})
			// Different caller with same attemptId gets fresh IDs
			.mockResolvedValueOnce({ status: 504, body: { outcome: 'unknown' } })
			.mockResolvedValueOnce({
				status: 200,
				body: {
					result: { id: '0193a5c0-0000-7000-8000-000000000209', revision: 1, generation: 8 }
				}
			});
		const read = vi.fn(async (path: string) => {
			if (path.includes('/roles/0193a5c0-0000-7000-8000-000000000202')) {
				// Simulate another editor having already advanced the role to rev 3 before replay readback.
				return {
					status: 200,
					body: {
						result: {
							id: '0193a5c0-0000-7000-8000-000000000202',
							name: 'Other Editor Name',
							body: '## Mission\nChanged later.\n',
							revision: 3,
							created_at: '2026-11-20T08:00:00Z',
							created_by_kind: 'employee',
							created_by_id: OWNER,
							updated_at: '2026-11-24T12:00:00Z',
							updated_by_kind: 'employee',
							updated_by_id: MEMBER_B
						}
					}
				};
			}
			return Promise.reject(new Error('offline'));
		});
		const api = createHttpRolesApi({
			http: { read, submit },
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) },
			sessionScope: () => currentCaller,
			newCommandId: nextId,
			newEntityId: nextId
		});

		const draft1 = {
			name: 'Keyless Role',
			bodyMarkdown: '## Mission\nShip once.\n',
			attemptId: 'save-1'
		};
		await expect(api.create(ORG, draft1)).rejects.toMatchObject({ code: 'unavailable' });
		await expect(api.create(ORG, draft1)).rejects.toMatchObject({ code: 'not_authorized' });
		await expect(api.create(ORG, draft1)).rejects.toMatchObject({ code: 'unavailable' });
		const created1 = await api.create(ORG, draft1);
		// Must report acknowledged rev 1 and submitted name, not readback rev 3 ("Other Editor Name").
		expect(created1).toMatchObject({
			id: '0193a5c0-0000-7000-8000-000000000202',
			name: 'Keyless Role',
			revision: 1
		});
		expect(submit.mock.calls[0]?.[1]).toEqual(submit.mock.calls[1]?.[1]);
		expect(submit.mock.calls[1]?.[1]).toEqual(submit.mock.calls[2]?.[1]);
		expect(submit.mock.calls[2]?.[1]).toEqual(submit.mock.calls[3]?.[1]);

		// Independent save with identical text uses distinct attemptId -> fresh command_id & role_id.
		const created2 = await api.create(ORG, {
			name: 'Keyless Role',
			bodyMarkdown: '## Mission\nShip once.\n',
			attemptId: 'save-2'
		});
		expect(created2.id).toBe('0193a5c0-0000-7000-8000-000000000204');
		expect(submit.mock.calls[4]?.[1]).not.toEqual(submit.mock.calls[3]?.[1]);

		// Update: 504 -> 401 -> replay rev 2 must NOT return rev 3 ("Other Editor Name").
		const edit = {
			name: 'My Revision Two',
			bodyMarkdown: '## Mission\nUpdated once.\n',
			expectedRevision: 1,
			attemptId: 'edit-1'
		};
		await expect(api.update(ORG, created1.id, edit)).rejects.toMatchObject({ code: 'unavailable' });
		await expect(api.update(ORG, created1.id, edit)).rejects.toMatchObject({
			code: 'not_authorized'
		});
		const updated = await api.update(ORG, created1.id, edit);
		expect(updated).toMatchObject({
			id: created1.id,
			name: 'My Revision Two',
			bodyMarkdown: '## Mission\nUpdated once.\n',
			revision: 2
		});
		expect(submit.mock.calls[5]?.[1]).toEqual(submit.mock.calls[6]?.[1]);
		expect(submit.mock.calls[6]?.[1]).toEqual(submit.mock.calls[7]?.[1]);

		// Caller isolation: pending 504 for acct-a must NOT be inherited by acct-b even with same attemptId.
		await expect(
			api.create(ORG, {
				name: 'Shared Text',
				bodyMarkdown: '## Mission\nBody.\n',
				attemptId: 'same-token'
			})
		).rejects.toMatchObject({ code: 'unavailable' });
		currentCaller = 'inst-1:acct-b';
		const otherCallerCreated = await api.create(ORG, {
			name: 'Shared Text',
			bodyMarkdown: '## Mission\nBody.\n',
			attemptId: 'same-token'
		});
		expect(otherCallerCreated.id).toBe('0193a5c0-0000-7000-8000-000000000209');
		expect(submit.mock.calls[9]?.[1]).not.toEqual(submit.mock.calls[8]?.[1]);
	});

	it('resolves an uncertain create before applying edited fields so editing after 504 never duplicates a role, and clears abandoned state on sign-out', async () => {
		let seq = 1;
		const nextId = () => `0193a5c0-0000-7000-8000-0000000003${String(seq++).padStart(2, '0')}`;
		const submit = vi
			.fn()
			// 1. Initial create hits 504 (committed on server as 0302 rev 1).
			.mockResolvedValueOnce({ status: 504, body: { outcome: 'unknown' } })
			// 2. User edits body and submits again -> replays original create -> 200 rev 1.
			.mockResolvedValueOnce({
				status: 200,
				body: {
					result: { id: '0193a5c0-0000-7000-8000-000000000302', revision: 1, generation: 10 }
				}
			})
			// 3. Follow-up update of 0302 with edited body hits 504.
			.mockResolvedValueOnce({ status: 504, body: { outcome: 'unknown' } })
			// 4. User retries -> skips create, replays exact same follow-up update -> 200 rev 2.
			.mockResolvedValueOnce({
				status: 200,
				body: {
					result: { id: '0193a5c0-0000-7000-8000-000000000302', revision: 2, generation: 11 }
				}
			})
			// 5. Another uncertain create followed by clearCaller -> next submit starts fresh.
			.mockResolvedValueOnce({ status: 504, body: { outcome: 'unknown' } })
			.mockResolvedValueOnce({
				status: 200,
				body: {
					result: { id: '0193a5c0-0000-7000-8000-000000000307', revision: 1, generation: 12 }
				}
			});
		const api = createHttpRolesApi({
			http: { read: vi.fn().mockRejectedValue(new Error('offline')), submit },
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) },
			sessionScope: () => 'inst-1:acct-a',
			newCommandId: nextId,
			newEntityId: nextId
		});

		await expect(
			api.create(ORG, { name: 'Reviewer', bodyMarkdown: 'Original body.', attemptId: 'att-edit' })
		).rejects.toMatchObject({ code: 'unavailable' });

		// User edits the body and submits with the same logical attemptId -> replays original create
		// first, then tries to update the committed role (which itself gets 504).
		await expect(
			api.create(ORG, { name: 'Reviewer', bodyMarkdown: 'Edited body.', attemptId: 'att-edit' })
		).rejects.toMatchObject({ code: 'unavailable' });
		expect(submit).toHaveBeenCalledTimes(3);
		expect(submit.mock.calls[0]).toEqual(submit.mock.calls[1]);
		expect(submit.mock.calls[2]?.[0]).toBe(`/api/v1/organizations/${ORG}/roles/update`);
		expect(submit.mock.calls[2]?.[1]).toMatchObject({
			role_id: '0193a5c0-0000-7000-8000-000000000302',
			expected_version: 1,
			body: 'Edited body.'
		});

		// Retrying again does NOT re-call POST /roles; it replays the exact follow-up update command.
		const saved = await api.create(ORG, {
			name: 'Reviewer',
			bodyMarkdown: 'Edited body.',
			attemptId: 'att-edit'
		});
		expect(submit).toHaveBeenCalledTimes(4);
		expect(submit.mock.calls[3]).toEqual(submit.mock.calls[2]);
		expect(saved).toMatchObject({
			id: '0193a5c0-0000-7000-8000-000000000302',
			name: 'Reviewer',
			bodyMarkdown: 'Edited body.',
			revision: 2
		});

		// Explicit sign-out clears abandoned attempts for that caller.
		await expect(
			api.create(ORG, { name: 'Abandoned', bodyMarkdown: 'Body.', attemptId: 'att-signout' })
		).rejects.toMatchObject({ code: 'unavailable' });
		api.clearCaller('inst-1:acct-a');
		const fresh = await api.create(ORG, {
			name: 'Abandoned',
			bodyMarkdown: 'Body.',
			attemptId: 'att-signout'
		});
		expect(fresh.id).toBe('0193a5c0-0000-7000-8000-000000000307');
		expect(submit.mock.calls[5]?.[1]).not.toEqual(submit.mock.calls[4]?.[1]);
	});
});
