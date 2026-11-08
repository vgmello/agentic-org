import { inTauri, officeApi } from '$lib/office/api';
import type { ApiErrorCode, AppError, CommandAccepted, OrgSnapshot, OrgView } from '$lib/proto';
import { ready, type OrgCommand, type OrgCommandInput } from './commands';
import { normalizeKinds } from './model';
import { applyCommand, badId } from './reducer';
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

/** What the mock HQ answered a command id, and (a hash of) the command it answered: a reused id for another command is refused. */
interface Answer {
	command: string;
	result: { accepted: CommandAccepted } | { error: AppError };
}

interface Stored {
	snapshot: OrgSnapshot;
	answers: Record<string, Answer>;
	/** The ids in `answers`, oldest first: the oldest is forgotten past `ANSWERS_KEPT`. */
	order?: string[];
}

const isStored = (x: unknown): x is Stored => {
	const s = (x as Stored | null)?.snapshot;
	const order = (x as Stored | null)?.order;
	return (order === undefined || (Array.isArray(order) && order.every((id) => typeof id === 'string'))) && !!s && Array.isArray(s.teams) && Array.isArray(s.members) && Array.isArray(s.memberships) && !!s.position && typeof (x as Stored).answers === 'object' && (x as Stored).answers !== null;
};
const isAnswer = (x: unknown): x is Answer => {
	const a = x as Answer | null;
	return !!a && typeof a.command === 'string' && !!a.result && typeof a.result === 'object' && ('accepted' in a.result || 'error' in a.result);
};

const refusal = (code: ApiErrorCode, message: string): AppError => ({ source: 'api', code, message });

/** As in the Rust mock HQ: only recent answers are kept, and a command can't be bigger than a command. */
const ANSWERS_KEPT = 500;
const COMMAND_MAX_BYTES = 16 * 1024;

/** A small stable hash (cyrb53) of the command's JSON: enough to tell two commands with one id apart. */
function fingerprint(command: OrgCommand): string | undefined {
	const text = JSON.stringify(command);
	// Bytes, as the Rust side counts them (a character may be up to three).
	if (new TextEncoder().encode(text).length > COMMAND_MAX_BYTES) return undefined;
	let h1 = 0xdeadbeef;
	let h2 = 0x41c6ce57;
	for (let i = 0; i < text.length; i++) {
		const c = text.charCodeAt(i);
		h1 = Math.imul(h1 ^ c, 2654435761);
		h2 = Math.imul(h2 ^ c, 1597334677);
	}
	h1 = Math.imul(h1 ^ (h1 >>> 16), 2246822507) ^ Math.imul(h2 ^ (h2 >>> 13), 3266489909);
	h2 = Math.imul(h2 ^ (h2 >>> 16), 2246822507) ^ Math.imul(h1 ^ (h1 >>> 13), 3266489909);
	return `${(h2 >>> 0).toString(16)}${(h1 >>> 0).toString(16)}:${text.length}`;
}

/**
 * A mock office's organization, for a plain browser (`bun run dev`, tests): the Tauri app asks the Rust
 * shell, whose mock office applies the same rules (`src-tauri/src/org_engine.rs`; the two are tested against
 * the same cases). Changes are kept in the browser's storage so they survive a reload. `?org=empty` in the
 * URL shows an organization with only its owner, which lives in memory for the page's session instead.
 * Always sample data.
 */
export function createMockOrg(
	hqOfficeId: (organizationId: string) => Promise<string>,
	{ empty = (): boolean => false, storage = globalThis.localStorage as MockStorage | undefined }: { empty?: () => boolean; storage?: MockStorage } = {}
): OrgApi {
	const memory = new Map<string, string>();
	const inMemory: MockStorage = { getItem: (k) => memory.get(k) ?? null, setItem: (k, v) => void memory.set(k, v) };
	const keyFor = (organizationId: string) => `agentic-org.mock.org.${organizationId}`;

	async function load(organizationId: string): Promise<{ key: string; store: MockStorage; state: Stored }> {
		// Asked even for an empty organization: one that isn't here is not_found either way.
		const office = await hqOfficeId(organizationId);
		const isEmpty = empty();
		const key = keyFor(organizationId) + (isEmpty ? ':empty' : '');
		const store = isEmpty || !storage ? inMemory : storage;
		try {
			const parsed: unknown = JSON.parse(store.getItem(key) ?? 'null');
			if (isStored(parsed)) return { key, store, state: parsed };
		} catch {
			// unreadable: start from the sample
		}
		return { key, store, state: { snapshot: isEmpty ? emptyOrg() : sampleOrg(office), answers: {}, order: [] } };
	}

	// Commands are answered one at a time, as HQ does, so two sent together can't overwrite each other.
	let queue: Promise<unknown> = Promise.resolve();

	async function run(organizationId: string, input: OrgCommandInput | OrgCommand): Promise<CommandAccepted> {
		const command = ready(input);
		const id = command.params.command_id;
		const print = fingerprint(command);
		const { key, store, state } = await load(organizationId); // not_found first, as in the Rust mock
		// A request that can't be read as a command is refused and never remembered.
		if (!print || badId(id)) throw refusal('invalid_request', "That request isn't valid.");
		const earlier = Object.hasOwn(state.answers, id) && isAnswer(state.answers[id]) ? state.answers[id] : undefined;
		if (earlier) {
			if (earlier.command !== print) throw refusal('invalid_request', 'That command id was already used for a different command.');
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
		// An answer that couldn't be read is replaced: its id goes to the back of the list once.
		const order = [...(state.order ?? Object.keys(state.answers)).filter((x) => x !== id), id];
		const answers = { ...state.answers, [id]: { command: print, result } };
		while (order.length > ANSWERS_KEPT) delete answers[order.shift()!];
		try {
			store.setItem(key, JSON.stringify({ snapshot, answers, order } satisfies Stored));
		} catch {
			throw { source: 'app', code: 'internal', message: "The change couldn't be saved in this browser." } satisfies AppError;
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
