import { afterEach, describe, expect, it, vi } from 'vitest';
import type { OfficeInfo } from '$lib/proto';

const probeCallbacks: ((next: 'checking' | 'up' | 'down', caps?: string[]) => void)[] = [];
const peekOrganizations = vi.fn<() => Promise<{ id: string; offices: OfficeInfo[] }[]>>();
const onOfficeStatus = vi.fn<() => Promise<() => void>>();

vi.mock('$lib/modules/offices/office-api', () => ({
	officeApi: {
		peekOrganizations: (...args: []) => peekOrganizations(...args),
		onOfficeStatus: (...args: []) => onOfficeStatus(...args)
	}
}));

vi.mock('$lib/modules/offices/probe', () => ({
	createProbe: (_api: unknown, cb: (next: 'checking' | 'up' | 'down', caps?: string[]) => void) => {
		probeCallbacks.push(cb);
		return { run: vi.fn(() => Promise.resolve()) };
	}
}));

import { OfficesLive } from './live.svelte';

function makeOffice(id: string, status: OfficeInfo['status'] = 'offline'): OfficeInfo {
	return { id, organization_id: 'org-1', name: id, status, host: 'local', role: 'hq' };
}

describe('OfficesLive controller', () => {
	afterEach(() => {
		probeCallbacks.length = 0;
		vi.clearAllMocks();
	});

	it('updates retryingProbe directly from probe transitions without an effect', () => {
		peekOrganizations.mockResolvedValue([]);
		const live = new OfficesLive({
			organizationId: () => 'org-1',
			saved: () => [makeOffice('off-1')],
			hereId: () => 'off-1',
			current: () => [makeOffice('off-1')],
			joined: () => new Set(),
			onback: vi.fn()
		});
		const emit = probeCallbacks.at(-1)!;
		expect(live.retryingProbe).toBe(false);
		emit('down');
		expect(live.check).toBe('down');
		expect(live.retryingProbe).toBe(true);
		emit('up', ['office.restart']);
		expect(live.check).toBe('up');
		expect(live.retryingProbe).toBe(false);
		expect(live.capabilities).toEqual(['office.restart']);
	});

	it('ignores stale refresh responses after organization switch', async () => {
		let orgId = 'org-1';
		let resolveFirst!: (v: { id: string; offices: OfficeInfo[] }[]) => void;
		peekOrganizations.mockImplementationOnce(
			() =>
				new Promise((resolve) => {
					resolveFirst = resolve;
				})
		);

		const saved = [makeOffice('off-saved')];
		const live = new OfficesLive({
			organizationId: () => orgId,
			saved: () => saved,
			hereId: () => 'off-saved',
			current: () => saved,
			joined: () => new Set(),
			onback: vi.fn()
		});

		const pending = live.refresh();
		orgId = 'org-2';
		resolveFirst([{ id: 'org-1', offices: [makeOffice('off-stale')] }]);
		await pending;

		expect(live.reported).toBeUndefined();
		expect(live.listed).toEqual(saved);
	});
});
