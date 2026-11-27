<script lang="ts">
	import { mergeProps, Tooltip } from 'bits-ui';
	import Activity from '@lucide/svelte/icons/activity';
	import Layers from '@lucide/svelte/icons/layers';
	import ListFilter from '@lucide/svelte/icons/list-filter';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { tooltipContentClass } from '$lib/components/classes';
	import ActivityDisplayMenu from './ActivityDisplayMenu.svelte';
	import { STREAM_TABS } from './activity-project-merge';
	import type { StreamFilterTab } from './assembly-model';

	let {
		viewMode = $bindable<'assembly' | 'feed'>('assembly'),
		streamFilter = $bindable<StreamFilterTab>('all'),
		displayMenuOpen = $bindable(false),
		filterMenuOpen = $bindable(false),
		focusedProjectView = $bindable<'assembly' | 'feed' | null>(null),
		onViewKeydown
	}: {
		viewMode?: 'assembly' | 'feed';
		streamFilter?: StreamFilterTab;
		displayMenuOpen?: boolean;
		filterMenuOpen?: boolean;
		focusedProjectView?: 'assembly' | 'feed' | null;
		onViewKeydown: (event: KeyboardEvent) => void;
	} = $props();

	function handleFocusOut(event: FocusEvent) {
		const container = event.currentTarget as HTMLElement | null;
		const related = event.relatedTarget;
		if (!container || !(related instanceof Node) || !container.contains(related)) {
			focusedProjectView = null;
		}
	}
</script>

<Tooltip.Provider delayDuration={200} ignoreNonKeyboardFocus>
	<div class="project-activity-controls" data-testid="project-activity-controls">
		<div
			class="project-view-tabs"
			role="tablist"
			aria-label="Project activity view"
			onfocusout={handleFocusOut}
		>
			<button
				type="button"
				role="tab"
				id="project-view-tab-assembly"
				aria-controls="project-view-panel-assembly"
				data-project-view="assembly"
				aria-selected={viewMode === 'assembly'}
				tabindex={(focusedProjectView ?? viewMode) === 'assembly' ? 0 : -1}
				onfocus={() => (focusedProjectView = 'assembly')}
				onclick={() => (viewMode = 'assembly')}
				onkeydown={onViewKeydown}
				class="project-view-tab min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
			>
				<Layers class="project-view-ic size-3.5" aria-hidden="true" />
				<span>Assembly line</span>
			</button>
			<button
				type="button"
				role="tab"
				id="project-view-tab-feed"
				aria-controls="project-view-panel-feed"
				data-project-view="feed"
				aria-selected={viewMode === 'feed'}
				tabindex={(focusedProjectView ?? viewMode) === 'feed' ? 0 : -1}
				onfocus={() => (focusedProjectView = 'feed')}
				onclick={() => (viewMode = 'feed')}
				onkeydown={onViewKeydown}
				class="project-view-tab min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
			>
				<Activity class="project-view-ic size-3.5" aria-hidden="true" />
				<span>Activity feed</span>
			</button>
		</div>

		<div class="queue-controls" role="group" aria-label="Activity view settings">
			{#if viewMode === 'assembly'}
				<ActivityDisplayMenu bind:open={displayMenuOpen} />
			{:else}
				<DropdownMenu.Root bind:open={filterMenuOpen}>
					<Tooltip.Root delayDuration={250} ignoreNonKeyboardFocus disabled={filterMenuOpen}>
						<DropdownMenu.Trigger>
							{#snippet child({ props: menuProps })}
								<Tooltip.Trigger>
									{#snippet child({ props: tipProps })}
										<button
											{...mergeProps(menuProps, tipProps)}
											type="button"
											aria-label="Filter activity"
											data-testid="project-feed-filter-btn"
											data-active={streamFilter !== 'all' ? '' : undefined}
											class="activity-icon-btn min-h-9 min-w-9 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
										>
											<ListFilter class="size-5" aria-hidden="true" />
										</button>
									{/snippet}
								</Tooltip.Trigger>
							{/snippet}
						</DropdownMenu.Trigger>
						<Tooltip.Portal>
							<Tooltip.Content
								side="bottom"
								sideOffset={6}
								escapeKeydownBehavior="defer-otherwise-close"
								class={tooltipContentClass}
							>
								Filter activity
							</Tooltip.Content>
						</Tooltip.Portal>
					</Tooltip.Root>
					<DropdownMenu.Content align="end" class="triage-menu-surface w-48">
						<DropdownMenu.Label>Event type</DropdownMenu.Label>
						<DropdownMenu.RadioGroup
							value={streamFilter}
							onValueChange={(val) => {
								if (val) streamFilter = val as StreamFilterTab;
							}}
						>
							{#each STREAM_TABS as tab (tab.id)}
								<DropdownMenu.RadioItem
									value={tab.id}
									closeOnSelect={true}
									class="min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
								>
									<span>{tab.label}</span>
								</DropdownMenu.RadioItem>
							{/each}
						</DropdownMenu.RadioGroup>
					</DropdownMenu.Content>
				</DropdownMenu.Root>
			{/if}
		</div>
	</div>
</Tooltip.Provider>

<style>
	.project-activity-controls {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px;
		max-inline-size: 100%;
	}
	.project-view-tabs {
		display: flex;
		align-items: center;
		gap: 2px;
		padding-inline-start: 8px;
		border-inline-start: 1px solid var(--line);
	}
	.project-view-tab {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		padding: 0 10px;
		border: 1px solid transparent;
		border-radius: var(--exec-ctrl-radius, 6px);
		background: transparent;
		color: var(--fg-secondary);
		font-size: 0.8125rem;
		font-weight: 600;
		white-space: nowrap;
		cursor: pointer;
	}
	.project-view-tab:hover {
		background: var(--surface-raised);
		color: var(--fg);
	}
	.project-view-tab[aria-selected='true'] {
		background: var(--surface-raised);
		border-color: var(--line-strong);
		color: var(--fg);
	}
	.project-view-tab:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -2px;
	}
	.activity-icon-btn {
		position: relative;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		border: 0;
		border-radius: var(--exec-ctrl-radius, 6px);
		background: transparent;
		color: var(--fg-secondary);
		cursor: pointer;
	}
	.activity-icon-btn:hover,
	.activity-icon-btn[aria-expanded='true'] {
		background: var(--surface-raised);
		color: var(--fg);
	}
	.activity-icon-btn[data-active] {
		color: var(--tone-accent-ink);
	}
	.activity-icon-btn[data-active]::after {
		content: '';
		position: absolute;
		inset: 5px 5px auto auto;
		width: 5px;
		height: 5px;
		border-radius: 50%;
		background: currentColor;
	}
	.activity-icon-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	@media (max-width: 640px) {
		.project-view-tab {
			padding-inline: 8px;
		}
		.project-view-tab :global(.project-view-ic) {
			display: none;
		}
	}
	@media (forced-colors: active) {
		.project-view-tab[aria-selected='true'] {
			background: Highlight;
			border-color: Highlight;
			color: HighlightText;
		}
	}
</style>
