<script lang="ts">
	import { mergeProps, Tooltip } from 'bits-ui';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import InlineSearchRow from '$lib/components/InlineSearchRow.svelte';
	import InlineSearchToggle from '$lib/components/InlineSearchToggle.svelte';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import NotificationsButton from '$lib/components/shell/NotificationsButton.svelte';
	import type { InboxScope } from '../../types';
	import InboxTriageMenus from './InboxTriageMenus.svelte';
	import {
		isOverflowTab,
		OVERFLOW_TABS,
		PRIMARY_TABS,
		type TriageOrder,
		type TriageTab
	} from './triage-model';
	import { moveCategoryTabFocus } from './triage-workspace-helpers';

	let {
		totalOpenCount,
		viewingNotifications,
		activeTab,
		counts,
		notifTotalCount,
		notifAttentionCount,
		shortcutsEnabled,
		searchOpen,
		searchQuery,
		urgentOnly,
		scope,
		scopeOpts,
		sortOrder,
		showDividers,
		isFiltered,
		displayActive,
		searchInputEl = $bindable(null),
		onSelectTab,
		onSelectNotificationsTab,
		onToggleInlineSearch,
		onCloseInlineSearch,
		onSearchQueryChange,
		onSetUrgentOnly,
		onSetScope,
		onClearFilterMenu,
		onSetSortOrder,
		onToggleRowDividers
	}: {
		totalOpenCount: number;
		viewingNotifications: boolean;
		activeTab: TriageTab;
		counts: Record<TriageTab, number>;
		notifTotalCount: number;
		notifAttentionCount: number;
		shortcutsEnabled: boolean;
		searchOpen: boolean;
		searchQuery: string;
		urgentOnly: boolean;
		scope: InboxScope;
		scopeOpts: {
			teams: { id: string; name: string }[];
			projects: { id: string; name: string }[];
		};
		sortOrder: TriageOrder;
		showDividers: boolean;
		isFiltered: boolean;
		displayActive: boolean;
		searchInputEl?: HTMLInputElement | null;
		onSelectTab: (tab: TriageTab) => void;
		onSelectNotificationsTab: () => void;
		onToggleInlineSearch: () => void;
		onCloseInlineSearch: () => void;
		onSearchQueryChange: (value: string) => void;
		onSetUrgentOnly: (urgent: boolean) => void;
		onSetScope: (nextScope: InboxScope) => void;
		onClearFilterMenu: () => void;
		onSetSortOrder: (order: TriageOrder) => void;
		onToggleRowDividers: (checked: boolean) => void;
	} = $props();

	const overflowActive = $derived(isOverflowTab(activeTab));
	const overflowActiveLabel = $derived(
		OVERFLOW_TABS.find((t) => t.id === activeTab)?.label ?? 'More'
	);
	const menuSurface =
		'triage-menu-surface motion-reduce:animate-none! motion-reduce:transition-none! motion-reduce:data-[state=closed]:animate-none! motion-reduce:data-[state=open]:animate-none!';
	const itemTouch = 'min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11';
</script>

<Tooltip.Provider delayDuration={150}>
	<header class="inbox-head triage-header queue-header section-header">
		<div class="queue-headline">
			<div class="ph-t">
				<h1 id="shell-page-title" tabindex="-1" class="triage-h1">Inbox</h1>
				<span role="status" class="queue-count">
					<i class={[totalOpenCount === 0 && 'is-ok']} aria-hidden="true"></i>
					<span class="tabular-nums">
						{totalOpenCount === 0 ? '0 waiting on you' : `${totalOpenCount} waiting on you`}
					</span>
				</span>
			</div>

			<div class="queue-headline-end">
				<NotificationsButton />
			</div>
		</div>

		<div class="triage-toolbar queue-toolbar section-toolbar">
			<div
				id="triage-categories"
				role="group"
				aria-label="Inbox categories"
				aria-keyshortcuts={shortcutsEnabled ? 'M' : undefined}
				data-overflow-tabs={JSON.stringify(
					OVERFLOW_TABS.map((t) => ({ id: t.id, label: t.label }))
				)}
				class="primary-tab-row"
			>
				{#each PRIMARY_TABS as tab (tab.id)}
					{@const isSelected = !viewingNotifications && activeTab === tab.id}
					<button
						type="button"
						data-triage-tab={tab.id}
						data-triage-label={tab.label}
						aria-pressed={isSelected}
						onclick={() => onSelectTab(tab.id)}
						onkeydown={moveCategoryTabFocus}
						class="queue-tab"
					>
						<span>{tab.label}</span>
						<span class="queue-tab-count">{counts[tab.id]}</span>
					</button>
				{/each}

				<button
					type="button"
					data-triage-tab="notifications"
					data-triage-label="Notifications"
					aria-pressed={viewingNotifications}
					onclick={onSelectNotificationsTab}
					onkeydown={moveCategoryTabFocus}
					class="queue-tab"
				>
					<span>Notifications</span>
					{#if notifAttentionCount > 0}
						<span class="queue-tab-dot" aria-hidden="true"></span>
					{/if}
					<span class="queue-tab-count">{notifTotalCount}</span>
				</button>

				<span class="triage-more-wrap">
					<DropdownMenu.Root>
						<DropdownMenu.Trigger>
							{#snippet child({ props })}
								<button
									{...mergeProps(props, { onkeydown: moveCategoryTabFocus })}
									type="button"
									data-triage-more=""
									data-current-kind={!viewingNotifications && overflowActive ? '' : undefined}
									title={!viewingNotifications && overflowActive
										? `Viewing ${overflowActiveLabel}`
										: undefined}
									class="queue-tab"
								>
									<span>More</span>
									<ChevronDown class="size-3.5 shrink-0 opacity-75" aria-hidden="true" />
								</button>
							{/snippet}
						</DropdownMenu.Trigger>
						<DropdownMenu.Content preventScroll={false} align="start" class="{menuSurface} w-52">
							<DropdownMenu.RadioGroup
								value={!viewingNotifications && overflowActive ? activeTab : ''}
								onValueChange={(val) => {
									if (val) onSelectTab(val as TriageTab);
								}}
							>
								{#each OVERFLOW_TABS as tab (tab.id)}
									<DropdownMenu.RadioItem
										value={tab.id}
										closeOnSelect={true}
										class="flex items-center justify-between gap-3 pr-8 {itemTouch}"
									>
										<span>{tab.label}</span>
										<span class="triage-menu-count font-mono text-xs tabular-nums">
											{counts[tab.id]}
										</span>
									</DropdownMenu.RadioItem>
								{/each}
							</DropdownMenu.RadioGroup>
						</DropdownMenu.Content>
					</DropdownMenu.Root>
				</span>
			</div>

			<div class="queue-controls" role="group" aria-label="Inbox view controls">
				<InlineSearchToggle
					open={searchOpen}
					hasQuery={searchQuery.trim().length > 0}
					label={viewingNotifications ? 'Search notifications' : 'Search inbox'}
					onToggle={onToggleInlineSearch}
				/>
				<InboxTriageMenus
					{urgentOnly}
					{scope}
					{scopeOpts}
					{sortOrder}
					{showDividers}
					{isFiltered}
					{displayActive}
					{onSetUrgentOnly}
					{onSetScope}
					{onClearFilterMenu}
					{onSetSortOrder}
					{onToggleRowDividers}
				/>
			</div>
		</div>
		{#if searchOpen || searchQuery.trim().length > 0}
			<InlineSearchRow
				bind:inputEl={searchInputEl}
				id="triage-search-input"
				value={searchQuery}
				onValueChange={onSearchQueryChange}
				label={viewingNotifications ? 'Search notifications' : 'Search inbox'}
				placeholder={viewingNotifications ? 'Search notifications…' : 'Search inbox…'}
				onClose={onCloseInlineSearch}
			/>
		{/if}
	</header>
</Tooltip.Provider>

<style>
	.queue-header {
		position: relative;
		z-index: 1;
		container: queue-header / inline-size;
		border-bottom: 1px solid var(--exec-border);
	}
	.queue-headline {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: flex-start;
		gap: 8px 16px;
		padding: 14px var(--triage-gutter) 4px;
	}
	.ph-t {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px 14px;
		min-width: 0;
	}
	.triage-h1 {
		color: var(--exec-ink);
		font-size: clamp(1.375rem, 2.2vw, 1.875rem);
		line-height: 1.22;
		font-weight: 600;
		letter-spacing: -0.03em;
	}
	.triage-h1:focus {
		outline: none;
	}
	.triage-h1:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	.queue-count {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		color: var(--exec-muted);
		font-size: 0.8125rem;
		font-weight: 500;
	}
	.queue-count i {
		display: inline-block;
		width: 7px;
		height: 7px;
		border-radius: 50%;
		background: var(--tone-accent);
		flex: none;
	}
	.queue-count i.is-ok {
		background: var(--tone-ok);
	}
	.queue-headline-end {
		display: flex;
		align-items: center;
		margin-inline-start: auto;
	}
	.queue-tab-dot {
		width: 6px;
		height: 6px;
		border-radius: 50%;
		background: var(--tone-accent);
		flex: none;
	}
	.queue-toolbar {
		display: flex;
		flex-wrap: wrap;
		align-items: stretch;
		gap: 6px 8px;
		padding: 4px var(--triage-gutter) 1px;
	}
	.queue-toolbar > :not(.primary-tab-row) {
		align-self: center;
	}
	.primary-tab-row {
		position: relative;
		display: flex;
		flex-wrap: wrap;
		align-items: stretch;
		gap: 2px 4px;
		min-inline-size: 0;
		max-inline-size: 100%;
	}
	.primary-tab-row::after {
		content: '';
		position: absolute;
		inset-block-start: 100%;
		inset-inline-start: calc(-1 * var(--triage-gutter));
		inline-size: 100cqi;
		border-block-start: 1px solid var(--exec-border);
		pointer-events: none;
	}
	.triage-more-wrap {
		display: inline-flex;
		flex: none;
		align-items: stretch;
	}
	.queue-controls {
		display: flex;
		align-items: center;
		gap: 4px;
		margin-block: 4px;
	}
	.queue-tab {
		display: inline-flex;
		flex: none;
		align-items: center;
		gap: 6px;
		padding: 6px 12px 5px;
		min-height: var(--triage-control-height);
		border: 1px solid transparent;
		border-block-end-width: 2px;
		border-radius: var(--exec-ctrl-radius) var(--exec-ctrl-radius) 0 0;
		background: transparent;
		color: var(--exec-body);
		font-size: 0.875rem;
		font-weight: 600;
		white-space: nowrap;
		cursor: pointer;
		transition:
			background-color 120ms ease-out,
			color 120ms ease-out;
	}
	.queue-tab:hover {
		background: var(--exec-btn-ghost-bg-hover);
		color: var(--exec-ink);
	}
	.queue-tab:is([aria-pressed='true'], [data-current-kind]) {
		border-block-end-color: var(--tone-accent-ink);
		color: var(--exec-ink);
	}
	.queue-tab[aria-expanded='true'] {
		background: var(--exec-btn-ghost-bg-hover);
		color: var(--exec-ink);
	}
	.queue-tab-count,
	.triage-menu-count {
		color: var(--exec-muted);
		font-variant-numeric: tabular-nums;
		font-size: 0.75rem;
	}
	.queue-tab[aria-pressed='true'] .queue-tab-count {
		color: var(--tone-accent-ink);
	}
	@container queue-header (max-width: 38rem) {
		.queue-tab {
			gap: 4px;
			padding-inline: 8px;
		}
	}
	.queue-tab:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -2px;
	}
	@media (forced-colors: active) {
		.queue-tab {
			border: 1px solid ButtonText;
		}
		.queue-tab:is([aria-pressed='true'], [data-current-kind]) {
			border: 2px solid Highlight;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.queue-tab {
			transition: none;
		}
	}
</style>
