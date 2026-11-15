// Test-owned equivalent of Rowan's dd39d344 seed, also available for a channel.
import { sampleChat as base } from '../../src/lib/modules/messaging/sample';
import type { ChatContext } from '../../src/lib/modules/messaging/engine';
import type { ChatMessage, ChatState } from '../../src/lib/modules/messaging/types';

const bodies = [
	'Status: storage layer is done; waiting on the contract review before I start the sync work.',
	'Can you check **ac-T-17**? The branch is at `9c1e0b2` and the tests are green. See [the branch](https://example.com/b).',
	'Short one.',
	'Plan:\\n1. Finish the reader\\n2. Wire the gateway adapter\\n3. Ask Tomas for review\\n4. Merge after the go',
	'I looked at the failing test again. It is a timing issue in the fake clock, not in the product code; I will fix the fake and re-run the whole suite tonight so we have a clean receipt in the morning.'
];

export function sampleChat(ctx: ChatContext): ChatState {
	const state = base(ctx);
	const conversation = globalThis.localStorage?.getItem('memory.conversation') ?? 'c-dm-sam';
	const n = Number(globalThis.localStorage?.getItem('memory.messages') ?? 0);
	if (!Number.isInteger(n) || n < 1 || n > 1000 || !['c-dm-sam', 'c-general'].includes(conversation)) return state;
	const now = Date.parse(ctx.now);
	const extra: ChatMessage[] = Array.from({ length: n }, (_, i) => ({
		id: `m-memory-${i}`, conversation_id: conversation, from: i % 2 === 0 ? 'sam' : ctx.owner,
		kind: i % 2 === 0 ? 'reply' : 'msg', body: `#${i + 1} ${bodies[i % bodies.length]}`,
		created_at: new Date(now - (n - i) * 7 * 60_000 - 3 * 24 * 3600_000).toISOString()
	}));
	const retained = globalThis.localStorage?.getItem('memory.exact') === 'true'
		? state.messages.filter(message => message.conversation_id !== conversation)
		: state.messages;
	return { ...state, messages: [...extra, ...retained] };
}
