<script lang="ts">
	import X from '@lucide/svelte/icons/x';
	import { Button } from '$lib/components/ui/button';
	import type { ChatContext } from '$lib/modules/messaging/engine';
	import { composerState, repliesTo, titleOf } from '$lib/modules/messaging/model';
	import type { ChatMessage, ChatState, Conversation, MuteMode, Reaction } from '$lib/modules/messaging/types';
	import Composer from '$lib/modules/messaging/features/send-message/Composer.svelte';
	import MessageRow from '$lib/modules/messaging/features/send-message/MessageRow.svelte';

	/** A thread on the right (CHAT-8): the message, its replies, and a box that can also send to the channel. */
	let {
		root,
		conversation,
		ctx,
		chatState,
		now,
		officeRunning,
		readOnly = false,
		onclose,
		onsend,
		onreact,
		onturn,
		onmute,
		onpin,
		onanswer,
		onaccept
	}: {
		root: ChatMessage;
		conversation: Conversation;
		ctx: ChatContext;
		chatState: ChatState;
		now: string;
		officeRunning: boolean;
		/** The bridge preview: asks to the owner and approvals are read-only, and nothing is posted. */
		readOnly?: boolean;
		onclose: () => void;
		onsend: (text: string, options: { alsoInChannel: boolean; attachments: string[] }) => Promise<boolean | void> | boolean | void;
		onreact: (m: ChatMessage, r: Reaction) => void;
		onturn: (m: ChatMessage, into: 'bug' | 'request' | 'task') => void;
		onmute: (m: ChatMessage, mode: MuteMode) => void;
		onpin: (m: ChatMessage) => void;
		onanswer: (m: ChatMessage, answer: { option_id?: string; text?: string }) => Promise<void> | void;
		onaccept: (m: ChatMessage) => Promise<void> | void;
	} = $props();

	const replies = $derived(repliesTo(chatState, root.id));
	const compose = $derived(composerState(chatState, ctx, conversation, officeRunning));
	const title = $derived(titleOf(conversation, ctx, ctx.owner));
</script>

<aside class="absolute inset-y-0 right-0 z-30 flex h-full w-96 max-w-full shrink-0 flex-col shadow-xl xl:static xl:shadow-none border-l border-white/10 bg-gray-800" aria-label="Thread" data-testid="thread-panel">
	<div class="flex items-center justify-between border-b border-white/10 px-4 py-3">
		<h2 class="text-sm/6 font-semibold text-white">Thread <span class="font-normal text-gray-400">in {title}</span></h2>
		<Button type="button" variant="ghost" size="icon-sm" aria-label="Close thread" onclick={onclose}><X aria-hidden="true" /></Button>
	</div>
	<div class="min-h-0 flex-1 space-y-1 overflow-y-auto px-2 py-3">
		<MessageRow message={root} {ctx} {chatState} {now} {readOnly} inThread {onreact} {onturn} {onmute} {onpin} {onanswer} {onaccept} />
		<p class="px-2 pt-2 text-xs/5 text-gray-400" data-testid="thread-count">{replies.length} {replies.length === 1 ? 'reply' : 'replies'}</p>
		{#each replies as r (r.id)}
			<MessageRow message={r} {ctx} {chatState} {now} {readOnly} inThread {onreact} {onturn} {onmute} {onpin} {onanswer} {onaccept} />
		{/each}
	</div>
	<Composer placeholder="Reply in thread" canWrite={compose.canWrite && !readOnly} note={compose.note} {conversation} {ctx} resetKey="thread:{root.id}" alsoIn={conversation.kind === 'channel' ? title : undefined} {onsend} />
</aside>
