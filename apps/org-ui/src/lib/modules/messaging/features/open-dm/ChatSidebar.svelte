<script lang="ts">
	import { tick, untrack } from 'svelte';
	import { Sheet, SheetContent, SheetHeader, SheetTitle, SheetDescription } from '$lib/components/ui/sheet';
	import Plus from '@lucide/svelte/icons/plus';
	import type { ChatContext } from '$lib/modules/messaging/engine';
	import { PRESENCE_WORD, type ChatView, type ConversationRow, type Sidebar } from '$lib/modules/messaging/model';
	import Avatar from '$lib/modules/messaging/components/Avatar.svelte';

	/** The left column (CHAT-1): Needs you, Mentions, Threads, channels, direct messages, and what is read as the owner. */
	let {
		bar,
		view,
		ctx,
		officeRunning,
		container,
		id,
		inline = $bindable(true),
		open = $bindable(false),
		onCloseFocus,
		onselect,
		onnewchannel,
		onnewmessage
	}: {
		bar: Sidebar;
		view: ChatView;
		ctx: ChatContext;
		officeRunning: boolean;
		container?: HTMLElement;
		id: string;
		inline?: boolean;
		open?: boolean;
		onCloseFocus: () => void;
		onselect: (v: ChatView) => void;
		onnewchannel: (opener: HTMLElement) => void;
		onnewmessage: (opener: HTMLElement) => void;
	} = $props();

	const item = 'flex w-full items-center gap-x-2 rounded-md px-2 py-1.5 text-left text-sm/6 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-indigo-500';
	const on = 'bg-white/10 text-white';
	const off = 'text-gray-300 hover:bg-white/5 hover:text-white';
	const here = (v: ChatView, id?: string) => view.type === v.type && (v.type !== 'conversation' || (view.type === 'conversation' && view.id === id));
	const unreadThreads = $derived(bar.threads.reduce((n, t) => n + t.new_replies, 0));
	let nav = $state<HTMLElement | undefined>();
	let bounds = $state({ left: 0, top: 0, width: 0, height: 0 });
	function placeSheet(rect: DOMRect) {
		const top = Math.max(0, rect.top);
		bounds = { left: rect.left, top, width: rect.width, height: Math.max(0, Math.min(rect.bottom, innerHeight) - top) };
	}
	// The product breakpoint is the available Chat content width, not the viewport.
	$effect(() => {
		const element = container;
		if (!element) return;
		const measure = () => {
			if (!element.isConnected) return;
			const rect = element.getBoundingClientRect();
			const active = document.activeElement;
			const wasInList = !!active && !!nav?.contains(active);
			const wasInline = inline;
			inline = rect.width >= 768;
			placeSheet(rect);
			if (inline) open = false;
			if (wasInline && !inline && wasInList) void tick().then(() => {
				if (!open && (document.activeElement === document.body || document.activeElement === active)) onCloseFocus();
			});
		};
		untrack(measure);
		const observer = new ResizeObserver(measure);
		observer.observe(element);
		window.addEventListener('resize', measure);
		return () => { observer.disconnect(); window.removeEventListener('resize', measure); };
	});
	export function focusSelected(preferred?: HTMLElement) {
		const item = preferred && nav?.contains(preferred) ? preferred : nav?.querySelector<HTMLElement>('[aria-current="true"]') ?? nav?.querySelector<HTMLElement>('button');
		const group = item?.closest('details');
		if (group instanceof HTMLDetailsElement) group.open = true;
		item?.focus();
	}
</script>

{#snippet row(r: ConversationRow)}
	{@const active = view.type === 'conversation' && view.id === r.conversation.id}
	<li>
		<button type="button" class="{item} {active ? on : off}" aria-current={active ? 'true' : undefined} aria-label={r.other && r.presence ? `${r.title}, ${PRESENCE_WORD[r.presence]}${r.mentions ? `, ${r.mentions} mentions` : r.unread ? `, ${r.unread} unread` : ''}` : undefined} data-testid="conversation-link" data-conversation={r.conversation.id} onclick={() => onselect({ type: 'conversation', id: r.conversation.id })}>
			{#if r.conversation.kind === 'channel'}
				<span class="w-4 shrink-0 text-center text-gray-400" aria-hidden="true">#</span>
				<span class="min-w-0 flex-1 truncate {r.unread ? 'font-semibold text-white' : ''}">{r.title.slice(1)}</span>
			{:else if r.other}
				<span class="flex" aria-hidden="true"><Avatar name={r.other.name} kind={r.other.kind} presence={r.presence} size="sm" /></span>
				<span class="min-w-0 flex-1 truncate {r.unread ? 'font-semibold text-white' : ''}">{r.title}</span>
				{#if r.presence}<span class="shrink-0 text-xs/5 text-gray-300" data-testid="dm-status">{PRESENCE_WORD[r.presence]}</span>{/if}
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

{#snippet navigation()}
<nav bind:this={nav} class="flex h-full w-full min-w-0 shrink-0 flex-col gap-y-5 overflow-y-auto border-r border-white/10 bg-black/10 p-3" aria-label="Chat">
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
		<button type="button" class="{item} {off} mt-1 disabled:opacity-50 disabled:hover:bg-transparent" disabled={!officeRunning} onclick={(event) => onnewchannel(event.currentTarget)}><Plus class="size-4" aria-hidden="true" /> Add channel</button>
	</div>

	<div>
		<h2 class="px-2 text-xs/6 font-semibold tracking-wide text-gray-400 uppercase">Direct messages</h2>
		<ul class="mt-1 space-y-0.5" data-testid="dms">
			{#each bar.dms as r (r.conversation.id)}{@render row(r)}{/each}
		</ul>
		<button type="button" class="{item} {off} mt-1 disabled:opacity-50 disabled:hover:bg-transparent" disabled={!officeRunning} onclick={(event) => onnewmessage(event.currentTarget)}><Plus class="size-4" aria-hidden="true" /> New message</button>
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
{/snippet}

{#if inline}
	<div class="h-full w-[256px] shrink-0">{@render navigation()}</div>
{:else}
	<Sheet bind:open>
		<SheetContent
			{id}
			side="left"
			class="gap-0 border-white/10 bg-gray-800 p-0 text-white"
			style={`left:${bounds.left}px;top:${bounds.top}px;width:${Math.min(256, bounds.width)}px;height:${bounds.height}px;bottom:auto;right:auto;`}
			onOpenAutoFocus={(event) => {
				event.preventDefault();
				// On short screens the shell can scroll before the list opens.
				if (container) placeSheet(container.getBoundingClientRect());
				void tick().then(() => open && focusSelected());
			}}
			onCloseAutoFocus={(event) => { event.preventDefault(); onCloseFocus(); }}
		>
			<SheetHeader class="shrink-0 border-b border-white/10 p-4 pr-12">
				<SheetTitle>Conversations</SheetTitle>
				<SheetDescription class="sr-only">Choose a conversation or start a new one.</SheetDescription>
			</SheetHeader>
			<div class="min-h-0 flex-1">{@render navigation()}</div>
		</SheetContent>
	</Sheet>
{/if}
