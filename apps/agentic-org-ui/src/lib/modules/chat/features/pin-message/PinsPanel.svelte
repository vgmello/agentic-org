<script lang="ts">
	import X from '@lucide/svelte/icons/x';
	import { mergeProps } from 'bits-ui';
	import { Button } from '$lib/components/ui/button';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import Rich from '$lib/modules/messaging/components/Rich.svelte';
	import type { ChatContext } from '$lib/modules/messaging/engine';
	import { clockTime, dayLabel, nameOf, titleOf } from '$lib/modules/messaging/model';
	import type { ChatMessage, Conversation } from '$lib/modules/messaging/types';

	/** A conversation's pins (chat-ux §4): pinned messages, newest first, each one link away from its place in the conversation. */
	let {
		conversation,
		ctx,
		pins,
		now,
		sidebarInline = true,
		officeRunning = true,
		onclose,
		onshow,
		onunpin
	}: {
		conversation: Conversation;
		ctx: ChatContext;
		pins: ChatMessage[];
		now: string;
		sidebarInline?: boolean;
		officeRunning?: boolean;
		onclose: () => void;
		onshow: (m: ChatMessage) => void;
		onunpin: (m: ChatMessage) => void;
	} = $props();

	let closeButton = $state<HTMLButtonElement | HTMLAnchorElement | null>(null);
	export function focusClose() {
		closeButton?.focus({ preventScroll: true });
		closeButton?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
	}
</script>

<aside
	data-chat-panel
	class="absolute inset-y-0 right-0 {sidebarInline
		? 'left-[256px]'
		: 'left-0'} z-30 flex h-full min-w-0 shrink-0 flex-col border-l border-line bg-surface shadow-xl min-[1280px]:static min-[1280px]:w-[384px] min-[1280px]:shadow-none"
	aria-label="Pinned"
	data-testid="pins-panel"
>
	<div class="flex items-center justify-between gap-3 border-b border-line px-4 py-3">
		<h2 class="min-w-0 text-sm/6 font-semibold break-words text-fg">
			Pinned <span class="font-normal text-fg-muted"
				>in {titleOf(conversation, ctx, ctx.owner)}</span
			>
		</h2>
		<Tooltip.Provider delayDuration={150} ignoreNonKeyboardFocus>
			<Tooltip.Root>
				<Tooltip.Trigger>
					{#snippet child({ props })}
						<Button
							{...mergeProps(props, { onclick: onclose })}
							type="button"
							variant="ghost"
							size="icon-sm"
							class="scroll-mt-16"
							aria-label="Close pinned"
							bind:ref={closeButton}><X aria-hidden="true" /></Button
						>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Content side="bottom">Close pinned</Tooltip.Content>
			</Tooltip.Root>
		</Tooltip.Provider>
	</div>
	<ul role="list" class="min-h-0 flex-1 divide-y divide-line-subtle overflow-y-auto">
		{#each pins as m (m.id)}
			<li class="px-4 py-3" data-testid="pin-row">
				<p class="text-xs/5 text-fg-muted">
					<span class="font-semibold text-fg">{nameOf(ctx, m.from)}</span> · {dayLabel(
						m.created_at,
						now
					)}
					{clockTime(m.created_at)}
				</p>
				<div class="mt-1 line-clamp-4"><Rich body={m.body} /></div>
				<div class="mt-1 flex flex-wrap gap-x-3">
					<button
						type="button"
						class="text-xs/5 font-semibold text-brand-text hover:text-brand-text-strong"
						onclick={() => onshow(m)}>Show in conversation</button
					>
					<button
						type="button"
						class="text-xs/5 text-fg-muted hover:text-fg disabled:opacity-50"
						disabled={!officeRunning}
						onclick={() => onunpin(m)}>Unpin</button
					>
				</div>
			</li>
		{:else}
			<li class="px-4 py-8 text-center text-sm/6 text-fg-muted">
				Nothing is pinned here. Pin a message from its actions menu.
			</li>
		{/each}
	</ul>
</aside>
