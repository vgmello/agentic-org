import type { OrgApi } from '$lib/org/api';
import { applyChat, emptyChat, syncAuto, tick } from './engine';
import { chatContext } from './model';
import { SAMPLE_DIRECTORY, sampleChatFull } from './sample';
import type {
	ChatAccepted,
	ChatCall,
	ChatCallStatus,
	ChatMessage,
	ChatRefusal,
	ChatSnapshot,
	ChatState
} from './types';

/**
 * What the Chat screen needs from an office: a snapshot, the calls of ADR-0003 section 6 (amended 2026-11-08: chat, proposed), and a
 * signal when something changed (the office pushes `message.received`, `conversation.changed` and `chat.typing`). Until the
 * office serves them, this is a mock that keeps the conversations in memory, like the first org and work mocks did.
 * Always sample data.
 */
export interface ChatApi {
	snapshot(organizationId: string): Promise<ChatSnapshot>;
	/** Rejects with the error the office gave (a `ChatRefusal`, which `toAppError` passes through). */
	command(organizationId: string, call: ChatCall): Promise<ChatAccepted>;
	/** Calls `listener` whenever the conversations change. Returns the way to stop. */
	subscribe(organizationId: string, listener: () => void): () => void;
	/** What became of a call whose answer was lost, by its change id (`msg.status`): reconcile before any retry (CHAT-40). */
	status(organizationId: string, changeId: string): Promise<ChatCallStatus>;
}

/**
 * The mock's states (chat-ux §6), chosen with `?chat=`: `offline` (read-only from the last copy), `nocopy` (offline with no
 * copy: unavailable), `hq` (HQ can't be reached: messages to people and to agents outside this office wait as "Sending…"),
 * `fail` (a send's answer is lost before the office got it), `lost` (the office got it but the answer was lost), `hr-down`
 * (the HR lead can't run).
 */
export type ChatScenario = 'healthy' | 'offline' | 'nocopy' | 'hq' | 'fail' | 'lost' | 'hr-down';
const SCENARIOS: ChatScenario[] = ['offline', 'nocopy', 'hq', 'fail', 'lost', 'hr-down'];

const scenarioFromUrl = (): ChatScenario => {
	if (typeof location === 'undefined') return 'healthy';
	const wanted = new URLSearchParams(location.search).get('chat') as ChatScenario | null;
	return wanted && SCENARIOS.includes(wanted) ? wanted : 'healthy';
};

const COPY_AGE_MS = 25 * 60_000;

export interface MockChatOptions {
	now?: () => string;
	/** `offline`: the office isn't running (read-only). `fail`: every message fails once, and sends on Retry. */
	scenario?: () => ChatScenario;
	/** How long a woken agent shows as typing before it answers. */
	replyMs?: number;
	schedule?: (run: () => void, ms: number) => void;
}

const unavailable = (): ChatRefusal => ({
	source: 'api',
	code: 'unavailable',
	message: "The office didn't answer."
});

export function createMockChat(
	org: Pick<OrgApi, 'view'>,
	{
		now = () => new Date().toISOString(),
		scenario = scenarioFromUrl,
		replyMs = 1800,
		schedule = (run, ms) => void setTimeout(run, ms)
	}: MockChatOptions = {}
): ChatApi {
	const states = new Map<string, ChatState>();
	const listeners = new Map<string, Set<() => void>>();
	const failed = new Set<string>();
	// One call at a time per organization, in the order sent: each reads the state, so two at once would lose one.
	const queues = new Map<string, Promise<unknown>>();
	const notify = (organizationId: string) => listeners.get(organizationId)?.forEach((l) => l());

	const context = async (organizationId: string, extra: { newId?: (p: string) => string } = {}) => {
		const { snapshot } = await org.view(organizationId);
		return chatContext(snapshot, {
			now: now(),
			typing_ms: replyMs + 1200,
			directory: snapshot.members.length > 1 ? SAMPLE_DIRECTORY : [],
			...extra
		});
	};
	const offline = () => scenario() === 'offline' || scenario() === 'nocopy';
	const load = async (organizationId: string): Promise<ChatState> => {
		const ctx = await context(organizationId);
		let state = states.get(organizationId);
		if (!state) {
			state = sampleChatFull(ctx);
			if (ctx.members.length <= 1) state = syncAuto(emptyChat(), ctx);
		}
		// The office keeps the automatic channels in step with the organization, and applies due defaults itself.
		state = tick(syncAuto(state, ctx), ctx.now);
		states.set(organizationId, state);
		return state;
	};

	const apply = async (organizationId: string, call: ChatCall): Promise<ChatAccepted> => {
		if (offline()) throw unavailable();
		const before = await load(organizationId);
		const ctx = await context(organizationId, { newId: (p) => `${p}-${before.seq + 1}` });
		// `fail`: a message goes wrong once, so "Not delivered" and Retry can be seen. A retry (same text) goes through.
		if (scenario() === 'fail' && call.method === 'msg.send' && !failed.has(call.body)) {
			failed.add(call.body);
			throw unavailable();
		}
		const { state, accepted } = applyChat(before, call, ctx);
		// `hq`: a message for someone outside this office is accepted and waits in the queue until HQ is back.
		if (scenario() === 'hq' && call.method === 'msg.send' && accepted.id) {
			const c = state.conversations.find((x) => x.id === accepted.conversation_id);
			const { snapshot } = await org.view(organizationId);
			const here = (id: string) =>
				snapshot.members.find((m) => m.id === id)?.office_id !== undefined;
			if (c?.members.some((id) => id !== ctx.actor && !here(id)))
				state.messages.find((m) => m.id === accepted.id)!.queued = true;
		}
		states.set(organizationId, state);
		notify(organizationId);
		// `lost`: the office has it, but the answer never reaches the app. Check status finds it.
		if (scenario() === 'lost' && call.method === 'msg.send' && !failed.has(call.change_id)) {
			failed.add(call.change_id);
			throw unavailable();
		}
		// A woken agent answers a moment later, with a canned line (sample data).
		if (call.method === 'msg.send' && accepted.id) {
			const sent = state.messages.find((m) => m.id === accepted.id)!;
			const c = state.conversations.find((x) => x.id === sent.conversation_id)!;
			const typing = state.typing.filter((t) => t.conversation_id === c.id).map((t) => t.member_id);
			if (c.kind === 'dm' && typing.length) {
				schedule(() => {
					const current = states.get(organizationId);
					if (!current) return;
					const reply: ChatMessage = {
						id: `m-r${current.seq + 1}`,
						conversation_id: c.id,
						from: typing[0],
						kind: 'reply',
						body: 'Sample reply: got it. I will only post again if something needs you.',
						created_at: now()
					};
					states.set(organizationId, {
						...current,
						seq: current.seq + 1,
						messages: [...current.messages, reply],
						typing: current.typing.filter(
							(t) => !(t.conversation_id === c.id && t.member_id === typing[0])
						)
					});
					notify(organizationId);
				}, replyMs);
			}
		}
		return accepted;
	};

	return {
		async snapshot(organizationId) {
			// No retained copy: unavailable, never an empty conversation (chat-ux §6).
			if (scenario() === 'nocopy')
				throw {
					source: 'api',
					code: 'unavailable',
					message: "The office isn't running, and there's no copy of the conversations to read."
				} satisfies ChatRefusal;
			const state = await load(organizationId);
			const ctx = await context(organizationId);
			const directory = ctx.members.filter((m) => SAMPLE_DIRECTORY.some((d) => d.id === m.id));
			return {
				state,
				sample: true,
				office_running: !offline(),
				...(offline() ? { copy_at: new Date(Date.parse(now()) - COPY_AGE_MS).toISOString() } : {}),
				...(directory.length ? { directory } : {}),
				...(scenario() === 'hr-down' && directory.some((m) => m.id === 'hr')
					? { cannot_run: ['hr'] }
					: {})
			};
		},
		async status(organizationId, changeId) {
			const state = await load(organizationId);
			const accepted = state.applied[changeId];
			return accepted ? { state: 'applied', accepted } : { state: 'not_found' };
		},
		command(organizationId, call) {
			const run = (queues.get(organizationId) ?? Promise.resolve()).then(() =>
				apply(organizationId, call)
			);
			queues.set(
				organizationId,
				run.catch(() => undefined)
			);
			return run;
		},
		subscribe(organizationId, listener) {
			const set = listeners.get(organizationId) ?? new Set();
			set.add(listener);
			listeners.set(organizationId, set);
			return () => void set.delete(listener);
		}
	};
}
