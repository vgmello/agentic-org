<!--
  @component
  Org Work header + primary tab bar + WorkToolbarMenus (shared by Org and Project Work surfaces).
-->
<script lang="ts">
	import InteriorSearchBar from '$lib/platform/shell/InteriorSearchBar.svelte';
	import type { LayoutMode, QueueFilters, SavedFilterId, ShowView } from '../../filters';
	import type { BoardMode, GroupBy, WorkSortBy, WorkSortDir } from '../../present';
	import type { TaskRow, TaskType } from '../../types';
	import { WORK_TABS } from './work-queue-helpers';
	import WorkToolbarMenus from './WorkToolbarMenus.svelte';

	let {
		uid,
		isOrgScope,
		hasData,
		openItemCount,
		needsMeCount,
		hasShellCommand,
		shortcutsEnabled,
		activeTab,
		showMode,
		visibleQueueRows,
		layout,
		boardMode,
		filters,
		scopeMine,
		savedCounts,
		hasFilterBadge,
		isListSurface,
		showGroupMenu,
		primaryGroup,
		groupOptions,
		subgroupCandidates,
		effectiveSubgroup,
		sortBy,
		sortDir,
		showDividers,
		showArchived,
		uniqueProjects,
		uniqueTeams,
		uniquePeople,
		showLayoutToggle,
		canCreate,
		createDisabled,
		onSelectTab,
		onShowChange,
		onLayoutChange,
		onBoardModeChange,
		onSavedSelect,
		onFiltersChange,
		onProjectChange,
		onAssigneeChange,
		onClearFilters,
		onGroupChange,
		onSubgroupChange,
		onSortByChange,
		onSortDirChange,
		onDividersChange,
		onShowArchivedChange,
		onCreate
	}: {
		uid: string;
		isOrgScope: boolean;
		hasData: boolean;
		openItemCount: number;
		needsMeCount: number;
		hasShellCommand: boolean;
		shortcutsEnabled: boolean;
		activeTab: TaskType;
		showMode: ShowView;
		visibleQueueRows: readonly TaskRow[];
		layout: LayoutMode;
		boardMode: BoardMode;
		filters: QueueFilters;
		scopeMine: boolean;
		savedCounts: Record<SavedFilterId, number>;
		hasFilterBadge: boolean;
		isListSurface: boolean;
		showGroupMenu: boolean;
		primaryGroup: GroupBy;
		groupOptions: GroupBy[];
		subgroupCandidates: GroupBy[];
		effectiveSubgroup: GroupBy;
		sortBy: WorkSortBy;
		sortDir: WorkSortDir;
		showDividers: boolean;
		showArchived: boolean;
		uniqueProjects: [string, string][];
		uniqueTeams: [string, string][];
		uniquePeople: [string, string][];
		showLayoutToggle: boolean;
		canCreate: boolean;
		createDisabled: boolean;
		onSelectTab: (next: TaskType) => void;
		onShowChange: (next: ShowView) => void;
		onLayoutChange: (next: LayoutMode) => void;
		onBoardModeChange: (next: BoardMode) => void;
		onSavedSelect: (saved: SavedFilterId) => void;
		onFiltersChange: (patch: Partial<QueueFilters>) => void;
		onProjectChange: (projectId: string) => void;
		onAssigneeChange: (assigneeId: string) => void;
		onClearFilters: () => void;
		onGroupChange: (next: GroupBy) => void;
		onSubgroupChange: (next: GroupBy) => void;
		onSortByChange: (next: WorkSortBy) => void;
		onSortDirChange: (next: WorkSortDir) => void;
		onDividersChange: (show: boolean) => void;
		onShowArchivedChange: (show: boolean) => void;
		onCreate: (kind: TaskType) => void;
	} = $props();

	function handleTabKey(event: KeyboardEvent, index: number) {
		if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
		const len = WORK_TABS.length;
		let next: number | undefined;
		if (event.key === 'ArrowRight') next = (index + 1) % len;
		else if (event.key === 'ArrowLeft') next = (index - 1 + len) % len;
		else if (event.key === 'Home') next = 0;
		else if (event.key === 'End') next = len - 1;
		if (next === undefined) return;
		event.preventDefault();
		onSelectTab(WORK_TABS[next].id);
		(event.currentTarget as HTMLElement | null)?.parentElement
			?.querySelector<HTMLButtonElement>(`#${CSS.escape(`${uid}-tab-${WORK_TABS[next].id}`)}`)
			?.focus();
	}
</script>

{#snippet toolbarBody()}
	<div
		class="primary-tab-row"
		data-page-menu={isOrgScope ? '' : undefined}
		aria-keyshortcuts={isOrgScope && shortcutsEnabled ? 'M' : undefined}
	>
		<div role="tablist" aria-label="Work" class="work-tablist">
			{#each WORK_TABS as t, index (t.id)}
				{@const selected = activeTab === t.id && (!isOrgScope || showMode === 'mine')}
				<button
					id="{uid}-tab-{t.id}"
					type="button"
					role="tab"
					data-menu-label={t.label}
					aria-selected={selected}
					aria-controls="{uid}-panel"
					tabindex={activeTab === t.id ? 0 : -1}
					onclick={() => onSelectTab(t.id)}
					onkeydown={(e) => handleTabKey(e, index)}
					class="queue-tab work-primary-tab {selected ? 'is-selected' : ''}"
				>
					<span>{t.label}</span>
					<span class="queue-tab-count">
						{visibleQueueRows.filter((r) => r.type === t.id).length}
					</span>
				</button>
			{/each}
		</div>
	</div>

	<WorkToolbarMenus
		{isOrgScope}
		{showMode}
		{needsMeCount}
		{activeTab}
		{layout}
		{boardMode}
		{filters}
		{scopeMine}
		{savedCounts}
		{hasFilterBadge}
		{isListSurface}
		{showGroupMenu}
		{primaryGroup}
		{groupOptions}
		{subgroupCandidates}
		{effectiveSubgroup}
		{sortBy}
		{sortDir}
		{showDividers}
		{showArchived}
		{uniqueProjects}
		{uniqueTeams}
		{uniquePeople}
		{showLayoutToggle}
		{canCreate}
		{createDisabled}
		{onShowChange}
		{onLayoutChange}
		{onBoardModeChange}
		{onSavedSelect}
		{onFiltersChange}
		{onProjectChange}
		{onAssigneeChange}
		{onClearFilters}
		{onGroupChange}
		{onSubgroupChange}
		{onSortByChange}
		{onSortDirChange}
		{onDividersChange}
		{onShowArchivedChange}
		{onCreate}
	/>
{/snippet}

{#if isOrgScope}
	<header class="work-header queue-header section-header">
		<div class="queue-headline">
			<div class="ph-t">
				<h1 id="shell-page-title" tabindex="-1" class="work-h1">Work</h1>
				{#if hasData}
					<span role="status" class="queue-count">
						<i class={[openItemCount === 0 && 'is-ok']} aria-hidden="true"></i>
						<span class="tabular-nums">
							{openItemCount} open{#if needsMeCount > 0}
								· {needsMeCount} need you{/if}
						</span>
					</span>
				{/if}
			</div>

			{#if hasShellCommand}
				<div class="queue-search-wrap">
					<InteriorSearchBar class="w-full" />
				</div>
			{/if}
		</div>

		{#if hasData}
			<div class="work-toolbar work-toolbar-bar queue-toolbar section-toolbar">
				{@render toolbarBody()}
			</div>
		{/if}
	</header>
{:else}
	<div class="work-toolbar work-project-toolbar">
		{@render toolbarBody()}
	</div>
{/if}

<style>
	.queue-header {
		position: relative;
		z-index: 1;
		container: queue-header / inline-size;
		border-bottom: 1px solid var(--exec-border, var(--line));
	}
	.queue-headline {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: flex-start;
		gap: 8px 16px;
		padding: 14px var(--work-gutter) 4px;
	}
	.ph-t {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px 14px;
		min-width: 0;
	}
	.work-h1 {
		color: var(--exec-ink, var(--fg));
		font-size: clamp(1.375rem, 2.2vw, 1.875rem);
		line-height: 1.22;
		font-weight: 600;
		letter-spacing: -0.03em;
	}
	.work-h1:focus {
		outline: none;
	}
	.work-h1:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	.queue-count {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		color: var(--exec-muted, var(--fg-muted));
		font-size: 0.8125rem;
		font-weight: 500;
	}
	.queue-count i {
		display: inline-block;
		width: 7px;
		height: 7px;
		border-radius: 50%;
		background: var(--tone-accent, var(--brand));
		flex: none;
	}
	.queue-count i.is-ok {
		background: var(--tone-ok, var(--success));
	}
	.queue-search-wrap {
		display: flex;
		align-items: center;
		min-width: 0;
		max-width: 100%;
		margin-inline-start: auto;
	}
	.queue-toolbar {
		display: flex;
		flex-wrap: wrap;
		align-items: stretch;
		gap: 6px 8px;
		padding: 4px var(--work-gutter) 1px;
	}
	.work-project-toolbar {
		display: flex;
		flex-wrap: wrap;
		align-items: stretch;
		gap: 6px 8px;
		border-bottom: 1px solid var(--exec-border, var(--line));
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
	.work-tablist {
		display: flex;
		flex-wrap: wrap;
		align-items: stretch;
		gap: 2px 4px;
	}
	.queue-tab {
		display: inline-flex;
		flex: none;
		align-items: center;
		gap: 6px;
		padding: 6px 12px 5px;
		min-height: var(--work-control-height);
		border: 1px solid transparent;
		border-block-end-width: 2px;
		border-radius: var(--exec-ctrl-radius, 6px) var(--exec-ctrl-radius, 6px) 0 0;
		background: transparent;
		color: var(--exec-body, var(--fg-secondary));
		font-size: 0.875rem;
		font-weight: 600;
		white-space: nowrap;
		cursor: pointer;
		transition:
			background-color 120ms ease-out,
			color 120ms ease-out;
	}
	.queue-tab:hover {
		background: var(--exec-btn-ghost-bg-hover, var(--surface-raised));
		color: var(--exec-ink, var(--fg));
	}
	.work-primary-tab.is-selected {
		border-block-end-color: var(--tone-accent-ink, var(--brand));
		color: var(--exec-ink, var(--fg));
	}
	.queue-tab:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -4px;
	}
	.queue-tab-count {
		color: var(--exec-muted, var(--fg-muted));
		font-variant-numeric: tabular-nums;
		font-size: 0.75rem;
	}
	.work-primary-tab.is-selected .queue-tab-count {
		color: var(--tone-accent-ink, var(--brand-text));
	}

	@container queue-header (min-width: 52rem) {
		.queue-headline {
			display: grid;
			grid-template-columns: minmax(0, 1fr) clamp(20rem, 34cqi, 30rem);
			align-items: center;
		}
		.queue-search-wrap {
			width: 100%;
			margin-inline: 0;
		}
	}
	@container queue-header (max-width: 51.99rem) {
		.queue-headline {
			gap: 6px 8px;
		}
		.queue-search-wrap {
			flex: 1 1 100%;
			width: 100%;
			margin-inline-start: 0;
		}
	}
	@container queue-header (max-width: 38rem) {
		.queue-tab {
			gap: 4px;
			padding-inline: 8px;
		}
	}

	@media (max-width: 720px), (pointer: coarse) {
		.queue-tab,
		.work-primary-tab {
			min-height: 44px;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.queue-tab,
		.work-primary-tab {
			transition: none;
		}
	}
	@media (forced-colors: active) {
		.work-primary-tab {
			border-block-end-color: Canvas;
		}
		.work-primary-tab.is-selected {
			border-block-end-color: Highlight;
		}
	}
</style>
