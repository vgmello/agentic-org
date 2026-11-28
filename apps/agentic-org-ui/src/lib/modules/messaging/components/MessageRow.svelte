<script lang="ts">
	import HeroIcon from '$lib/components/HeroIcon.svelte';
	import type { MicroIconName } from '$lib/components/heroicons';
	import Logo from '$lib/components/Logo.svelte';
	import AttachmentGallery from '$lib/components/attachments/AttachmentGallery.svelte';
	import MessageSquareReply from '@lucide/svelte/icons/message-square-reply';
	import { Button, buttonVariants } from '$lib/components/ui/button';
	import { ModalTooltip } from '$lib/components/ui/tooltip';
	import {
		DropdownMenu,
		DropdownMenuContent,
		DropdownMenuItem,
		DropdownMenuSeparator,
		DropdownMenuTrigger
	} from '$lib/components/ui/dropdown-menu';
	import { OFFICE, SYSTEM_NO_REPLY, type ChatContext } from '../engine';
	import type { DraftText } from '../drafts/store';
	import { clockTime, collapse, KIND_WORD, nameOf } from '../model';
	import {
		REASON_LABEL,
		REACTIONS,
		type ChatMessage,
		type MuteMode,
		type QuestionAnswer,
		type Reaction
	} from '../types';
	import type { MessageRowData } from '../row-data';
	import AskCard from '../features/answer-ask/AskCard.svelte';
	import Avatar from './Avatar.svelte';
	import Rich from './Rich.svelte';

	/** One message in a conversation, a thread, a search result or a list (CHAT-14). */
	let {
		message,
		ctx,
		rowData,
		now,
		readOnly = false,
		officeRunning = true,
		active = false,
		where,
		inThread = false,
		askDraftFor,
		delivery,
		onreact,
		onthread,
		onmenu,
		onturn,
		onmute,
		onpin,
		onanswer,
		onaccept,
		onhandback,
		onwithdraw,
		onopen,
		onretry,
		oncheck,
		ondismiss
	}: {
		message: ChatMessage;
		ctx: ChatContext;
		rowData: MessageRowData;
		now: string;
		readOnly?: boolean;
		officeRunning?: boolean;
		active?: boolean;
		where?: string;
		inThread?: boolean;
		askDraftFor?: (askId: string) => DraftText;
		/** A send in flight; `unknown`: the answer was lost, so the outcome is checked before any retry (chat-ux §6). */
		delivery?: { status: 'sending' | 'failed' | 'unknown' | 'checking'; error?: string };
		onreact?: (m: ChatMessage, r: Reaction) => void;
		onthread?: (m: ChatMessage, opener: HTMLElement) => void;
		onmenu?: (m: ChatMessage, open: boolean, opener: HTMLElement) => void;
		onturn?: (m: ChatMessage, into: 'bug' | 'request' | 'task', opener: HTMLElement) => void;
		onmute?: (m: ChatMessage, mode: MuteMode) => void;
		onpin?: (m: ChatMessage) => void;
		onanswer?: (
			m: ChatMessage,
			answer: {
				option_id?: string;
				text?: string;
				answers?: QuestionAnswer[];
				attachment_ids?: readonly string[];
			}
		) => Promise<void> | void;
		onaccept?: (m: ChatMessage) => Promise<void> | void;
		onhandback?: (m: ChatMessage, reason: string) => Promise<void> | void;
		onwithdraw?: (m: ChatMessage, reason: string) => Promise<void> | void;
		onopen?: (m: ChatMessage) => void;
		onretry?: (button: HTMLButtonElement) => void;
		oncheck?: (button: HTMLButtonElement) => void;
		ondismiss?: () => void;
	} = $props();

	const actionFocus =
		'focus-visible:outline-solid focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus focus-visible:ring-0';
	const kindIcon: Partial<Record<ChatMessage['kind'], MicroIconName>> = {
		request: 'question-mark-circle',
		announcement: 'megaphone',
		handoff: 'arrows-right-left'
	};
	const reactionIcon: Record<Reaction, MicroIconName> = { seen: 'check-circle', looking: 'eye' };
	let expanded = $state(false);
	let menuOpen = $state(false);
	let hovered = $state(false);
	let focusWithin = $state(false);
	let trigger = $state<HTMLButtonElement | null>(null);
	const toolbar = $derived(hovered || focusWithin || active);
	const from = $derived(ctx.members.find((m) => m.id === message.from));
	const replies = $derived(inThread ? [] : rowData.replies);
	const folded = $derived(rowData.folded);
	// Product notices show as System, with no person's affordances and no replies (chat-ux §4.2).
	const system = $derived(message.from === OFFICE);
	const replyable = $derived(!inThread && !!onthread && !system);
	const long = $derived(collapse(message.body));
	const mode = $derived(from?.kind === 'agent' ? rowData.mode : 'none');
	const mark = $derived(system ? undefined : kindIcon[message.kind]);
	const card = $derived(message.kind === 'request' || message.kind === 'handoff');
	const messageId = $derived(message.id);
	const isRequest = $derived(message.kind === 'request');
	const askDraft = $derived(isRequest ? askDraftFor?.(messageId) : undefined);
	const bridgeName = $derived(
		{ slack: 'Slack', teams: 'Teams', discord: 'Discord' }[message.bridge ?? 'slack']
	);
	const reactions = $derived(
		REACTIONS.map((r) => ({ ...r, who: message.reactions?.[r.id] ?? [] }))
	);
	const metadata = $derived(
		mark || message.bridge || message.also_in_channel || message.pinned || where || mode !== 'none'
	);
	// An announcement is acknowledged with Seen or Looking, so both stay in reach on it (chat-ux §2).
	const acknowledge = $derived(
		message.kind === 'announcement' && !system && !delivery && !readOnly
	);
	const hasReactions = $derived(acknowledge || reactions.some((r) => r.who.length));
	const whoWords = (ids: string[]) =>
		ids.length === 0
			? ''
			: ids.length <= 2
				? ids.map((id) => (id === ctx.actor ? 'you' : nameOf(ctx, id).split(' ')[0])).join(', ')
				: String(ids.length);
	const details = $derived(
		(!card && (message.attachment_meta?.length || message.attachments?.length)) ||
			hasReactions ||
			(replies.length && onthread) ||
			onopen ||
			delivery ||
			message.queued
	);
</script>

<!-- Metadata threshold: (212.4375 kind + 72 avatar + 24 gap + 32 padding + 4 safety) / 32 = 10.765625rem rounded up. -->
<!-- Announcement is the widest calibrated KIND_WORD; message-layout.test.ts guards the vocabulary. -->
<div class="@container">
	<article
		onpointerenter={() => (hovered = true)}
		onpointerleave={() => (hovered = false)}
		onfocusin={() => (focusWithin = true)}
		onfocusout={(event) =>
			(focusWithin =
				event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget))}
		class="group relative grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 rounded-md px-2 py-2 transition-colors [@media(width<=640px)]:gap-x-[12px] [@media(width<=640px)]:px-[8px] hover:bg-surface-raised focus-within:bg-surface-raised"
		data-testid="message"
		data-kind={message.kind}
		data-id={message.id}
		aria-label="{nameOf(ctx, message.from)}, {clockTime(message.created_at)}"
	>
		<div class="row-span-2 @max-[10.765625rem]:row-span-1">
			{#if system}<span class="flex size-9 items-center justify-center" data-testid="system-avatar"
					><Logo class="size-9 grayscale" /></span
				>{:else}<Avatar name={nameOf(ctx, message.from)} kind={from?.kind ?? 'person'} />{/if}
		</div>
		<div
			class="col-start-2 flex min-w-0 flex-wrap items-baseline gap-x-2 gap-y-1 text-sm/6 @max-[10.765625rem]:col-span-2 @max-[10.765625rem]:col-start-1"
		>
			<span class="font-semibold text-fg">{nameOf(ctx, message.from)}</span>
			<time class="text-xs/5 text-fg-muted" datetime={message.created_at}
				>{clockTime(message.created_at)}</time
			>
			{#if metadata}
				{#if mark}<span class="text-xs/5 text-fg-secondary" title={KIND_WORD[message.kind]}
						><span class="inline-flex items-center gap-x-1 whitespace-nowrap"
							><HeroIcon class="text-current" variant="micro" name={mark} />{KIND_WORD[
								message.kind
							]}</span
						>{#if message.reason}{' · ' + REASON_LABEL[message.reason]}{/if}</span
					>{/if}
				{#if message.bridge}<span
						class="rounded bg-surface-overlay px-1.5 text-xs/4 font-semibold text-fg-body"
						data-testid="bridge-badge"
						title="Posted from {bridgeName} as {nameOf(ctx, message.from)}, never as the owner"
						>{bridgeName}</span
					>{/if}
				{#if message.also_in_channel}<span class="text-xs/5 text-fg-muted">replied to a thread</span
					>{/if}
				{#if message.pinned}<span class="text-xs/5 text-fg-muted" title="Pinned">Pinned</span>{/if}
				{#if where}<span class="text-xs/5 text-fg-muted">{'in ' + where}</span>{/if}
				{#if mode !== 'none'}<span class="text-xs/5 text-fg-muted"
						>{mode === 'muted' ? 'muted' : 'digest'}</span
					>{/if}
			{/if}
		</div>
		<div class="col-start-2 min-w-0">
			{#if card}
				<div class="mt-1">
					{#key askDraft}
						<AskCard
							{message}
							{ctx}
							{now}
							{readOnly}
							{officeRunning}
							draft={askDraft}
							onanswer={(a) => onanswer?.(message, a)}
							onaccept={() => onaccept?.(message)}
							onhandback={onhandback ? (reason) => onhandback(message, reason) : undefined}
							onwithdraw={onwithdraw ? (reason) => onwithdraw(message, reason) : undefined}
							onreplyinthread={inThread ? undefined : (opener) => onthread?.(message, opener)}
						/>
					{/key}
				</div>
			{:else}
				<div class="mt-0.5 min-w-0 [&_a]:whitespace-normal" data-testid="message-body">
					<Rich body={expanded ? message.body : long.shown} />{#if long.hidden > 0}
						<button
							type="button"
							class="mt-1 text-sm/6 font-semibold text-brand-text hover:text-brand-text-strong"
							aria-expanded={expanded}
							onclick={() => (expanded = !expanded)}
							>{expanded ? 'Show less' : `Show more (${long.hidden} more lines)`}</button
						>
					{/if}
				</div>
			{/if}{#if details}
				{#if !card && !delivery && message.attachment_meta?.length}
					<AttachmentGallery attachments={message.attachment_meta} />
				{:else if !card && message.attachments?.length}
					<ul class="mt-1 flex flex-wrap gap-1.5" aria-label="Attachments">
						{#each message.attachments as a (a)}<li
								class="inline-flex max-w-full items-start gap-x-1 rounded bg-surface-overlay px-2 py-0.5 text-xs/5 text-fg-body"
							>
								<HeroIcon class="mt-0.5" variant="micro" name="paper-clip" /><span
									class="min-w-0 [overflow-wrap:anywhere]">{a}</span
								>
							</li>{/each}
					</ul>
				{/if}

				{#if hasReactions}
					<div class="mt-1 flex flex-wrap gap-1.5" aria-live="off">
						{#each reactions as r (r.id)}
							{#if r.who.length || acknowledge}
								<button
									type="button"
									class={[
										'inline-flex items-center gap-x-1 rounded-full px-2 py-0.5 text-xs/5 transition-colors disabled:opacity-50 focus-visible:outline-solid focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus',
										r.who.includes(ctx.actor)
											? 'chat-react-on font-medium'
											: 'bg-surface-raised text-fg-body outline-1 -outline-offset-1 outline-line hover:bg-surface-overlay'
									]}
									disabled={!officeRunning || readOnly}
									aria-pressed={r.who.includes(ctx.actor)}
									aria-label="{r.label}, {r.who.length}"
									title={r.who.length
										? `${r.label}: ${r.who.map((id) => nameOf(ctx, id)).join(', ')}`
										: r.label}
									onclick={() => onreact?.(message, r.id)}
									data-testid="reaction"
									><HeroIcon class="text-current" variant="micro" name={reactionIcon[r.id]} />
									{r.label}{#if r.who.length}<span class="text-fg-muted" aria-hidden="true"
											>{' · ' + whoWords(r.who)}</span
										>{/if}</button
								>
							{/if}
						{/each}
					</div>
				{/if}

				{#if replies.length && onthread}
					<button
						type="button"
						class="mt-1 text-xs/5 font-semibold text-brand-text hover:text-brand-text-strong"
						aria-live="off"
						onclick={(event) => onthread?.(message, event.currentTarget)}
						data-thread-opener={message.id}
						data-testid="replies-link"
						>{replies.length}
						{replies.length === 1 ? 'reply' : 'replies'}{#if folded}{' · ' + folded}{/if}</button
					>
				{/if}

				{#if onopen}
					<button
						type="button"
						class="mt-1 block text-xs/5 font-semibold text-brand-text hover:text-brand-text-strong"
						onclick={() => onopen?.(message)}>Open in conversation</button
					>
				{/if}

				{#if delivery?.status === 'sending' || delivery?.status === 'checking' || message.queued}
					<p class="mt-1 text-xs/5 text-fg-muted" data-testid="sending">
						{delivery?.status === 'checking' ? 'Checking status…' : 'Sending…'}
					</p>
				{:else if delivery?.status === 'unknown'}
					<p
						class="mt-1 text-xs/5 text-warning-text-strong"
						role="alert"
						data-testid="not-confirmed"
					>
						Delivery not confirmed
						<button
							type="button"
							class="font-semibold underline"
							onclick={(event) => oncheck?.(event.currentTarget)}>Check status</button
						>
					</p>
				{:else if delivery?.status === 'failed'}
					<p class="mt-1 text-xs/5 text-danger" role="alert" data-testid="not-delivered">
						Not delivered: {delivery.error}
						<button
							type="button"
							class="font-semibold underline"
							onclick={(event) => onretry?.(event.currentTarget)}>Retry</button
						>
						· <button type="button" class="underline" onclick={ondismiss}>Dismiss</button>
					</p>
				{/if}
			{/if}
		</div>

		{#if !delivery && !readOnly}
			<div
				class="absolute -top-3 right-2 flex items-center gap-0.5 rounded-md bg-surface-sunken p-0.5 shadow ring-1 ring-line {toolbar
					? 'opacity-100'
					: 'opacity-0'}"
				role="toolbar"
				aria-label="Message actions"
			>
				<DropdownMenu
					bind:open={menuOpen}
					onOpenChange={(open) => {
						if (trigger) onmenu?.(message, open, trigger);
					}}
				>
					<ModalTooltip label="Message actions" disabled={menuOpen}>
						<DropdownMenuTrigger bind:ref={trigger}>
							{#snippet child({ props })}
								<button
									{...props}
									class="{buttonVariants({ variant: 'ghost', size: 'icon-xs' })} {actionFocus}"
									type="button"
									aria-label="Actions for {nameOf(ctx, message.from)}'s message, {clockTime(
										message.created_at
									)}"
									data-testid="message-menu"
									><svg
										class="size-3"
										aria-hidden="true"
										focusable="false"
										viewBox="0 0 24 24"
										fill="none"
										stroke="currentColor"
										stroke-width="2"
										stroke-linecap="round"
										stroke-linejoin="round"
										><path
											d="M13 12a1 1 0 1 1-2 0 1 1 0 1 1 2 0Zm7 0a1 1 0 1 1-2 0 1 1 0 1 1 2 0ZM6 12a1 1 0 1 1-2 0 1 1 0 1 1 2 0Z"
										/></svg
									></button
								>
							{/snippet}
						</DropdownMenuTrigger>
					</ModalTooltip>{#if menuOpen}
						<DropdownMenuContent align="end">
							{#if system}
								<DropdownMenuItem disabled>{SYSTEM_NO_REPLY}</DropdownMenuItem>
							{:else}
								{#each REACTIONS as r (r.id)}
									<DropdownMenuItem
										disabled={!officeRunning}
										onSelect={() => onreact?.(message, r.id)}>React {r.label}</DropdownMenuItem
									>
								{/each}
							{/if}
							{#if replyable}
								<DropdownMenuItem
									onSelect={() => {
										if (trigger) onthread?.(message, trigger);
									}}>Reply in thread</DropdownMenuItem
								>
							{/if}
							<DropdownMenuSeparator />
							<DropdownMenuItem
								onSelect={() => {
									if (trigger) onturn?.(message, 'bug', trigger);
								}}>Turn into a bug…</DropdownMenuItem
							>
							<DropdownMenuItem
								onSelect={() => {
									if (trigger) onturn?.(message, 'request', trigger);
								}}>Turn into a request…</DropdownMenuItem
							>
							<DropdownMenuItem
								onSelect={() => {
									if (trigger) onturn?.(message, 'task', trigger);
								}}>Turn into a task…</DropdownMenuItem
							>
							<DropdownMenuSeparator />
							<DropdownMenuItem disabled={!officeRunning} onSelect={() => onpin?.(message)}
								>{message.pinned ? 'Unpin' : 'Pin to the conversation'}</DropdownMenuItem
							>
							{#if from?.kind === 'agent' && onmute}
								<DropdownMenuSeparator />
								{#if mode !== 'muted'}<DropdownMenuItem
										disabled={!officeRunning}
										onSelect={() => onmute?.(message, 'muted')}
										>Mute {from.name} here</DropdownMenuItem
									>{/if}
								{#if mode !== 'digest'}<DropdownMenuItem
										disabled={!officeRunning}
										onSelect={() => onmute?.(message, 'digest')}
										>Daily digest from {from.name}</DropdownMenuItem
									>{/if}
								{#if mode !== 'none'}<DropdownMenuItem
										disabled={!officeRunning}
										onSelect={() => onmute?.(message, 'none')}
										>Show all from {from.name}</DropdownMenuItem
									>{/if}
							{/if}
						</DropdownMenuContent>
					{/if}
				</DropdownMenu>{#if toolbar}
					{#if !system}
						{#each REACTIONS as r (r.id)}
							<ModalTooltip label={r.label}>
								<Button
									class={actionFocus}
									type="button"
									variant="ghost"
									size="icon-xs"
									aria-label="React {r.label}"
									disabled={!officeRunning}
									onclick={() => onreact?.(message, r.id)}
									><HeroIcon
										class="text-current"
										variant="micro"
										name={reactionIcon[r.id]}
									/></Button
								>
							</ModalTooltip>
						{/each}
					{/if}
					{#if replyable}
						<ModalTooltip label="Reply in thread">
							<Button
								class={actionFocus}
								type="button"
								variant="ghost"
								size="icon-xs"
								aria-label="Reply in thread"
								onclick={(event) => onthread?.(message, event.currentTarget)}
								><MessageSquareReply aria-hidden="true" /></Button
							>
						</ModalTooltip>
					{/if}
				{/if}
			</div>
		{/if}
	</article>
</div>

<style>
	.chat-react-on {
		background: var(--tone-accent-bg);
		color: var(--tone-accent-ink);
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
	}
</style>
