import { describe, expect, it, vi } from 'vitest';
import type { OrgSnapshot } from '$lib/proto';
import { homeApi } from './api';
import { createHttpHomeApi } from './http-api';

const ORG = '0193a5c0-0000-7000-8000-000000000001';

const SNAPSHOT: OrgSnapshot = {
	position: { epoch: 1, seq: 5 },
	teams: [
		{ id: 't-plat', name: 'Platform', version: 1 },
		{ id: 't-web', name: 'Web', version: 1 }
	],
	members: [
		{ id: 'm-owner', kind: 'person', name: 'Owner', owner: true, version: 1 },
		{
			id: 'm-kai',
			kind: 'agent',
			name: 'Kai Lindqvist',
			home_team_id: 't-plat',
			created_at: '2026-11-20T09:00:00Z',
			agent: {
				role: 'Engineer',
				title: 'Staff Engineer',
				runtime: 'claude_code',
				messaging_name: 'kai',
				status: 'active'
			},
			version: 1
		},
		{
			id: 'm-paused',
			kind: 'agent',
			name: 'Pax Moreau',
			home_team_id: 't-plat',
			agent: {
				role: 'Reviewer',
				title: 'Reviewer',
				runtime: 'codex',
				messaging_name: 'pax',
				status: 'paused'
			},
			version: 1
		},
		{
			id: 'm-offboarded',
			kind: 'agent',
			name: 'Former Agent',
			home_team_id: 't-plat',
			agent: {
				role: 'Engineer',
				title: 'Engineer',
				runtime: 'codex',
				messaging_name: 'former',
				status: 'offboarded'
			},
			version: 1
		},
		{
			id: 'm-person',
			kind: 'person',
			name: 'Avery Stone',
			home_team_id: 't-web',
			version: 1
		}
	],
	projects: [
		{ id: 'p-beacon', key: 'BCN', name: 'Beacon', version: 1 },
		{ id: 'p-atlas', key: 'ATL', name: 'Atlas', version: 1 },
		{ id: 'p-old', key: 'OLD', name: 'Legacy', archived: true, version: 1 }
	],
	project_teams: [
		{ id: 'pt-1', project_id: 'p-atlas', team_id: 't-plat', version: 1 },
		{ id: 'pt-2', project_id: 'p-beacon', team_id: 't-web', version: 1 }
	]
};

describe('createHttpHomeApi', () => {
	it('maps live non-archived projects with active member counts, active working agents, and null month', async () => {
		const orgApi = {
			view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false })
		};
		const api = createHttpHomeApi({ orgApi, now: () => '2026-11-21T12:00:00Z' });

		const projects = await api.projects(ORG);
		expect(projects).toEqual([
			{ id: 'p-atlas', name: 'Atlas', done: 0, total: 0, open: 0, blocked: 0, members: 2 },
			{ id: 'p-beacon', name: 'Beacon', done: 0, total: 0, open: 0, blocked: 0, members: 1 }
		]);

		const working = await api.workingNow(ORG);
		expect(working).toEqual({
			live: true,
			agents: [{ member: { id: 'm-kai', name: 'Kai Lindqvist' }, since: '2026-11-20T09:00:00Z' }]
		});

		expect(await api.month(ORG)).toBeNull();
	});

	it('fails closed when no signed-in session exists', async () => {
		await expect(homeApi.projects(ORG)).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable'
		});
		await expect(homeApi.workingNow(ORG)).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable'
		});
		await expect(homeApi.month(ORG)).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable'
		});
	});
});
