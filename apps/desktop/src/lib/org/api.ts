import { officeApi } from '$lib/office/api';
import type { CommandAccepted, OrgCommand, OrgCommandInput } from './commands';
import { withId } from './commands';
import { normalizeKinds } from './model';
import { applyCommand } from './reducer';
import type { OrgSnapshot } from '$lib/proto';
import { emptyOrg, sampleOrg } from './sample';

/** What the org chart needs from an office: `org.snapshot`, and the commands that change the structure. */
export interface OrgApi {
	/** True while the data is the TEMPORARY invented sample, not the organization's own. */
	readonly sample: boolean;
	snapshot(): Promise<OrgSnapshot>;
	/**
	 * Ask HQ to change the organization. Resolves with its answer, or rejects with the error HQ gave (an
	 * `AppError` with source `api`). A repeated `command_id` gets the same answer and changes nothing.
	 */
	command(input: OrgCommandInput | OrgCommand): Promise<CommandAccepted>;
}

export interface MockStorage {
	getItem(key: string): string | null;
	setItem(key: string, value: string): void;
}

interface Stored {
	snapshot: OrgSnapshot;
	/** Answers to commands already applied, so a retry gets the same answer. */
	answers: Record<string, CommandAccepted>;
}

/**
 * A mock office's organization, kept in the browser's storage so edits survive a reload. `?org=empty`
 * in the URL shows an organization with only its owner.
 * TEMPORARY: until the real host and `org-client` exist (the Rust shell then answers `org.snapshot` and
 * forwards commands), the browser and the app both use this TypeScript mock. It applies commands with
 * the contract's rules (`reducer.ts`); the real HQ is the authority.
 */
export function createMockOrg(
	hqOfficeId: () => Promise<{ organizationId: string; officeId: string }>,
	{ empty = (): boolean => false, storage = globalThis.localStorage as MockStorage | undefined }: { empty?: () => boolean; storage?: MockStorage } = {}
): OrgApi {
	const memory = new Map<string, string>();
	const store: MockStorage = storage ?? { getItem: (k) => memory.get(k) ?? null, setItem: (k, v) => void memory.set(k, v) };
	const keyFor = (organizationId: string) => `agentic-org.mock.org.${organizationId}`;

	async function load(): Promise<{ key: string; state: Stored }> {
		const { organizationId, officeId } = await hqOfficeId();
		const key = keyFor(organizationId);
		if (!empty()) {
			try {
				const raw = store.getItem(key);
				if (raw) return { key, state: JSON.parse(raw) as Stored };
			} catch {
				// start from the sample
			}
		}
		return { key, state: { snapshot: empty() ? emptyOrg() : sampleOrg(officeId), answers: {} } };
	}

	return {
		sample: true,
		async snapshot() {
			return normalizeKinds((await load()).state.snapshot);
		},
		async command(input) {
			const command = 'command_id' in input.params ? (input as OrgCommand) : withId(input as OrgCommandInput);
			const { key, state } = await load();
			const earlier = state.answers[command.params.command_id];
			if (earlier) return earlier;
			const snapshot = applyCommand(state.snapshot, command); // throws HQ's error
			const accepted: CommandAccepted = { status: 'committed', position: snapshot.position };
			if (!empty()) store.setItem(key, JSON.stringify({ snapshot, answers: { ...state.answers, [command.params.command_id]: accepted } } satisfies Stored));
			return accepted;
		}
	};
}

const wantsEmpty = () => typeof location !== 'undefined' && new URLSearchParams(location.search).get('org') === 'empty';

export const orgApi: OrgApi = createMockOrg(
	async () => {
		const connection = (await officeApi.connections()).at(-1);
		return { organizationId: connection?.organization.id ?? 'none', officeId: connection?.office.id ?? 'office-local' };
	},
	{ empty: wantsEmpty }
);
