<script lang="ts">
	import Pin from '@lucide/svelte/icons/pin';
	import Users from '@lucide/svelte/icons/users';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import type { Conversation } from '$lib/modules/messaging';
	import InteriorSearchBar from '$lib/platform/shell/InteriorSearchBar.svelte';

	let {
		viewTitle,
		subtitle,
		conversation,
		searching,
		sample,
		sidebarInline,
		sidebarOpen,
		conversationsId,
		inboxCount,
		pinCount,
		showPins,
		showMembers,
		query,
		heading = $bindable<HTMLHeadingElement | undefined>(),
		sidebarToggle = $bindable<HTMLButtonElement | HTMLAnchorElement | null>(null),
		onopensidebar,
		ontogglepins,
		ontogglemembers,
		onquerychange
	}: {
		viewTitle: string;
		subtitle: string;
		conversation?: Conversation;
		searching: boolean;
		sample: boolean;
		sidebarInline: boolean;
		sidebarOpen: boolean;
		conversationsId: string;
		inboxCount?: number;
		pinCount: number;
		showPins: boolean;
		showMembers: boolean;
		query: string;
		heading?: HTMLHeadingElement;
		sidebarToggle?: HTMLButtonElement | HTMLAnchorElement | null;
		onopensidebar: () => void;
		ontogglepins: (opener: HTMLElement) => void;
		ontogglemembers: (opener: HTMLElement) => void;
		onquerychange: (value: string) => void;
	} = $props();
</script>

<div
	class="chat-conv-head flex flex-wrap items-center justify-between gap-3 border-b border-line px-5 py-3 max-[640px]:px-[8px]"
>
	<div class="min-w-0 max-w-full">
		<div class="flex min-w-0 flex-wrap items-center gap-2">
			{#if !sidebarInline}
				<Button
					bind:ref={sidebarToggle}
					variant="outline"
					size="sm"
					class="h-auto min-h-8 max-w-full flex-wrap px-[min(.625rem,8px)] py-1.5"
					aria-haspopup="dialog"
					aria-expanded={sidebarOpen}
					aria-controls={conversationsId}
					aria-label={`Conversations${inboxCount ? `, ${inboxCount} ${inboxCount === 1 ? 'needs' : 'need'} you` : ''}`}
					onclick={onopensidebar}
				>
					Conversations
					{#if inboxCount}<span
							class="rounded-full bg-warning px-1.5 text-xs/5 font-semibold text-on-warning"
							aria-hidden="true">{inboxCount}</span
						>{/if}
				</Button>
			{/if}
			<h2
				bind:this={heading}
				tabindex="-1"
				class="min-w-0 max-w-full whitespace-normal {conversation && !searching
					? '[overflow-wrap:anywhere]'
					: ''} text-base/7 font-semibold text-fg focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring"
				data-testid="conversation-title"
			>
				{viewTitle}
			</h2>
		</div>
		{#if conversation && !searching}<p class="text-xs/5 text-fg-muted">
				{subtitle}{conversation.archived ? ' · Archived' : ''}
			</p>{/if}
	</div>
	<div class="flex max-w-full flex-wrap items-center gap-2 max-[640px]:w-full">
		{#if sample}<Badge variant="secondary" data-testid="sample-data-note">Sample data</Badge>{/if}
		{#if conversation && !searching}
			<Button
				type="button"
				variant="outline"
				size="sm"
				aria-pressed={showPins}
				data-testid="pins-button"
				onclick={(event) => ontogglepins(event.currentTarget)}
				><Pin aria-hidden="true" /> Pins{#if pinCount}<span
						class="ml-0.5 text-xs/5 text-fg-muted"
						data-testid="pins-count">{pinCount}</span
					>{/if}</Button
			>
			<Button
				type="button"
				variant="outline"
				size="sm"
				aria-pressed={showMembers}
				onclick={(event) => ontogglemembers(event.currentTarget)}
				><Users aria-hidden="true" /> Members</Button
			>
		{/if}
		<InteriorSearchBar
			id="chat-search"
			inputTestId="chat-search"
			value={query}
			onValueChange={onquerychange}
			label="Search chat"
			placeholder="Search · from:@ada in:#atlas has:ask"
			class="w-[clamp(18rem,28vw,25rem)] max-w-full max-[640px]:w-full"
		/>
	</div>
</div>
