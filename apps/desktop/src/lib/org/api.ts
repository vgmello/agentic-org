import { inTauri, officeApi } from '$lib/office/api';
import type { ApiErrorCode, AppError, CommandAccepted, OrgFeedNotice, OrgSnapshot, OrgView } from '$lib/proto';
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
	/** Be told when an organization's structure changes (anyone's command, ours included) and whether the feed is live. Resolves to the function that stops listening. */
	onFeed(listener: (notice: OrgFeedNotice) => void): Promise<() => void>;
	/** "Live updates paused" has a Retry: connect the live feed now. Resolves when asked, not when it is back (a `live` notice says that). */
	retryFeed(organizationId: string): Promise<void>;
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

/** A snapshot saved before projects had areas gets each project's General area (the browser mock's old storage). */
function withGeneralAreas(s: OrgSnapshot): OrgSnapshot {
	const areas = s.areas ?? [];
	const missing = (s.projects ?? []).filter((p) => !areas.some((a) => a.project_id === p.id && a.general));
	if (missing.length === 0) return s;
	const owner = (id: string) => (s.project_teams ?? []).find((l) => l.project_id === id)?.team_id;
	return { ...s, areas: [...areas, ...missing.map((p) => ({ id: `${p.id}-general`, project_id: p.id, name: 'General', general: true, version: 1, ...(owner(p.id) ? { team_id: owner(p.id) } : {}) }))] };
}

const refusal = (code: ApiErrorCode, message: string): AppError => ({ source: 'api', code, message });

/** As in the Rust mock HQ: only recent answers are kept, and a command can't be bigger than a command. */
const ANSWERS_KEPT = 1000;
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
	{
		empty = (): boolean => false,
		fails = (): boolean => false,
		delayMs = (): number => 0,
		storage = globalThis.localStorage as MockStorage | undefined
	}: { empty?: () => boolean; fails?: () => boolean; delayMs?: () => number; storage?: MockStorage } = {}
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
			if (isStored(parsed)) return { key, store, state: { ...parsed, snapshot: withGeneralAreas(parsed.snapshot) } };
		} catch {
			// unreadable: start from the sample
		}
		return { key, store, state: { snapshot: isEmpty ? emptyOrg() : sampleOrg(office), answers: {}, order: [] } };
	}

	// Commands are answered one at a time, as HQ does, so two sent together can't overwrite each other.
	let queue: Promise<unknown> = Promise.resolve();
	const listeners = new Set<(notice: OrgFeedNotice) => void>();

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
			// HQ's clock, and the offices that exist (the mock organization has its HQ office).
			snapshot = applyCommand(state.snapshot, command, { now: new Date().toISOString(), offices: [await hqOfficeId(organizationId)] }); // throws HQ's error
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
		const { position } = result.accepted;
		if (position) for (const listener of listeners) listener({ kind: 'changed', organization_id: organizationId, position });
		return result.accepted;
	}

	// A test makes a change as if another person had: `window.__mockOrgChange(organizationId, command)`.
	const api: OrgApi = {
		async view(organizationId) {
			if (delayMs() > 0) await new Promise((resolve) => setTimeout(resolve, delayMs()));
			if (fails()) throw refusal('storage_error', "The office couldn't read its storage.");
			return { snapshot: normalizeKinds((await load(organizationId)).state.snapshot), sample: true };
		},
		command(organizationId, input) {
			const answer = queue.then(() => run(organizationId, input));
			queue = answer.catch(() => undefined);
			return answer;
		},
		async onFeed(listener) {
			listeners.add(listener);
			return () => void listeners.delete(listener);
		},
		async retryFeed(organizationId) {
			// The mock feed is always reachable again when asked.
			setTimeout(() => listeners.forEach((l) => l({ kind: 'live', organization_id: organizationId, live: true })), 300);
		}
	};
	if (typeof window !== 'undefined') {
		const hooks = window as unknown as { __mockOrgChange: OrgApi['command']; __mockOrgLive: (organizationId: string, live: boolean) => void };
		hooks.__mockOrgChange = api.command;
		// A test makes the live feed drop (or come back): `window.__mockOrgLive(organizationId, false)`.
		hooks.__mockOrgLive = (organization_id, live) => listeners.forEach((l) => l({ kind: 'live', organization_id, live }));
	}
	return api;
}

const wantsEmpty = () => typeof location !== 'undefined' && new URLSearchParams(location.search).get('org') === 'empty';

/**
 * `?org=fails` makes reading the structure fail, `?org=fails_once` only the first time (so Retry can be shown to
 * work), `?org=slow` makes it take a moment: to show how a screen copes.
 */
let failedOnce = false;
const wantsFailure = () => {
	if (typeof location === 'undefined') return false;
	const mode = new URLSearchParams(location.search).get('org');
	if (mode === 'fails_once') return !failedOnce && (failedOnce = true);
	return mode === 'fails';
};
const wantsSlow = () => (typeof location !== 'undefined' && new URLSearchParams(location.search).get('org') === 'slow' ? 2500 : 0);

export const orgApi: OrgApi = inTauri()
	? tauriOrg
	: createMockOrg(
			async (organizationId) => {
				const connection = (await officeApi.connections()).find((c) => c.organization.id === organizationId);
				if (!connection) throw refusal('not_found', "That organization isn't here.");
				return connection.office.id;
			},
			{ empty: wantsEmpty, fails: wantsFailure, delayMs: wantsSlow }
		);
