import type { OrgApi } from '$lib/org/api';
import type { ChatApi } from './api';
import { applyChat, emptyChat, syncAuto, tick } from './engine';
import { chatContext } from './model';
import { sampleChat } from './sample';
import type { ChatAccepted, ChatCall, ChatMessage, ChatRefusal, ChatState } from './types';

export type ChatScenario = 'healthy' | 'offline' | 'fail';

const scenarioFromUrl = (): ChatScenario => {
	if (typeof location === 'undefined') return 'healthy';
	const wanted = new URLSearchParams(location.search).get('chat');
	return wanted === 'offline' || wanted === 'fail' ? wanted : 'healthy';
};

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
		return chatContext(snapshot, { now: now(), typing_ms: replyMs + 1200, ...extra });
	};
	const load = async (organizationId: string): Promise<ChatState> => {
		const ctx = await context(organizationId);
		let state = states.get(organizationId);
		if (!state) {
			state = sampleChat(ctx);
			if (ctx.members.length <= 1) state = syncAuto(emptyChat(), ctx);
		}
		// The office keeps the automatic channels in step with the organization, and applies due defaults itself.
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
		if (scenario() === 'offline') throw unavailable();
		const before = await load(organizationId);
		const ctx = await context(organizationId, { newId: (p) => `${p}-${before.seq + 1}` });
		// `fail`: a message goes wrong once, so "Not delivered" and Retry can be seen. A retry (same text) goes through.
		if (scenario() === 'fail' && call.method === 'msg.send' && !failed.has(call.body)) {
			failed.add(call.body);
			throw unavailable();
		}
		const { state, accepted } = applyChat(before, call, ctx);
		states.set(organizationId, state);
		notify(organizationId);
		// A woken agent answers a moment later, with a canned line (sample data).
		if (call.method === 'msg.send' && accepted.id)
			scheduleWokenReply(organizationId, state, accepted.id);
		return accepted;
	};

	return {
		async snapshot(organizationId) {
			return {
				state: await load(organizationId),
				sample: true,
				office_running: scenario() !== 'offline'
			};
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
