import { officeApi } from '$lib/office/api';
import { ADD_STEPS, validateOfficeName, type AddStep } from './model';
import type { AppError, OfficeWithExtras } from '$lib/proto';

/**
 * The organization's offices and the calls that change them. PROVISIONAL (see proto.ts): `office.list`
 * is in org-proto; `office.add` / `office.remove` are not yet. TEMPORARY: a TypeScript mock keeps the
 * list in the browser until the real host and `org-client` answer.
 */
export interface OfficesApi {
	list(): Promise<OfficeWithExtras[]>;
	/** Add an office on this Mac. `onStep` is told each step as it begins. Fails with the office left as Stopped. */
	addHere(name: string, onStep?: (step: AddStep) => void): Promise<OfficeWithExtras>;
	/** Start a stopped office again (Try again after a failed start). Same steps and failure as adding. */
	restart(officeId: string, onStep?: (step: AddStep) => void): Promise<OfficeWithExtras>;
	remove(officeId: string): Promise<void>;
}

/** Why an office didn't start: the error, and which office is now listed as Stopped. */
export type OfficeStartError = AppError & { office_id: string };

const fail = (code: string, message: string, extra: Record<string, string> = {}): never => {
	throw { source: 'app', code, message, ...extra } satisfies AppError;
};
export { ADD_STEPS, type AddStep };
const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export interface MockStorage {
	getItem(key: string): string | null;
	setItem(key: string, value: string): void;
}

/** `?mock=office_start_fails` makes the next added office fail to start, to show that state. */
export function createMockOffices(
	organization: () => Promise<{ id: string; offices: OfficeWithExtras[] }>,
	{ stepMs = 450, failStart = false, storage = globalThis.localStorage as MockStorage | undefined } = {}
): OfficesApi {
	const memory = new Map<string, string>();
	const store: MockStorage = storage ?? { getItem: (k) => memory.get(k) ?? null, setItem: (k, v) => void memory.set(k, v) };
	const keyFor = (id: string) => `agentic-org.mock.offices.${id}`;
	const load = async () => {
		const org = await organization();
		try {
			const stored = store.getItem(keyFor(org.id));
			if (stored) return { org, offices: JSON.parse(stored) as OfficeWithExtras[] };
		} catch {
			// fall through to the organization's own offices
		}
		return { org, offices: org.offices };
	};
	const save = (orgId: string, offices: OfficeWithExtras[]) => store.setItem(keyFor(orgId), JSON.stringify(offices));

	return {
		async list() {
			return (await load()).offices;
		},
		async addHere(rawName, onStep) {
			const { org, offices } = await load();
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
			if (failStart) {
				const stopped = { ...office, status: 'unavailable' as const, stopped_reason: "The office server didn't answer in time." };
				save(org.id, [...offices, stopped]);
				fail('office_start_failed', `${name} couldn't start: ${stopped.stopped_reason}`, { office_id: office.id });
			}
			const started = { ...office, status: 'active' as const };
			save(org.id, [...offices, started]);
			return started;
		},
		async restart(officeId, onStep) {
			const { org, offices } = await load();
			const office = offices.find((o) => o.id === officeId) ?? fail('not_found', "That office isn't in this organization any more.");
			for (const step of ADD_STEPS) {
				onStep?.(step);
				await wait(stepMs);
			}
			if (failStart) fail('office_start_failed', `${office.name} couldn't start: ${office.stopped_reason ?? 'It did not answer.'}`, { office_id: office.id });
			const started: OfficeWithExtras = { ...office, status: 'active', stopped_reason: undefined };
			delete started.stopped_reason;
			save(org.id, offices.map((o) => (o.id === officeId ? started : o)));
			return started;
		},
		async remove(officeId) {
			const { org, offices } = await load();
			const target = offices.find((o) => o.id === officeId);
			if (!target) fail('not_found', "That office isn't in this organization any more.");
			if (target?.role === 'hq') fail('owner_protected', "HQ can't be removed.");
			save(org.id, offices.filter((o) => o.id !== officeId));
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
