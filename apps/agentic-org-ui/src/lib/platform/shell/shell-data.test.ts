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
});
