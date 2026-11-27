<!--
  @component
  POC-aligned Work toolbar controls: Show view menu, Layout toggle (List/Board + 4 stages/6 statuses),
  Filter icon popover menu, Display icon popover menu, and primary split Create CTA.
-->
<script lang="ts">
	import { ChevronDown, Columns3, List, Plus } from '@lucide/svelte';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import {
		SHOW_VIEWS,
		type LayoutMode,
		type QueueFilters,
		type SavedFilterId,
		type ShowView
	} from '../../filters';
	import type { BoardMode, GroupBy, WorkSortBy, WorkSortDir } from '../../present';
	import type { TaskType } from '../../types';
	import WorkDisplayMenu from './WorkDisplayMenu.svelte';
	import WorkFilterMenu from './WorkFilterMenu.svelte';

	let {
		isOrgScope,
		showMode,
		needsMeCount,
		activeTab,
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
		showArchived = false,
		uniqueProjects,
		uniqueTeams,
		uniquePeople,
		showLayoutToggle,
		canCreate,
		createDisabled,
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
		isOrgScope: boolean;
		showMode: ShowView;
		needsMeCount: number;
		activeTab: TaskType;
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
		showArchived?: boolean;
		uniqueProjects: [string, string][];
		uniqueTeams: [string, string][];
		uniquePeople: [string, string][];
		showLayoutToggle: boolean;
		canCreate: boolean;
		createDisabled: boolean;
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
		onShowArchivedChange?: (show: boolean) => void;
		onCreate: (kind: TaskType) => void;
	} = $props();

	let showMenuOpen = $state(false);
	let createMenuOpen = $state(false);

	const showLabel = $derived(SHOW_VIEWS.find((v) => v.id === showMode)?.label ?? 'Mine');

	const primaryCreate = $derived.by<{ kind: TaskType; label: string }>(() => {
		if (!isOrgScope || showMode === 'mine') {
			if (activeTab === 'bug') return { kind: 'bug', label: 'Report bug' };
			if (activeTab === 'request') return { kind: 'request', label: 'New request' };
		}
		return { kind: 'task', label: 'New task' };
	});

	const secondaryCreates = $derived(
		(
			[
				{ kind: 'task', label: 'New task' },
				{ kind: 'bug', label: 'Report bug' },
				{ kind: 'request', label: 'New request' }
			] as const
		).filter((c) => c.kind !== primaryCreate.kind)
	);

	const itemHit = 'min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11';
	const menuMotion =
		'triage-menu-surface motion-reduce:animate-none! motion-reduce:transition-none! motion-reduce:data-[state=closed]:animate-none! motion-reduce:data-[state=open]:animate-none!';
</script>

<div class="work-controls">
	{#if isOrgScope}
		<DropdownMenu.Root bind:open={showMenuOpen}>
			<DropdownMenu.Trigger>
				{#snippet child({ props })}
					<button
						{...props}
						type="button"
						aria-label="Show: {showLabel}"
						data-testid="show-menu"
						data-value={showMode}
						class="work-menu-trigger"
					>
						<span>{showLabel}</span>
						{#if showMode === 'needs_me' && needsMeCount > 0}
							<span class="work-trigger-count">{needsMeCount}</span>
						{/if}
						<ChevronDown class="size-3.5 shrink-0 opacity-75" aria-hidden="true" />
					</button>
				{/snippet}
			</DropdownMenu.Trigger>
			<DropdownMenu.Content preventScroll={false} align="end" class="{menuMotion} w-48">
				<DropdownMenu.Label>View</DropdownMenu.Label>
				<DropdownMenu.RadioGroup
					value={showMode}
					onValueChange={(val) => {
						if (val) onShowChange(val as ShowView);
					}}
				>
					{#each SHOW_VIEWS as v (v.id)}
						<DropdownMenu.RadioItem
							value={v.id}
							closeOnSelect={true}
							data-show-option={v.id}
							class="flex items-center justify-between gap-2 pr-8 {itemHit}"
						>
							<span>{v.label}</span>
							{#if v.id === 'needs_me'}
								<span class="font-mono text-xs text-fg-muted tabular-nums">{needsMeCount}</span>
							{/if}
						</DropdownMenu.RadioItem>
					{/each}
				</DropdownMenu.RadioGroup>
			</DropdownMenu.Content>
		</DropdownMenu.Root>
	{/if}

	{#if showLayoutToggle}
		<div class="work-layout-toggle" role="group" aria-label="Layout">
			<button
				type="button"
				aria-pressed={layout === 'list'}
				onclick={() => onLayoutChange('list')}
				class="queue-tab work-toggle-tab"
			>
				<List class="size-3.5 shrink-0" aria-hidden="true" />
				<span>List</span>
			</button>
			<button
				type="button"
				aria-pressed={layout === 'board'}
				onclick={() => onLayoutChange('board')}
				class="queue-tab work-toggle-tab"
			>
				<Columns3 class="size-3.5 shrink-0" aria-hidden="true" />
				<span>Board</span>
			</button>
		</div>

		{#if layout === 'board'}
			<div
				role="group"
				aria-label="Board columns"
				data-testid="board-mode-toggle"
				class="work-board-mode"
			>
				<button
					type="button"
					aria-pressed={boardMode === 'stages'}
					onclick={() => onBoardModeChange('stages')}
					class="work-mode-btn">4 stages</button
				>
				<button
					type="button"
					aria-pressed={boardMode === 'statuses'}
					onclick={() => onBoardModeChange('statuses')}
					class="work-mode-btn">6 statuses</button
				>
			</div>
		{/if}
	{/if}

	{#if !isOrgScope || showMode !== 'needs_me'}
		<div class="work-icon-controls" role="group" aria-label="Work filter and display controls">
			<WorkFilterMenu
				{isOrgScope}
				{showMode}
				{activeTab}
				{filters}
				{scopeMine}
				{savedCounts}
				{hasFilterBadge}
				{uniqueProjects}
				{uniqueTeams}
				{uniquePeople}
				{itemHit}
				{menuMotion}
				{onSavedSelect}
				{onFiltersChange}
				{onProjectChange}
				{onAssigneeChange}
				{onClearFilters}
			/>

			<WorkDisplayMenu
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
				{boardMode}
				{itemHit}
				{menuMotion}
				{onGroupChange}
				{onSubgroupChange}
				{onSortByChange}
				{onSortDirChange}
				{onDividersChange}
				{onBoardModeChange}
				{onShowArchivedChange}
			/>
		</div>
	{/if}

	{#if canCreate}
		<div class="work-create-group">
			<button
				type="button"
				disabled={createDisabled}
				onclick={() => onCreate(primaryCreate.kind)}
				class="work-create-primary"
			>
				<Plus class="size-3.5 shrink-0" aria-hidden="true" />
				<span>{primaryCreate.label}</span>
			</button>
			<DropdownMenu.Root bind:open={createMenuOpen}>
				<DropdownMenu.Trigger>
					{#snippet child({ props })}
						<button
							{...props}
							type="button"
							disabled={createDisabled}
							aria-label="More create options"
							class="work-create-chevron"
						>
							<ChevronDown class="size-3.5" aria-hidden="true" />
						</button>
					{/snippet}
				</DropdownMenu.Trigger>
				<DropdownMenu.Content preventScroll={false} align="end" class="{menuMotion} w-44">
					{#each secondaryCreates as item (item.kind)}
						<DropdownMenu.Item onSelect={() => onCreate(item.kind)} class={itemHit}>
							{item.label}
						</DropdownMenu.Item>
					{/each}
				</DropdownMenu.Content>
			</DropdownMenu.Root>
		</div>
	{/if}
</div>

<style>
	.work-controls {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px 16px;
		margin-inline-start: auto;
		margin-block: 4px;
	}
	.work-controls > :not(:first-child) {
		position: relative;
	}
	.work-controls > :not(:first-child)::before {
		content: '';
		position: absolute;
		inset-inline-start: -8px;
		top: 50%;
		width: 1px;
		height: 18px;
		transform: translateY(-50%);
		background: var(--exec-border, var(--line));
		pointer-events: none;
	}
	.work-menu-trigger {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		min-height: var(--work-control-height, 36px);
		padding: 0 10px;
		border: 1px solid transparent;
		border-radius: var(--exec-ctrl-radius, 6px);
		background: transparent;
		color: var(--exec-body, var(--fg-secondary));
		font-size: 0.8125rem;
		font-weight: 600;
		white-space: nowrap;
		cursor: pointer;
		transition:
			background-color 120ms ease-out,
			color 120ms ease-out;
	}
	.work-menu-trigger:hover,
	.work-menu-trigger[aria-expanded='true'] {
		background: var(--exec-btn-ghost-bg-hover, var(--surface-raised));
		color: var(--exec-ink, var(--fg));
	}
	.work-trigger-count {
		color: var(--tone-accent-ink, var(--brand-text));
		font-size: 0.75rem;
		font-variant-numeric: tabular-nums;
	}
	.work-layout-toggle,
	.work-board-mode {
		display: inline-flex;
		align-items: center;
		gap: 2px;
		min-height: var(--work-control-height, 36px);
		padding: 2px;
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--toolbar-seg-bg, var(--exec-well, var(--surface-raised)));
		box-shadow: inset 0 0 0 1px var(--exec-border, var(--line));
	}
	.work-toggle-tab {
		display: inline-flex;
		align-items: center;
		gap: 5px;
		min-height: calc(var(--work-control-height, 36px) - 6px);
		padding: 0 10px;
		border: 1px solid transparent;
		border-radius: calc(var(--exec-ctrl-radius, 8px) - 2px);
		background: transparent;
		color: var(--exec-body, var(--fg-secondary));
		font-size: 0.8125rem;
		font-weight: 600;
		white-space: nowrap;
		cursor: pointer;
		transition:
			background-color 120ms ease-out,
			color 120ms ease-out;
	}
	.work-toggle-tab:hover,
	.work-mode-btn:hover {
		color: var(--exec-ink, var(--fg));
	}
	.work-toggle-tab[aria-pressed='true'] {
		background: var(
			--toolbar-seg-active-bg,
			var(--workspace-mode-selected, var(--exec-surface-hi, var(--surface-overlay)))
		);
		border-color: var(--exec-border-hi, var(--line-strong));
		color: var(--exec-ink, var(--fg));
		box-shadow: var(--exec-panel-shadow, none);
	}
	.work-mode-btn {
		display: inline-flex;
		align-items: center;
		min-height: calc(var(--work-control-height, 36px) - 6px);
		padding: 0 9px;
		border-radius: calc(var(--exec-ctrl-radius, 8px) - 2px);
		background: transparent;
		color: var(--exec-body, var(--fg-secondary));
		font-size: 0.75rem;
		font-weight: 600;
		white-space: nowrap;
		cursor: pointer;
	}
	.work-mode-btn[aria-pressed='true'] {
		background: var(--toolbar-seg-active-bg, var(--exec-surface-hi, var(--surface-overlay)));
		color: var(--exec-ink, var(--fg));
	}
	.work-icon-controls {
		display: inline-flex;
		align-items: center;
		gap: 2px;
	}
	.work-icon-controls :global(.queue-icon-btn) {
		position: relative;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		min-width: var(--work-control-height, 36px);
		min-height: var(--work-control-height, 36px);
		border-radius: var(--exec-ctrl-radius, 6px);
		background: transparent;
		color: var(--exec-body, var(--fg-secondary));
		cursor: pointer;
		transition:
			background-color 120ms ease-out,
			color 120ms ease-out;
	}
	.work-icon-controls :global(.queue-icon-btn:hover),
	.work-icon-controls :global(.queue-icon-btn[aria-expanded='true']) {
		background: var(--exec-btn-ghost-bg-hover, var(--surface-raised));
		color: var(--exec-ink, var(--fg));
	}
	.work-icon-controls :global(.queue-icon-btn[data-active]) {
		color: var(--tone-accent-ink, var(--brand-text));
	}
	.work-icon-controls :global(.queue-icon-btn[data-active]::after) {
		content: '';
		position: absolute;
		inset: 5px 5px auto auto;
		width: 5px;
		height: 5px;
		border-radius: 50%;
		background: currentColor;
	}
	.work-create-group {
		display: inline-flex;
		align-items: stretch;
		border-radius: var(--exec-ctrl-radius, 8px);
		overflow: hidden;
		background: var(--exec-cta-bg, var(--primary));
		box-shadow: var(--exec-cta-shadow, none);
	}
	.work-create-primary,
	.work-create-chevron {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		min-height: var(--work-control-height, 36px);
		color: var(--exec-on-accent, var(--on-emphasis));
		font-size: 0.8125rem;
		font-weight: 600;
		cursor: pointer;
		transition: background-color 120ms ease-out;
	}
	.work-create-primary {
		gap: 5px;
		padding: 0 11px 0 10px;
		white-space: nowrap;
	}
	.work-create-chevron {
		min-width: 28px;
		padding: 0 6px;
		border-inline-start: 1px solid var(--exec-border-hi, var(--line-strong));
	}
	.work-create-primary:hover:not(:disabled),
	.work-create-chevron:hover:not(:disabled) {
		background: var(--exec-cta-fill, var(--primary-hover));
	}
	.work-create-primary:disabled,
	.work-create-chevron:disabled {
		opacity: 0.5;
		cursor: not-allowed;
	}
	.work-menu-trigger:focus-visible,
	.work-toggle-tab:focus-visible,
	.work-mode-btn:focus-visible,
	.work-icon-controls :global(.queue-icon-btn:focus-visible),
	.work-create-primary:focus-visible,
	.work-create-chevron:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -2px;
	}

	@media (max-width: 720px), (pointer: coarse) {
		.work-mode-btn {
			min-height: 44px;
		}
		.work-create-chevron {
			min-width: 44px;
		}
	}

	@media (forced-colors: active) {
		.work-toggle-tab[aria-pressed='true'],
		.work-mode-btn[aria-pressed='true'] {
			background: Highlight;
			border-color: Highlight;
			color: HighlightText;
		}
		.work-icon-controls :global(.queue-icon-btn),
		.work-menu-trigger {
			border: 1px solid ButtonText;
		}
		.work-icon-controls :global(.queue-icon-btn[data-active]) {
			border: 2px solid Highlight;
		}
	}
</style>
