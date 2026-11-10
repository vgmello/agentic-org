import { describe, expect, it } from 'vitest';
import { createMockOffice, type Scenario } from './office/mock';
import type { OfficeApi } from './office/api';
import { formatDate, loadOrganizations, localOffice, openOrganization, openable } from './open-flow';
import type { OfficeInfo, Organization } from './proto';

const memory = () => {
	const m = new Map<string, string>();
	return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) };
};
const mock = (scenario: Scenario = 'healthy') => createMockOffice({ delayMs: 0, scenario, storage: memory() });

const office = (over: Partial<OfficeInfo> = {}): OfficeInfo => ({
	id: 'o1',
	organization_id: 'org',
	name: 'HQ',
	role: 'hq',
	host: 'This Mac',
	status: 'active',
	...over
});
const org = (offices: OfficeInfo[]): Organization => ({ id: 'org', name: 'Acme', created_at: '2026-09-14T09:30:00.000Z', offices });

describe('loadOrganizations', () => {
	it('starts the office and lists what this Mac already has', async () => {
		const listing = await loadOrganizations(mock());
		expect(listing.kind).toBe('ready');
		const names = listing.kind === 'ready' ? listing.organizations.map((o) => o.name) : [];
		expect(names).toEqual(['Northwind Labs', 'Globex Research']);
		expect(listing.kind === 'ready' && listing.current).toBeUndefined();
	});

	it('marks the organization the app is connected to', async () => {
		const api = mock();
		await api.ensureRunning();
		await api.openOrganization('sample-globex');
		const listing = await loadOrganizations(api);
		expect(listing.kind === 'ready' && listing.current).toBe('sample-globex');
	});

	it.each(['unreachable'] as const)('a %s office is a failed listing', async (scenario) => {
		const listing = await loadOrganizations(mock(scenario));
		expect(listing.kind === 'failed' && listing.error.code).toBe(scenario);
	});

	it('reports a failure while listing as a failed listing, not a throw', async () => {
		const api: OfficeApi = {
			...mock(),
			listOrganizations: async () => Promise.reject({ source: 'api', code: 'storage_error', message: 'disk' })
		};
		const listing = await loadOrganizations(api);
		expect(listing.kind === 'failed' && listing.error.code).toBe('storage_error');
	});
});

describe('an unreadable list of remembered connections', () => {
	it('still shows the organizations, just without the open-now mark', async () => {
		const api: OfficeApi = { ...mock(), connections: async () => Promise.reject({ source: 'app', code: 'connections_unreadable', message: 'x' }) };
		const listing = await loadOrganizations(api);
		expect(listing.kind).toBe('ready');
		expect(listing.kind === 'ready' && listing.current).toBeUndefined();
	});
});

describe('an organization whose office is unavailable', () => {
	it('is listed but cannot be opened', async () => {
		const storage = memory();
		const down: Organization = {
			id: 'down',
			name: 'Down Under',
			created_at: '2026-09-14T09:30:00.000Z',
			offices: [office({ id: 'od', organization_id: 'down', status: 'unavailable' })]
		};
		storage.setItem('agentic-org.mock.organizations', JSON.stringify([down]));
		const api = createMockOffice({ delayMs: 0, storage });
		await api.ensureRunning();
		expect((await api.listOrganizations()).map((o) => o.id)).toEqual(['down']);
		const result = await openOrganization(api, 'down');
		expect(!result.ok && result.error.code).toBe('office_unavailable');
		expect(await api.connections()).toEqual([]);
	});
});

describe('openOrganization', () => {
	it('remembers the connection to the organization and its HQ here', async () => {
		const api = mock();
		await api.ensureRunning();
		const result = await openOrganization(api, 'sample-northwind');
		expect(result.ok && result.connection.organization.name).toBe('Northwind Labs');
		expect(result.ok && result.connection.office.role).toBe('hq');
		expect((await api.connections()).map((c) => c.organization.id)).toEqual(['sample-northwind']);
	});

	it('opening again moves it to the newest, without duplicating', async () => {
		const api = mock();
		await api.ensureRunning();
		await openOrganization(api, 'sample-northwind');
		await openOrganization(api, 'sample-globex');
		await openOrganization(api, 'sample-northwind');
		expect((await api.connections()).map((c) => c.organization.id)).toEqual(['sample-globex', 'sample-northwind']);
	});

	it('refuses an organization the Mac no longer has, and a closed office', async () => {
		const api = mock();
		const closed = await openOrganization(api, 'sample-globex');
		expect(!closed.ok && closed.error.code).toBe('not_connected');
		await api.ensureRunning();
		const missing = await openOrganization(api, 'nope');
		expect(!missing.ok && missing.error.code).toBe('not_found');
		expect(await api.connections()).toEqual([]);
	});

	it('an organization created here shows up in the list afterwards', async () => {
		const api = mock();
		await api.ensureRunning();
		await api.createOrganization('Fresh Start');
		const names = (await api.listOrganizations()).map((o) => o.name);
		expect(names).toEqual(['Northwind Labs', 'Globex Research', 'Fresh Start']);
	});
});

describe('which office, and whether it can be opened', () => {
	it('prefers HQ over a satellite listed first', () => {
		const o = org([office({ id: 's', role: 'satellite' }), office({ id: 'h' })]);
		expect(localOffice(o)?.id).toBe('h');
	});
	it('can be opened unless its office here is unavailable or missing', () => {
		expect(openable(org([office()]))).toBe(true);
		expect(openable(org([office({ status: 'offline' })]))).toBe(true);
		expect(openable(org([office({ status: 'unavailable' })]))).toBe(false);
		expect(openable(org([]))).toBe(false);
	});
});

describe('formatDate', () => {
	it('formats a timestamp in UTC and passes unreadable text through', () => {
		expect(formatDate('2026-09-14T09:30:00.000Z')).toBe('Sep 14, 2026');
		expect(formatDate('not a date')).toBe('not a date');
	});
});
