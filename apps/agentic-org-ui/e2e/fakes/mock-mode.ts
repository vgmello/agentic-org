/**
 * Mock-only switch for looking at states: `?mock=error`, `?mock=empty`, `?mock=poc`, ... on the address the app was
 * opened at. Read when a call is made. It exists only in e2e fakes, which no production build contains.
 */
import type { Connection, OfficeInfo, Organization } from '$lib/proto';

const DATASET_STORAGE_KEY = 'agentic-org.mock.dataset';
const CONNECTIONS_KEY = 'agentic-org.mock.connections';
const ORGANIZATIONS_KEY = 'agentic-org.mock.organizations';
let pendingPocSelect = false;

export const POC_ORG_ID = 'poc-agentic-org';
export const POC_ORG_NAME = 'Agentic Org';
export const POC_HQ_OFFICE_ID = 'office-poc-agentic-org';
export const POC_BUILD_OFFICE_ID = 'office-poc-agentic-org-build-box';

export const mockMode = (): string => {
	try {
		return typeof location === 'undefined'
			? ''
			: (new URLSearchParams(location.search).get('mock') ?? '');
	} catch {
		return '';
	}
};

export const mockLatency = (ms = 0) => {
	const delay = mockMode().includes('slow') ? 4000 : ms;
	return delay > 0 ? new Promise<void>((done) => setTimeout(done, delay)) : Promise.resolve();
};

export function clearPocDataset(): void {
	pendingPocSelect = false;
	try {
		if (typeof localStorage !== 'undefined') localStorage.removeItem(DATASET_STORAGE_KEY);
		if (typeof location !== 'undefined' && typeof history !== 'undefined') {
			const url = new URL(location.href);
			const raw = url.searchParams.get('mock');
			if (raw) {
				const remaining = raw
					.split(',')
					.map((t) => t.trim())
					.filter((t) => t && t !== 'poc');
				if (remaining.length === 0) url.searchParams.delete('mock');
				else url.searchParams.set('mock', remaining.join(','));
				history.replaceState(history.state, '', url);
			}
		}
	} catch {
		// ignore storage/history errors
	}
}

/**
 * POC-mirror dataset mode (`?mock=poc`, persisted across route navigations in localStorage, cleared by `?mock=default`).
 * Never affects existing default Northwind/Atlas E2E tests unless explicitly activated.
 */
export function isPocDataset(): boolean {
	try {
		const tokens = mockMode()
			.split(',')
			.map((t) => t.trim())
			.filter(Boolean);
		if (tokens.includes('default')) {
			clearPocDataset();
			return false;
		}
		if (tokens.includes('poc')) {
			if (typeof localStorage !== 'undefined') {
				if (localStorage.getItem(DATASET_STORAGE_KEY) !== 'poc') {
					pendingPocSelect = true;
				}
				localStorage.setItem(DATASET_STORAGE_KEY, 'poc');
			}
			return true;
		}
		if (typeof localStorage !== 'undefined') {
			return localStorage.getItem(DATASET_STORAGE_KEY) === 'poc';
		}
	} catch {
		// ignore storage/location errors
	}
	return false;
}

export function pocHqOffice(): OfficeInfo {
	return {
		id: POC_HQ_OFFICE_ID,
		organization_id: POC_ORG_ID,
		name: 'HQ',
		role: 'hq',
		host: 'This Mac',
		status: 'active'
	};
}

export function pocBuildOffice(): OfficeInfo {
	return {
		id: POC_BUILD_OFFICE_ID,
		organization_id: POC_ORG_ID,
		name: 'Build box',
		role: 'satellite',
		host: 'mac-mini-2',
		status: 'offline'
	};
}

export function pocOrganization(): Organization {
	return {
		id: POC_ORG_ID,
		name: POC_ORG_NAME,
		created_at: '2026-10-15T09:00:00.000Z',
		offices: [pocHqOffice()]
	};
}

/**
 * When `?mock=poc` is opened on a fresh browser tab, auto-seed a remembered connection for `Agentic Org` so visiting
 * `/home?theme=report&mock=poc` works immediately without opening `/welcome` or DevTools first.
 */
export function ensurePocConnection(): Connection | undefined {
	if (!isPocDataset()) return undefined;
	const org = pocOrganization();
	const connection: Connection = { office: pocHqOffice(), organization: org };
	try {
		if (typeof localStorage === 'undefined') return connection;
		const existingOrgs: Organization[] = JSON.parse(
			localStorage.getItem(ORGANIZATIONS_KEY) ?? '[]'
		) as Organization[];
		const mergedOrgs = [...existingOrgs.filter((o) => o.id !== POC_ORG_ID), org];
		localStorage.setItem(ORGANIZATIONS_KEY, JSON.stringify(mergedOrgs));

		const existingConns: Connection[] = JSON.parse(
			localStorage.getItem(CONNECTIONS_KEY) ?? '[]'
		) as Connection[];
		const hasPoc = existingConns.some((c) => c.organization.id === POC_ORG_ID);
		if (existingConns.length === 0 || pendingPocSelect || !hasPoc) {
			const shouldSelectPoc = existingConns.length === 0 || pendingPocSelect;
			const mergedConns = shouldSelectPoc
				? [...existingConns.filter((c) => c.organization.id !== POC_ORG_ID), connection]
				: [connection, ...existingConns.filter((c) => c.organization.id !== POC_ORG_ID)];
			localStorage.setItem(CONNECTIONS_KEY, JSON.stringify(mergedConns));
			pendingPocSelect = false;
		}
	} catch {
		// fall back to in-memory connection object
	}
	return connection;
}
