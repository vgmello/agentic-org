import { inTauri, officeApi } from '$lib/office/api';
import type { CommandAccepted, OrgSnapshot, OrgView } from '$lib/proto';
import { ready, type OrgCommand, type OrgCommandInput } from './commands';
import { normalizeKinds } from './model';
import { applyCommand } from './reducer';
import { emptyOrg, sampleOrg } from './sample';
import { tauriOrg } from './tauri';

/** What the org chart needs from an office: `org.snapshot`, and the commands that change the structure. */
export interface OrgApi {
	/** The structure of an organization, and whether it is the TEMPORARY invented sample. */
	view(organizationId: string): Promise<OrgView>;
	/**
	 * Ask HQ to change the organization. Resolves with its answer, or rejects with the error HQ gave (an
	 * `AppError` with source `api`). A repeated `command_id` gets the same answer and changes nothing.
	 */
	command(organizationId: string, input: OrgCommandInput | OrgCommand): Promise<CommandAccepted>;
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
 * A mock office's organization, for a plain browser (`bun run dev`, tests): the Tauri app asks the Rust
 * shell, whose mock office applies the same rules (`src-tauri/src/org_engine.rs`). Changes are kept in
 * the browser's storage so they survive a reload. `?org=empty` in the URL shows an organization with
 * only its owner (nothing is stored then). Always sample data.
 */
export function createMockOrg(
	hqOfficeId: (organizationId: string) => Promise<string>,
	{ empty = (): boolean => false, storage = globalThis.localStorage as MockStorage | undefined }: { empty?: () => boolean; storage?: MockStorage } = {}
): OrgApi {
	const memory = new Map<string, string>();
	const store: MockStorage = storage ?? { getItem: (k) => memory.get(k) ?? null, setItem: (k, v) => void memory.set(k, v) };
	const keyFor = (organizationId: string) => `agentic-org.mock.org.${organizationId}`;

	async function load(organizationId: string): Promise<{ key: string; state: Stored }> {
		const key = keyFor(organizationId);
		if (!empty()) {
			try {
				const raw = store.getItem(key);
				if (raw) return { key, state: JSON.parse(raw) as Stored };
			} catch {
				// unreadable: start from the sample
			}
		}
		return { key, state: { snapshot: empty() ? emptyOrg() : sampleOrg(await hqOfficeId(organizationId)), answers: {} } };
	}

	return {
		async view(organizationId) {
			return { snapshot: normalizeKinds((await load(organizationId)).state.snapshot), sample: true };
		},
		async command(organizationId, input) {
			const command = ready(input);
			const { key, state } = await load(organizationId);
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

export const orgApi: OrgApi = inTauri()
	? tauriOrg
	: createMockOrg(
			async (organizationId) => {
				const connections = await officeApi.connections();
				return (connections.find((c) => c.organization.id === organizationId) ?? connections.at(-1))?.office.id ?? 'office-local';
			},
			{ empty: wantsEmpty }
		);
