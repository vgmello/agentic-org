<script lang="ts">
	import Plus from '@lucide/svelte/icons/plus';
	import type { ChatContext } from '$lib/chat/engine';
	import type { ChatView, ConversationRow, Sidebar } from '$lib/chat/model';
	import Avatar from './Avatar.svelte';

	/** The left column (CHAT-1): Needs you, Mentions, Threads, channels, direct messages, and what is read as the owner. */
	let {
		bar,
		view,
		ctx,
		onselect,
		onnewchannel,
		onnewmessage
	}: {
		bar: Sidebar;
		view: ChatView;
		ctx: ChatContext;
		onselect: (v: ChatView) => void;
		onnewchannel: () => void;
		onnewmessage: () => void;
	} = $props();

	const item = 'flex w-full items-center gap-x-2 rounded-md px-2 py-1.5 text-left text-sm/6 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-indigo-500';
	const on = 'bg-white/10 text-white';
	const off = 'text-gray-300 hover:bg-white/5 hover:text-white';
	const here = (v: ChatView, id?: string) => view.type === v.type && (v.type !== 'conversation' || (view.type === 'conversation' && view.id === id));
	const unreadThreads = $derived(bar.threads.reduce((n, t) => n + t.new_replies, 0));
</script>

{#snippet row(r: ConversationRow)}
	{@const active = view.type === 'conversation' && view.id === r.conversation.id}
	<li>
		<button type="button" class="{item} {active ? on : off}" aria-current={active ? 'true' : undefined} data-testid="conversation-link" data-conversation={r.conversation.id} onclick={() => onselect({ type: 'conversation', id: r.conversation.id })}>
			{#if r.conversation.kind === 'channel'}
				<span class="w-4 shrink-0 text-center text-gray-400" aria-hidden="true">#</span>
				<span class="min-w-0 flex-1 truncate {r.unread ? 'font-semibold text-white' : ''}">{r.title.slice(1)}</span>
			{:else if r.other}
				<Avatar name={r.other.name} kind={r.other.kind} presence={r.presence} size="sm" />
				<span class="min-w-0 flex-1 truncate {r.unread ? 'font-semibold text-white' : ''}">{r.title}</span>
				{#if r.presence && r.presence !== 'online'}<span class="shrink-0 text-xs/5 text-gray-400">{({ working: 'Working', waiting: 'Waiting', asleep: 'Asleep', offline: 'Offline', paused: 'Paused', online: '' })[r.presence]}</span>{/if}
			{:else}
				<span class="w-4 shrink-0 text-center text-gray-400" aria-hidden="true">👥</span>
				<span class="min-w-0 flex-1 truncate {r.unread ? 'font-semibold text-white' : ''}">{r.title}</span>
			{/if}
			{#if r.mentions > 0}
				<span class="shrink-0 rounded-full bg-indigo-500 px-1.5 text-xs/5 font-semibold text-white" aria-label="{r.mentions} mentions" data-testid="mention-count">{r.mentions}</span>
			{:else if r.unread > 0}
				<span class="shrink-0 text-xs/5 font-semibold text-gray-200" aria-label="{r.unread} unread" data-testid="unread-count">{r.unread}</span>
			{/if}
		</button>
	</li>
{/snippet}

<nav class="flex h-full w-64 shrink-0 flex-col gap-y-5 overflow-y-auto border-r border-white/10 bg-black/10 p-3" aria-label="Chat">
	<ul class="space-y-0.5">
		<li>
			<button type="button" class="{item} {here({ type: 'needs' }) ? on : off}" aria-current={here({ type: 'needs' }) ? 'true' : undefined} data-testid="needs-you-link" onclick={() => onselect({ type: 'needs' })}>
				<span aria-hidden="true">⭐</span><span class="flex-1 {bar.needsYou.length ? 'font-semibold text-white' : ''}">Needs you</span>
				{#if bar.needsYou.length}<span class="rounded-full bg-amber-400 px-1.5 text-xs/5 font-semibold text-gray-900" data-testid="needs-you-count">{bar.needsYou.length}</span>{/if}
			</button>
		</li>
		<li>
			<button type="button" class="{item} {here({ type: 'mentions' }) ? on : off}" aria-current={here({ type: 'mentions' }) ? 'true' : undefined} data-testid="mentions-link" onclick={() => onselect({ type: 'mentions' })}>
				<span aria-hidden="true">@</span><span class="flex-1">Mentions</span>
				{#if bar.mentions.length}<span class="text-xs/5 font-semibold text-gray-200">{bar.mentions.length}</span>{/if}
			</button>
		</li>
		<li>
			<button type="button" class="{item} {here({ type: 'threads' }) ? on : off}" aria-current={here({ type: 'threads' }) ? 'true' : undefined} data-testid="threads-link" onclick={() => onselect({ type: 'threads' })}>
				<span aria-hidden="true">🧵</span><span class="flex-1">Threads</span>
				{#if unreadThreads}<span class="text-xs/5 font-semibold text-gray-200">{unreadThreads}</span>{/if}
			</button>
		</li>
	</ul>

	<div>
		<h2 class="px-2 text-xs/6 font-semibold tracking-wide text-gray-400 uppercase">Channels</h2>
		<ul class="mt-1 space-y-0.5" data-testid="channels">
			{#each bar.channels as r (r.conversation.id)}{@render row(r)}{/each}
		</ul>
		<button type="button" class="{item} {off} mt-1" onclick={onnewchannel}><Plus class="size-4" aria-hidden="true" /> Add channel</button>
	</div>

	<div>
		<h2 class="px-2 text-xs/6 font-semibold tracking-wide text-gray-400 uppercase">Direct messages</h2>
		<ul class="mt-1 space-y-0.5" data-testid="dms">
			{#each bar.dms as r (r.conversation.id)}{@render row(r)}{/each}
		</ul>
		<button type="button" class="{item} {off} mt-1" onclick={onnewmessage}><Plus class="size-4" aria-hidden="true" /> New message</button>
	</div>

	{#if bar.asOwner.length}
		<details class="group" data-testid="as-owner">
			<summary class="cursor-pointer px-2 text-xs/6 font-semibold tracking-wide text-gray-400 uppercase hover:text-white">Reading as the owner · {bar.asOwner.length}</summary>
			<ul class="mt-1 space-y-0.5">
				{#each bar.asOwner as r (r.conversation.id)}{@render row(r)}{/each}
			</ul>
		</details>
	{/if}
</nav>
