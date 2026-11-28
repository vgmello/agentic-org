import { afterEach, describe, expect, it, vi } from 'vitest';
import type { InboxItem } from '$lib/modules/inbox/types';
import type { OfficeInfo } from '$lib/proto';

const orgView = vi.fn();
const workSnapshot = vi.fn();
const inboxList = vi.fn();

vi.mock('$lib/modules/organization/api', () => ({
	orgApi: {
		view: (...args: unknown[]) => orgView(...args)
	}
}));
vi.mock('$lib/modules/work/client', () => ({
	workApi: {
		snapshot: (...args: unknown[]) => workSnapshot(...args)
	}
}));
vi.mock('$lib/modules/inbox/api', () => ({
	inboxApi: {
		list: (...args: unknown[]) => inboxList(...args)
	}
}));

import { ShellData } from './shell-data.svelte';

const baseOffices: OfficeInfo[] = [
	{
		id: 'office-1',
		organization_id: 'org-1',
		name: 'HQ',
		role: 'hq',
		host: 'This Mac',
		status: 'active'
	}
];

afterEach(() => {
	vi.clearAllMocks();
});

describe('ShellData inboxLoadedOrganizationId', () => {
	it('stays undefined when inboxApi.list rejects, sets on fulfillment, and clears on reset and dispose', async () => {
		orgView.mockResolvedValue({ snapshot: { members: [], teams: [], projects: [] } });
		workSnapshot.mockResolvedValue({ items: [] });
		inboxList.mockRejectedValueOnce(new Error('Inbox unavailable'));

		let currentOrg = 'org-1';
		const shell = new ShellData({
			getOrganizationId: () => currentOrg,
			getBaseOffices: () => baseOffices,
			loadOffices: async (_id, offices) => offices
		});

		const rejectedResult = await shell.load('org-1', true).promise;
		expect(rejectedResult).toEqual({ inboxUpdated: false, workUpdated: true });
		expect(shell.loaded).toBe(true);
		expect(shell.loadedOrganizationId).toBe('org-1');
		expect(shell.inboxLoadedOrganizationId).toBeUndefined();

		const inboxRows = [{ id: 'inbox-1', state: 'open' }] as unknown as InboxItem[];
		inboxList.mockResolvedValueOnce(inboxRows);
		const fulfilledResult = await shell.refresh();
		expect(fulfilledResult).toEqual({ inboxUpdated: true, workUpdated: true });
		expect(shell.inboxLoadedOrganizationId).toBe('org-1');
		expect(shell.inboxItems).toEqual(inboxRows);

		currentOrg = 'org-2';
		inboxList.mockRejectedValueOnce(new Error('Org 2 inbox unavailable'));
		const switchedResult = await shell.load('org-2', true).promise;
		expect(switchedResult).toEqual({ inboxUpdated: false, workUpdated: true });
		expect(shell.loadedOrganizationId).toBe('org-2');
		expect(shell.inboxLoadedOrganizationId).toBeUndefined();

		inboxList.mockResolvedValueOnce([]);
		await shell.refresh();
		expect(shell.inboxLoadedOrganizationId).toBe('org-2');

		shell.dispose();
		expect(shell.inboxLoadedOrganizationId).toBeUndefined();
	});

	it('forwards AbortSignal to all 4 shell loaders and aborts superseded or cancelled loads', async () => {
		const orgSignals: (AbortSignal | undefined)[] = [];
		const workSignals: (AbortSignal | undefined)[] = [];
		const inboxSignals: (AbortSignal | undefined)[] = [];
		const officeSignals: (AbortSignal | undefined)[] = [];

		orgView.mockImplementation((_id: string, signal?: AbortSignal) => {
			orgSignals.push(signal);
			return Promise.resolve({ snapshot: { members: [], teams: [], projects: [] } });
		});
		workSnapshot.mockImplementation(
			(_id: string, _p?: string, _c?: string, signal?: AbortSignal) => {
				workSignals.push(signal);
				return Promise.resolve({ items: [] });
			}
		);
		inboxList.mockImplementation((_id: string, signal?: AbortSignal) => {
			inboxSignals.push(signal);
			return Promise.resolve([]);
		});

		const shell = new ShellData({
			getOrganizationId: () => 'org-1',
			getBaseOffices: () => baseOffices,
			loadOffices: async (_id, offices, signal) => {
				officeSignals.push(signal);
				return offices;
			}
		});

		const first = shell.load('org-1', true);
		await Promise.resolve();
		expect(orgSignals[0]?.aborted).toBe(false);
		expect(workSignals[0]?.aborted).toBe(false);
		expect(inboxSignals[0]?.aborted).toBe(false);
		expect(officeSignals[0]?.aborted).toBe(false);

		const second = shell.load('org-1', false);
		await Promise.resolve();
		expect(orgSignals[0]?.aborted).toBe(true);
		expect(workSignals[0]?.aborted).toBe(true);
		expect(inboxSignals[0]?.aborted).toBe(true);
		expect(officeSignals[0]?.aborted).toBe(true);
		expect(orgSignals[1]?.aborted).toBe(false);

		await Promise.all([first.promise, second.promise]);

		const third = shell.load('org-1', false);
		third.cancel();
		expect(orgSignals[2]?.aborted).toBe(true);
		await third.promise;
	});

	it('stores frozen projectHealthById overlays, filters stale team IDs, ignores wrong org/post-dispose, and clears on reset', async () => {
		orgView.mockResolvedValue({
			snapshot: {
				members: [],
				teams: [{ id: 'team-1', name: 'Platform', version: 1 }],
				projects: [{ id: 'p-1', name: 'Federation' }],
				project_teams: [
					{ id: 'pt-valid', project_id: 'p-1', team_id: 'team-1', version: 1 },
					{ id: 'pt-stale', project_id: 'p-1', team_id: 'team-deleted', version: 1 }
				]
			}
		});
		workSnapshot.mockResolvedValue({ items: [] });
		inboxList.mockResolvedValue([]);

		let currentOrg = 'org-1';
		const shell = new ShellData({
			getOrganizationId: () => currentOrg,
			getBaseOffices: () => baseOffices,
			loadOffices: async (_id, offices) => offices
		});

		await shell.load('org-1', true).promise;

		// validProjectTeams strips dangling project_teams entries not present in snapshot.teams.
		expect(shell.validProjectTeams).toEqual([
			{ id: 'pt-valid', project_id: 'p-1', team_id: 'team-1', version: 1 }
		]);

		// Wrong organization updates are ignored.
		shell.setProjectHealthOverlays('org-other', {
			'p-1': { due: '2026-11-20', blockedCount: 2, rowTeamIds: ['team-1'] }
		});
		expect(shell.projectHealthById).toEqual({});

		// Active org overlays are frozen and stale rowTeamIds are filtered against snapshot.teams.
		shell.setProjectHealthOverlays('org-1', {
			'p-1': {
				due: '2026-11-20',
				blockedCount: 1,
				criticalBugs: 2,
				rowTeamIds: ['team-1', 'team-deleted']
			}
		});
		expect(Object.isFrozen(shell.projectHealthById)).toBe(true);
		expect(Object.isFrozen(shell.projectHealthById['p-1'])).toBe(true);
		expect(shell.projectHealthById['p-1']).toEqual({
			due: '2026-11-20',
			blockedCount: 1,
			criticalBugs: 2,
			rowTeamIds: ['team-1']
		});

		// Switching org with reset=true clears stale overlays.
		currentOrg = 'org-2';
		await shell.load('org-2', true).promise;
		expect(shell.projectHealthById).toEqual({});

		// Updates after dispose() are ignored.
		shell.dispose();
		shell.setProjectHealthOverlays('org-2', { 'p-1': { due: '2026-12-01' } });
		expect(shell.projectHealthById).toEqual({});
	});
});
