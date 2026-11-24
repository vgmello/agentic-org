import { orgApi } from '$lib/modules/organization/api';
import type { OrgApi } from '../../../src/lib/modules/organization/api';
import { chatSender, type ChatApi, type ChatDraft } from '../../../src/lib/modules/messaging/api';
import { applyChat, emptyChat, syncAuto, tick } from '../../../src/lib/modules/messaging/engine';
import { chatContext } from '../../../src/lib/modules/messaging/model';
import type {
	ChatAccepted,
	ChatCall,
	ChatMessage,
	ChatRefusal,
	ChatState
} from '../../../src/lib/modules/messaging/types';
import { SAMPLE_DIRECTORY, sampleChatFull } from './chat-sample';

export { chatSender, type ChatApi, type ChatDraft };

/**
 * The mock's states (chat-ux §6), chosen with `?chat=`:
 * - `offline`: read-only from the last copy (with `copy_at` freshness)
 * - `nocopy`: offline with no copy (unavailable error with Retry)
 * - `hq`: HQ unreachable (messages to members outside this office wait as "Sending…")
 * - `fail`: send answer lost before the office got it (Check status -> not_found -> Retry sends once)
 * - `lost`: office applied it but the answer was lost (Check status -> applied -> reconciles)
 * - `hr-down`: the HR lead can't run right now
 * - `error`: first snapshot fails once so Retry can be exercised
 */
export type ChatScenario =
	'healthy' | 'offline' | 'nocopy' | 'hq' | 'fail' | 'lost' | 'hr-down' | 'error';

const SCENARIOS: readonly ChatScenario[] = [
	'offline',
	'nocopy',
	'hq',
	'fail',
	'lost',
	'hr-down',
	'error'
];

const scenarioFromUrl = (): ChatScenario => {
	if (typeof location === 'undefined') return 'healthy';
	const wanted = new URLSearchParams(location.search).get('chat') as ChatScenario | null;
	return wanted && SCENARIOS.includes(wanted) ? wanted : 'healthy';
};

const COPY_AGE_MS = 25 * 60_000;

export interface MockChatOptions {
	now?: () => string;
	scenario?: () => ChatScenario;
	replyMs?: number;
	schedule?: (run: () => void, ms: number) => void;
}

const unavailable = (message = "The office didn't answer."): ChatRefusal => ({
	source: 'api',
	code: 'unavailable',
	message
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
	const errorShown = new Set<string>();
	const queues = new Map<string, Promise<unknown>>();
	const notify = (organizationId: string) => listeners.get(organizationId)?.forEach((l) => l());
	const offline = () => scenario() === 'offline' || scenario() === 'nocopy';

	const context = async (organizationId: string, extra: { newId?: (p: string) => string } = {}) => {
		const { snapshot } = await org.view(organizationId);
		return chatContext(snapshot, {
			now: now(),
			typing_ms: replyMs + 1200,
			directory: snapshot.members.length > 1 ? SAMPLE_DIRECTORY : [],
			...extra
		});
	};

	const load = async (organizationId: string): Promise<ChatState> => {
		const ctx = await context(organizationId);
		let state = states.get(organizationId);
		if (!state) {
			state = ctx.members.length <= 1 ? syncAuto(emptyChat(), ctx) : sampleChatFull(ctx);
		}
		state = tick(syncAuto(state, ctx), ctx.now);
		states.set(organizationId, state);
		return state;
	};

	const scheduleWokenReply = (organizationId: string, state: ChatState, messageId: string) => {
		const sent = state.messages.find((m) => m.id === messageId)!;
		const c = state.conversations.find((x) => x.id === sent.conversation_id)!;
		const typing = state.typing.filter((t) => t.conversation_id === c.id).map((t) => t.member_id);
		if (c.kind !== 'dm' || !typing.length) return;
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
	};

	const apply = async (organizationId: string, call: ChatCall): Promise<ChatAccepted> => {
		if (offline()) throw unavailable();
		const before = await load(organizationId);
		const ctx = await context(organizationId, { newId: (p) => `${p}-${before.seq + 1}` });
		if (scenario() === 'fail' && call.method === 'msg.send' && !failed.has(call.body)) {
			failed.add(call.body);
			throw unavailable();
		}
		const { state, accepted } = applyChat(before, call, ctx);
		if (scenario() === 'hq' && call.method === 'msg.send' && accepted.id) {
			const c = state.conversations.find((x) => x.id === accepted.conversation_id);
			const { snapshot } = await org.view(organizationId);
			const here = (id: string) =>
				snapshot.members.find((m) => m.id === id)?.office_id !== undefined;
			if (c?.members.some((id) => id !== ctx.actor && !here(id))) {
				state.messages.find((m) => m.id === accepted.id)!.queued = true;
			}
		}
		states.set(organizationId, state);
		notify(organizationId);
		if (scenario() === 'lost' && call.method === 'msg.send' && !failed.has(call.change_id)) {
			failed.add(call.change_id);
			throw unavailable();
		}
		if (call.method === 'msg.send' && accepted.id) {
			scheduleWokenReply(organizationId, state, accepted.id);
		}
		return accepted;
	};

	return {
		async snapshot(organizationId) {
			if (scenario() === 'nocopy') {
				throw unavailable(
					"The office isn't running, and there's no copy of the conversations to read."
				);
			}
			if (scenario() === 'error' && !errorShown.has(organizationId)) {
				errorShown.add(organizationId);
				throw unavailable('Could not load conversations from the office.');
			}
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

export const chatApi = createMockChat(orgApi);
export const sender = chatSender(chatApi);
