import { describe, expect, it, vi } from 'vitest';
import type { OrgSnapshot } from '$lib/proto';
import { NotDispatchedError } from '$lib/platform/http/errors';
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
			{
				id: ROLE_SWE,
				name: 'Pre-epoch UTC',
				body: '',
				revision: 1,
				created_at: '1969-12-31T23:30:00Z'
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
										updated_at: '9999-12-31T23:59:60Z'
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

		// 1969-12-31T23:30:00-01:00 resolves to 1970-01-01T00:30:00Z (valid in Rust time.rs).
		const apiEpochOffset = createHttpRolesApi({
			http: {
				read: vi.fn(async (path: string) =>
					path.includes('/roles/')
						? {
								status: 200,
								body: {
									result: {
										id: ROLE_SWE,
										name: 'Epoch Offset Role',
										body: '## Mission\nEpoch.\n',
										revision: 1,
										created_at: '1969-12-31T23:30:00-01:00'
									}
								}
							}
						: orgListReply(OWNER)
				),
				submit: vi.fn()
			},
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) }
		});
		const epochOffsetDetail = await apiEpochOffset.role(ORG, ROLE_SWE);
		expect(epochOffsetDetail.role.created.at).toBe('1969-12-31T23:30:00-01:00');

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

	it('never continues a pending create or update under a new account when session changes mid-replay', async () => {
		let seq = 1;
		const nextId = () => `0193a5c0-0000-7000-8000-0000000004${String(seq++).padStart(2, '0')}`;
		let currentCaller = 'inst-1:acct-a';
		let generation = 1;

		const submit = vi
			.fn()
			// 1. Account A create hits 504.
			.mockResolvedValueOnce({ status: 504, body: { outcome: 'unknown' } })
			// 2. Account A edited retry replays original create, but another tab switches to Account B mid-flight.
			.mockImplementationOnce(async () => {
				currentCaller = 'inst-1:acct-b';
				generation++;
				api.clearCaller('inst-1:acct-a');
				throw new Error('Session changed');
			})
			// 3. Account B update hits 504.
			.mockResolvedValueOnce({ status: 504, body: { outcome: 'unknown' } })
			// 4. Account B edited retry replays original update and returns 200 just as session switches back to A.
			.mockImplementationOnce(async () => {
				currentCaller = 'inst-1:acct-a';
				generation++;
				api.clearCaller('inst-1:acct-b');
				return {
					status: 200,
					body: { result: { id: ROLE_SWE, revision: 2, generation: 20 } }
				};
			});

		const api = createHttpRolesApi({
			http: { read: vi.fn().mockRejectedValue(new Error('offline')), submit },
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) },
			sessionScope: () => currentCaller,
			sessionGeneration: () => generation,
			newCommandId: nextId,
			newEntityId: nextId
		});

		await expect(
			api.create(ORG, { name: 'Lead', bodyMarkdown: 'Original.', attemptId: 'att-cross-create' })
		).rejects.toMatchObject({ code: 'unavailable' });

		await expect(
			api.create(ORG, { name: 'Lead', bodyMarkdown: 'Edited.', attemptId: 'att-cross-create' })
		).rejects.toMatchObject({ source: 'api', code: 'not_authorized' });
		// Must NOT fall through and submit a fresh create under Account B!
		expect(submit).toHaveBeenCalledTimes(2);

		await expect(
			api.update(ORG, ROLE_SWE, {
				name: 'SWE',
				bodyMarkdown: 'Edit one.',
				expectedRevision: 1,
				attemptId: 'att-cross-update'
			})
		).rejects.toMatchObject({ code: 'unavailable' });

		await expect(
			api.update(ORG, ROLE_SWE, {
				name: 'SWE',
				bodyMarkdown: 'Edit two.',
				expectedRevision: 1,
				attemptId: 'att-cross-update'
			})
		).rejects.toMatchObject({ source: 'api', code: 'not_authorized' });
		// Must NOT submit the follow-up update under Account A!
		expect(submit).toHaveBeenCalledTimes(4);
	});

	it('discards unsent NotDispatchedError attempts so oversized payloads do not trap subsequent corrected saves', async () => {
		let seq = 1;
		const nextId = () => `0193a5c0-0000-7000-8000-0000000005${String(seq++).padStart(2, '0')}`;
		const submit = vi
			.fn()
			// 1. Fresh create rejected preflight for exceeding 256 KiB.
			.mockRejectedValueOnce(new NotDispatchedError('JSON request exceeds its limit'))
			// 2. Shortened create submits once cleanly without replaying the oversized payload.
			.mockResolvedValueOnce({
				status: 200,
				body: {
					result: { id: '0193a5c0-0000-7000-8000-000000000504', revision: 1, generation: 30 }
				}
			})
			// 3. Fresh update rejected preflight for exceeding 256 KiB.
			.mockRejectedValueOnce(new NotDispatchedError('JSON request exceeds its limit'))
			// 4. Shortened update submits once cleanly.
			.mockResolvedValueOnce({
				status: 200,
				body: { result: { id: ROLE_SWE, revision: 2, generation: 31 } }
			});

		const api = createHttpRolesApi({
			http: { read: vi.fn().mockRejectedValue(new Error('offline')), submit },
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) },
			sessionScope: () => 'inst-1:acct-a',
			sessionGeneration: () => 1,
			// Exercise the production retention callback returning false (open editor has no kept-draft copy).
			isAttemptRetained: () => false,
			newCommandId: nextId,
			newEntityId: nextId
		});

		await expect(
			api.create(ORG, { name: 'Huge', bodyMarkdown: 'x'.repeat(100), attemptId: 'att-huge-c' })
		).rejects.toMatchObject({ source: 'api', code: 'too_large' });

		const created = await api.create(ORG, {
			name: 'Huge',
			bodyMarkdown: 'Short.',
			attemptId: 'att-huge-c'
		});
		expect(submit).toHaveBeenCalledTimes(2);
		expect(submit.mock.calls[1]?.[1]).toMatchObject({ body: 'Short.' });
		expect(created.revision).toBe(1);

		await expect(
			api.update(ORG, ROLE_SWE, {
				name: 'SWE',
				bodyMarkdown: 'y'.repeat(100),
				expectedRevision: 1,
				attemptId: 'att-huge-u'
			})
		).rejects.toMatchObject({ source: 'api', code: 'too_large' });

		const updated = await api.update(ORG, ROLE_SWE, {
			name: 'SWE',
			bodyMarkdown: 'Short edit.',
			expectedRevision: 1,
			attemptId: 'att-huge-u'
		});
		expect(submit).toHaveBeenCalledTimes(4);
		expect(submit.mock.calls[3]?.[1]).toMatchObject({ body: 'Short edit.', expected_version: 1 });
		expect(updated.revision).toBe(2);
	});

	it('reconciles a 504 update even when the user restores the original text', async () => {
		let seq = 1;
		const nextId = () => `0193a5c0-0000-7000-8000-0000000006${String(seq++).padStart(2, '0')}`;
		const submit = vi
			.fn()
			// 1. Update with changed body hits 504 (actually committed on server as rev 2).
			.mockResolvedValueOnce({ status: 504, body: { outcome: 'unknown' } })
			// 2. User restores original text and saves again -> replays rev 1 -> 2 first...
			.mockResolvedValueOnce({
				status: 200,
				body: { result: { id: ROLE_SWE, revision: 2, generation: 40 } }
			})
			// 3. ...then commits the restored original text as rev 3!
			.mockResolvedValueOnce({
				status: 200,
				body: { result: { id: ROLE_SWE, revision: 3, generation: 41 } }
			});

		const api = createHttpRolesApi({
			http: { read: vi.fn().mockRejectedValue(new Error('offline')), submit },
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) },
			sessionScope: () => 'inst-1:acct-a',
			sessionGeneration: () => 1,
			newCommandId: nextId
		});

		await expect(
			api.update(ORG, ROLE_SWE, {
				name: 'Software Engineer',
				bodyMarkdown: 'Temporary draft.',
				expectedRevision: 1,
				attemptId: 'att-restore'
			})
		).rejects.toMatchObject({ code: 'unavailable' });

		const restored = await api.update(ORG, ROLE_SWE, {
			name: 'Software Engineer',
			bodyMarkdown: 'Original text.',
			expectedRevision: 1,
			attemptId: 'att-restore'
		});
		expect(submit).toHaveBeenCalledTimes(3);
		expect(submit.mock.calls[1]).toEqual(submit.mock.calls[0]);
		expect(submit.mock.calls[2]?.[1]).toMatchObject({
			role_id: ROLE_SWE,
			expected_version: 2,
			body: 'Original text.'
		});
		expect(restored).toMatchObject({
			id: ROLE_SWE,
			bodyMarkdown: 'Original text.',
			revision: 3
		});
	});

	it('coalesces and reuses an acknowledged create/update across the readback gap so reopening a draft never duplicates', async () => {
		let seq = 1;
		const nextId = () => `0193a5c0-0000-7000-8000-0000000007${String(seq++).padStart(2, '0')}`;
		let releaseReadback!: () => void;
		const readbackGate = new Promise<void>((resolve) => {
			releaseReadback = resolve;
		});

		const submit = vi
			.fn()
			// 1. Initial create commits as 0702 rev 1.
			.mockResolvedValueOnce({
				status: 200,
				body: {
					result: { id: '0193a5c0-0000-7000-8000-000000000702', revision: 1, generation: 50 }
				}
			})
			// 2. Edited retry against the same attemptId updates 0702 in place to rev 2.
			.mockResolvedValueOnce({
				status: 200,
				body: {
					result: { id: '0193a5c0-0000-7000-8000-000000000702', revision: 2, generation: 51 }
				}
			});

		let readCount = 0;
		const read = vi.fn(async (path: string) => {
			readCount++;
			if (readCount === 1) await readbackGate;
			if (path.includes('/roles/0193a5c0-0000-7000-8000-000000000702')) {
				return {
					status: 200,
					body: {
						result: {
							id: '0193a5c0-0000-7000-8000-000000000702',
							name: 'Analyst',
							body: readCount === 1 ? 'First.' : 'Edited.',
							revision: readCount === 1 ? 1 : 2
						}
					}
				};
			}
			return Promise.reject(new Error('offline'));
		});

		const api = createHttpRolesApi({
			http: { read, submit },
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) },
			sessionScope: () => 'inst-1:acct-a',
			sessionGeneration: () => 1,
			newCommandId: nextId,
			newEntityId: nextId
		});

		const firstCall = api.create(ORG, {
			name: 'Analyst',
			bodyMarkdown: 'First.',
			attemptId: 'att-readback-gap'
		});
		// Allow POST /roles to resolve and enter fetchAuthoritativeRole (which pauses on readbackGate).
		await Promise.resolve();
		await Promise.resolve();
		expect(submit).toHaveBeenCalledTimes(1);

		// Reopened editor submits the SAME attemptId during the readback gap!
		const concurrentCall = api.create(ORG, {
			name: 'Analyst',
			bodyMarkdown: 'First.',
			attemptId: 'att-readback-gap'
		});
		releaseReadback();

		const [firstRole, concurrentRole] = await Promise.all([firstCall, concurrentCall]);
		expect(submit).toHaveBeenCalledTimes(1);
		expect(firstRole.id).toBe('0193a5c0-0000-7000-8000-000000000702');
		expect(concurrentRole).toEqual(firstRole);

		// A post-readback retry with the same attemptId and identical text also reuses the acknowledged result.
		const postReadbackSame = await api.create(ORG, {
			name: 'Analyst',
			bodyMarkdown: 'First.',
			attemptId: 'att-readback-gap'
		});
		expect(submit).toHaveBeenCalledTimes(1);
		expect(postReadbackSame).toEqual(firstRole);

		// A post-readback retry with EDITED text updates the committed role in place rather than posting a duplicate.
		const edited = await api.create(ORG, {
			name: 'Analyst',
			bodyMarkdown: 'Edited.',
			attemptId: 'att-readback-gap'
		});
		expect(submit).toHaveBeenCalledTimes(2);
		expect(submit.mock.calls[1]?.[0]).toBe(`/api/v1/organizations/${ORG}/roles/update`);
		expect(submit.mock.calls[1]?.[1]).toMatchObject({
			role_id: '0193a5c0-0000-7000-8000-000000000702',
			expected_version: 1,
			body: 'Edited.'
		});
		expect(edited.revision).toBe(2);
	});

	it('serializes simultaneous same-attempt creates and multiple edited waiters gated on readback without conflicts', async () => {
		let seq = 1;
		const nextId = () => `0193a5c0-0000-7000-8000-0000000008${String(seq++).padStart(2, '0')}`;
		let releaseReadback!: () => void;
		const readbackGate = new Promise<void>((resolve) => {
			releaseReadback = resolve;
		});

		let rev = 0;
		const submit = vi.fn(async (path: string, body: Record<string, unknown>) => {
			if (path.endsWith('/roles')) {
				rev = 1;
				return { status: 200, body: { result: { id: body.role_id, revision: 1, generation: 60 } } };
			}
			// Enforce expected_version strictly so concurrent unserialized updates would 409!
			if (body.expected_version !== rev) {
				return { status: 409, body: { error: { code: 'conflict', message: 'stale' } } };
			}
			rev += 1;
			return {
				status: 200,
				body: { result: { id: body.role_id, revision: rev, generation: 60 + rev } }
			};
		});
		const read = vi.fn(async () => {
			await readbackGate;
			return Promise.reject(new Error('offline'));
		});

		const api = createHttpRolesApi({
			http: { read, submit },
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) },
			sessionScope: () => 'inst-1:acct-a',
			sessionGeneration: () => 1,
			newCommandId: nextId,
			newEntityId: nextId
		});

		// Fire two simultaneous creates + two edited waiters before any microtask resolves.
		const c1 = api.create(ORG, { name: 'Planner', bodyMarkdown: 'V1.', attemptId: 'att-sim' });
		const c2 = api.create(ORG, { name: 'Planner', bodyMarkdown: 'V1.', attemptId: 'att-sim' });
		const w1 = api.create(ORG, { name: 'Planner', bodyMarkdown: 'V2.', attemptId: 'att-sim' });
		const w2 = api.create(ORG, { name: 'Planner', bodyMarkdown: 'V2.', attemptId: 'att-sim' });
		releaseReadback();

		const [r1, r2, r3, r4] = await Promise.all([c1, c2, w1, w2]);
		expect(r1.id).toBe(r2.id);
		expect(r1.revision).toBe(1);
		expect(r2.revision).toBe(1);
		expect(r3.id).toBe(r1.id);
		expect(r3.revision).toBe(2);
		expect(r4).toEqual(r3);
		// Exactly 1 POST /roles and 1 POST /roles/update across all four concurrent calls!
		expect(submit).toHaveBeenCalledTimes(2);
	});

	it('reconciles a 504 follow-up update instead of returning stale rev 1 from cache when original text is retried', async () => {
		let seq = 1;
		const nextId = () => `0193a5c0-0000-7000-8000-0000000009${String(seq++).padStart(2, '0')}`;
		const submit = vi
			.fn()
			// 1. Create commits rev 1 ("Original.").
			.mockResolvedValueOnce({
				status: 200,
				body: {
					result: { id: '0193a5c0-0000-7000-8000-000000000902', revision: 1, generation: 70 }
				}
			})
			// 2. Follow-up edit ("Edited.") commits rev 2 on server but returns 504.
			.mockResolvedValueOnce({ status: 504, body: { outcome: 'unknown' } })
			// 3. User reverts to "Original." -> must replay the 504 follow-up (returns rev 2)...
			.mockResolvedValueOnce({
				status: 200,
				body: {
					result: { id: '0193a5c0-0000-7000-8000-000000000902', revision: 2, generation: 71 }
				}
			})
			// 4. ...and write "Original." as rev 3!
			.mockResolvedValueOnce({
				status: 200,
				body: {
					result: { id: '0193a5c0-0000-7000-8000-000000000902', revision: 3, generation: 72 }
				}
			});

		const api = createHttpRolesApi({
			http: { read: vi.fn().mockRejectedValue(new Error('offline')), submit },
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) },
			sessionScope: () => 'inst-1:acct-a',
			sessionGeneration: () => 1,
			// Exercise the production retention callback returning false (open editor has no kept-draft copy).
			isAttemptRetained: () => false,
			newCommandId: nextId,
			newEntityId: nextId
		});

		const created = await api.create(ORG, {
			name: 'Editor',
			bodyMarkdown: 'Original.',
			attemptId: 'att-stale-cache'
		});
		expect(created.revision).toBe(1);

		await expect(
			api.create(ORG, { name: 'Editor', bodyMarkdown: 'Edited.', attemptId: 'att-stale-cache' })
		).rejects.toMatchObject({ code: 'unavailable' });

		const reconciled = await api.create(ORG, {
			name: 'Editor',
			bodyMarkdown: 'Original.',
			attemptId: 'att-stale-cache'
		});
		expect(submit).toHaveBeenCalledTimes(4);
		expect(submit.mock.calls[2]).toEqual(submit.mock.calls[1]);
		expect(submit.mock.calls[3]?.[1]).toMatchObject({
			role_id: '0193a5c0-0000-7000-8000-000000000902',
			expected_version: 2,
			body: 'Original.'
		});
		expect(reconciled).toMatchObject({
			id: '0193a5c0-0000-7000-8000-000000000902',
			bodyMarkdown: 'Original.',
			revision: 3
		});
	});

	it('evicts cached role details past 64 saves while preserving retained draft identity so retries never duplicate', async () => {
		let seq = 1;
		const nextId = () => `0193a5c0-0000-7000-8000-${String(seq++).padStart(12, '0')}`;
		const retained = new Set(['cap-1']);
		const submit = vi.fn(async (path: string, body: Record<string, unknown>) => {
			if (path.endsWith('/roles')) {
				return {
					status: 200,
					body: { result: { id: body.role_id, revision: 1, generation: seq } }
				};
			}
			return {
				status: 200,
				body: { result: { id: body.role_id, revision: 2, generation: seq } }
			};
		});
		const read = vi.fn().mockRejectedValue(new Error('offline'));
		const api = createHttpRolesApi({
			http: { read, submit },
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) },
			sessionScope: () => 'inst-1:acct-a',
			sessionGeneration: () => 1,
			isAttemptRetained: (attemptId) => retained.has(attemptId),
			newCommandId: nextId,
			newEntityId: nextId
		});

		const first = await api.create(ORG, {
			name: 'Role 1',
			bodyMarkdown: 'Body.',
			attemptId: 'cap-1'
		});
		for (let i = 2; i <= 65; i++) {
			await api.create(ORG, { name: `Role ${i}`, bodyMarkdown: 'Body.', attemptId: `cap-${i}` });
		}
		expect(submit).toHaveBeenCalledTimes(65);
		const readsAfter65 = read.mock.calls.length;

		// #65 is still cached in detail memory (0 extra read, 0 extra submit).
		await api.create(ORG, { name: 'Role 65', bodyMarkdown: 'Body.', attemptId: 'cap-65' });
		expect(submit).toHaveBeenCalledTimes(65);
		expect(read).toHaveBeenCalledTimes(readsAfter65);

		// #1's heavy resolved detail was evicted (triggers readback), but its entity identity stayed
		// alive because `cap-1` is retained: returns the SAME role ID with ZERO duplicate POST /roles!
		const retriedFirst = await api.create(ORG, {
			name: 'Role 1',
			bodyMarkdown: 'Body.',
			attemptId: 'cap-1'
		});
		expect(submit).toHaveBeenCalledTimes(65);
		expect(read.mock.calls.length).toBeGreaterThan(readsAfter65);
		expect(retriedFirst.id).toBe(first.id);

		// Editing retained `cap-1` after detail eviction updates the existing role in place (no duplicate).
		const editedFirst = await api.create(ORG, {
			name: 'Role 1',
			bodyMarkdown: 'Edited after 65 saves.',
			attemptId: 'cap-1'
		});
		expect(submit).toHaveBeenCalledTimes(66);
		expect(submit.mock.calls[65]?.[0]).toBe(`/api/v1/organizations/${ORG}/roles/update`);
		expect(submit.mock.calls[65]?.[1]).toMatchObject({ role_id: first.id, expected_version: 1 });
		expect(editedFirst.id).toBe(first.id);
		expect(editedFirst.revision).toBe(2);

		// A finalized non-retained attempt (#2) is completely evicted once 64 newer entries settle.
		await api.create(ORG, { name: 'Role 66', bodyMarkdown: 'Body.', attemptId: 'cap-66' });
		const beforeNonRetained = submit.mock.calls.length;
		await api.create(ORG, { name: 'Role 2', bodyMarkdown: 'Body.', attemptId: 'cap-2' });
		expect(submit.mock.calls.length).toBe(beforeNonRetained + 1);

		// Once retained `cap-1` has its detail evicted again and its draft reference is dropped,
		// the next save reclaims the evicted identity entry so discarded draft text is not retained.
		for (let i = 67; i <= 131; i++) {
			await api.create(ORG, { name: `Role ${i}`, bodyMarkdown: 'Body.', attemptId: `cap-${i}` });
		}
		retained.delete('cap-1');
		await api.create(ORG, { name: 'Role 132', bodyMarkdown: 'Body.', attemptId: 'cap-132' });
		const beforeReclaimed = submit.mock.calls.length;
		const freshAfterReclaim = await api.create(ORG, {
			name: 'Role 1',
			bodyMarkdown: 'Body.',
			attemptId: 'cap-1'
		});
		expect(submit.mock.calls.length).toBe(beforeReclaimed + 1);
		expect(freshAfterReclaim.id).not.toBe(first.id);
	});

	it('does not write a redundant rev 2 when a timed-out create is followed by a refused validation edit and then retried with original text', async () => {
		let seq = 1;
		const nextId = () => `0193a5c0-0000-7000-8000-0000000011${String(seq++).padStart(2, '0')}`;
		const submit = vi
			.fn()
			// 1. Initial create times out (committed rev 1 on server, no cached Role object).
			.mockResolvedValueOnce({ status: 504, body: { outcome: 'unknown' } })
			// 2. Edited retry with duplicate key replays create -> confirms rev 1...
			.mockResolvedValueOnce({
				status: 200,
				body: {
					result: { id: '0193a5c0-0000-7000-8000-000000001102', revision: 1, generation: 90 }
				}
			})
			// 3. ...then follow-up update is deterministically refused (already_exists key).
			.mockResolvedValueOnce({
				status: 409,
				body: { error: { code: 'already_exists', message: 'a role with that key already exists' } }
			});

		const api = createHttpRolesApi({
			http: { read: vi.fn().mockRejectedValue(new Error('offline')), submit },
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) },
			sessionScope: () => 'inst-1:acct-a',
			sessionGeneration: () => 1,
			// Exercise the production retention callback returning false (open editor has no kept-draft copy).
			isAttemptRetained: () => false,
			newCommandId: nextId,
			newEntityId: nextId
		});

		await expect(
			api.create(ORG, { name: 'SRE', bodyMarkdown: 'Original.', attemptId: 'att-val-refusal' })
		).rejects.toMatchObject({ code: 'unavailable' });

		await expect(
			api.create(ORG, {
				name: 'SRE',
				key: 'software-engineer',
				bodyMarkdown: 'Original.',
				attemptId: 'att-val-refusal'
			})
		).rejects.toMatchObject({ code: 'already_exists', field: 'key' });

		// Retrying original text returns rev 1 directly with ZERO redundant POST /roles/update!
		const restored = await api.create(ORG, {
			name: 'SRE',
			bodyMarkdown: 'Original.',
			attemptId: 'att-val-refusal'
		});
		expect(submit).toHaveBeenCalledTimes(3);
		expect(restored).toMatchObject({
			id: '0193a5c0-0000-7000-8000-000000001102',
			name: 'SRE',
			bodyMarkdown: 'Original.',
			revision: 1
		});
	});

	it('never silently adopts a conflict revision or restores stale cached success on conflict', async () => {
		let seq = 1;
		const nextId = () => `0193a5c0-0000-7000-8000-0000000010${String(seq++).padStart(2, '0')}`;
		let serverRev = 0;
		let readbackFails = false;
		let createdId = '';

		const submit = vi.fn(async (path: string, body: Record<string, unknown>) => {
			if (path.endsWith('/roles')) {
				serverRev = 1;
				createdId = String(body.role_id);
				return { status: 200, body: { result: { id: createdId, revision: 1, generation: 80 } } };
			}
			if (body.expected_version !== serverRev) {
				return {
					status: 409,
					body: { error: { code: 'conflict', message: 'changed by another editor' } }
				};
			}
			serverRev += 1;
			return {
				status: 200,
				body: { result: { id: createdId, revision: serverRev, generation: 80 + serverRev } }
			};
		});
		const read = vi.fn(async () => {
			if (readbackFails) throw new Error('offline');
			return {
				status: 200,
				body: {
					result: {
						id: createdId,
						name: 'Role',
						body: 'Independent edit at rev 3.',
						revision: serverRev
					}
				}
			};
		});

		const api = createHttpRolesApi({
			http: { read, submit },
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) },
			sessionScope: () => 'inst-1:acct-a',
			sessionGeneration: () => 1,
			newCommandId: nextId,
			newEntityId: nextId
		});

		// Create rev 1.
		const rev1 = await api.create(ORG, {
			name: 'Role',
			bodyMarkdown: 'V1.',
			attemptId: 'att-conflict'
		});
		expect(rev1.revision).toBe(1);

		// Another editor independently advances the role to rev 3.
		serverRev = 3;

		// Retry 1 with edited text conflicts against rev 3 (conflict readback succeeds).
		const err1 = await api
			.create(ORG, { name: 'Role', bodyMarkdown: 'My edit.', attemptId: 'att-conflict' })
			.catch((e) => e);
		expect(err1).toMatchObject({ code: 'conflict' });
		expect(conflictLatest(err1)?.revision).toBe(3);

		// Retry 2 without explicit rebase must STILL send expected_version: 1 and conflict (NOT overwrite rev 3!).
		readbackFails = true;
		await expect(
			api.create(ORG, { name: 'Role', bodyMarkdown: 'My edit.', attemptId: 'att-conflict' })
		).rejects.toMatchObject({ code: 'conflict' });
		expect(serverRev).toBe(3);

		// Even after conflict with failed readback, retrying original text must NOT return stale rev 1 cache.
		await expect(
			api.create(ORG, { name: 'Role', bodyMarkdown: 'V1.', attemptId: 'att-conflict' })
		).rejects.toMatchObject({ code: 'conflict' });
		expect(serverRev).toBe(3);
	});

	it('does not retain an obsolete pre-401 draft when no earlier request may have committed', async () => {
		let seq = 1;
		const nextId = () => `0193a5c0-0000-7000-8000-0000000012${String(seq++).padStart(2, '0')}`;
		const submit = vi
			.fn()
			// 1. Initial create refused with 401 (definitely not admitted).
			.mockResolvedValueOnce({
				status: 401,
				body: { error: { code: 'not_authorized', message: 'Sign in again.' } }
			})
			// 2. After same-account reauth, user saves revised draft with a duplicate key -> refused directly
			//    without ever committing the obsolete pre-401 payload!
			.mockResolvedValueOnce({
				status: 409,
				body: { error: { code: 'already_exists', message: 'a role with that key already exists' } }
			})
			// 3. Initial update refused with 401 (definitely not admitted).
			.mockResolvedValueOnce({
				status: 401,
				body: { error: { code: 'not_authorized', message: 'Sign in again.' } }
			})
			// 4. After same-account reauth, revised update submits ONLY the revised body directly.
			.mockResolvedValueOnce({
				status: 200,
				body: { result: { id: ROLE_SWE, revision: 2, generation: 95 } }
			});

		const api = createHttpRolesApi({
			http: { read: vi.fn().mockRejectedValue(new Error('offline')), submit },
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) },
			sessionScope: () => 'inst-1:acct-a',
			sessionGeneration: () => 1,
			isAttemptRetained: () => false,
			newCommandId: nextId,
			newEntityId: nextId
		});

		await expect(
			api.create(ORG, {
				name: 'Obsolete',
				bodyMarkdown: 'Old text.',
				attemptId: 'att-401-create'
			})
		).rejects.toMatchObject({ code: 'not_authorized' });

		await expect(
			api.create(ORG, {
				name: 'Revised',
				key: 'software-engineer',
				bodyMarkdown: 'New text.',
				attemptId: 'att-401-create'
			})
		).rejects.toMatchObject({ code: 'already_exists', field: 'key' });
		expect(submit).toHaveBeenCalledTimes(2);
		expect(submit.mock.calls[1]?.[1]).toMatchObject({
			name: 'Revised',
			key: 'software-engineer',
			body: 'New text.'
		});

		await expect(
			api.update(ORG, ROLE_SWE, {
				name: 'Obsolete Edit',
				bodyMarkdown: 'Old edit.',
				expectedRevision: 1,
				attemptId: 'att-401-update'
			})
		).rejects.toMatchObject({ code: 'not_authorized' });

		const updated = await api.update(ORG, ROLE_SWE, {
			name: 'Revised Edit',
			bodyMarkdown: 'New edit.',
			expectedRevision: 1,
			attemptId: 'att-401-update'
		});
		expect(submit).toHaveBeenCalledTimes(4);
		expect(submit.mock.calls[3]?.[1]).toMatchObject({
			role_id: ROLE_SWE,
			expected_version: 1,
			name: 'Revised Edit',
			body: 'New edit.'
		});
		expect(updated.revision).toBe(2);
	});

	it('clears unresolved follow-up when a revised follow-up gets initial 401 so restoring confirmed text after reauth writes no extra revision', async () => {
		let seq = 1;
		const nextId = () => `0193a5c0-0000-7000-8000-0000000013${String(seq++).padStart(2, '0')}`;
		const submit = vi
			.fn()
			// Create flow: 1. initial create 504 -> 2. replay confirms rev 1 -> 3. revised follow-up gets initial 401.
			.mockResolvedValueOnce({ status: 504, body: { outcome: 'unknown' } })
			.mockResolvedValueOnce({
				status: 200,
				body: {
					result: { id: '0193a5c0-0000-7000-8000-000000001302', revision: 1, generation: 96 }
				}
			})
			.mockResolvedValueOnce({
				status: 401,
				body: { error: { code: 'not_authorized', message: 'Sign in again.' } }
			})
			// Update flow: 4. initial update 504 -> 5. replay confirms rev 2 -> 6. revised follow-up gets initial 401.
			.mockResolvedValueOnce({ status: 504, body: { outcome: 'unknown' } })
			.mockResolvedValueOnce({
				status: 200,
				body: { result: { id: ROLE_SWE, revision: 2, generation: 97 } }
			})
			.mockResolvedValueOnce({
				status: 401,
				body: { error: { code: 'not_authorized', message: 'Sign in again.' } }
			});

		const api = createHttpRolesApi({
			http: { read: vi.fn().mockRejectedValue(new Error('offline')), submit },
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) },
			sessionScope: () => 'inst-1:acct-a',
			sessionGeneration: () => 1,
			isAttemptRetained: () => false,
			newCommandId: nextId,
			newEntityId: nextId
		});

		await expect(
			api.create(ORG, { name: 'SecOps', bodyMarkdown: 'Original.', attemptId: 'att-401-fup-c' })
		).rejects.toMatchObject({ code: 'unavailable' });

		await expect(
			api.create(ORG, { name: 'SecOps', bodyMarkdown: 'Revised.', attemptId: 'att-401-fup-c' })
		).rejects.toMatchObject({ code: 'not_authorized' });

		// After same-account reauth, restoring the confirmed text returns rev 1 with ZERO extra update!
		const createRestored = await api.create(ORG, {
			name: 'SecOps',
			bodyMarkdown: 'Original.',
			attemptId: 'att-401-fup-c'
		});
		expect(submit).toHaveBeenCalledTimes(3);
		expect(createRestored).toMatchObject({
			id: '0193a5c0-0000-7000-8000-000000001302',
			bodyMarkdown: 'Original.',
			revision: 1
		});

		await expect(
			api.update(ORG, ROLE_SWE, {
				name: 'SWE',
				bodyMarkdown: 'Confirmed edit.',
				expectedRevision: 1,
				attemptId: 'att-401-fup-u'
			})
		).rejects.toMatchObject({ code: 'unavailable' });

		await expect(
			api.update(ORG, ROLE_SWE, {
				name: 'SWE',
				bodyMarkdown: 'Revised edit.',
				expectedRevision: 1,
				attemptId: 'att-401-fup-u'
			})
		).rejects.toMatchObject({ code: 'not_authorized' });

		const updateRestored = await api.update(ORG, ROLE_SWE, {
			name: 'SWE',
			bodyMarkdown: 'Confirmed edit.',
			expectedRevision: 1,
			attemptId: 'att-401-fup-u'
		});
		expect(submit).toHaveBeenCalledTimes(6);
		expect(updateRestored).toMatchObject({
			id: ROLE_SWE,
			bodyMarkdown: 'Confirmed edit.',
			revision: 2
		});
	});
});
