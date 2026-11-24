import { describe, expect, it, vi } from 'vitest';
import type { OrgSnapshot } from '$lib/proto';
import { createHttpAccessApi } from './http-api';

const ORG = '0193a5c0-0000-7000-8000-000000000001';
const TEAM = '0193a5c0-0000-7000-8000-000000000002';
const OWNER = '0193a5c0-0000-7000-8000-000000000003';
const SAM = '0193a5c0-0000-7000-8000-000000000004';

const SNAPSHOT: OrgSnapshot = {
	position: { epoch: 1, seq: 1 },
	teams: [{ id: TEAM, name: 'Acme', version: 1 }],
	members: [
		{
			id: OWNER,
			kind: 'person',
			name: 'Alex',
			handle: 'alex',
			home_team_id: TEAM,
			owner: true,
			version: 1
		},
		{ id: SAM, kind: 'person', name: 'Sam', handle: 'sam', home_team_id: TEAM, version: 1 },
		{ id: 'bot', kind: 'agent', name: 'Bot', handle: 'bot', home_team_id: TEAM, version: 1 }
	]
};

describe('createHttpAccessApi', () => {
	it('lists people only (never agents) with live sign-in and pending invite states', async () => {
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
		const api = createHttpAccessApi({
			http: { read, submit: vi.fn() },
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) },
			session: () => ({ accountAdmin: true, login: 'alex' })
		});

		const overview = await api.overview(ORG);
		expect(overview.people.map((p) => [p.memberId, p.state])).toEqual([
			[OWNER, 'signed_in'],
			[SAM, 'invited']
		]);
		expect(overview.administrator).toEqual({
			username: 'alex',
			memberId: OWNER,
			name: 'Alex',
			isYou: true
		});
		expect(overview.viewer.canManage).toBe(true);
	});

	it('keeps viewer.canManage false when GET /access returns 403', async () => {
		const read = vi.fn(async (path: string) => {
			if (path.endsWith('/access')) {
				return { status: 403, body: { error: { code: 'not_authorized', message: 'Forbidden.' } } };
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
		const api = createHttpAccessApi({
			http: { read, submit: vi.fn() },
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) },
			session: () => ({ accountAdmin: true, login: 'alex' })
		});

		const overview = await api.overview(ORG);
		expect(overview.viewer.canManage).toBe(false);
	});
});
