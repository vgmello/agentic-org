import { officeApi } from '$lib/office/api';
import { ADD_STEPS, validateOfficeName, type AddStep } from './model';
import type { AppError, OfficeWithExtras } from '$lib/proto';

// PROVISIONAL: `office.list` is in org-proto; everything else here is invented until ada publishes the
// real calls: `add`, `restart` and `remove`, the failure code and the `office_id` carried by a failed start,
// and the idea of a restartable "Stopped" entry (the offices design says a failed --create leaves nothing
// behind: open question for ada and Eden). Only existing error codes are used (invalid_name, already_exists,
// not_found, unavailable, invalid_request).

/** Why an office didn't start: the error, and which office is now listed as Stopped. PROVISIONAL. */
export type OfficeStartError = AppError & { office_id: string };

/**
 * The organization's offices and the calls that change them.
 * TEMPORARY: a TypeScript mock keeps changes in the browser until the real host and `org-client` answer. It
 * never replaces what the host lists: it stores only the offices it added and the ones it removed, and merges
 * them over the real list.
 */
export interface OfficesApi {
	/** True while adding, starting and removing are simulated, not done by a real host. */
	readonly sample: boolean;
	list(): Promise<OfficeWithExtras[]>;
	/** Add an office on this Mac. `onStep` is told each step as it begins. Fails with the office left as Stopped. */
	addHere(name: string, onStep?: (step: AddStep) => void): Promise<OfficeWithExtras>;
	/** Start a stopped office again (Try again after a failed start). Same steps and failure as adding. */
	restart(officeId: string, onStep?: (step: AddStep) => void): Promise<OfficeWithExtras>;
	remove(officeId: string): Promise<void>;
}

const fail = (code: AppError['code'], message: string, extra: Record<string, string> = {}): never => {
	throw { source: 'api', code, message, ...extra } as AppError;
};
const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
export { ADD_STEPS, type AddStep };

export interface MockStorage {
	getItem(key: string): string | null;
	setItem(key: string, value: string): void;
}

/** What the mock keeps: the offices it added (and changed) and the ids it removed. */
interface Changes {
	added: OfficeWithExtras[];
	removed: string[];
}

const isOffice = (x: unknown): x is OfficeWithExtras =>
	!!x && typeof x === 'object' && typeof (x as OfficeWithExtras).id === 'string' && typeof (x as OfficeWithExtras).name === 'string';

/** `?mock=office_start_fails` makes the next added office fail to start, to show that state. */
export function createMockOffices(
	organization: () => Promise<{ id: string; offices: OfficeWithExtras[] }>,
	{ stepMs = 450, failStart = false, storage }: { stepMs?: number; failStart?: boolean; storage?: MockStorage } = {}
): OfficesApi {
	const memory = new Map<string, string>();
	const fallback: MockStorage = { getItem: (k) => memory.get(k) ?? null, setItem: (k, v) => void memory.set(k, v) };
	const store = (): MockStorage => {
		if (storage) return storage;
		try {
			return globalThis.localStorage ?? fallback;
		} catch {
			return fallback; // storage blocked: changes last for this page only
		}
	};
	const keyFor = (id: string) => `agentic-org.mock.offices.${id}`;

	async function load() {
		const org = await organization();
		let changes: Changes = { added: [], removed: [] };
		try {
			const raw = JSON.parse(store().getItem(keyFor(org.id)) ?? 'null');
			if (raw && Array.isArray(raw.added) && Array.isArray(raw.removed)) changes = { added: raw.added.filter(isOffice), removed: raw.removed.filter((x: unknown) => typeof x === 'string') };
		} catch {
			// unreadable: no changes
		}
		const base = org.offices.filter((o) => !changes.removed.includes(o.id) && !changes.added.some((a) => a.id === o.id));
		return { org, changes, offices: [...base, ...changes.added] };
	}
	const save = (orgId: string, changes: Changes) => store().setItem(keyFor(orgId), JSON.stringify(changes));

	return {
		sample: true,
		async list() {
			return (await load()).offices;
		},
		async addHere(rawName, onStep) {
			const { org, changes, offices } = await load();
			const checked = validateOfficeName(rawName, offices);
			if (!checked.ok) fail('invalid_name', checked.message);
			const name = checked.ok ? checked.name : rawName;
			const office: OfficeWithExtras = {
				id: `office-${Date.now().toString(36)}-${offices.length}`,
				organization_id: org.id,
				name,
				role: 'satellite',
				host: 'This Mac',
				status: 'starting',
				app_version: 'this app'
			};
			for (const step of ADD_STEPS) {
				onStep?.(step);
				await wait(stepMs);
			}
			// Re-read: another add or remove may have happened during the steps.
			const latest = await load();
			if (!validateOfficeName(name, latest.offices).ok) fail('already_exists', `There's already an office called ${name}.`);
			if (failStart) {
				const stopped: OfficeWithExtras = { ...office, status: 'unavailable', stopped_reason: "The office server didn't answer in time." };
				save(org.id, { ...latest.changes, added: [...latest.changes.added, stopped] });
				fail('unavailable', `${name} couldn't start: ${stopped.stopped_reason}`, { office_id: office.id });
			}
			const started: OfficeWithExtras = { ...office, status: 'active' };
			save(org.id, { ...latest.changes, added: [...latest.changes.added, started] });
			void changes;
			return started;
		},
		async restart(officeId, onStep) {
			const { org, offices } = await load();
			const office = offices.find((o) => o.id === officeId) ?? fail('not_found', "That office isn't in this organization any more.");
			for (const step of ADD_STEPS) {
				onStep?.(step);
				await wait(stepMs);
			}
			if (failStart) fail('unavailable', `${office.name} couldn't start: ${office.stopped_reason ?? 'It did not answer.'}`, { office_id: office.id });
			const latest = await load();
			const started: OfficeWithExtras = { ...office, status: 'active' };
			delete started.stopped_reason;
			save(org.id, { ...latest.changes, added: [...latest.changes.added.filter((a) => a.id !== officeId), started] });
			return started;
		},
		async remove(officeId) {
			const { org, changes, offices } = await load();
			const target = offices.find((o) => o.id === officeId);
			if (!target) fail('not_found', "That office isn't in this organization any more.");
			if (target?.role === 'hq') fail('invalid_request', "HQ can't be removed.");
			save(org.id, { added: changes.added.filter((a) => a.id !== officeId), removed: [...new Set([...changes.removed, officeId])] });
		}
	};
}

const wantsFailure = () => typeof location !== 'undefined' && new URLSearchParams(location.search).get('mock') === 'office_start_fails';

export const officesApi: OfficesApi = createMockOffices(
	async () => {
		const connection = (await officeApi.connections()).at(-1);
		if (!connection) return { id: 'none', offices: [] };
		return { id: connection.organization.id, offices: connection.organization.offices };
	},
	{ failStart: wantsFailure() }
);
