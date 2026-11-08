// Shared by the chat tests: the sample organization, a fixed clock and small helpers.
import { sampleOrg } from '$lib/org/sample';
import { applyChat, type ChatContext } from './engine';
import { chatContext } from './model';
import { sampleChat } from './sample';
import type { ChatAccepted, ChatCall, ChatMessage, ChatState } from './types';

export const NOW = '2026-11-08T12:00:00.000Z';
export const org = sampleOrg('office-1');
export const minutesAfter = (iso: string, minutes: number) => new Date(Date.parse(iso) + minutes * 60_000).toISOString();

export const ctxAs = (actor = 'you', surface: ChatContext['surface'] = 'app', now = NOW, extra: Partial<ChatContext> = {}): ChatContext => ({ ...chatContext(org, { now, actor, surface }), ...extra });
export const fresh = (): ChatState => sampleChat(ctxAs());

let n = 0;
type Draft = ChatCall extends infer C ? (C extends unknown ? Omit<C, 'change_id'> : never) : never;
export const call = (draft: Draft): ChatCall => ({ ...draft, change_id: `chg-${++n}` }) as ChatCall;

/** Apply a call and return the new state and the answer. */
export function run(state: ChatState, draft: Draft, ctx: ChatContext = ctxAs()): { state: ChatState; accepted: ChatAccepted } {
	return applyChat(state, call(draft), ctx);
}

/** The error a call is refused with, or undefined when it goes through. */
export function refusal(state: ChatState, draft: Draft, ctx: ChatContext = ctxAs()): { code: string; message: string } | undefined {
	try {
		run(state, draft, ctx);
		return undefined;
	} catch (e) {
		return e as { code: string; message: string };
	}
}

export const byId = (state: ChatState, id: string): ChatMessage => state.messages.find((m) => m.id === id)!;
export const conv = (state: ChatState, id: string) => state.conversations.find((c) => c.id === id)!;
export const channelNamed = (state: ChatState, name: string) => state.conversations.find((c) => c.kind === 'channel' && c.name === name)!;
/** The sample's ask for the owner with options (Kai's, in #atlas). */
export const kaiAsk = (state: ChatState) => state.messages.find((m) => m.kind === 'request' && m.from === 'kai')!;
export const miraApproval = (state: ChatState) => state.messages.find((m) => m.approval)!;
