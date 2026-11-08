import type { AppError, Connection, HelloResult, OfficeInfo, Organization } from '$lib/proto';
import { validateName } from '$lib/name';
import type { OfficeApi } from './api';

/**
 * A scripted office for running the UI in a plain browser. Add `?mock=<scenario>` to the URL to
 * force a failure: not_installed, needs_approval, unreachable, storage_error. It mirrors the
 * Rust mock (`src-tauri/src/mock.rs`), which does the same inside the Tauri shell.
 */
export type Scenario = 'healthy' | 'not_installed' | 'needs_approval' | 'unreachable' | 'storage_error';

const SCENARIOS: Scenario[] = ['not_installed', 'needs_approval', 'unreachable', 'storage_error'];
const STORAGE_KEY = 'agentic-org.mock.connections';
const ORGANIZATIONS_KEY = 'agentic-org.mock.organizations';

/** The HQ office every organization starts with, on this Mac. */
const hqOffice = (organizationId: string): OfficeInfo => ({
	id: `office-${organizationId}`,
	organization_id: organizationId,
	name: 'HQ',
	role: 'hq',
	host: 'This Mac',
	status: 'active'
});

/** What a fresh mock Mac already has, so there is something to open on first launch. */
const sampleOrganizations = (): Organization[] =>
	[
		['sample-northwind', 'Northwind Labs', '2026-09-14T09:30:00.000Z'],
		['sample-globex', 'Globex Research', '2026-10-02T16:05:00.000Z']
	].map(([id, name, created_at]) => ({ id, name, created_at, offices: [hqOffice(id)] }));

function scenarioFromUrl(): Scenario {
	if (typeof location === 'undefined') return 'healthy';
	const wanted = new URLSearchParams(location.search).get('mock');
	return SCENARIOS.find((s) => s === wanted) ?? 'healthy';
}

/** Where the mock remembers connections: the browser's localStorage, or memory if unavailable. */
export interface MockStorage {
	getItem(key: string): string | null;
	setItem(key: string, value: string): void;
}

function browserStorage(): MockStorage {
	const memory = new Map<string, string>();
	const fallback: MockStorage = {
		getItem: (k) => memory.get(k) ?? null,
		setItem: (k, v) => void memory.set(k, v)
	};
	try {
		const ls = globalThis.localStorage;
		ls.setItem(STORAGE_KEY + '.probe', '1');
		return ls;
	} catch {
		return fallback;
	}
}

const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

function fail(error: AppError): never {
	throw error;
}

export function createMockOffice({
	delayMs = 900,
	scenario = scenarioFromUrl(),
	storage = browserStorage()
}: { delayMs?: number; scenario?: Scenario; storage?: MockStorage } = {}): OfficeApi {
	let connected = false;
	// Oldest first, like the Rust store.
	const load = (): Connection[] => {
		try {
			return JSON.parse(storage.getItem(STORAGE_KEY) ?? '[]') as Connection[];
		} catch {
			return [];
		}
	};
	const save = (list: Connection[]) => storage.setItem(STORAGE_KEY, JSON.stringify(list));
	// The organizations this mock Mac hosts: the office's own state, apart from what the app remembers.
	const hosted = (): Organization[] => {
		try {
			const stored = storage.getItem(ORGANIZATIONS_KEY);
			if (stored) return JSON.parse(stored) as Organization[];
		} catch {
			// fall through to the samples
		}
		return sampleOrganizations();
	};
	const saveHosted = (list: Organization[]) => storage.setItem(ORGANIZATIONS_KEY, JSON.stringify(list));
	const remember = (organization: Organization): Connection => {
		const office = organization.offices.find((o) => o.role === 'hq') ?? organization.offices[0];
		if (!office) fail({ source: 'app', code: 'no_office', message: 'That organization has no office on this Mac.' });
		const connection: Connection = { office, organization };
		save([...load().filter((c) => c.organization.id !== organization.id), connection]);
		return connection;
	};
	return {
		async connections() {
			return load();
		},
		async ensureRunning(): Promise<HelloResult> {
			await wait(delayMs);
			if (scenario === 'not_installed')
				fail({ source: 'setup', code: 'not_installed', message: "The office server isn't installed on this Mac." });
			if (scenario === 'needs_approval')
				fail({
					source: 'setup',
					code: 'needs_approval',
					message: 'Allow Agentic Org to run in the background in System Settings, then try again.'
				});
			if (scenario === 'unreachable')
				fail({ source: 'setup', code: 'unreachable', message: "The office server started but isn't answering." });
			connected = true;
			return { api_version: 1, server_version: '0.0.0-mock', host: 'This Mac' };
		},
		async createOrganization(rawName): Promise<Connection> {
			if (!connected) fail({ source: 'app', code: 'not_connected', message: "The office isn't running yet." });
			const checked = validateName(rawName);
			if (!checked.ok) fail({ source: 'api', code: 'invalid_name', message: checked.message });
			await wait(delayMs / 3);
			if (scenario === 'storage_error')
				fail({ source: 'api', code: 'storage_error', message: "The office couldn't write the organization's folder." });
			const list = hosted();
			if (list.some((o) => o.name.toLowerCase() === checked.name.toLowerCase()))
				fail({ source: 'api', code: 'already_exists', message: 'An organization with that name already exists.' });
			const id = `org-${Date.now().toString(36)}-${list.length}`;
			const organization: Organization = {
				id,
				name: checked.name,
				created_at: new Date().toISOString(),
				offices: [hqOffice(id)]
			};
			saveHosted([...list, organization]);
			const connection = remember(organization);
			return connection;
		},
		async listOrganizations() {
			if (!connected) fail({ source: 'app', code: 'not_connected', message: "The office isn't running yet." });
			return hosted();
		},
		async openOrganization(id) {
			if (!connected) fail({ source: 'app', code: 'not_connected', message: "The office isn't running yet." });
			const organization = hosted().find((o) => o.id === id);
			if (!organization)
				fail({ source: 'app', code: 'not_found', message: "That organization isn't on this Mac any more." });
			const here = organization.offices.find((o) => o.role === 'hq') ?? organization.offices[0];
			if (here?.status === 'unavailable')
				fail({ source: 'app', code: 'office_unavailable', message: "That organization's office isn't available right now." });
			return remember(organization);
		}
	};
}

export const mockOffice = createMockOffice();
