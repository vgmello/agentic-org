import { afterEach, describe, expect, it, vi } from 'vitest';
import type { InboxItem } from '$lib/modules/inbox/types';
import type { OfficeInfo, WorkItemSummary } from '$lib/proto';

const untrackSpy = vi.fn(<T>(fn: () => T): T => fn());
const workSnapshot = vi.fn();
const inboxList = vi.fn();

vi.mock('svelte', async (importOriginal) => {
	const actual = await importOriginal<typeof import('svelte')>();
	return {
		...actual,
		untrack: <T>(fn: () => T) => untrackSpy(fn)
	};
});
vi.mock('$lib/modules/work', () => ({
	workApi: {
		snapshot: (...args: unknown[]) => workSnapshot(...args)
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
vi.mock('$lib/modules/offices/office-api', async (importOriginal) => {
	const actual = await importOriginal<typeof import('$lib/modules/offices/office-api')>();
	return {
		...actual,
		officeApi: {
			peekOrganizations: vi.fn().mockResolvedValue([])
		}
	};
});
vi.mock('$lib/modules/preferences/account-zone.svelte', () => ({
	accountZone: {
		zone: 'UTC',
		ensure: vi.fn().mockResolvedValue(undefined)
	}
}));
vi.mock('$lib/modules/costs/api', () => ({
	costsApi: {
		organizationCost: vi.fn().mockResolvedValue(undefined),
		agentBudgets: vi.fn().mockResolvedValue(undefined)
	}
}));
vi.mock('$lib/modules/members/api', () => ({
	membersApi: {
		directory: vi.fn().mockResolvedValue(undefined)
	}
}));
vi.mock('$lib/modules/projects/api', () => ({
	projectsApi: {
		list: vi.fn().mockResolvedValue({ projects: [] })
	}
}));
vi.mock('$lib/modules/home/api', () => ({
	homeApi: {
		projects: vi.fn().mockResolvedValue(undefined)
	}
}));
vi.mock('$lib/modules/activity/api', () => ({
	activityApi: {
		feed: vi.fn().mockResolvedValue({ rows: [] })
	}
}));

import { HomeLiveState } from './home-live.svelte';

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

const flushMicrotasks = async () => {
	for (let i = 0; i < 6; i++) await Promise.resolve();
};

afterEach(() => {
	vi.clearAllMocks();
});

describe('HomeLiveState', () => {
	it('routes transient unavailable errors to retryable loadError without masking behind seeded items', async () => {
		workSnapshot.mockRejectedValueOnce({
			source: 'api',
			code: 'unavailable',
			message: 'Work service temporarily unavailable.'
		});
		inboxList.mockResolvedValue([]);

		const live = new HomeLiveState({
			overview: vi.fn().mockResolvedValue(undefined),
			layer: (base) => base
		});

		const cancel = live.connect('org-1', baseOffices, {
			items: [] as WorkItemSummary[],
			inboxItems: [],
			offices: baseOffices
		});

		try {
			expect(live.loadedOrganizationId).toBe('org-1');
			expect(live.workFromSeed).toBe(true);
			expect(live.items).toEqual([]);
			expect(untrackSpy).toHaveBeenCalledTimes(1);

			await flushMicrotasks();

			expect(workSnapshot).toHaveBeenCalledTimes(1);
			expect(live.loadedOrganizationId).toBe('org-1');
			expect(live.loadError).toBe('Work service temporarily unavailable.');
			expect(live.items).toBeUndefined();
			expect(live.workFromSeed).toBe(false);
			expect(live.retryingWork).toBe(false);
			expect(live.allowShellFallback).toBe(false);

			// Explicit Retry stays in retryingWork loading state until settled, and routes another unavailable error to loadError.
			let rejectRetry!: (reason: unknown) => void;
			workSnapshot.mockImplementationOnce(
				() =>
					new Promise((_resolve, reject) => {
						rejectRetry = reject;
					})
			);
			const retryFail = live.load('org-1', baseOffices);
			expect(live.loadError).toBeUndefined();
			expect(live.retryingWork).toBe(true);
			expect(live.items).toBeUndefined();
			expect(live.allowShellFallback).toBe(false);

			await flushMicrotasks();
			rejectRetry({
				source: 'api',
				code: 'unavailable',
				message: 'Still unavailable.'
			});
			await retryFail;

			expect(workSnapshot).toHaveBeenCalledTimes(2);
			expect(live.retryingWork).toBe(false);
			expect(live.items).toBeUndefined();
			expect(live.loadError).toBe('Still unavailable.');

			// A subsequent Retry that succeeds clears loadError and populates live items.
			workSnapshot.mockResolvedValueOnce({ items: [] });
			await live.load('org-1', baseOffices);

			expect(workSnapshot).toHaveBeenCalledTimes(3);
			expect(live.retryingWork).toBe(false);
			expect(live.items).toEqual([]);
			expect(live.workFromSeed).toBe(false);
			expect(live.loadError).toBeUndefined();
		} finally {
			cancel();
			live.destroy();
		}
	});

	it('treats unsupported Work API as an empty live list without loadError', async () => {
		workSnapshot.mockRejectedValueOnce({
			source: 'api',
			code: 'unsupported',
			message: 'Work is not built on this server.'
		});
		inboxList.mockResolvedValue([]);

		const live = new HomeLiveState({
			overview: vi.fn().mockResolvedValue(undefined),
			layer: (base) => base
		});

		const cancel = live.connect('org-1', baseOffices);

		try {
			await flushMicrotasks();

			expect(workSnapshot).toHaveBeenCalledTimes(1);
			expect(live.loadError).toBeUndefined();
			expect(live.items).toEqual([]);
			expect(live.workFromSeed).toBe(false);
			expect(live.allowShellFallback).toBe(false);
		} finally {
			cancel();
			live.destroy();
		}
	});

	it('tracks inboxSettled independently of inboxLoaded and preserves seeded Inbox items on rejection', async () => {
		const seededInbox = [{ id: 'inbox-1', state: 'open' }] as unknown as InboxItem[];
		let rejectInbox!: (reason: unknown) => void;
		workSnapshot.mockResolvedValue({ items: [] });
		inboxList.mockImplementationOnce(
			() =>
				new Promise((_resolve, reject) => {
					rejectInbox = reject;
				})
		);

		const live = new HomeLiveState({
			overview: vi.fn().mockResolvedValue(undefined),
			layer: (base) => base
		});

		const cancelSeeded = live.connect('org-1', baseOffices, {
			items: [],
			inboxItems: seededInbox,
			offices: baseOffices
		});

		try {
			expect(live.inboxSettled).toBe(false);
			expect(live.inboxLoaded).toBe(false);
			rejectInbox(new Error('Inbox unreachable'));
			await flushMicrotasks();
			expect(live.inboxSettled).toBe(true);
			expect(live.inboxLoaded).toBe(false);
			expect(live.inboxItems).toEqual(seededInbox);
		} finally {
			cancelSeeded();
		}

		inboxList.mockRejectedValueOnce(new Error('Inbox unreachable'));
		const cancelCold = live.connect('org-2', baseOffices);
		try {
			expect(live.inboxSettled).toBe(false);
			await flushMicrotasks();
			expect(live.inboxSettled).toBe(true);
			expect(live.inboxLoaded).toBe(false);
			expect(live.inboxItems).toEqual([]);
		} finally {
			cancelCold();
			live.destroy();
		}
	});

	it('forwards an AbortSignal to work, inbox, and offices loads and aborts on cancel, overlap, and destroy', async () => {
		const workSignals: AbortSignal[] = [];
		const inboxSignals: AbortSignal[] = [];
		const officeSignals: AbortSignal[] = [];

		workSnapshot.mockImplementation(
			(_orgId: string, _a?: unknown, _b?: unknown, signal?: AbortSignal) => {
				if (signal) workSignals.push(signal);
				return new Promise(() => {});
			}
		);
		inboxList.mockImplementation((_orgId: string, signal?: AbortSignal) => {
			if (signal) inboxSignals.push(signal);
			return new Promise(() => {});
		});
		const overview = vi.fn((_orgId: string, signal?: AbortSignal) => {
			if (signal) officeSignals.push(signal);
			return new Promise(() => {});
		});

		const live = new HomeLiveState({ overview, layer: (base) => base });
		const cancelFirst = live.connect('org-1', baseOffices);
		await flushMicrotasks();

		expect(workSignals).toHaveLength(1);
		expect(inboxSignals).toHaveLength(1);
		expect(officeSignals).toHaveLength(1);
		expect(workSignals[0].aborted).toBe(false);
		expect(inboxSignals[0].aborted).toBe(false);
		expect(officeSignals[0].aborted).toBe(false);

		cancelFirst();
		expect(workSignals[0].aborted).toBe(true);
		expect(inboxSignals[0].aborted).toBe(true);
		expect(officeSignals[0].aborted).toBe(true);

		void live.load('org-1', baseOffices);
		await flushMicrotasks();
		expect(workSignals).toHaveLength(2);
		expect(workSignals[1].aborted).toBe(false);

		void live.load('org-2', baseOffices);
		await flushMicrotasks();
		expect(workSignals[1].aborted).toBe(true);
		expect(inboxSignals[1].aborted).toBe(true);
		expect(officeSignals[1].aborted).toBe(true);
		expect(workSignals[2].aborted).toBe(false);

		live.destroy();
		expect(workSignals[2].aborted).toBe(true);
		expect(inboxSignals[2].aborted).toBe(true);
		expect(officeSignals[2].aborted).toBe(true);

		// Post-destroy load/connect calls short-circuit without firing new requests.
		await live.load('org-3', baseOffices);
		live.connect('org-3', baseOffices);
		await flushMicrotasks();
		expect(workSignals).toHaveLength(3);
		expect(inboxSignals).toHaveLength(3);
		expect(officeSignals).toHaveLength(3);
	});
});
