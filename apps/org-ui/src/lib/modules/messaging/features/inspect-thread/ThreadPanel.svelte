<script lang="ts">
	import type { Snippet } from 'svelte';
	import X from '@lucide/svelte/icons/x';
	import { Button } from '$lib/components/ui/button';
	import type { ChatContext } from '$lib/modules/messaging/engine';
	import { composerState, repliesTo, titleOf } from '$lib/modules/messaging/model';
	import type { ChatMessage, ChatState, Conversation, MuteMode, Reaction } from '$lib/modules/messaging/types';
	import Composer from '$lib/modules/messaging/features/send-message/Composer.svelte';
	import MessageRow from '../../components/MessageRow.svelte';
	import { useDraftStore } from '../../drafts/context';

	/** A thread on the right (CHAT-8): the message, its replies, and a box that can also send to the channel. */
	let {
		root,
		conversation,
		ctx,
		chatState,
		sidebarInline = true,
		now,
		officeRunning,
		readOnly = false,
		onclose,
		undelivered,
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
		/** Whether the conversation list occupies its inline 256px column. */
		sidebarInline?: boolean;
		now: string;
		officeRunning: boolean;
		/** The bridge preview: asks to the owner and approvals are read-only, and nothing is posted. */
		readOnly?: boolean;
		onclose: () => void;
		undelivered?: Snippet;
		onsend: (text: string, options: { alsoInChannel: boolean; attachments: string[]; asMessage?: boolean }) => Promise<boolean | void> | boolean | void;
		onreact: (m: ChatMessage, r: Reaction) => void;
		onturn: (m: ChatMessage, into: 'bug' | 'request' | 'task') => void;
		onmute: (m: ChatMessage, mode: MuteMode) => void;
		onpin: (m: ChatMessage) => void;
		onanswer: (m: ChatMessage, answer: { option_id?: string; text?: string }) => Promise<void> | void;
		onaccept: (m: ChatMessage) => Promise<void> | void;
	} = $props();

	const draftStore = useDraftStore();
	const threadId = $derived(root.id);
	const drafts = $derived(draftStore.forThread(threadId));
	const replies = $derived(repliesTo(chatState, root.id));
	const compose = $derived(composerState(chatState, ctx, conversation, officeRunning));
	const title = $derived(titleOf(conversation, ctx, ctx.owner));
	let closeButton = $state<HTMLButtonElement | HTMLAnchorElement | null>(null);
	export function focusClose() {
		closeButton?.focus({ preventScroll: true });
		closeButton?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
	}
</script>

<aside data-chat-panel class="absolute inset-y-0 right-0 {sidebarInline ? 'left-[256px]' : 'left-0'} z-30 flex h-full min-w-0 shrink-0 flex-col overflow-y-auto shadow-xl min-[1280px]:static min-[1280px]:w-[384px] min-[1280px]:shadow-none border-l border-white/10 bg-gray-800" aria-label="Thread" data-testid="thread-panel">
	<div class="flex items-center justify-between gap-3 border-b border-white/10 px-4 py-3">
		<h2 class="min-w-0 text-sm/6 font-semibold break-words text-white">Thread <span class="font-normal text-gray-400">in {title}</span></h2>
		<Button type="button" variant="ghost" size="icon-sm" class="scroll-mt-16" aria-label="Close thread" bind:ref={closeButton} onclick={onclose}><X aria-hidden="true" /></Button>
	</div>
	<div class="min-h-16 flex-1 space-y-1 overflow-y-auto px-2 py-3 max-[640px]:px-[8px]" data-focus-scope>
		{#key threadId}
			<div class="space-y-1" role="log" aria-label="Thread messages" aria-live="polite" aria-relevant="additions">
				<MessageRow message={root} {ctx} {chatState} {now} {readOnly} {officeRunning} inThread askDraftFor={drafts.ask} {onreact} {onturn} {onmute} {onpin} {onanswer} {onaccept} />
				<p class="px-2 pt-2 text-xs/5 text-gray-400" aria-live="off" data-testid="thread-count">{replies.length} {replies.length === 1 ? 'reply' : 'replies'}</p>
				{#each replies as r (r.id)}
					<MessageRow message={r} {ctx} {chatState} {now} {readOnly} {officeRunning} inThread askDraftFor={drafts.ask} {onreact} {onturn} {onmute} {onpin} {onanswer} {onaccept} />
				{/each}
			</div>
		{/key}
		{@render undelivered?.()}
	</div>
	<Composer placeholder="Reply in thread" canWrite={compose.canWrite && !readOnly} note={compose.note} {conversation} {ctx} resetKey="thread:{root.id}" draft={drafts.reply} alsoIn={conversation.kind === 'channel' ? title : undefined} {onsend} />
</aside>
