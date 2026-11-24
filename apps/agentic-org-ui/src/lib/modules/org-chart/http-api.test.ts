import { describe, expect, it, vi } from 'vitest';
import type { OrgSnapshot } from '$lib/proto';
import { createHttpOrgChartApi } from './http-api';

const ORG = '0193a5c0-0000-7000-8000-000000000001';
const TEAM = '0193a5c0-0000-7000-8000-000000000002';
const OWNER = '0193a5c0-0000-7000-8000-000000000003';
const SAM = '0193a5c0-0000-7000-8000-000000000004';

const SNAPSHOT: OrgSnapshot = {
	position: { epoch: 1, seq: 1 },
	teams: [{ id: TEAM, name: 'Acme', lead_member_id: OWNER, version: 1 }],
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
		{ id: SAM, kind: 'person', name: 'Sam', handle: 'sam', home_team_id: TEAM, version: 4 },
		{
			id: 'former',
			kind: 'agent',
			name: 'Former',
			handle: 'former',
			agent: {
				role: 'Engineer',
				title: 'Engineer',
				runtime: 'codex',
				messaging_name: 'former',
				status: 'offboarded'
			},
			version: 2
		}
	]
};

describe('createHttpOrgChartApi', () => {
	it('returns active employees with revisions and submits changeManager to member.set_reports_to', async () => {
		const command = vi.fn().mockResolvedValue({ status: 'committed' });
		const api = createHttpOrgChartApi({
			http: {
				read: vi.fn().mockResolvedValue({
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
				}),
				submit: vi.fn()
			},
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }), command },
			membersApi: {
				directory: vi.fn().mockResolvedValue([
					{
						id: OWNER,
						kind: 'person',
						name: 'Alex',
						handle: 'alex',
						status: 'active',
						owner: true,
						canSignIn: true,
						directReports: 0
					},
					{
						id: SAM,
						kind: 'person',
						name: 'Sam',
						handle: 'sam',
						status: 'active',
						owner: false,
						canSignIn: false,
						directReports: 0
					}
				])
			}
		});

		const chart = await api.chart(ORG);
		expect(chart.organization).toEqual({ id: ORG, name: 'Acme' });
		expect(chart.ownerId).toBe(OWNER);
		expect(chart.viewerId).toBe(OWNER);
		expect(chart.employees.map((e) => e.id)).toEqual([OWNER, SAM]);
		expect(chart.employees[1].revision).toBe(4);

		await api.changeManager(ORG, SAM, OWNER, 4);
		expect(command).toHaveBeenCalledWith(ORG, {
			method: 'member.set_reports_to',
			params: { member_id: SAM, expected_version: 4, reports_to_member_id: OWNER }
		});

		await api.changeManager(ORG, SAM, null, 5);
		expect(command).toHaveBeenCalledWith(ORG, {
			method: 'member.set_reports_to',
			params: { member_id: SAM, expected_version: 5 }
		});
	});
});
