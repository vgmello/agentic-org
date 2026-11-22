/**
 * Mock-only switch for looking at states: `?mock=error`, `?mock=empty`, `?mock=poc`, ... on the address the app was
 * opened at. Read when a call is made. It exists only in e2e fakes, which no production build contains.
 */
import type { Connection, OfficeInfo, Organization } from '$lib/proto';

const DATASET_STORAGE_KEY = 'agentic-org.mock.dataset';
const CONNECTIONS_KEY = 'agentic-org.mock.connections';
const ORGANIZATIONS_KEY = 'agentic-org.mock.organizations';

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

export const mockLatency = (ms = 250) =>
	new Promise<void>((done) => setTimeout(done, mockMode().includes('slow') ? 4000 : ms));

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
			if (typeof localStorage !== 'undefined') localStorage.removeItem(DATASET_STORAGE_KEY);
			return false;
		}
		if (tokens.includes('poc')) {
			if (typeof localStorage !== 'undefined') localStorage.setItem(DATASET_STORAGE_KEY, 'poc');
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
		status: 'offline',
		status_reason: 'unreachable'
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
		const last = existingConns[existingConns.length - 1];
		if (!last || last.organization.id !== POC_ORG_ID) {
			const mergedConns = [
				...existingConns.filter((c) => c.organization.id !== POC_ORG_ID),
				connection
			];
			localStorage.setItem(CONNECTIONS_KEY, JSON.stringify(mergedConns));
		}
	} catch {
		// fall back to in-memory connection object
	}
	return connection;
}
