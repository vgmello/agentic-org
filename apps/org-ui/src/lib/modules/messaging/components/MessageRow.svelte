<script lang="ts">
	import Ellipsis from '@lucide/svelte/icons/ellipsis';
	import HeroIcon from '$lib/components/HeroIcon.svelte';
	import type { MicroIconName } from '$lib/components/heroicons';
	import MessageSquareReply from '@lucide/svelte/icons/message-square-reply';
	import { Button } from '$lib/components/ui/button';
	import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '$lib/components/ui/dropdown-menu';
	import type { ChatContext } from '../engine';
	import type { DraftText } from '../drafts/store';
	import { clockTime, collapse, foldedWords, KIND_WORD, muteOf, nameOf, repliesTo } from '../model';
	import { REASON_LABEL, REACTIONS, type ChatMessage, type ChatState, type MuteMode, type Reaction } from '../types';
	import AskCard from '../features/answer-ask/AskCard.svelte';
	import Avatar from './Avatar.svelte';
	import Rich from './Rich.svelte';

	/** One message in a conversation, a thread, a search result or a list (CHAT-14). */
	let {
		message,
		ctx,
		chatState,
		now,
		readOnly = false,
		officeRunning = true,
		where,
		inThread = false,
		askDraftFor,
		delivery,
		onreact,
		onthread,
		onturn,
		onmute,
		onpin,
		onanswer,
		onaccept,
		onopen,
		onretry,
		ondismiss
	}: {
		message: ChatMessage;
		ctx: ChatContext;
		chatState: ChatState;
		now: string;
		readOnly?: boolean;
		officeRunning?: boolean;
		where?: string;
		inThread?: boolean;
		askDraftFor?: (askId: string) => DraftText;
		delivery?: { status: 'sending' | 'failed'; error?: string };
		onreact?: (m: ChatMessage, r: Reaction) => void;
		onthread?: (m: ChatMessage, opener: HTMLElement) => void;
		onturn?: (m: ChatMessage, into: 'bug' | 'request' | 'task') => void;
		onmute?: (m: ChatMessage, mode: MuteMode) => void;
		onpin?: (m: ChatMessage) => void;
		onanswer?: (m: ChatMessage, answer: { option_id?: string; text?: string }) => Promise<void> | void;
		onaccept?: (m: ChatMessage) => Promise<void> | void;
		onopen?: (m: ChatMessage) => void;
		onretry?: (button: HTMLButtonElement) => void;
		ondismiss?: () => void;
	} = $props();

	const actionFocus = 'focus-visible:outline-solid focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400 focus-visible:ring-0';
	const kindIcon: Partial<Record<ChatMessage['kind'], MicroIconName>> = { request: 'question-mark-circle', announcement: 'megaphone', handoff: 'arrows-right-left' };
	const reactionIcon: Record<Reaction, MicroIconName> = { seen: 'check-circle', looking: 'eye' };
	let expanded = $state(false);
	const from = $derived(ctx.members.find((m) => m.id === message.from));
	const replies = $derived(inThread ? [] : repliesTo(chatState, message.id));
	const folded = $derived(foldedWords(chatState, ctx, message.id));
	const long = $derived(collapse(message.body));
	const mode = $derived(from?.kind === 'agent' ? muteOf(chatState, message.conversation_id, message.from) : 'none');
	const mark = $derived(kindIcon[message.kind]);
	const card = $derived(message.kind === 'request' || message.kind === 'handoff');
	const messageId = $derived(message.id);
	const isRequest = $derived(message.kind === 'request');
	const askDraft = $derived(isRequest ? askDraftFor?.(messageId) : undefined);
	const bridgeName = $derived({ slack: 'Slack', teams: 'Teams', discord: 'Discord' }[message.bridge ?? 'slack']);
	const reactions = $derived(REACTIONS.map((r) => ({ ...r, who: message.reactions?.[r.id] ?? [] })));
</script>

<!-- Metadata threshold: (212.4375 kind + 72 avatar + 24 gap + 32 padding + 4 safety) / 32 = 10.765625rem rounded up. -->
<!-- Announcement is the widest calibrated KIND_WORD; message-layout.test.ts guards the vocabulary. -->
<div class="@container">
<article class="group relative grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 rounded-md px-2 py-2 max-[640px]:gap-x-[12px] max-[640px]:px-[8px] hover:bg-white/[0.03] focus-within:bg-white/[0.03]" data-testid="message" data-kind={message.kind} data-id={message.id} aria-label="{nameOf(ctx, message.from)}, {clockTime(message.created_at)}">
	<div class="row-span-2 @max-[10.765625rem]:row-span-1"><Avatar name={nameOf(ctx, message.from)} kind={from?.kind ?? 'person'} /></div>
		<div class="col-start-2 flex min-w-0 flex-wrap items-baseline gap-x-2 gap-y-1 text-sm/6 @max-[10.765625rem]:col-span-2 @max-[10.765625rem]:col-start-1">
			<span class="font-semibold text-white">{nameOf(ctx, message.from)}</span>
			<time class="text-xs/5 text-gray-400" datetime={message.created_at}>{clockTime(message.created_at)}</time>
			{#if mark}<span class="text-xs/5 text-gray-300" title={KIND_WORD[message.kind]}><span class="inline-flex items-center gap-x-1 whitespace-nowrap"><HeroIcon class="text-current" variant="micro" name={mark} />{KIND_WORD[message.kind]}</span>{#if message.reason}{' · ' + REASON_LABEL[message.reason]}{/if}</span>{/if}
			{#if message.bridge}<span class="rounded bg-white/10 px-1.5 text-[10px]/4 font-semibold text-gray-200" data-testid="bridge-badge" title="Posted from {bridgeName} as {nameOf(ctx, message.from)}, never as the owner">{bridgeName}</span>{/if}
			{#if message.also_in_channel}<span class="text-xs/5 text-gray-400">replied to a thread</span>{/if}
			{#if message.pinned}<span class="text-xs/5 text-gray-400" title="Pinned">Pinned</span>{/if}
			{#if where}<span class="text-xs/5 text-gray-400">{'in ' + where}</span>{/if}
			{#if mode !== 'none'}<span class="text-xs/5 text-gray-400">{mode === 'muted' ? 'muted' : 'digest'}</span>{/if}
		</div>
	<div class="col-start-2 min-w-0">
		{#if card}
			<div class="mt-1">
				<AskCard {message} {ctx} {now} {readOnly} {officeRunning} draft={askDraft} onanswer={(a) => onanswer?.(message, a)} onaccept={() => onaccept?.(message)} onreplyinthread={inThread ? undefined : (opener) => onthread?.(message, opener)} />
			</div>
		{:else}
			<div class="mt-0.5" data-testid="message-body">
				<Rich body={expanded ? message.body : long.shown} />
				{#if long.hidden > 0}
					<button type="button" class="mt-1 text-sm/6 font-semibold text-indigo-300 hover:text-indigo-200" aria-expanded={expanded} onclick={() => (expanded = !expanded)}>{expanded ? 'Show less' : `Show more (${long.hidden} more lines)`}</button>
				{/if}
			</div>
		{/if}

		{#if message.attachments?.length}
			<ul class="mt-1 flex flex-wrap gap-1.5" aria-label="Attachments">
				{#each message.attachments as a (a)}<li class="inline-flex max-w-full items-start gap-x-1 rounded bg-white/10 px-2 py-0.5 text-xs/5 text-gray-200"><HeroIcon class="mt-0.5" variant="micro" name="paper-clip" /><span class="min-w-0 [overflow-wrap:anywhere]">{a}</span></li>{/each}
			</ul>
		{/if}

		{#if reactions.some((r) => r.who.length)}
			<div class="mt-1 flex flex-wrap gap-1.5" aria-live="off">
				{#each reactions as r (r.id)}
					{#if r.who.length}
						<button type="button" class="inline-flex items-center gap-x-1 rounded-full px-2 py-0.5 text-xs/5 disabled:opacity-50 focus-visible:outline-solid focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400 outline-1 -outline-offset-1 {r.who.includes(ctx.actor) ? 'bg-indigo-500/20 text-indigo-100 outline-indigo-400/50' : 'bg-white/5 text-gray-200 outline-white/10'}" disabled={!officeRunning} aria-pressed={r.who.includes(ctx.actor)} aria-label="{r.label}, {r.who.length}" title={r.label} onclick={() => onreact?.(message, r.id)}><HeroIcon class="text-current" variant="micro" name={reactionIcon[r.id]} /> {r.who.length}</button>
					{/if}
				{/each}
			</div>
		{/if}

		{#if replies.length && onthread}
			<button type="button" class="mt-1 text-xs/5 font-semibold text-indigo-300 hover:text-indigo-200" aria-live="off" onclick={(event) => onthread?.(message, event.currentTarget)} data-testid="replies-link">{replies.length} {replies.length === 1 ? 'reply' : 'replies'}{#if folded}{' · ' + folded}{/if}</button>
		{/if}

		{#if onopen}
			<button type="button" class="mt-1 block text-xs/5 font-semibold text-indigo-300 hover:text-indigo-200" onclick={() => onopen?.(message)}>Open in conversation</button>
		{/if}

		{#if delivery?.status === 'sending'}
			<p class="mt-1 text-xs/5 text-gray-400" data-testid="sending">Sending…</p>
		{:else if delivery?.status === 'failed'}
			<p class="mt-1 text-xs/5 text-red-400" role="alert" data-testid="not-delivered">Not delivered: {delivery.error} <button type="button" class="font-semibold underline" onclick={(event) => onretry?.(event.currentTarget)}>Retry</button> · <button type="button" class="underline" onclick={ondismiss}>Dismiss</button></p>
		{/if}
	</div>

	{#if !delivery && !readOnly}
		<div class="absolute -top-3 right-2 flex items-center gap-0.5 rounded-md bg-gray-900 p-0.5 opacity-0 shadow ring-1 ring-white/10 group-focus-within:opacity-100 group-hover:opacity-100" role="toolbar" aria-label="Message actions">
			{#each REACTIONS as r (r.id)}
				<Button class={actionFocus} type="button" variant="ghost" size="icon-xs" aria-label="React {r.label}" title={r.label} disabled={!officeRunning} onclick={() => onreact?.(message, r.id)}><HeroIcon class="text-current" variant="micro" name={reactionIcon[r.id]} /></Button>
			{/each}
			{#if !inThread && onthread}
				<Button class={actionFocus} type="button" variant="ghost" size="icon-xs" aria-label="Reply in thread" title="Reply in thread" onclick={(event) => onthread?.(message, event.currentTarget)}><MessageSquareReply aria-hidden="true" /></Button>
			{/if}
			<DropdownMenu>
				<DropdownMenuTrigger>
					{#snippet child({ props })}
						<Button {...props} class={actionFocus} type="button" variant="ghost" size="icon-xs" aria-label="More actions" data-testid="message-menu"><Ellipsis aria-hidden="true" /></Button>
					{/snippet}
				</DropdownMenuTrigger>
				<DropdownMenuContent align="end">
					<DropdownMenuItem onSelect={() => onturn?.(message, 'bug')}>Turn into a bug…</DropdownMenuItem>
					<DropdownMenuItem onSelect={() => onturn?.(message, 'request')}>Turn into a request…</DropdownMenuItem>
					<DropdownMenuItem onSelect={() => onturn?.(message, 'task')}>Turn into a task…</DropdownMenuItem>
					<DropdownMenuSeparator />
					<DropdownMenuItem disabled={!officeRunning} onSelect={() => onpin?.(message)}>{message.pinned ? 'Unpin' : 'Pin to the conversation'}</DropdownMenuItem>
					{#if from?.kind === 'agent' && onmute}
						<DropdownMenuSeparator />
						{#if mode !== 'muted'}<DropdownMenuItem disabled={!officeRunning} onSelect={() => onmute?.(message, 'muted')}>Mute {from.name} here</DropdownMenuItem>{/if}
						{#if mode !== 'digest'}<DropdownMenuItem disabled={!officeRunning} onSelect={() => onmute?.(message, 'digest')}>Daily digest from {from.name}</DropdownMenuItem>{/if}
						{#if mode !== 'none'}<DropdownMenuItem disabled={!officeRunning} onSelect={() => onmute?.(message, 'none')}>Show all from {from.name}</DropdownMenuItem>{/if}
					{/if}
				</DropdownMenuContent>
			</DropdownMenu>
		</div>
	{/if}
</article>
</div>
