import type { AppError, Connection, HelloResult, OfficeInfo } from '$lib/proto';
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

const office: OfficeInfo = { id: 'office-local', name: 'This Mac', host: 'This Mac' };

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
					message: 'Allow agentic.org to run in the background in System Settings, then try again.'
				});
			if (scenario === 'unreachable')
				fail({ source: 'setup', code: 'unreachable', message: "The office server started but isn't answering." });
			connected = true;
			return { api_version: 1, server_version: '0.0.0-mock', office };
		},
		async createOrganization(rawName): Promise<Connection> {
			if (!connected) fail({ source: 'app', code: 'not_connected', message: "The office isn't running yet." });
			const checked = validateName(rawName);
			if (!checked.ok) fail({ source: 'api', code: 'invalid_name', message: checked.message });
			await wait(delayMs / 3);
			if (scenario === 'storage_error')
				fail({ source: 'api', code: 'storage_error', message: "The office couldn't write the organization's folder." });
			const list = load();
			if (list.some((c) => c.organization.name.toLowerCase() === checked.name.toLowerCase()))
				fail({ source: 'api', code: 'already_exists', message: 'An organization with that name already exists.' });
			const connection: Connection = {
				office,
				organization: { id: `org-${Date.now().toString(36)}-${list.length}`, name: checked.name, created_at: new Date().toISOString() }
			};
			save([...list, connection]);
			return connection;
		}
	};
}

export const mockOffice = createMockOffice();
