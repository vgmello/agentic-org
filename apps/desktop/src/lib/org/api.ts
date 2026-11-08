import { inTauri, officeApi } from '$lib/office/api';
import type { ApiErrorCode, AppError, CommandAccepted, OrgSnapshot, OrgView } from '$lib/proto';
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

/** What the mock HQ answered a command id, and the command it answered (a reused id for another command is refused). */
interface Answer {
	command: string;
	result: { accepted: CommandAccepted } | { error: AppError };
}

interface Stored {
	snapshot: OrgSnapshot;
	answers: Record<string, Answer>;
}

const isStored = (x: unknown): x is Stored => {
	const s = (x as Stored | null)?.snapshot;
	return !!s && Array.isArray(s.teams) && Array.isArray(s.members) && Array.isArray(s.memberships) && !!s.position && typeof (x as Stored).answers === 'object' && (x as Stored).answers !== null;
};

const refusal = (code: ApiErrorCode, message: string): AppError => ({ source: 'api', code, message });

/**
 * A mock office's organization, for a plain browser (`bun run dev`, tests): the Tauri app asks the Rust
 * shell, whose mock office applies the same rules (`src-tauri/src/org_engine.rs`; the two are tested against
 * the same cases). Changes are kept in the browser's storage so they survive a reload. `?org=empty` in the
 * URL shows an organization with only its owner (nothing is stored then). Always sample data.
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
				const parsed: unknown = JSON.parse(store.getItem(key) ?? 'null');
				if (isStored(parsed)) return { key, state: parsed };
			} catch {
				// unreadable: start from the sample
			}
		}
		return { key, state: { snapshot: empty() ? emptyOrg() : sampleOrg(await hqOfficeId(organizationId)), answers: {} } };
	}

	// Commands are answered one at a time, as HQ does, so two sent together can't overwrite each other.
	let queue: Promise<unknown> = Promise.resolve();

	async function run(organizationId: string, input: OrgCommandInput | OrgCommand): Promise<CommandAccepted> {
		const command = ready(input);
		const id = command.params.command_id;
		const fingerprint = JSON.stringify(command);
		const { key, state } = await load(organizationId);
		const earlier = Object.hasOwn(state.answers, id) ? state.answers[id] : undefined;
		if (earlier) {
			if (earlier.command !== fingerprint) throw refusal('invalid_request', 'That command id was already used for a different command.');
			if ('error' in earlier.result) throw earlier.result.error;
			return earlier.result.accepted;
		}
		let result: Answer['result'];
		let snapshot = state.snapshot;
		try {
			snapshot = applyCommand(state.snapshot, command); // throws HQ's error
			result = { accepted: { status: 'committed', position: snapshot.position } };
		} catch (e) {
			if (!(e && typeof e === 'object' && 'code' in e && 'source' in e)) throw e;
			result = { error: e as AppError };
		}
		if (!empty()) {
			try {
				store.setItem(key, JSON.stringify({ snapshot, answers: { ...state.answers, [id]: { command: fingerprint, result } } } satisfies Stored));
			} catch {
				throw { source: 'app', code: 'internal', message: "The change couldn't be saved in this browser." } satisfies AppError;
			}
		}
		if ('error' in result) throw result.error;
		return result.accepted;
	}

	return {
		async view(organizationId) {
			return { snapshot: normalizeKinds((await load(organizationId)).state.snapshot), sample: true };
		},
		command(organizationId, input) {
			const answer = queue.then(() => run(organizationId, input));
			queue = answer.catch(() => undefined);
			return answer;
		}
	};
}

const wantsEmpty = () => typeof location !== 'undefined' && new URLSearchParams(location.search).get('org') === 'empty';

export const orgApi: OrgApi = inTauri()
	? tauriOrg
	: createMockOrg(
			async (organizationId) => {
				const connection = (await officeApi.connections()).find((c) => c.organization.id === organizationId);
				if (!connection) throw refusal('not_found', "That organization isn't here.");
				return connection.office.id;
			},
			{ empty: wantsEmpty }
		);
