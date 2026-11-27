<script lang="ts">
	import { tick, untrack } from 'svelte';
	import { resolve } from '$app/paths';
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription
	} from '$lib/components/ui/sheet';
	import Plus from '@lucide/svelte/icons/plus';
	import HeroIcon from '$lib/components/HeroIcon.svelte';
	import { icons } from '$lib/components/icons';
	import type { ChatContext } from '$lib/modules/messaging/engine';
	import {
		PRESENCE_WORD,
		type ChatView,
		type ConversationRow,
		type Sidebar
	} from '$lib/modules/messaging/model';
	import Avatar from '$lib/modules/messaging/components/Avatar.svelte';
	import {
		isTextEntryTarget,
		nextSidebarIndex,
		sidebarNavAction
	} from '$lib/modules/messaging/sidebar-nav';

	/**
	 * The left column (CHAT-1): Inbox (a link, with the common owner-waiting count: Chat keeps no second queue), Mentions,
	 * Threads, channels, direct messages, and Between agents (what the owner reads as the owner).
	 */
	let {
		bar,
		inboxCount,
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
		onnewmessage,
		onbrowse
	}: {
		bar: Sidebar;
		/** The Inbox count the navigation shows; undefined while unknown. */
		inboxCount?: number;
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
		onbrowse: (opener: HTMLElement) => void;
	} = $props();

	const item =
		'flex w-full items-start gap-x-[min(.75rem,12px)] rounded-md px-[min(.5rem,8px)] py-1.5 text-left text-sm/6 transition-colors focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring';
	const off = 'text-fg-secondary hover:bg-surface-raised hover:text-fg';
	const here = (v: ChatView, conversationId?: string) =>
		view.type === v.type &&
		(v.type !== 'conversation' || (view.type === 'conversation' && view.id === conversationId));
	const unreadThreads = $derived(bar.threads.reduce((n, t) => n + t.new_replies, 0));
	let nav = $state<HTMLElement | undefined>();
	let bounds = $state({ left: 0, top: 0, width: 0, height: 0 });
	function placeSheet(rect: DOMRect) {
		const top = Math.max(0, rect.top);
		bounds = {
			left: rect.left,
			top,
			width: rect.width,
			height: Math.max(0, Math.min(rect.bottom, innerHeight) - top)
		};
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
			if (wasInline && !inline && wasInList)
				void tick().then(() => {
					if (
						!open &&
						(document.activeElement === document.body || document.activeElement === active)
					)
						onCloseFocus();
				});
		};
		untrack(measure);
		const observer = new ResizeObserver(measure);
		observer.observe(element);
		window.addEventListener('resize', measure);
		return () => {
			observer.disconnect();
			window.removeEventListener('resize', measure);
		};
	});
	export function focusSelected(preferred?: HTMLElement) {
		const target =
			preferred && nav?.contains(preferred)
				? preferred
				: (nav?.querySelector<HTMLElement>('[aria-current="true"]') ??
					nav?.querySelector<HTMLElement>('button'));
		target?.focus();
	}

	function handleNavKeydown(event: KeyboardEvent) {
		if (isTextEntryTarget(event.target)) return;
		const direction = sidebarNavAction(event);
		if (!direction || !nav) return;
		const target = event.target;
		if (!(target instanceof HTMLElement)) return;
		const currentItem = target.closest<HTMLElement>('[data-chat-nav-item]');
		if (!currentItem || !nav.contains(currentItem)) return;
		const items = [...nav.querySelectorAll<HTMLElement>('[data-chat-nav-item]')].filter(
			(el) =>
				!(el instanceof HTMLButtonElement && el.disabled) && !el.closest('details:not([open])')
		);
		const currentIndex = items.indexOf(currentItem);
		const nextIndex = nextSidebarIndex(currentIndex, items.length, direction);
		if (nextIndex < 0) return;
		event.preventDefault();
		items[nextIndex]?.focus();
	}
</script>

{#snippet row(r: ConversationRow)}
	{@const active = view.type === 'conversation' && view.id === r.conversation.id}
	<li>
		<button
			type="button"
			class={[item, active ? 'chat-nav-on font-medium text-fg' : off]}
			aria-current={active ? 'true' : undefined}
			aria-label={r.other && r.presence
				? `${r.title}, ${PRESENCE_WORD[r.presence]}${r.mentions ? `, ${r.mentions} mentions` : r.unread ? `, ${r.unread} unread` : ''}`
				: undefined}
			data-chat-nav-item
			data-testid="conversation-link"
			data-conversation={r.conversation.id}
			onclick={() => onselect({ type: 'conversation', id: r.conversation.id })}
		>
			{#if r.conversation.kind === 'channel'}
				<HeroIcon class="size-[min(1.5rem,32px)] text-current" variant="outline" name="hashtag" />
			{:else if r.other}
				<span class="flex shrink-0" aria-hidden="true"
					><Avatar name={r.other.name} kind={r.other.kind} presence={r.presence} size="sm" /></span
				>
			{:else}
				<HeroIcon
					class="size-[min(1.5rem,32px)] text-current"
					variant="outline"
					name="user-group"
				/>
			{/if}
			<span class="flex min-w-0 flex-1 flex-wrap items-baseline gap-x-2">
				<span
					class="min-w-0 max-w-full whitespace-normal [overflow-wrap:anywhere] {r.unread
						? 'font-semibold text-fg'
						: ''}">{r.conversation.kind === 'channel' ? r.title.slice(1) : r.title}</span
				>
				{#if r.other && r.presence}<span
						class="max-w-full shrink-0 text-xs/5 text-fg-secondary"
						data-testid="dm-status">{PRESENCE_WORD[r.presence]}</span
					>{/if}
				{#if r.mentions > 0}
					<span
						class="shrink-0 rounded-full bg-primary px-1.5 text-xs/5 font-semibold text-on-emphasis tabular-nums"
						aria-label="{r.mentions} mentions"
						data-testid="mention-count">{r.mentions}</span
					>
				{:else if r.unread > 0}
					<span
						class="chat-unread-pill shrink-0 rounded-full px-1.5 text-xs/5 font-semibold tabular-nums"
						aria-label="{r.unread} unread"
						data-testid="unread-count">{r.unread}</span
					>
				{/if}
			</span>
		</button>
	</li>
{/snippet}

{#snippet navigation()}
	<!-- keyboard roving handler delegated at nav landmark -->
	<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
	<nav
		bind:this={nav}
		onkeydown={handleNavKeydown}
		class="chat-rail flex h-full w-full min-w-0 shrink-0 flex-col gap-y-5 overflow-y-auto border-r border-line p-[min(.75rem,12px)]"
		aria-label="Chat"
	>
		<ul class="flex flex-col gap-0.5">
			<li>
				<a
					href={resolve('/inbox')}
					class="{item} {off}"
					aria-label={inboxCount
						? `Inbox, ${inboxCount} ${inboxCount === 1 ? 'needs' : 'need'} you`
						: 'Inbox'}
					data-chat-nav-item
					data-testid="inbox-link"
				>
					<svg
						viewBox="0 0 24 24"
						fill="none"
						stroke="currentColor"
						stroke-width="1.5"
						aria-hidden="true"
						class="size-[min(1.5rem,32px)] shrink-0 text-current"
						><path d={icons.inbox} stroke-linecap="round" stroke-linejoin="round" /></svg
					>
					<span class="flex min-w-0 flex-1 flex-wrap items-baseline gap-x-2">
						<span
							class="max-w-full whitespace-normal [overflow-wrap:normal] [word-break:normal] {inboxCount
								? 'font-semibold text-fg'
								: ''}">Inbox</span
						>
						{#if inboxCount}<span
								class="t-amber shrink-0 rounded-full px-1.5 text-xs/5 font-semibold tabular-nums"
								aria-hidden="true"
								data-testid="inbox-count">{inboxCount}</span
							>{/if}
					</span>
				</a>
			</li>
			<li>
				<button
					type="button"
					class={[item, here({ type: 'mentions' }) ? 'chat-nav-on font-medium text-fg' : off]}
					aria-current={here({ type: 'mentions' }) ? 'true' : undefined}
					data-chat-nav-item
					data-testid="mentions-link"
					onclick={() => onselect({ type: 'mentions' })}
				>
					<HeroIcon
						class="size-[min(1.5rem,32px)] text-current"
						variant="outline"
						name="at-symbol"
					/>
					<span class="flex min-w-0 flex-1 flex-wrap items-baseline gap-x-2">
						<span class="max-w-full whitespace-normal [overflow-wrap:normal] [word-break:normal]"
							>Mentions</span
						>
						{#if bar.mentions.length}<span
								class="shrink-0 text-xs/5 font-semibold text-fg-body tabular-nums"
								>{bar.mentions.length}</span
							>{/if}
					</span>
				</button>
			</li>
			<li>
				<button
					type="button"
					class={[item, here({ type: 'threads' }) ? 'chat-nav-on font-medium text-fg' : off]}
					aria-current={here({ type: 'threads' }) ? 'true' : undefined}
					data-chat-nav-item
					data-testid="threads-link"
					onclick={() => onselect({ type: 'threads' })}
				>
					<HeroIcon
						class="size-[min(1.5rem,32px)] text-current"
						variant="outline"
						name="chat-bubble-left-right"
					/>
					<span class="flex min-w-0 flex-1 flex-wrap items-baseline gap-x-2">
						<span class="max-w-full whitespace-normal [overflow-wrap:normal] [word-break:normal]"
							>Threads</span
						>
						{#if unreadThreads}<span
								class="shrink-0 text-xs/5 font-semibold text-fg-body tabular-nums"
								>{unreadThreads}</span
							>{/if}
					</span>
				</button>
			</li>
		</ul>

		<div>
			<h2 class="px-2 text-xs/6 font-semibold tracking-wider text-fg-muted uppercase">Channels</h2>
			<ul class="mt-1 flex flex-col gap-0.5" data-testid="channels">
				{#each bar.channels as r (r.conversation.id)}{@render row(r)}{/each}
			</ul>
			<button
				type="button"
				class="{item} {off} mt-1"
				data-testid="browse-channels"
				onclick={(event) => onbrowse(event.currentTarget)}
				><HeroIcon
					class="size-[min(1.5rem,32px)] text-current"
					variant="outline"
					name="hashtag"
				/><span class="min-w-0 flex-1 whitespace-normal">Browse channels</span></button
			>
			<button
				type="button"
				class="{item} {off} disabled:opacity-50 disabled:hover:bg-transparent"
				disabled={!officeRunning}
				onclick={(event) => onnewchannel(event.currentTarget)}
				><Plus class="size-[min(1.5rem,32px)] shrink-0 text-current" aria-hidden="true" /><span
					class="min-w-0 flex-1 whitespace-normal">Add channel</span
				></button
			>
		</div>

		<div>
			<h2 class="px-2 text-xs/6 font-semibold tracking-wider text-fg-muted uppercase">
				Direct messages
			</h2>
			<ul class="mt-1 flex flex-col gap-0.5" data-testid="dms">
				{#each bar.dms as r (r.conversation.id)}{@render row(r)}{/each}
			</ul>
			<button
				type="button"
				class="{item} {off} mt-1 disabled:opacity-50 disabled:hover:bg-transparent"
				disabled={!officeRunning}
				onclick={(event) => onnewmessage(event.currentTarget)}
				><Plus class="size-[min(1.5rem,32px)] shrink-0 text-current" aria-hidden="true" /><span
					class="min-w-0 flex-1 whitespace-normal">New message</span
				></button
			>
		</div>

		{#if bar.asOwner.length}
			<div data-testid="as-owner">
				<h2 class="px-2 text-xs/6 font-semibold tracking-wide text-fg-muted uppercase">
					Between agents
				</h2>
				<p class="px-2 text-xs/5 text-fg-muted">You read these as the owner.</p>
				<ul class="mt-1 flex flex-col gap-0.5">
					{#each bar.asOwner as r (r.conversation.id)}{@render row(r)}{/each}
				</ul>
			</div>
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
			class="gap-0 border-line bg-surface p-0 text-fg"
			style={`left:${bounds.left}px;top:${bounds.top}px;width:${Math.min(256, bounds.width)}px;height:${bounds.height}px;bottom:auto;right:auto;`}
			onOpenAutoFocus={(event) => {
				event.preventDefault();
				// On short screens the shell can scroll before the list opens.
				if (container) placeSheet(container.getBoundingClientRect());
				void tick().then(() => open && focusSelected());
			}}
			onCloseAutoFocus={(event) => {
				event.preventDefault();
				onCloseFocus();
			}}
		>
			<SheetHeader class="shrink-0 border-b border-line p-4 pr-12">
				<SheetTitle class="min-w-0 whitespace-normal [overflow-wrap:anywhere]"
					>Conversations</SheetTitle
				>
				<SheetDescription class="sr-only"
					>Choose a conversation or start a new one.</SheetDescription
				>
			</SheetHeader>
			<div class="min-h-0 flex-1">{@render navigation()}</div>
		</SheetContent>
	</Sheet>
{/if}

<style>
	.chat-rail {
		background: var(--exec-sidebar-bg);
	}
	.chat-nav-on {
		background: var(--tone-accent-bg);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
	}
	.chat-unread-pill {
		background: var(--tone-accent-bg);
		color: var(--tone-accent-ink);
	}
</style>
