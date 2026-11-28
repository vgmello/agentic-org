<script lang="ts">
	import type { Snippet } from 'svelte';
	import {
		dayLabel,
		nameOf,
		titleOf,
		type ChatContext,
		type ChatMessage,
		type ChatView,
		type Conversation,
		type MessageRows
	} from '$lib/modules/messaging';
	import { MessageRow } from '$lib/modules/messaging/ui';
	import type { ConversationView, Sidebar } from '$lib/modules/messaging/model';
	import type { ChatState } from '$lib/modules/messaging/types';
	import { dayChanged } from '../../helpers';
	import { trackFeedScroll, type FeedScrollState } from './chat-scroll';

	const SEARCH_FILTERS = [
		'from:@name',
		'in:#channel',
		'member:@name',
		'has:ask',
		'has:thread',
		'has:image',
		'has:file',
		'has:attachment',
		'kind:announcement',
		'kind:handoff',
		'before:2026-11-08',
		'after:2026-11-01'
	];

	let {
		listEl = $bindable<HTMLElement | undefined>(),
		searching,
		results,
		view,
		bar,
		conversation,
		convView,
		chatState,
		ctx,
		rows,
		now,
		newAt,
		highlight,
		showMuted = $bindable(false),
		openDigest = $bindable<string | undefined>(),
		channelRowLive,
		rowFor,
		undelivered,
		onopenthread,
		onScrollStateChange
	}: {
		listEl?: HTMLElement;
		searching: boolean;
		results: ChatMessage[];
		view: ChatView;
		bar: Sidebar;
		conversation?: Conversation;
		convView?: ConversationView;
		chatState: ChatState;
		ctx: ChatContext;
		rows: MessageRows;
		now: string;
		newAt?: string;
		highlight?: string;
		showMuted?: boolean;
		openDigest?: string;
		channelRowLive: (message: ChatMessage) => 'off' | undefined;
		rowFor: Snippet<[ChatMessage, { where?: string; open?: boolean }?]>;
		undelivered: Snippet;
		onopenthread: (message: ChatMessage, opener: HTMLElement) => void;
		onScrollStateChange?: (state: FeedScrollState) => void;
	} = $props();

	const feedVersion = $derived(
		`${conversation?.id ?? ''}:${convView?.shown.length ?? 0}:${convView?.shown[convView.shown.length - 1]?.id ?? ''}:${highlight ?? ''}`
	);
</script>

<div
	bind:this={listEl}
	use:trackFeedScroll={{
		active: Boolean(conversation && convView && !searching && view.type === 'conversation'),
		version: feedVersion,
		onState: (state) => onScrollStateChange?.(state)
	}}
	data-focus-scope
	class="chat-msgs-canvas min-h-16 flex-1 overflow-y-auto px-3 py-3 max-[640px]:px-[8px]"
	data-testid="message-list"
	tabindex="-1"
	role="region"
	aria-label="Message area"
>
	{#if searching}
		<div class="px-2 pb-3">
			<p class="text-sm/6 text-fg-secondary" data-testid="search-count">
				{results.length}
				{results.length === 1 ? 'result' : 'results'}
			</p>
			<ul
				class="mt-1 flex flex-wrap items-center gap-1.5 text-xs/5 text-fg-muted"
				aria-label="Search filters"
				data-testid="search-help"
			>
				<li>Filter by</li>
				{#each SEARCH_FILTERS as f (f)}
					<li class="rounded bg-surface-overlay px-1.5 py-0.5 font-mono text-fg-body">{f}</li>
				{/each}
			</ul>
		</div>
		<div class="space-y-1">
			{#each results as m (m.id)}
				{@const c = chatState.conversations.find((x) => x.id === m.conversation_id)}
				{@render rowFor(m, { where: c ? titleOf(c, ctx, ctx.owner) : undefined, open: true })}
			{:else}
				<p class="px-2 text-sm/6 text-fg-muted" data-testid="no-results">Nothing matches.</p>
			{/each}
		</div>
	{:else if view.type === 'mentions'}
		<div class="space-y-1">
			{#each bar.mentions as m (m.id)}
				{@const c = chatState.conversations.find((x) => x.id === m.conversation_id)}
				{@render rowFor(m, { where: c ? titleOf(c, ctx, ctx.owner) : undefined, open: true })}
			{:else}
				<p class="px-2 py-8 text-center text-sm/6 text-fg-muted">Nobody has mentioned you.</p>
			{/each}
		</div>
	{:else if view.type === 'threads'}
		<ul class="mx-auto max-w-3xl space-y-2" data-testid="threads-list">
			{#each bar.threads as t (t.root.id)}
				<li>
					<button
						type="button"
						class="w-full rounded-lg border border-line bg-tint/[0.03] p-3 text-left hover:bg-surface-raised"
						onclick={(event) => onopenthread(t.root, event.currentTarget)}
					>
						<span class="block text-xs/5 text-fg-muted"
							>{titleOf(t.conversation, ctx, ctx.owner)} · {nameOf(ctx, t.root.from)}</span
						>
						<span class="mt-0.5 block truncate text-sm/6 text-fg"
							>{t.root.body.trim()
								? t.root.body.split('\n')[0]
								: (t.root.attachment_meta?.[0]?.filename ??
									t.root.attachments?.[0] ??
									'Attachment')}</span
						>
						<span class="mt-1 block text-xs/5 text-brand-text"
							>{t.replies}
							{t.replies === 1 ? 'reply' : 'replies'}{#if t.new_replies}{' · ' +
									t.new_replies +
									' new'}{/if}</span
						>
					</button>
				</li>
			{:else}
				<li class="px-2 py-8 text-center text-sm/6 text-fg-muted">No threads with you in them.</li>
			{/each}
		</ul>
	{:else if conversation && convView}
		{#key conversation.id}
			<div role="log" aria-label="Messages" aria-live="polite" aria-relevant="additions">
				{#if !conversation.members.includes(ctx.owner)}
					<p
						class="mb-2 rounded-md bg-surface-raised px-3 py-2 text-sm/6 text-fg-secondary"
						data-testid="reading-as-owner"
					>
						You're reading as the owner. This conversation is read-only.
					</p>
				{/if}
				{#each convView.shown as m, i (m.id)}
					{#if dayChanged(convView.shown, i, now)}
						{@const label = dayLabel(m.created_at, now)}
						{#if label === 'Today'}
							{#if i > 0}
								<div
									class="chat-day is-today"
									aria-hidden="true"
									data-testid="chat-day-today"
								></div>
							{/if}
						{:else}
							<div class="chat-day" aria-live="off" data-testid="chat-day-divider">
								<span class="chat-day-text">{label}</span>
							</div>
						{/if}
					{/if}
					{#if m.id === newAt}
						<div
							class="my-2 flex items-center gap-x-3 px-2 text-xs/5 font-semibold text-brand-text"
							aria-live="off"
							data-testid="new-divider"
						>
							<span class="h-px flex-1 bg-brand/40"></span>New<span class="h-px flex-1 bg-brand/40"
							></span>
						</div>
					{/if}
					<div
						class={highlight === m.id ? 'rounded-md ring-2 ring-brand' : ''}
						aria-live={channelRowLive(m)}
					>
						{@render rowFor(m)}
					</div>
				{:else}
					{#if conversation.auto?.type === 'general'}
						<div
							class="mx-auto my-8 max-w-md rounded-lg border border-line bg-tint/[0.03] p-5 text-center"
							data-testid="welcome-card"
						>
							<h3 class="text-sm/6 font-semibold text-fg">Welcome to #general</h3>
							<p class="mt-1 text-sm/6 text-fg-secondary">
								Agents and people talk here. Agents post questions and updates that need someone.
								Everything else stays in Activity.
							</p>
						</div>
					{:else}
						<p class="px-2 py-8 text-center text-sm/6 text-fg-muted">No messages yet.</p>
					{/if}
				{/each}

				{#each convView.digests as g (g.member_id)}
					<div
						class="mt-2 rounded-md border border-line bg-tint/[0.03] px-3 py-2"
						data-testid="digest-row"
					>
						<button
							type="button"
							class="text-sm/6 font-semibold text-brand-text"
							aria-expanded={openDigest === g.member_id}
							onclick={() => (openDigest = openDigest === g.member_id ? undefined : g.member_id)}
							>Daily digest · {nameOf(ctx, g.member_id)} · {g.messages.length}
							{g.messages.length === 1 ? 'update' : 'updates'}</button
						>
						{#if openDigest === g.member_id}
							<div class="mt-1 space-y-1">
								{#each g.messages as m (m.id)}<div aria-live={channelRowLive(m)}>
										<MessageRow message={m} {ctx} rowData={rows.forMessage(m)} {now} readOnly />
									</div>{/each}
							</div>
						{/if}
					</div>
				{/each}
				{#if convView.muted.length}
					<div
						class="mt-2 rounded-md border border-line bg-tint/[0.03] px-3 py-2"
						data-testid="muted-row"
					>
						<button
							type="button"
							class="text-sm/6 font-semibold text-brand-text"
							aria-expanded={showMuted}
							onclick={() => (showMuted = !showMuted)}
							>{convView.muted.length} muted {convView.muted.length === 1 ? 'message' : 'messages'} ·
							{showMuted ? 'Hide' : 'Show'}</button
						>
						<span class="text-xs/5 text-fg-muted"> They still reach Activity.</span>
						{#if showMuted}<div class="mt-1 space-y-1">
								{#each convView.muted as m (m.id)}<div aria-live={channelRowLive(m)}>
										<MessageRow message={m} {ctx} rowData={rows.forMessage(m)} {now} readOnly />
									</div>{/each}
							</div>{/if}
					</div>
				{/if}
			</div>
		{/key}
		{@render undelivered()}
	{/if}
</div>

<style>
	.chat-day {
		position: sticky;
		top: 0;
		z-index: 2;
		display: flex;
		align-items: center;
		gap: 12px;
		margin: 8px -12px 4px;
		padding: 4px 12px;
		font-size: 0.75rem;
		line-height: 1.25rem;
		font-weight: 600;
		color: var(--exec-muted, var(--fg-muted));
	}
	@media (width <= 640px) {
		.chat-day {
			margin-inline: -8px;
		}
	}
	.chat-day::before,
	.chat-day::after {
		content: '';
		flex: 1 1 auto;
		border-block-start: 1px solid var(--exec-border, var(--line-subtle));
	}
	.chat-day.is-today {
		position: relative;
		top: auto;
		gap: 0;
	}
	.chat-day.is-today::after {
		display: none;
	}
	.chat-day:global(.is-stuck) {
		background: color-mix(in srgb, var(--exec-panel-bg, var(--surface)) 92%, transparent);
		-webkit-backdrop-filter: blur(8px);
		backdrop-filter: blur(8px);
		box-shadow: 0 1px 0 var(--exec-border, var(--line-subtle));
	}
	.chat-day:global(.is-past) {
		visibility: hidden;
	}
	.chat-day-text {
		flex: none;
	}
</style>
