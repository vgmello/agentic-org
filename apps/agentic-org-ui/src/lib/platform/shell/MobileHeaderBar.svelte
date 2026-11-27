<!--
  @component
  Mobile (<640px) shell header bar: drawer menu trigger, area title, and Command Palette search trigger.
-->
<script lang="ts">
	import { Tooltip } from 'bits-ui';
	import Search from '@lucide/svelte/icons/search';
	import { tooltipContentClass } from '$lib/components/classes';
	import type { Connection, ProjectSummary, ProjectTeam, WorkItemSummary } from '$lib/proto';
	import type { InboxItem } from '$lib/platform/workspace/inbox-types';
	import MobileNavigation from './MobileNavigation.svelte';

	let {
		open = $bindable(false),
		areaLabel,
		connection,
		needsYouCount = 0,
		projects = [],
		workItems = [],
		inboxItems = [],
		projectTeams,
		tessOpen = false,
		offline = false,
		onOpenSearch,
		onToggleTess,
		onWiden
	}: {
		open?: boolean;
		areaLabel: string;
		connection: Connection;
		needsYouCount?: number;
		projects?: ProjectSummary[];
		workItems?: readonly WorkItemSummary[];
		inboxItems?: readonly InboxItem[];
		projectTeams?: readonly ProjectTeam[];
		tessOpen?: boolean;
		offline?: boolean;
		onOpenSearch?: () => void;
		onToggleTess?: () => void;
		onWiden: () => void;
	} = $props();
</script>

<div data-shell-header class="flex shrink-0 items-center gap-2 px-4 min-[640px]:hidden">
	<MobileNavigation
		bind:open
		{connection}
		{needsYouCount}
		{projects}
		{workItems}
		{inboxItems}
		{projectTeams}
		{tessOpen}
		{offline}
		{onOpenSearch}
		{onToggleTess}
		{onWiden}
	/>
	<span class="min-w-0 flex-1 truncate text-sm font-semibold">{areaLabel}</span>
	{#if onOpenSearch}
		<Tooltip.Provider delayDuration={150}>
			<Tooltip.Root>
				<Tooltip.Trigger>
					{#snippet child({ props })}
						<button
							{...props}
							type="button"
							disabled={offline}
							onclick={() => {
								if (!offline) onOpenSearch();
							}}
							aria-label={offline ? 'Search unavailable while offline' : 'Search'}
							data-testid="mobile-search-trigger"
							class="-mr-2 inline-flex size-11 shrink-0 items-center justify-center rounded-md text-fg-muted hover:bg-surface-raised hover:text-fg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus disabled:cursor-not-allowed disabled:opacity-50"
						>
							<Search class="size-5" aria-hidden="true" />
						</button>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Portal>
					<Tooltip.Content side="bottom" sideOffset={6} class={tooltipContentClass}
						>{offline ? 'Search unavailable while offline' : 'Search'}</Tooltip.Content
					>
				</Tooltip.Portal>
			</Tooltip.Root>
		</Tooltip.Provider>
	{/if}
</div>
